//! The registry of embedding models (`embedding_models`): every model that ever embedded,
//! configured or not, with its dimension. Startup checks the configured models against it, and
//! Admin lists the stored embeddings and deletes those of models that left the config.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

use crate::{
    StartupError, config,
    embeddings::{Embedding, Model},
};

/// The partial HNSW index of the model `id`.
pub fn index_name(id: &str) -> String {
    format!("chunk_embeddings_hnsw_{}", config::slug(id))
}

/// Serializes creating and dropping the models' indexes: replicas starting together would race on
/// an index name, and a delete on an index being created.
pub async fn lock_indexes(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query!("select pg_advisory_xact_lock(hashtext('chunk_embeddings_hnsw'))")
        .execute(tx)
        .await?;
    Ok(())
}

/// Records the configured models the registry doesn't know yet. Refuses a model whose stored
/// embeddings have another dimension, which its index and its vectors couldn't share, and one
/// whose index name the stored embeddings of another model already have. A refused start records
/// nothing.
pub async fn register(pool: &PgPool, models: &[Model]) -> Result<(), StartupError> {
    let ids: Vec<String> = models.iter().map(|model| model.name().to_owned()).collect();
    let dimensions: Vec<i32> = models.iter().map(dimension).collect();
    // Inserting before checking also catches a replica that registered another dimension at the
    // same time.
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "insert into embedding_models (id, dimension)
         select * from unnest($1::text[], $2::int[])
         on conflict do nothing",
        &ids,
        &dimensions,
    )
    .execute(&mut *tx)
    .await?;
    let registered = sqlx::query!("select id, dimension from embedding_models")
        .fetch_all(&mut *tx)
        .await?;
    for model in models {
        for stored in &registered {
            if stored.id == model.name() && stored.dimension != dimension(model) {
                return Err(format!(
                    "embedding model {} has stored embeddings of {} dimensions, the config says {}. \
                     To change it, remove the model from the config, delete its embeddings in \
                     Admin and add it again, or give it another name",
                    model.name(),
                    stored.dimension,
                    model.dimension()
                )
                .into());
            }
            if stored.id != model.name() && index_name(&stored.id) == index_name(model.name()) {
                return Err(format!(
                    "embedding model {} would share its index name with the stored embeddings of \
                     {}: delete those in Admin first, or give it another name",
                    model.name(),
                    stored.id
                )
                .into());
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

fn dimension(model: &Model) -> i32 {
    i32::try_from(model.dimension()).expect("a dimension fits an i32")
}

/// A model of the registry, with what its embeddings take up.
pub struct StoredModel {
    pub id: String,
    pub dimension: i32,
    pub first_seen: DateTime<Utc>,
    pub configured: bool,
    /// Its chunk embeddings, across users.
    pub embedded: i64,
    /// Of its HNSW index, `None` while it has none.
    pub index_bytes: Option<i64>,
    /// Its vectors in `chunk_embeddings`, uncompressed. The table is shared by every model, so its
    /// size can't be split exactly. The other columns add roughly 150 bytes per row.
    pub row_bytes_estimate: i64,
}

/// Every registered model, the configured ones first.
// ponytail: counts every embedding on each call, fine for a personal index. Keep running counts
// if the Admin page gets slow.
pub async fn list(pool: &PgPool, embedding: &Embedding) -> Result<Vec<StoredModel>, sqlx::Error> {
    let embedded: HashMap<String, i64> =
        sqlx::query!(r#"select model, count(*) as "count!" from chunk_embeddings group by model"#)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| (row.model, row.count))
            .collect();
    let registered =
        sqlx::query!("select id, dimension, first_seen from embedding_models order by id")
            .fetch_all(pool)
            .await?;
    let mut models = Vec::new();
    for stored in registered {
        let index_bytes = sqlx::query_scalar!(
            "select pg_relation_size(to_regclass($1))",
            index_name(&stored.id)
        )
        .fetch_one(pool)
        .await?;
        let embedded = embedded.get(&stored.id).copied().unwrap_or(0);
        models.push(StoredModel {
            configured: embedding.model(&stored.id).is_some(),
            embedded,
            index_bytes,
            // pgvector's halfvec: an 8-byte header, then 2 bytes per dimension.
            row_bytes_estimate: embedded * (8 + 2 * i64::from(stored.dimension)),
            id: stored.id,
            dimension: stored.dimension,
            first_seen: stored.first_seen,
        });
    }
    models.sort_by_key(|model| !model.configured);
    Ok(models)
}

#[derive(Debug)]
pub enum DeleteError {
    NotStored,
    /// It must leave the config first, so nothing embeds for it and no user's default is it.
    Configured,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for DeleteError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Deletes the registry entry of the model `id`, its embeddings, cache entries and failure records
/// with it (foreign keys), and its index. Documents, chunks and other models stay.
pub async fn delete(pool: &PgPool, embedding: &Embedding, id: &str) -> Result<(), DeleteError> {
    if embedding.model(id).is_some() {
        return Err(DeleteError::Configured);
    }
    let mut tx = pool.begin().await?;
    lock_indexes(&mut tx).await?;
    let deleted = sqlx::query_scalar!(
        "delete from embedding_models where id = $1 returning id",
        id
    )
    .fetch_optional(&mut *tx)
    .await?;
    if deleted.is_none() {
        return Err(DeleteError::NotStored);
    }
    // Registered ids come from validated configs, so the index name needs no quoting.
    assert!(config::is_valid_name(id), "registered model id {id:?}");
    // Last: dropping the index locks chunk_embeddings against every search and pump batch until
    // the commit.
    sqlx::query(AssertSqlSafe(format!(
        "drop index if exists {}",
        index_name(id)
    )))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
