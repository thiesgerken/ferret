//! Embedding models and the TEI pool: which TEI serves which model, query embeddings, and the
//! embed pump that fills `chunk_embeddings` in the background, so ingest never waits for a model.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, PgConnection, PgPool, postgres::PgListener};
use tokio::{sync::watch, task::JoinSet};
use uuid::Uuid;

use crate::{
    StartupError,
    chunking::MAX_CHUNK_CHARS,
    config::{EmbeddingConfig, ModelConfig},
    model_registry, tei,
    tei_load::{Load, TeiLoad},
    tei_pool::{self, BATCH_TIMEOUT, Discovery, Tei, TeiState, Use},
};

const MAX_EMBEDDED_TITLE_CHARS: usize = 200;
const TITLE_SEPARATOR: &str = "\n\n";
/// The longest embedded input, without the model's document prefix. Every TEI must take it.
const MAX_EMBEDDED_CHARS: usize =
    MAX_EMBEDDED_TITLE_CHARS + TITLE_SEPARATOR.len() + MAX_CHUNK_CHARS;
/// A search waits this long for its query embedding, then answers by keyword only. A TEI busy
/// with pump batches answers slower than an idle one.
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// How the claim query looks ahead for chunks no other pump has locked.
const CLAIM_CANDIDATES: i64 = 256;
/// The pump also looks for work this often without a notification, e.g. after an outage.
const IDLE_POLL: Duration = Duration::from_secs(30);
/// How often every TEI is asked which model it serves, and the switches are read again.
const DISCOVERY_POLL: Duration = Duration::from_secs(10);

pub struct Model {
    pub config: ModelConfig,
}

impl Model {
    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn dimension(&self) -> usize {
        self.config.dimension
    }

    fn index_name(&self) -> String {
        model_registry::index_name(self.name())
    }
}

pub struct Embedding {
    http: reqwest::Client,
    /// The default model first.
    models: Vec<Model>,
    /// The models' configs in the same order, for discovery.
    configs: Arc<Vec<ModelConfig>>,
    /// In config order.
    teis: Vec<Arc<Tei>>,
    tei_load: TeiLoad,
    /// Wakes the pump loops: new chunks, or a TEI that changed.
    wake: Arc<watch::Sender<()>>,
}

