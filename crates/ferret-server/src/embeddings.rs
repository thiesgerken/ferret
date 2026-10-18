//! Embedding models and their OpenAI-compatible endpoints: query embeddings, and the embed pump
//! that fills `chunk_embeddings` in the background, so ingest never waits for a model.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, PgConnection, PgPool, postgres::PgListener};
use tokio::{sync::watch, task::JoinSet};
use uuid::Uuid;

use crate::{
    StartupError,
    config::{EmbeddingConfig, ModelConfig},
    embedding_api::{self, Endpoint, EndpointError, Health},
    model_registry,
};

const MAX_EMBEDDED_TITLE_CHARS: usize = 200;
const TITLE_SEPARATOR: &str = "\n\n";
/// A search waits this long for its query embedding, then answers by keyword only.
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// How the claim query looks ahead for chunks no other pump has locked.
const CLAIM_CANDIDATES: i64 = 256;
/// The pump also looks for work this often without a notification, e.g. after an outage.
const IDLE_POLL: Duration = Duration::from_secs(30);
/// Chunks per pump batch at most, TEI's default client batch size, which other endpoints take too.
const MAX_BATCH_SIZE: usize = 32;
/// A pump batch may take this long, see `BatchSize` for an endpoint that is slower.
const BATCH_TIMEOUT: Duration = Duration::from_secs(60);
/// What a pump batch should take on an endpoint that also answers queries: a query waits behind
/// it.
const SHARED_BATCH_TARGET: Duration = Duration::from_secs(2);
/// A failing endpoint is tried again after this, doubling up to `MAX_BACKOFF`.
const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// What an admin's test embeds.
const TEST_TEXT: &str = "Does this endpoint embed?";

pub struct Model {
    pub config: ModelConfig,
    /// Computes the embeddings of chunks.
    pub embedding_api: Arc<Endpoint>,
    /// Computes the embeddings of queries. The same as `embedding_api` when the config names
    /// one endpoint for both.
    pub query_api: Arc<Endpoint>,
    batch_size: BatchSize,
}

impl Model {
    fn new(config: ModelConfig) -> Result<Self, StartupError> {
        let embedding_api = Arc::new(Endpoint::new(config.embedding_api())?);
        let query_api = if config.query_api() == config.embedding_api() {
            embedding_api.clone()
        } else {
            Arc::new(Endpoint::new(config.query_api())?)
        };
        Ok(Self {
            config,
            embedding_api,
            query_api,
            batch_size: BatchSize::new(),
        })
    }

    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub fn dimension(&self) -> usize {
        self.config.dimension
    }

    fn index_name(&self) -> String {
        model_registry::index_name(self.id())
    }

    /// Embedding requests at once per server replica.
    fn concurrency(&self) -> usize {
        self.config.embedding_api().concurrency.unwrap_or(1)
    }

    /// What a pump batch should take: short on an endpoint that also answers queries, so a
    /// query never waits long behind one.
    fn batch_target(&self) -> Duration {
        if Arc::ptr_eq(&self.embedding_api, &self.query_api) {
            SHARED_BATCH_TARGET
        } else {
            BATCH_TIMEOUT
        }
    }
}

pub struct Embedding {
    http: reqwest::Client,
    /// The default model first.
    models: Vec<Model>,
    /// Wakes the pump loops when chunks are ingested.
    wake: Arc<watch::Sender<()>>,
}

