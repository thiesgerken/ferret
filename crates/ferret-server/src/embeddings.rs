//! Embedding models: their setup at startup, query embeddings, and the embed pump that fills
//! `chunk_embeddings` in the background, so ingest never waits for a model.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
    time::Duration,
};

use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, PgConnection, PgPool, postgres::PgListener};
use uuid::Uuid;

use crate::{
    StartupError,
    chunking::MAX_CHUNK_CHARS,
    config::{EmbeddingConfig, ModelConfig},
    tei,
};

/// Chunks per pump transaction, within TEI's default client batch size (32).
const BATCH_SIZE: i64 = 32;
const MAX_EMBEDDED_TITLE_CHARS: usize = 200;
const TITLE_SEPARATOR: &str = "\n\n";
/// The longest embedded input, without the model's document prefix. Every model must take it.
const MAX_EMBEDDED_CHARS: usize =
    MAX_EMBEDDED_TITLE_CHARS + TITLE_SEPARATOR.len() + MAX_CHUNK_CHARS;
/// A search waits this long for its query embedding, then answers by keyword only. A CPU TEI
/// that's busy with pump batches answers slower than an idle one.
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// A pump batch of up to 32 chunks may take this long on a CPU.
const BATCH_TIMEOUT: Duration = Duration::from_secs(60);
/// `/info` and the probe embedding at startup.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// How the claim query looks ahead for chunks no other pump has locked.
const CLAIM_CANDIDATES: i64 = 256;
/// A health check that takes longer counts as down: the Admin page waits for all of them.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
/// How long the startup waits for a TEI that doesn't accept connections yet.
const TEI_STARTUP_WAIT: Duration = Duration::from_secs(600);
/// The pump also looks for work this often without a notification, e.g. after an outage.
const IDLE_POLL: Duration = Duration::from_secs(30);

pub struct Model {
    pub config: ModelConfig,
    /// Probed at startup: TEI's `/info` doesn't report it.
    pub dimension: usize,
}