#[derive(Debug)]
pub enum EmbedError {
    Tei(reqwest::Error),
    /// A TEI answered with another model or dimension than it should have: nothing is written.
    Mismatch(String),
    /// No TEI serves the model for this.
    Unavailable(String),
    Database(sqlx::Error),
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tei(error) => write!(f, "TEI: {error}"),
            Self::Mismatch(message) | Self::Unavailable(message) => f.write_str(message),
            Self::Database(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for EmbedError {}

impl From<reqwest::Error> for EmbedError {
    fn from(error: reqwest::Error) -> Self {
        Self::Tei(error)
    }
}

impl From<sqlx::Error> for EmbedError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Sets up the configured models and TEIs and asks every TEI once which model it serves. TEIs
/// that are down don't keep the server from starting: searches then answer by keyword.
pub async fn prepare(pool: &PgPool, config: EmbeddingConfig) -> Result<Embedding, StartupError> {
    config.validate()?;
    let mut configs = config.models;
    configs.sort_by_key(|model| model.name != config.default);
    let models: Vec<Model> = configs.into_iter().map(|config| Model { config }).collect();
    model_registry::register(pool, &models).await?;
    for model in &models {
        create_index(pool, model).await?;
    }
    reset_removed_defaults(pool, &models).await?;
    tei_pool::sync_config(pool, &config.teis).await?;
    let embedding = Embedding {
        http: tei::client(),
        configs: Arc::new(models.iter().map(|model| model.config.clone()).collect()),
        models,
        teis: config
            .teis
            .iter()
            .map(|tei| Arc::new(Tei::new(tei)))
            .collect(),
        tei_load: TeiLoad::default(),
        wake: Arc::new(watch::Sender::new(())),
    };
    embedding.refresh(pool).await?;
    Ok(embedding)
}

// ponytail: exact counts, and neither table has an index on user_id, so both scan the whole
// table. Fine for a household's data. An index on (user_id, model) or cached counts if Settings
// gets slow.
/// How many of `user_id`'s chunks `model` has embedded.
pub async fn embedded_chunks(
    pool: &PgPool,
    user_id: Uuid,
    model: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from chunk_embeddings where user_id = $1 and model = $2"#,
        user_id,
        model,
    )
    .fetch_one(pool)
    .await
}

/// The minutes the pump's rate is averaged over.
const RATE_WINDOW_MINUTES: i32 = 10;

/// Chunks the pump embedded for `model` per minute lately, across users and server processes.
pub async fn rate_per_minute(pool: &PgPool, model: &str) -> Result<f64, sqlx::Error> {
    let embedded = sqlx::query_scalar!(
        r#"select count(*) as "count!" from chunk_embeddings
           where model = $1 and embedded_at > now() - make_interval(mins => $2)"#,
        model,
        RATE_WINDOW_MINUTES,
    )
    .fetch_one(pool)
    .await?;
    Ok(embedded as f64 / f64::from(RATE_WINDOW_MINUTES))
}

/// Seconds until `pending` chunks are embedded at `rate_per_minute`, `None` when nothing is
/// pending or nothing gets embedded.
pub fn eta_seconds(pending: i64, rate_per_minute: f64) -> Option<i64> {
    (pending > 0 && rate_per_minute > 0.0)
        .then(|| (pending as f64 / rate_per_minute * 60.0).ceil() as i64)
}

/// `user_id`'s chunks still waiting for `model`, and those TEI refused.
pub async fn user_backlog(
    pool: &PgPool,
    user_id: Uuid,
    model: &str,
) -> Result<(i64, i64), sqlx::Error> {
    let backlog = sqlx::query!(
        r#"select count(*) filter (where f.chunk_id is null) as "pending!",
                  count(f.chunk_id) as "failed!"
           from chunks c
           left join embedding_failures f on f.chunk_id = c.id and f.model = $2
           where c.user_id = $1
             and not exists (select 1 from chunk_embeddings e
                             where e.chunk_id = c.id and e.model = $2)"#,
        user_id,
        model,
    )
    .fetch_one(pool)
    .await?;
    Ok((backlog.pending, backlog.failed))
}

pub async fn chunks(pool: &PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"select count(*) as "count!" from chunks where user_id = $1"#,
        user_id
    )
    .fetch_one(pool)
    .await
}

/// Users whose default model is no longer configured fall back to the configured default.
async fn reset_removed_defaults(pool: &PgPool, models: &[Model]) -> Result<(), sqlx::Error> {
    let names: Vec<String> = models.iter().map(|model| model.name().to_owned()).collect();
    let reset = sqlx::query!(
        r#"with removed as (
               select id, default_embedding_model from users
               where default_embedding_model <> all($1)
               for update
           )
           update users u set default_embedding_model = null
           from removed where u.id = removed.id
           returning u.username, removed.default_embedding_model as "model!""#,
        &names,
    )
    .fetch_all(pool)
    .await?;
    for user in reset {
        tracing::warn!(user = %user.username, model = %user.model, "the user's default embedding model is no longer configured, reset to the configured default");
    }
    Ok(())
}

async fn create_index(pool: &PgPool, model: &Model) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    model_registry::lock_indexes(&mut tx).await?;
    // Validated names (config.rs) need no quoting, and DDL takes no bind parameters.
    sqlx::query(AssertSqlSafe(format!(
        "create index if not exists {} on chunk_embeddings
             using hnsw ((embedding::halfvec({})) halfvec_cosine_ops)
             where model = '{}'",
        model.index_name(),
        model.dimension(),
        model.config.name,
    )))
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Every TEI's state for Admin.
pub struct TeiStatus {
    pub url: String,
    pub external: bool,
    pub state: TeiState,
    /// The configured model it serves.
    pub model: Option<String>,
    /// Why it's of no use, for TEIs not serving.
    pub problem: Option<String>,
    pub last_error: Option<(String, chrono::DateTime<chrono::Utc>)>,
    pub busy: bool,
    /// While it may embed: chunks per pump batch in this process.
    pub pump_batch_size: Option<usize>,
    /// `None` while its metrics don't answer.
    pub load: Option<Load>,
    /// While it may embed: until its model's backlog is embedded at its throughput.
    pub eta_seconds: Option<i64>,
}

