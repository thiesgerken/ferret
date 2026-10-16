//! Embedding models: their setup at startup, query embeddings, and the embed pump that fills
//! `chunk_embeddings` in the background, so ingest never waits for a model.

use std::{collections::HashSet, fmt, sync::Arc, time::Duration};

use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, PgPool, postgres::PgListener};

use crate::{
    StartupError,
    chunking::MAX_CHUNK_CHARS,
    config::{EmbeddingConfig, ModelConfig},
    tei,
};

/// Chunks per pump transaction, within TEI's default client batch size (32).
const BATCH_SIZE: i64 = 32;
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

    fn index_name(&self) -> String {
        format!(
            "chunk_embeddings_hnsw_{}",
            self.config.name.replace(['.', '-'], "_")
        )
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
    let http = reqwest::Client::new();
    let mut configs = config.models;
    configs.sort_by_key(|model| model.name != config.default);

    let mut models = Vec::new();
    for config in configs {
        let name = &config.name;
        let max_input_length = tei::max_input_length(&http, &config.url)
            .await
            .map_err(|error| format!("embedding model {name}: {error}"))?;
        // Tokens against characters, one to one: Latin text practically never takes more than
        // one token per character.
        if max_input_length < MAX_CHUNK_CHARS {
            return Err(format!(
                "embedding model {name} takes at most {max_input_length} tokens, chunks have up \
                 to {MAX_CHUNK_CHARS} characters"
            )
            .into());
        }
        let probe = tei::embed(
            &http,
            &config.url,
            &[format!("{}probe", config.document_prefix)],
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
    Ok(Embedding { http, models })
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

impl Embedding {
    pub fn default_model(&self) -> &Model {
        &self.models[0]
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
            let vectors = tei::embed(&self.http, &model.config.url, &[text]).await?;
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
/// the queue through `SKIP LOCKED`.
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

/// Embeds the newest chunks `model` has no embedding for yet. Returns how many.
async fn embed_batch(
    pool: &PgPool,
    http: &reqwest::Client,
    model: &Model,
) -> Result<usize, EmbedError> {
    let mut tx = pool.begin().await?;
    // ponytail: the claimed chunks stay locked while TEI works, so a PUT that changes one of
    // them waits for the batch (seconds on CPU). A claim table would avoid that if it hurts.
    let chunks = sqlx::query!(
        "select c.id, c.text from chunks c
         where not exists (
             select 1 from chunk_embeddings e where e.chunk_id = c.id and e.model = $1
         )
         order by c.ts desc
         limit $2
         for update of c skip locked",
        model.config.name,
        BATCH_SIZE,
    )
    .fetch_all(&mut *tx)
    .await?;
    if chunks.is_empty() {
        return Ok(0);
    }

    let texts: Vec<String> = chunks
        .iter()
        .map(|chunk| format!("{}{}", model.config.document_prefix, chunk.text))
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
    if !missing.is_empty() {
        let (missing_hashes, missing_texts): (Vec<_>, Vec<_>) = missing.into_iter().unzip();
        let vectors = tei::embed(http, model.pump_url(), &missing_texts).await?;
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
            &missing_hashes,
            &literals,
        )
        .execute(&mut *tx)
        .await?;
    }

    let ids: Vec<_> = chunks.iter().map(|chunk| chunk.id).collect();
    sqlx::query!(
        "insert into chunk_embeddings
             (chunk_id, model, document_id, user_id, source, content_type, ts, embedding)
         select c.id, $1, c.document_id, c.user_id, c.source, c.content_type, c.ts, cache.embedding
         from unnest($2::uuid[], $3::bytea[]) as batch(chunk_id, text_hash)
         join chunks c on c.id = batch.chunk_id
         join embedding_cache cache on cache.text_hash = batch.text_hash and cache.model = $1",
        model.config.name,
        &ids,
        &hashes,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(chunks.len())
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

/// The cache key of a prefixed text.
fn text_hash(prefixed: &str) -> Vec<u8> {
    Sha256::digest(prefixed).to_vec()
}

/// pgvector's text form, `[0.1,0.2,...]`.
fn vector_literal(vector: &[f32]) -> String {
    let values: Vec<String> = vector.iter().map(f32::to_string).collect();
    format!("[{}]", values.join(","))
}