#[derive(Debug)]
pub enum EmbedError {
    Endpoint(EndpointError),
    Database(sqlx::Error),
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Endpoint(error) => error.fmt(f),
            Self::Database(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for EmbedError {}

impl From<EndpointError> for EmbedError {
    fn from(error: EndpointError) -> Self {
        Self::Endpoint(error)
    }
}

impl From<sqlx::Error> for EmbedError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Sets up the configured models: their indexes and endpoints. Endpoints aren't contacted:
/// one that is down doesn't keep the server from starting, searches then answer by keyword.
pub async fn prepare(pool: &PgPool, config: EmbeddingConfig) -> Result<Embedding, StartupError> {
    config.validate()?;
    let mut configs = config.models;
    configs.sort_by_key(|model| model.id != config.default);
    let models = configs
        .into_iter()
        .map(Model::new)
        .collect::<Result<Vec<_>, _>>()?;
    model_registry::register(pool, &models).await?;
    for model in &models {
        create_index(pool, model).await?;
    }
    reset_removed_defaults(pool, &models).await?;
    Ok(Embedding {
        http: embedding_api::client(),
        models,
        wake: Arc::new(watch::Sender::new(())),
    })
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

/// `user_id`'s chunks still waiting for `model`, and those its endpoint rejected.
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
    let names: Vec<String> = models.iter().map(|model| model.id().to_owned()).collect();
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
    // Validated ids (config.rs) need no quoting, and DDL takes no bind parameters.
    sqlx::query(AssertSqlSafe(format!(
        "create index if not exists {} on chunk_embeddings
             using hnsw ((embedding::halfvec({})) halfvec_cosine_ops)
             where model = '{}'",
        model.index_name(),
        model.dimension(),
        model.config.id,
    )))
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// How one configured model is doing.
pub struct ModelStatus {
    pub id: String,
    pub is_default: bool,
    pub dimension: usize,
    pub embedding_api: EndpointStatus,
    pub query_api: EndpointStatus,
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

/// An endpoint and how its requests from this server process went.
pub struct EndpointStatus {
    pub url: String,
    pub model: String,
    pub health: Health,
}

impl EndpointStatus {
    fn of(endpoint: &Endpoint) -> Self {
        Self {
            url: endpoint.url.clone(),
            model: endpoint.model.clone(),
            health: endpoint.health(),
        }
    }
}

pub struct SourceBacklog {
    pub source: String,
    /// Waiting for the pump.
    pub pending: i64,
    /// The endpoint rejected them, so the pump leaves them out.
    pub failed: i64,
}

/// What a model uses an endpoint for.
#[derive(Clone, Copy)]
pub enum EndpointRole {
    ChunksAndQueries,
    Chunks,
    Queries,
}

/// How one of a model's endpoints answered an admin's test.
pub struct EndpointTest {
    pub role: EndpointRole,
    pub url: String,
    pub model: String,
    pub took: Duration,
    /// The returned vector's dimension.
    pub result: Result<usize, EndpointError>,
}

impl Embedding {
    pub fn default_model(&self) -> &Model {
        &self.models[0]
    }

    /// Every configured model, the default first.
    pub fn models(&self) -> &[Model] {
        &self.models
    }

    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id() == id)
    }

    /// Every configured model's backlog, the default first.
    // ponytail: the backlog counts every chunk on each call, fine for a personal index. Keep
    // running counts if the Admin page gets slow.
    pub async fn status(&self, pool: &PgPool) -> Result<Vec<ModelStatus>, sqlx::Error> {
        let mut statuses = Vec::new();
        for (index, model) in self.models.iter().enumerate() {
            let id = model.id();
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
                id,
            )
            .fetch_all(pool)
            .await?;
            let rate_per_minute = rate_per_minute(pool, id).await?;
            let pending = backlog.iter().map(|source| source.pending).sum();
            let embedded = sqlx::query_scalar!(
                r#"select count(*) as "count!" from chunk_embeddings where model = $1"#,
                id
            )
            .fetch_one(pool)
            .await?;
            let chunks = sqlx::query_scalar!(r#"select count(*) as "count!" from chunks"#)
                .fetch_one(pool)
                .await?;
            statuses.push(ModelStatus {
                id: id.to_owned(),
                embedding_api: EndpointStatus::of(&model.embedding_api),
                query_api: EndpointStatus::of(&model.query_api),
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

    /// Embeds `query` with `model`'s query prefix unless it's cached, and returns its cache key.
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
            model.config.id,
        )
        .fetch_one(pool)
        .await?;
        if cached {
            return Ok(hash);
        }
        let vectors = model
            .query_api
            .embed(&self.http, &[text], model.dimension(), QUERY_TIMEOUT)
            .await?;
        sqlx::query!(
            "insert into embedding_cache (text_hash, model, embedding)
             values ($1, $2, $3::text::halfvec)
             on conflict do nothing",
            hash,
            model.config.id,
            vector_literal(&vectors[0]),
        )
        .execute(pool)
        .await?;
        Ok(hash)
    }

    /// Embeds a short text through each of `model`'s endpoints, with the prefix and timeout of
    /// the role it has and checked like every answer. Stores nothing and leaves the endpoints'
    /// health alone.
    pub async fn test(&self, model: &Model) -> Vec<EndpointTest> {
        if Arc::ptr_eq(&model.embedding_api, &model.query_api) {
            return vec![
                self.test_endpoint(model, EndpointRole::ChunksAndQueries)
                    .await,
            ];
        }
        let (chunks, queries) = tokio::join!(
            self.test_endpoint(model, EndpointRole::Chunks),
            self.test_endpoint(model, EndpointRole::Queries)
        );
        vec![chunks, queries]
    }

    async fn test_endpoint(&self, model: &Model, role: EndpointRole) -> EndpointTest {
        let (endpoint, prefix, timeout) = match role {
            EndpointRole::ChunksAndQueries | EndpointRole::Chunks => (
                &model.embedding_api,
                &model.config.document_prefix,
                BATCH_TIMEOUT,
            ),
            EndpointRole::Queries => (&model.query_api, &model.config.query_prefix, QUERY_TIMEOUT),
        };
        let started = Instant::now();
        let result = endpoint
            .embed_unrecorded(
                &self.http,
                &[format!("{prefix}{TEST_TEXT}")],
                model.dimension(),
                timeout,
            )
            .await;
        EndpointTest {
            role,
            url: endpoint.url.clone(),
            model: endpoint.model.clone(),
            took: started.elapsed(),
            result: result.map(|vectors| vectors[0].len()),
        }
    }
}

/// Embeds chunks in the background until the process ends, with `concurrency` loops per model.
/// Woken by `chunks_pending` after ingests. Several replicas and loops share the queue through
/// the claim's advisory locks. Dropping the future stops every loop.
pub async fn run_pump(pool: PgPool, embedding: Arc<Embedding>) {
    let mut tasks = JoinSet::new();
    // Listening before the first claim: a chunk ingested in between would wait for `IDLE_POLL`.
    let listener = listen(&pool).await;
    tasks.spawn(wake_on_chunks(pool.clone(), embedding.clone(), listener));
    for (index, model) in embedding.models.iter().enumerate() {
        for _ in 0..model.concurrency() {
            tasks.spawn(pump(pool.clone(), embedding.clone(), index));
        }
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

/// One of a model's pump loops. A failing endpoint is tried again after a growing pause, also
/// when chunks arrive meanwhile: a burst of ingests must not hammer it.
async fn pump(pool: PgPool, embedding: Arc<Embedding>, model: usize) {
    let model = &embedding.models[model];
    let mut wake = embedding.wake.subscribe();
    let mut backoff = MIN_BACKOFF;
    loop {
        wake.borrow_and_update();
        match embed_batch(&pool, &embedding.http, model).await {
            Ok(0) => {
                backoff = MIN_BACKOFF;
                tokio::select! {
                    _ = wake.changed() => {}
                    () = tokio::time::sleep(IDLE_POLL) => {}
                }
            }
            Ok(count) => {
                backoff = MIN_BACKOFF;
                tracing::debug!(model = %model.id(), count, "embedded chunks");
            }
            // The endpoint keeps the pause, see `cache_embeddings`.
            Err(EmbedError::Endpoint(EndpointError::RateLimited(_))) => {}
            Err(error) => {
                // The endpoint logs its own failures, once per outage.
                if let EmbedError::Database(error) = &error {
                    tracing::error!(model = %model.id(), %error, "embed pump batch failed");
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

async fn listen(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen("chunks_pending").await?;
    Ok(listener)
}

/// Embeds the newest chunks `model` has no embedding for yet. Returns how many it claimed.
async fn embed_batch(
    pool: &PgPool,
    http: &reqwest::Client,
    model: &Model,
) -> Result<usize, EmbedError> {
    let mut tx = pool.begin().await?;
    // Advisory locks rather than row locks: other pumps skip what this one works on, but an
    // ingest that changes or deletes one of these chunks never waits for the endpoint. The lock is tried
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
        model.config.id,
        CLAIM_CANDIDATES,
        model.batch_size.get() as i64,
    )
    .fetch_all(&mut *tx)
    .await?;
    if chunks.is_empty() {
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
        model.config.id,
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
    let rejected = cache_embeddings(&mut tx, http, model, missing).await?;
    if !rejected.is_empty() {
        let rejected_chunks: Vec<_> = chunks
            .iter()
            .zip(&hashes)
            .filter_map(|(chunk, hash)| rejected.get(hash).map(|error| (chunk.id, error.clone())))
            .collect();
        let (ids, errors): (Vec<_>, Vec<_>) = rejected_chunks.into_iter().unzip();
        tracing::error!(model = %model.config.id, chunks = ?ids, "the endpoint rejected chunks, they stay without an embedding");
        sqlx::query!(
            "insert into embedding_failures (chunk_id, model, error, at)
             select chunk_id, $1, error, now() from unnest($2::uuid[], $3::text[]) as f(chunk_id, error)
             on conflict do nothing",
            model.config.id,
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
        model.config.id,
        &ids,
        &hashes,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(chunks.len())
}

/// Embeds `missing` texts on the model's endpoint and caches their vectors. Returns the texts the
/// endpoint rejected (too long for the model, untokenizable) with its answer: one bad text must
/// not keep the others, and every newer chunk, from ever being embedded. Adapts the batch size to
/// how long it took.
async fn cache_embeddings(
    tx: &mut PgConnection,
    http: &reqwest::Client,
    model: &Model,
    missing: Vec<(Vec<u8>, String)>,
) -> Result<HashMap<Vec<u8>, String>, EmbedError> {
    let mut rejected = HashMap::new();
    if missing.is_empty() {
        return Ok(rejected);
    }
    let (hashes, texts): (Vec<_>, Vec<_>) = missing.into_iter().unzip();
    let embed = |texts: Vec<String>| async move {
        model
            .embedding_api
            .embed(http, &texts, model.dimension(), BATCH_TIMEOUT)
            .await
    };
    // Every loop of the model waits out a 429, not only the one that got it. Right before the
    // request: a loop that checked earlier may have claimed chunks the 429'd loop just released.
    if let Some(until) = model.embedding_api.paused_until() {
        tokio::time::sleep_until(until.into()).await;
    }
    let started = Instant::now();
    let (hashes, vectors) = match embed(texts.clone()).await {
        Ok(vectors) => {
            model
                .batch_size
                .succeeded(started.elapsed(), model.batch_target());
            (hashes, vectors)
        }
        Err(error) if error.is_timeout() => {
            model.batch_size.timed_out();
            return Err(error.into());
        }
        Err(EndpointError::Rejected(_)) => {
            // Find the culprits one by one.
            let batch = texts.len();
            let mut embedded_hashes = Vec::new();
            let mut vectors = Vec::new();
            for (hash, text) in hashes.into_iter().zip(texts) {
                match embed(vec![text]).await {
                    Ok(mut vector) => {
                        embedded_hashes.push(hash);
                        vectors.push(vector.remove(0));
                    }
                    Err(EndpointError::Rejected(message)) => {
                        rejected.insert(hash, message);
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            // Every input of a batch at once points at the endpoint (a proxy answers an unknown
            // model with a 400), not at the inputs: recording them would fail the whole backlog.
            if batch > 1 && embedded_hashes.is_empty() {
                let example = rejected.into_values().next().unwrap_or_default();
                let error = EndpointError::Failed(format!(
                    "rejected every input of a batch of {batch}, e.g. with {example}"
                ));
                model.embedding_api.failed(&error);
                return Err(error.into());
            }
            (embedded_hashes, vectors)
        }
        Err(error) => return Err(error.into()),
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
        model.config.id,
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

/// How many chunks a model's pump claims per batch. A TEI doesn't cancel a request the pump gave
/// up on, so on a CPU that needs longer than `BATCH_TIMEOUT` for a full batch, retrying the same
/// size queues behind the abandoned one and times out again, forever. Halving on a timeout lets
/// the abandoned work drain. Growing only after a batch that took less than half its target keeps
/// the size below the next timeout, and a fast endpoint at the maximum. A batch that took longer
/// than its target halves the size too, which keeps batches short on an endpoint that answers
/// queries. A 429 leaves it alone: the endpoint is busy, not slow.
struct BatchSize(AtomicUsize);

impl BatchSize {
    fn new() -> Self {
        Self(AtomicUsize::new(MAX_BATCH_SIZE))
    }

    fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    fn timed_out(&self) {
        self.0.update(Ordering::Relaxed, Ordering::Relaxed, |size| {
            (size / 2).max(1)
        });
    }

    fn succeeded(&self, took: Duration, target: Duration) {
        if took > target {
            self.timed_out();
        } else if took < target / 2 {
            self.0.update(Ordering::Relaxed, Ordering::Relaxed, |size| {
                (size + 1).min(MAX_BATCH_SIZE)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An endpoint that computes every request to the end, one after another, even the ones the pump
    /// gave up on. Returns how many chunks the pump embedded within `within`, and the longest
    /// successful batch.
    fn pump_against(
        size: &BatchSize,
        per_chunk: Duration,
        within: Duration,
        target: Duration,
    ) -> (usize, Duration) {
        let (mut now, mut busy_until, mut embedded) = (Duration::ZERO, Duration::ZERO, 0);
        let mut longest = Duration::ZERO;
        while now < within {
            let batch = size.get();
            busy_until = now.max(busy_until) + per_chunk * batch as u32;
            let took = busy_until - now;
            if took > BATCH_TIMEOUT {
                size.timed_out();
                now += BATCH_TIMEOUT;
            } else {
                size.succeeded(took, target);
                embedded += batch;
                longest = longest.max(took);
                now = busy_until;
            }
        }
        (embedded, longest)
    }

    #[test]
    fn an_endpoint_too_slow_for_a_full_batch_still_makes_progress_and_a_fast_one_gets_full_batches()
    {
        let size = BatchSize::new();
        // A CPU that needs 70 s for 32 long chunks. A fixed batch of 32 embeds nothing.
        let slow = Duration::from_secs(70) / 32;
        let ten_minutes = Duration::from_secs(600);
        let (embedded, _) = pump_against(&size, slow, ten_minutes, BATCH_TIMEOUT);
        // At least three quarters of what the endpoint could compute in that time.
        assert!(
            embedded * 4 >= (ten_minutes.as_secs_f64() / slow.as_secs_f64()) as usize * 3,
            "{embedded}"
        );

        pump_against(
            &size,
            Duration::from_millis(10),
            Duration::from_secs(60),
            BATCH_TIMEOUT,
        );
        assert_eq!(size.get(), MAX_BATCH_SIZE);
    }

    #[test]
    fn batches_on_an_endpoint_that_answers_queries_settle_below_their_target() {
        let size = BatchSize::new();
        let per_chunk = Duration::from_millis(400);

        // The first batches are too long, after that they stay at the target or below.
        pump_against(
            &size,
            per_chunk,
            Duration::from_secs(60),
            SHARED_BATCH_TARGET,
        );
        let (_, longest) = pump_against(
            &size,
            per_chunk,
            Duration::from_secs(120),
            SHARED_BATCH_TARGET,
        );

        assert!(longest <= SHARED_BATCH_TARGET, "{longest:?}");
        assert!(size.get() >= 2, "{}", size.get());
    }
}