/// How one configured model is doing.
pub struct ModelStatus {
    pub name: String,
    pub model_id: String,
    pub is_default: bool,
    pub dimension: usize,
    /// Chunks still to embed per source, across users. Sources without any are left out.
    pub backlog: Vec<SourceBacklog>,
    /// Chunks with an embedding for this model, across users.
    pub embedded: i64,
    /// Every user's chunks.
    pub chunks: i64,
    pub rate_per_minute: f64,
    /// Until the whole backlog is embedded.
    pub eta_seconds: Option<i64>,
}

pub struct SourceBacklog {
    pub source: String,
    /// Waiting for the pump.
    pub pending: i64,
    /// TEI refused them, so the pump leaves them out.
    pub failed: i64,
}

impl Embedding {
    pub fn default_model(&self) -> &Model {
        &self.models[0]
    }

    /// Every configured model, the default first.
    pub fn models(&self) -> &[Model] {
        &self.models
    }

    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.name() == name)
    }

    fn index_of(&self, model: &Model) -> usize {
        self.models
            .iter()
            .position(|configured| configured.name() == model.name())
            .expect("models come from this list")
    }

    /// Whether an enabled TEI serves `model` for `use_for`, as of the last discovery.
    pub fn is_served(&self, model: &Model, use_for: Use) -> bool {
        let index = self.index_of(model);
        self.teis.iter().any(|tei| {
            tei.serving(use_for)
                .is_some_and(|(model, _)| model == index)
        })
    }

    /// Asks every TEI which model it serves and reads the switches. Wakes the pump when anything
    /// changed.
    async fn refresh(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        let mut discoveries = JoinSet::new();
        for tei in &self.teis {
            let (http, configs, tei) = (self.http.clone(), self.configs.clone(), tei.clone());
            discoveries.spawn(async move {
                let discovery =
                    tei_pool::discover(&http, &tei.url, &configs, MAX_EMBEDDED_CHARS).await;
                (tei, discovery)
            });
        }
        let mut changed = false;
        while let Some(joined) = discoveries.join_next().await {
            let (tei, discovery) = joined.expect("discovery doesn't panic");
            changed |= set_discovery(&self.configs, &tei, discovery);
        }
        // Read after the discovery, which may take seconds: closer to an admin's latest change.
        for (url, stored) in tei_pool::switches(pool).await? {
            // A TEI only another replica's config has isn't this server's business.
            if let Some(tei) = self.teis.iter().find(|tei| tei.url == url) {
                changed |= tei.apply_stored_switches(stored);
            }
        }
        if changed {
            self.wake.send_replace(());
        }
        Ok(())
    }

    /// Changes a TEI's switches (those given), on every replica within `DISCOVERY_POLL`, here at
    /// once. `None` for a TEI that isn't configured.
    pub async fn update_tei(
        &self,
        pool: &PgPool,
        url: &str,
        enabled: Option<bool>,
        queries: Option<bool>,
        embedding: Option<bool>,
    ) -> Result<Option<TeiStatus>, sqlx::Error> {
        let Some(tei) = self.teis.iter().find(|tei| tei.url == url) else {
            return Ok(None);
        };
        let Some(switches) =
            tei_pool::update_switches(pool, url, enabled, queries, embedding).await?
        else {
            return Ok(None);
        };
        if tei.update(|state| state.switches = switches) {
            self.wake.send_replace(());
        }
        let pending = self.pending_by_model(pool).await?;
        Ok(Some(self.tei_status(pool, tei, &pending).await?))
    }

    /// Every TEI, in config order.
    pub async fn tei_statuses(&self, pool: &PgPool) -> Result<Vec<TeiStatus>, sqlx::Error> {
        let pending = self.pending_by_model(pool).await?;
        let mut statuses = Vec::new();
        for tei in &self.teis {
            statuses.push(self.tei_status(pool, tei, &pending).await?);
        }
        Ok(statuses)
    }

    async fn tei_status(
        &self,
        pool: &PgPool,
        tei: &Tei,
        pending: &[i64],
    ) -> Result<TeiStatus, sqlx::Error> {
        let state = tei.state();
        let model = match &state.discovery {
            Discovery::Serving { model, .. } | Discovery::Unsuitable { model, .. } => Some(*model),
            _ => None,
        };
        let embedding = tei.serving(Use::Embedding).is_some();
        // A TEI that doesn't answer `/info` won't answer `/metrics` either.
        let load = match state.discovery {
            Discovery::Offline(_) | Discovery::Pending => None,
            _ => self.tei_load.read(&self.http, &tei.url).await,
        };
        let eta_seconds = model
            .filter(|_| embedding)
            .zip(load.as_ref().and_then(|load| load.throughput_per_minute))
            .and_then(|(model, throughput)| eta_seconds(pending[model], throughput));
        Ok(TeiStatus {
            url: tei.url.clone(),
            external: tei.external,
            model: model.map(|model| self.models[model].name().to_owned()),
            problem: state.discovery.problem(),
            last_error: tei_pool::last_error(pool, &tei.url).await?,
            busy: tei.is_busy(),
            pump_batch_size: embedding.then(|| tei.batch_size()),
            load,
            eta_seconds,
            state,
        })
    }

    /// Chunks waiting for each model, in model order.
    async fn pending_by_model(&self, pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
        let mut pending = Vec::new();
        for model in &self.models {
            pending.push(
                sqlx::query_scalar!(
                    r#"select count(*) as "count!" from chunks c
                       where not exists (select 1 from chunk_embeddings e
                                         where e.chunk_id = c.id and e.model = $1)
                         and not exists (select 1 from embedding_failures f
                                         where f.chunk_id = c.id and f.model = $1)"#,
                    model.name(),
                )
                .fetch_one(pool)
                .await?,
            );
        }
        Ok(pending)
    }

    /// Every configured model's backlog, the default first.
    // ponytail: the backlog counts every chunk on each call, fine for a personal index. Keep
    // running counts if the Admin page gets slow.
    pub async fn status(&self, pool: &PgPool) -> Result<Vec<ModelStatus>, sqlx::Error> {
        let mut statuses = Vec::new();
        for (index, model) in self.models.iter().enumerate() {
            let name = model.name();
            // Pending as the pump sees it: no embedding and no recorded failure.
            let backlog = sqlx::query_as!(
                SourceBacklog,
                r#"select c.source,
                          count(*) filter (where f.chunk_id is null) as "pending!",
                          count(f.chunk_id) as "failed!"
                   from chunks c
                   left join embedding_failures f on f.chunk_id = c.id and f.model = $1
                   where not exists (select 1 from chunk_embeddings e
                                     where e.chunk_id = c.id and e.model = $1)
                   group by c.source
                   order by c.source"#,
                name,
            )
            .fetch_all(pool)
            .await?;
            let rate_per_minute = rate_per_minute(pool, name).await?;
            let pending = backlog.iter().map(|source| source.pending).sum();
            let embedded = sqlx::query_scalar!(
                r#"select count(*) as "count!" from chunk_embeddings where model = $1"#,
                name
            )
            .fetch_one(pool)
            .await?;
            let chunks = sqlx::query_scalar!(r#"select count(*) as "count!" from chunks"#)
                .fetch_one(pool)
                .await?;
            statuses.push(ModelStatus {
                name: name.to_owned(),
                model_id: model.config.model_id.clone(),
                is_default: index == 0,
                dimension: model.dimension(),
                backlog,
                embedded,
                chunks,
                rate_per_minute,
                eta_seconds: eta_seconds(pending, rate_per_minute),
            });
        }
        Ok(statuses)
    }

    /// Embeds `texts` on `tei` for `model`, which it serves as `served`. Discards the answer
    /// unless it comes from that model with the model's dimension: a TEI may switch models at any
    /// time, and its vectors must never mix with another model's.
    async fn embed_on(
        &self,
        tei: &Tei,
        model: &Model,
        served: &str,
        texts: &[String],
        timeout: Duration,
    ) -> Result<Vec<Vec<f32>>, EmbedError> {
        let embedded = tei::embed(&self.http, &tei.url, texts, timeout).await?;
        if embedded.model != served {
            return Err(EmbedError::Mismatch(format!(
                "answered with the model {:?} instead of {served:?}",
                embedded.model
            )));
        }
        if embedded.vectors.len() != texts.len() {
            return Err(EmbedError::Mismatch(format!(
                "answered {} texts with {} vectors",
                texts.len(),
                embedded.vectors.len()
            )));
        }
        if let Some(vector) = embedded
            .vectors
            .iter()
            .find(|vector| vector.len() != model.dimension())
        {
            return Err(EmbedError::Mismatch(format!(
                "answered with {} dimensions, the model {} has {}",
                vector.len(),
                model.name(),
                model.dimension()
            )));
        }
        Ok(embedded.vectors)
    }

    /// Embeds `query` with `model`'s query prefix unless it's cached, and returns its cache key.
    /// Tries the TEIs that serve the model for queries, idle ones first, until one answers.
    pub async fn cache_query(
        &self,
        pool: &PgPool,
        model: &Model,
        query: &str,
    ) -> Result<Vec<u8>, EmbedError> {
        let text = format!("{}{query}", model.config.query_prefix);
        let hash = text_hash(&text);
        let cached = sqlx::query_scalar!(
            r#"select exists (
                   select 1 from embedding_cache where text_hash = $1 and model = $2
               ) as "cached!""#,
            hash,
            model.config.name,
        )
        .fetch_one(pool)
        .await?;
        if cached {
            return Ok(hash);
        }
        let index = self.index_of(model);
        let mut candidates: Vec<(&Arc<Tei>, String)> = self
            .teis
            .iter()
            .filter_map(|tei| match tei.serving(Use::Queries) {
                Some((served_model, served)) if served_model == index => Some((tei, served)),
                _ => None,
            })
            .collect();
        // Stable: config order among equally busy ones.
        candidates.sort_by_key(|(tei, _)| tei.is_busy());
        let deadline = Instant::now() + QUERY_TIMEOUT;
        let mut failure = EmbedError::Unavailable(format!(
            "no TEI serves the embedding model {} for queries",
            model.name()
        ));
        for (tei, served) in candidates {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let texts = [text.clone()];
            match self.embed_on(tei, model, &served, &texts, remaining).await {
                Ok(vectors) => {
                    tei_pool::clear_error(pool, &tei.url).await;
                    sqlx::query!(
                        "insert into embedding_cache (text_hash, model, embedding)
                         values ($1, $2, $3::text::halfvec)
                         on conflict do nothing",
                        hash,
                        model.config.name,
                        vector_literal(&vectors[0]),
                    )
                    .execute(pool)
                    .await?;
                    return Ok(hash);
                }
                Err(EmbedError::Database(error)) => return Err(error.into()),
                Err(error) => {
                    self.failed(pool, tei, &error).await;
                    failure = error;
                }
            }
        }
        Err(failure)
    }

    /// Records a TEI's failure, and asks it again which model it serves after an unexpected
    /// answer. In the background: a search falls through to the next TEI meanwhile.
    async fn failed(&self, pool: &PgPool, tei: &Arc<Tei>, error: &EmbedError) {
        tracing::error!(url = %tei.url, %error, "TEI failed");
        tei_pool::record_error(pool, &tei.url, &error.to_string()).await;
        if matches!(error, EmbedError::Mismatch(_)) {
            let (http, configs, wake, tei) = (
                self.http.clone(),
                self.configs.clone(),
                self.wake.clone(),
                tei.clone(),
            );
            tokio::spawn(async move {
                let discovery =
                    tei_pool::discover(&http, &tei.url, &configs, MAX_EMBEDDED_CHARS).await;
                if set_discovery(&configs, &tei, discovery) {
                    wake.send_replace(());
                }
            });
        }
    }
}