impl Model {
    /// The pump's endpoint: `bulk_url` if set, never falling back to `url`, which would turn a
    /// GPU backfill into a CPU one without anyone noticing.
    fn pump_url(&self) -> &str {
        self.config.bulk_url.as_deref().unwrap_or(&self.config.url)
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    fn index_name(&self) -> String {
        format!("chunk_embeddings_hnsw_{}", self.config.slug())
    }
}

pub struct Embedding {
    http: reqwest::Client,
    /// The default model first.
    models: Vec<Model>,
}

#[derive(Debug)]
pub enum EmbedError {
    Tei(reqwest::Error),
    Database(sqlx::Error),
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tei(error) => write!(f, "TEI: {error}"),
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

/// Reads every configured model's limits from its TEI and creates its partial HNSW index. Fails
/// when a TEI is unreachable or a model can't take the chunker's longest chunk.
pub async fn prepare(pool: &PgPool, config: EmbeddingConfig) -> Result<Embedding, StartupError> {
    config.validate()?;
    let http = tei::client();
    let mut configs = config.models;
    configs.sort_by_key(|model| model.name != config.default);

    let mut models = Vec::new();
    for config in configs {
        let name = &config.name;
        let max_input_length = wait_for_tei(&http, &config).await?;
        // Tokens against characters, one to one: Latin text practically never takes more than
        // one token per character.
        if max_input_length < MAX_EMBEDDED_CHARS {
            return Err(format!(
                "embedding model {name} takes at most {max_input_length} tokens, embedded inputs \
                 (title and chunk) have up to {MAX_EMBEDDED_CHARS} characters"
            )
            .into());
        }
        let probe = tei::embed(
            &http,
            &config.url,
            &[format!("{}probe", config.document_prefix)],
            STARTUP_TIMEOUT,
        )
        .await
        .map_err(|error| format!("embedding model {name}: {error}"))?;
        let model = Model {
            dimension: probe[0].len(),
            config,
        };
        create_index(pool, &model).await?;
        tracing::info!(model = %model.config.name, dimension = model.dimension, max_input_length, "embedding model ready");
        models.push(model);
    }
    reset_removed_defaults(pool, &models).await?;
    Ok(Embedding { http, models })
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

/// TEI's `max_input_length`, waiting while it isn't up yet: on a fresh deployment it may still
/// be downloading its model. Any other failure stops the startup.
async fn wait_for_tei(http: &reqwest::Client, config: &ModelConfig) -> Result<usize, StartupError> {
    let deadline = tokio::time::Instant::now() + TEI_STARTUP_WAIT;
    loop {
        match tei::max_input_length(http, &config.url, STARTUP_TIMEOUT).await {
            Ok(length) => return Ok(length),
            Err(error) if error.is_connect() && tokio::time::Instant::now() < deadline => {
                tracing::warn!(model = %config.name, url = %config.url, "waiting for TEI");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(error) => return Err(format!("embedding model {}: {error}", config.name).into()),
        }
    }
}

async fn create_index(pool: &PgPool, model: &Model) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Replicas starting together would race on the index name.
    sqlx::query!("select pg_advisory_xact_lock(hashtext('chunk_embeddings_hnsw'))")
        .execute(&mut *tx)
        .await?;
    // Validated names (config.rs) need no quoting, and DDL takes no bind parameters.
    sqlx::query(AssertSqlSafe(format!(
        "create index if not exists {} on chunk_embeddings
             using hnsw ((embedding::halfvec({})) halfvec_cosine_ops)
             where model = '{}'",
        model.index_name(),
        model.dimension,
        model.config.name,
    )))
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// How one configured model is doing.
pub struct ModelStatus {
    pub name: String,
    pub is_default: bool,
    /// Whether the TEI for searches answers.
    pub online: bool,
    /// Whether the pump's TEI answers, `None` when the pump uses the same one.
    pub bulk_online: Option<bool>,
    /// The pump's last failure, until its next success.
    pub last_error: Option<(String, chrono::DateTime<chrono::Utc>)>,
    /// Chunks still to embed per source, across users. Sources without any are left out.
    pub backlog: Vec<SourceBacklog>,
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

    /// Every configured model's health and backlog, the default first.
    // ponytail: the backlog counts every chunk on each call, fine for a personal index. Keep
    // running counts if the Admin page gets slow.
    pub async fn status(&self, pool: &PgPool) -> Result<Vec<ModelStatus>, sqlx::Error> {
        // Every health check at once, so TEIs that are down cost one timeout, not one each.
        let check = |url: &str| {
            let (http, url) = (self.http.clone(), url.to_owned());
            tokio::spawn(async move { tei::healthy(&http, &url, HEALTH_TIMEOUT).await })
        };
        let checks: Vec<_> = self
            .models
            .iter()
            .map(|model| {
                (
                    check(&model.config.url),
                    model.config.bulk_url.as_deref().map(check),
                )
            })
            .collect();
        let mut statuses = Vec::new();
        for ((index, model), (online, bulk_online)) in self.models.iter().enumerate().zip(checks) {
            let name = model.name();
            let online = online.await.expect("a health check doesn't panic");
            let bulk_online = match bulk_online {
                Some(check) => Some(check.await.expect("a health check doesn't panic")),
                None => None,
            };
            let last_error = sqlx::query!(
                "select error, at from embedding_errors where model = $1",
                name
            )
            .fetch_optional(pool)
            .await?
            .map(|row| (row.error, row.at));
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
            statuses.push(ModelStatus {
                name: name.to_owned(),
                is_default: index == 0,
                online,
                bulk_online,
                last_error,
                backlog,
            });
        }
        Ok(statuses)
    }

    /// Every configured model, the default first.
    pub fn models(&self) -> &[Model] {
        &self.models
    }

    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.name() == name)
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
            model.config.name,
        )
        .fetch_one(pool)
        .await?;
        if !cached {
            let vectors = tei::embed(&self.http, &model.config.url, &[text], QUERY_TIMEOUT).await?;
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
        }
        Ok(hash)
    }
}

/// Embeds chunks in the background until the process ends: the default model first, newest
/// chunks first. Woken by `chunks_pending` after ingests. Several pumps (server replicas) share
/// the queue through the claim's advisory locks.
pub async fn run_pump(pool: PgPool, embedding: Arc<Embedding>) {
    let mut listener = loop {
        match listen(&pool).await {
            Ok(listener) => break listener,
            Err(error) => {
                tracing::error!(%error, "embed pump can't listen for chunks_pending");
                tokio::time::sleep(IDLE_POLL).await;
            }
        }
    };
    loop {
        let mut embedded = false;
        for model in &embedding.models {
            match embed_batch(&pool, &embedding.http, model).await {
                Ok(0) => {}
                Ok(count) => {
                    tracing::debug!(model = %model.config.name, count, "embedded chunks");
                    clear_error(&pool, model).await;
                    // Back to the default model.
                    embedded = true;
                    break;
                }
                Err(error) => record_error(&pool, model, &error).await,
            }
        }
        if !embedded {
            tokio::select! {
                notification = listener.recv() => if let Err(error) = notification {
                    tracing::warn!(%error, "embed pump lost its notification connection");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                },
                () = tokio::time::sleep(IDLE_POLL) => {}
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
        BATCH_SIZE,
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
    let rejected = cache_embeddings(&mut tx, http, model, missing).await?;
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

/// Embeds `missing` texts and caches their vectors. Returns the texts TEI rejected (a 4xx, e.g.
/// too long for the model) with TEI's answer: one bad text must not keep the others, and every
/// newer chunk, from ever being embedded.
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
    let (hashes, vectors) = match tei::embed(http, model.pump_url(), &texts, BATCH_TIMEOUT).await {
        Ok(vectors) => (hashes, vectors),
        Err(error) if is_rejection(&error) => {
            // Find the culprits one by one.
            let mut embedded_hashes = Vec::new();
            let mut vectors = Vec::new();
            for (hash, text) in hashes.into_iter().zip(texts) {
                match tei::embed(http, model.pump_url(), &[text], BATCH_TIMEOUT).await {
                    Ok(mut vector) => {
                        embedded_hashes.push(hash);
                        vectors.push(vector.remove(0));
                    }
                    Err(error) if is_rejection(&error) => {
                        rejected.insert(hash, error.to_string());
                    }
                    Err(error) => return Err(error.into()),
                }
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
        model.config.name,
        &hashes,
        &literals,
    )
    .execute(&mut *tx)
    .await?;
    Ok(rejected)
}

/// TEI refused the input itself, as opposed to failing or being unreachable.
fn is_rejection(error: &reqwest::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.is_client_error())
}

/// Shown in Admin until the model's next successful batch. The pump keeps retrying.
async fn record_error(pool: &PgPool, model: &Model, error: &EmbedError) {
    tracing::error!(model = %model.config.name, url = model.pump_url(), %error, "embed pump failed");
    let recorded = sqlx::query!(
        "insert into embedding_errors (model, error, at) values ($1, $2, now())
         on conflict (model) do update set error = excluded.error, at = excluded.at",
        model.config.name,
        error.to_string(),
    )
    .execute(pool)
    .await;
    if let Err(error) = recorded {
        tracing::error!(%error, "embed pump can't record its error");
    }
}

async fn clear_error(pool: &PgPool, model: &Model) {
    let cleared = sqlx::query!(
        "delete from embedding_errors where model = $1",
        model.config.name
    )
    .execute(pool)
    .await;
    if let Err(error) = cleared {
        tracing::error!(%error, "embed pump can't clear its error");
    }
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
