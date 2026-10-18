//! Current content attributed through documents, never source facets or ingestion history.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ferret_api::Writer;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{model_registry, writers::Writers};

#[derive(Default)]
pub struct Storage {
    pub documents: f64,
    pub chunks: f64,
    pub embeddings: f64,
    pub indexes: f64,
}

impl Storage {
    pub fn total(&self) -> f64 {
        self.documents + self.chunks + self.embeddings + self.indexes
    }
}

#[derive(Default)]
pub struct ModelStorage {
    pub model: String,
    pub embedded: i64,
    pub row_bytes: f64,
    pub index_bytes: f64,
}

#[derive(Default)]
pub struct Row {
    pub writer: Option<Writer>,
    pub sources: BTreeSet<String>,
    pub documents: i64,
    pub chunks: i64,
    pub storage: Storage,
    pub models: BTreeMap<String, ModelStorage>,
}

pub struct Statistics {
    pub rows: Vec<Row>,
    pub documents: i64,
    pub chunks: i64,
    pub storage: Storage,
}

fn share(value: f64, total: i64) -> f64 {
    if total == 0 {
        0.0
    } else {
        value / total as f64
    }
}

/// Each poll reads narrow persisted sizes and counts, not document text, chunk text or vectors.
/// One repeatable-read snapshot keeps attribution, counts and allocation denominators consistent.
pub async fn load(pool: &PgPool, user_id: Uuid) -> Result<Statistics, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!("set transaction isolation level repeatable read, read only")
        .execute(&mut *tx)
        .await?;
    let writers = Writers::load(&mut tx, user_id).await?;
    let global = sqlx::query!(
        r#"select
            (select count(*) from documents) as "documents!",
            (select count(*) from chunks) as "chunks!",
            (select coalesce(sum(indexed_text_bytes), 0)::bigint from chunks) as "text_bytes!",
            (select count(*) from chunk_embeddings) as "embeddings!""#
    )
    .fetch_one(&mut *tx)
    .await?;
    let indexes = sqlx::query!(
        "select t.relname as table_name, i.relname as index_name, a.amname,
                pg_relation_size(i.oid) as \"bytes!\"
         from pg_index x join pg_class t on t.oid = x.indrelid
         join pg_class i on i.oid = x.indexrelid
         join pg_am a on a.oid = i.relam
         where t.oid in ('documents'::regclass, 'chunks'::regclass, 'chunk_embeddings'::regclass)"
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut document_indexes = 0.0;
    let mut chunk_indexes = 0.0;
    let mut embedding_indexes = 0.0;
    let mut bm25 = 0.0;
    let mut hnsw = HashMap::new();
    for index in indexes {
        let bytes = index.bytes as f64;
        match (index.table_name.as_str(), index.amname.as_str()) {
            (_, "hnsw") => {
                hnsw.insert(index.index_name, bytes);
            }
            ("chunks", "bm25" | "paradedb") => bm25 += bytes,
            ("documents", _) => document_indexes += bytes,
            ("chunks", _) => chunk_indexes += bytes,
            ("chunk_embeddings", _) => embedding_indexes += bytes,
            _ => unreachable!("query restricts content tables"),
        }
    }
    let documents = sqlx::query!(
        r#"select source, api_key_id, count(*) as "documents!",
                  sum(storage_bytes)::bigint as "bytes!"
           from documents where user_id = $1 group by source, api_key_id"#,
        user_id
    )
    .fetch_all(&mut *tx)
    .await?;
    let chunks = sqlx::query!(
        r#"select d.source, d.api_key_id, count(*) as "chunks!",
                  sum(c.storage_bytes)::bigint as "bytes!",
                  sum(c.indexed_text_bytes)::bigint as "text_bytes!"
           from chunks c join documents d on d.id = c.document_id
           where d.user_id = $1 group by d.source, d.api_key_id"#,
        user_id
    )
    .fetch_all(&mut *tx)
    .await?;
    let models = sqlx::query!(
        r#"select model, count(*) as "embedded!" from chunk_embeddings group by model"#
    )
    .fetch_all(&mut *tx)
    .await?;
    let embeddings = sqlx::query!(
        r#"select d.source, d.api_key_id, e.model, count(*) as "embedded!",
                  sum(e.storage_bytes)::bigint as "bytes!"
           from chunk_embeddings e join documents d on d.id = e.document_id
           where d.user_id = $1 group by d.source, d.api_key_id, e.model"#,
        user_id
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut rows: BTreeMap<Option<Uuid>, Row> = BTreeMap::new();
    for document in documents {
        let writer = writers.of(&document.source, document.api_key_id);
        let row = rows
            .entry(writer.as_ref().map(|writer| writer.id))
            .or_default();
        row.writer = writer;
        row.sources.insert(document.source);
        row.documents += document.documents;
        row.storage.documents += document.bytes as f64;
        row.storage.indexes +=
            share(document_indexes, global.documents) * document.documents as f64;
    }
    for chunk in chunks {
        let writer = writers.of(&chunk.source, chunk.api_key_id);
        let row = rows
            .get_mut(&writer.as_ref().map(|writer| writer.id))
            .expect("a chunk's document has a statistics row");
        row.chunks += chunk.chunks;
        row.storage.chunks += chunk.bytes as f64;
        row.storage.indexes += share(chunk_indexes, global.chunks) * chunk.chunks as f64
            + share(bm25, global.text_bytes) * chunk.text_bytes as f64;
    }
    let model_totals: HashMap<_, _> = models
        .into_iter()
        .map(|model| (model.model, model.embedded))
        .collect();
    for embedding in embeddings {
        let writer = writers.of(&embedding.source, embedding.api_key_id);
        let row = rows
            .get_mut(&writer.as_ref().map(|writer| writer.id))
            .expect("an embedding's document has a statistics row");
        let model = row
            .models
            .entry(embedding.model.clone())
            .or_insert_with(|| ModelStorage {
                model: embedding.model.clone(),
                ..ModelStorage::default()
            });
        let index_bytes = hnsw
            .get(&model_registry::index_name(&embedding.model))
            .copied()
            .unwrap_or(0.0);
        let index_share =
            share(index_bytes, model_totals[&embedding.model]) * embedding.embedded as f64;
        model.embedded += embedding.embedded;
        model.row_bytes += embedding.bytes as f64;
        model.index_bytes += index_share;
        row.storage.embeddings += embedding.bytes as f64;
        row.storage.indexes +=
            index_share + share(embedding_indexes, global.embeddings) * embedding.embedded as f64;
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by(|a, b| match (&a.writer, &b.writer) {
        (Some(a), Some(b)) => a.name.cmp(&b.name).then(a.id.cmp(&b.id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    let mut result = Statistics {
        rows,
        documents: 0,
        chunks: 0,
        storage: Storage::default(),
    };
    for row in &result.rows {
        result.documents += row.documents;
        result.chunks += row.chunks;
        result.storage.documents += row.storage.documents;
        result.storage.chunks += row.storage.chunks;
        result.storage.embeddings += row.storage.embeddings;
        result.storage.indexes += row.storage.indexes;
    }
    Ok(result)
}