/// Embeds chunks in the background until the process ends, one loop per TEI: a faster TEI simply
/// claims more batches. Woken by `chunks_pending` after ingests and by TEIs that changed. Every
/// `DISCOVERY_POLL`, the TEIs are asked which model they serve. Several replicas share the queue
/// through the claim's advisory locks. Dropping the future stops every loop.
pub async fn run_pump(pool: PgPool, embedding: Arc<Embedding>) {
    let mut tasks = JoinSet::new();
    // Listening before the first claim: a chunk ingested in between would wait for `IDLE_POLL`.
    let listener = listen(&pool).await;
    tasks.spawn(wake_on_chunks(pool.clone(), embedding.clone(), listener));
    tasks.spawn(poll_teis(pool.clone(), embedding.clone()));
    for tei in &embedding.teis {
        tasks.spawn(pump(pool.clone(), embedding.clone(), tei.clone()));
    }
    while let Some(ended) = tasks.join_next().await {
        if let Err(error) = ended {
            tracing::error!(%error, "an embed pump task ended");
        }
    }
}

async fn wake_on_chunks(
    pool: PgPool,
    embedding: Arc<Embedding>,
    mut listener: Result<PgListener, sqlx::Error>,
) {
    loop {
        match listener {
            Ok(mut listening) => {
                // Notifications sent while no one listened are lost, so the pumps look again.
                // `try_recv` returns `None` after it reconnected by itself, for the same reason.
                embedding.wake.send_replace(());
                while listening.try_recv().await.is_ok() {
                    embedding.wake.send_replace(());
                }
                tracing::warn!("embed pump lost its notification connection");
            }
            Err(error) => {
                tracing::error!(%error, "embed pump can't listen for chunks_pending");
                tokio::time::sleep(IDLE_POLL).await;
            }
        }
        listener = listen(&pool).await;
    }
}

async fn poll_teis(pool: PgPool, embedding: Arc<Embedding>) {
    loop {
        tokio::time::sleep(DISCOVERY_POLL).await;
        if let Err(error) = embedding.refresh(&pool).await {
            tracing::error!(%error, "can't read the TEIs' switches");
        }
    }
}

/// One TEI's share of the pump.
async fn pump(pool: PgPool, embedding: Arc<Embedding>, tei: Arc<Tei>) {
    let mut wake = embedding.wake.subscribe();
    loop {
        wake.borrow_and_update();
        if !embedding.pump_once(&pool, &tei).await {
            tokio::select! {
                _ = wake.changed() => {}
                () = tokio::time::sleep(IDLE_POLL) => {}
            }
        }
    }
}

impl Embedding {
    /// Embeds one batch on `tei` if it may embed, and returns whether it embedded something.
    async fn pump_once(&self, pool: &PgPool, tei: &Arc<Tei>) -> bool {
        let Some((model, served)) = tei.serving(Use::Embedding) else {
            return false;
        };
        let model = &self.models[model];
        match embed_batch(pool, self, tei, model, &served).await {
            Ok(0) => false,
            Ok(count) => {
                tracing::debug!(url = %tei.url, model = %model.name(), count, "embedded chunks");
                tei_pool::clear_error(pool, &tei.url).await;
                true
            }
            Err(error) => {
                self.failed(pool, tei, &error).await;
                false
            }
        }
    }
}

/// Takes what a TEI's `/info` said, logs a change, and returns whether there was one.
fn set_discovery(configs: &[ModelConfig], tei: &Tei, discovery: Discovery) -> bool {
    tei.update(|state| {
        if state.discovery != discovery {
            match &discovery {
                Discovery::Serving { model, .. } => {
                    tracing::info!(url = %tei.url, model = %configs[*model].name, "TEI serves a model")
                }
                other => {
                    let problem = other.problem().unwrap_or_default();
                    tracing::warn!(url = %tei.url, %problem, "TEI is of no use")
                }
            }
        }
        state.discovery = discovery;
    })
}

async fn listen(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen("chunks_pending").await?;
    Ok(listener)
}

/// Embeds on `tei` the newest chunks `model` has no embedding for yet. Returns how many it
/// claimed.
async fn embed_batch(
    pool: &PgPool,
    embedding: &Embedding,
    tei: &Tei,
    model: &Model,
    served: &str,
) -> Result<usize, EmbedError> {
    let mut tx = pool.begin().await?;
    // Advisory locks rather than row locks: other pumps skip what this one works on, but an
    // ingest that changes or deletes one of these chunks never waits for TEI. The lock is tried
    // on a bounded set of candidates only, so a transaction never collects thousands of them.
    // No outer `order by`: sorting would try the lock of every candidate before the limit,
    // leaving a second pump nothing to claim.
    let chunks = sqlx::query!(
        "select id, text, title from (
             select c.id, c.text, c.title from chunks c
             where not exists (
                 select 1 from chunk_embeddings e where e.chunk_id = c.id and e.model = $1
             )
             and not exists (
                 select 1 from embedding_failures f where f.chunk_id = c.id and f.model = $1
             )
             order by c.ts desc
             limit $2
         ) candidates
         where pg_try_advisory_xact_lock(hashtextextended(id::text || $1, 0))
         limit $3",
        model.config.name,
        CLAIM_CANDIDATES,
        tei.batch_size() as i64,
    )
    .fetch_all(&mut *tx)
    .await?;
    if chunks.is_empty() {
        return Ok(0);
    }
    // An admin may have switched the TEI off while the claim waited for a connection: chunks
    // ingested after the switch go to another TEI. The rollback releases the claim before the
    // other pumps look: dropping the transaction would only queue it.
    if tei.serving(Use::Embedding) != Some((embedding.index_of(model), served.to_owned())) {
        tx.rollback().await?;
        embedding.wake.send_replace(());
        return Ok(0);
    }

    let texts: Vec<String> = chunks
        .iter()
        .map(|chunk| {
            embedded_input(
                &model.config.document_prefix,
                chunk.title.as_deref(),
                &chunk.text,
            )
        })
        .collect();
    let hashes: Vec<Vec<u8>> = texts.iter().map(|text| text_hash(text)).collect();
    let cached: HashSet<Vec<u8>> = sqlx::query_scalar!(
        "select text_hash from embedding_cache where model = $1 and text_hash = any($2)",
        model.config.name,
        &hashes,
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let mut missing: Vec<(Vec<u8>, String)> = Vec::new();
    for (hash, text) in hashes.iter().zip(&texts) {
        if !cached.contains(hash) && !missing.iter().any(|(seen, _)| seen == hash) {
            missing.push((hash.clone(), text.clone()));
        }
    }
    let rejected = cache_embeddings(&mut tx, embedding, tei, model, served, missing).await?;
    if !rejected.is_empty() {
        let rejected_chunks: Vec<_> = chunks
            .iter()
            .zip(&hashes)
            .filter_map(|(chunk, hash)| rejected.get(hash).map(|error| (chunk.id, error.clone())))
            .collect();
        let (ids, errors): (Vec<_>, Vec<_>) = rejected_chunks.into_iter().unzip();
        tracing::error!(model = %model.config.name, chunks = ?ids, "TEI rejected chunks, they stay without an embedding");
        sqlx::query!(
            "insert into embedding_failures (chunk_id, model, error, at)
             select chunk_id, $1, error, now() from unnest($2::uuid[], $3::text[]) as f(chunk_id, error)
             on conflict do nothing",
            model.config.name,
            &ids,
            &errors,
        )
        .execute(&mut *tx)
        .await?;
    }

    let ids: Vec<_> = chunks.iter().map(|chunk| chunk.id).collect();
    // `for share` waits for an ingest that is changing one of these chunks right now and then
    // reads its new values, so the denormalized columns never go stale. Chunks deleted since
    // the claim drop out of the join.
    sqlx::query!(
        "insert into chunk_embeddings
             (chunk_id, model, document_id, user_id, source, content_type, ts, embedding)
         select c.id, $1, c.document_id, c.user_id, c.source, c.content_type, c.ts, cache.embedding
         from unnest($2::uuid[], $3::bytea[]) as batch(chunk_id, text_hash)
         join chunks c on c.id = batch.chunk_id
         join embedding_cache cache on cache.text_hash = batch.text_hash and cache.model = $1
         for share of c",
        model.config.name,
        &ids,
        &hashes,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(chunks.len())
}

/// Embeds `missing` texts on `tei` and caches their vectors. Returns the texts TEI rejected (too
/// long for the model, untokenizable) with TEI's answer: one bad text must not keep the others,
/// and every newer chunk, from ever being embedded. Adapts the TEI's batch size to how long it
/// took.
async fn cache_embeddings(
    tx: &mut PgConnection,
    embedding: &Embedding,
    tei: &Tei,
    model: &Model,
    served: &str,
    missing: Vec<(Vec<u8>, String)>,
) -> Result<HashMap<Vec<u8>, String>, EmbedError> {
    let mut rejected = HashMap::new();
    if missing.is_empty() {
        return Ok(rejected);
    }
    let (hashes, texts): (Vec<_>, Vec<_>) = missing.into_iter().unzip();
    let _busy = tei.busy();
    let embed = |texts: &[String]| {
        let texts = texts.to_vec();
        async move {
            embedding
                .embed_on(tei, model, served, &texts, BATCH_TIMEOUT)
                .await
        }
    };
    let started = Instant::now();
    let (hashes, vectors) = match embed(&texts).await {
        Ok(vectors) => {
            tei.batch_succeeded(started.elapsed());
            (hashes, vectors)
        }
        Err(EmbedError::Tei(error)) if error.is_timeout() => {
            tei.batch_timed_out();
            return Err(error.into());
        }
        Err(EmbedError::Tei(error)) if tei::is_rejection(&error) => {
            // Find the culprits one by one.
            let mut embedded_hashes = Vec::new();
            let mut vectors = Vec::new();
            for (hash, text) in hashes.into_iter().zip(texts) {
                match embed(&[text]).await {
                    Ok(mut vector) => {
                        embedded_hashes.push(hash);
                        vectors.push(vector.remove(0));
                    }
                    Err(EmbedError::Tei(error)) if tei::is_rejection(&error) => {
                        rejected.insert(hash, error.to_string());
                    }
                    Err(error) => return Err(error),
                }
            }
            (embedded_hashes, vectors)
        }
        Err(error) => return Err(error),
    };
    let literals: Vec<String> = vectors
        .iter()
        .map(|vector| vector_literal(vector))
        .collect();
    sqlx::query!(
        "insert into embedding_cache (text_hash, model, embedding)
         select text_hash, $1, embedding::halfvec
         from unnest($2::bytea[], $3::text[]) as new(text_hash, embedding)
         on conflict do nothing",
        model.config.name,
        &hashes,
        &literals,
    )
    .execute(&mut *tx)
    .await?;
    Ok(rejected)
}

/// What the model embeds for a chunk: the title gives every chunk of a document its context,
/// e.g. a Paperless title naming the insurance a bill is for. Capped, so a garbage title can't
/// push the input over the model's limit. Keyword search sees the whole title.
fn embedded_input(prefix: &str, title: Option<&str>, text: &str) -> String {
    match title {
        Some(title) => {
            let title: String = title.chars().take(MAX_EMBEDDED_TITLE_CHARS).collect();
            format!("{prefix}{title}{TITLE_SEPARATOR}{text}")
        }
        None => format!("{prefix}{text}"),
    }
}

/// The cache key of an embedded input or a prefixed query.
fn text_hash(prefixed: &str) -> Vec<u8> {
    Sha256::digest(prefixed).to_vec()
}

/// pgvector's text form, `[0.1,0.2,...]`.
fn vector_literal(vector: &[f32]) -> String {
    let values: Vec<String> = vector.iter().map(f32::to_string).collect();
    format!("[{}]", values.join(","))
}
