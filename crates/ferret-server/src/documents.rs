//! Ingest: documents in, chunks out. `PUT` is idempotent and cheap to repeat, so plugins can
//! resend everything on every run.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::StatusCode,
};
use ferret_api::{Document, DocumentPut, PutOutcome, PutResponse};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, types::Json as Jsonb};
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, Scope},
    chunking, content_type,
    error::ApiError,
    sources,
};

pub async fn put(
    State(state): State<AppState>,
    caller: ApiCaller,
    document: Result<Json<DocumentPut>, JsonRejection>,
) -> Result<Json<PutResponse>, ApiError> {
    caller.require(Scope::IngestWrite)?;
    let Json(document) = document?;
    if !sources::is_valid(&document.source) {
        return Err(ApiError::bad_request(&format!(
            "{:?} is not a source path like `notes` or `mail/zoho`",
            document.source
        )));
    }
    if document.external_id.is_empty() {
        return Err(ApiError::bad_request("external_id must not be empty"));
    }
    check_access(&caller, &document.source)?;

    let mut tx = state.pool.begin().await?;
    let response = upsert(&mut tx, caller.user_id, &document).await?;
    tx.commit().await?;
    Ok(Json(response))
}

#[derive(Deserialize)]
pub struct DocumentKey {
    source: String,
    external_id: String,
}

pub async fn delete(
    State(state): State<AppState>,
    caller: ApiCaller,
    key: Result<Query<DocumentKey>, QueryRejection>,
) -> Result<StatusCode, ApiError> {
    caller.require(Scope::IngestWrite)?;
    let Query(key) = key?;
    check_access(&caller, &key.source)?;
    let deleted = sqlx::query!(
        "delete from documents where user_id = $1 and source = $2 and external_id = $3",
        caller.user_id,
        key.source,
        key.external_id,
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("no such document"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get(
    State(state): State<AppState>,
    caller: ApiCaller,
    id: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Document>, ApiError> {
    caller
        .require(Scope::SearchRead)
        .or_else(|_| caller.require(Scope::IngestRead))?;
    let Path(id) = id?;
    let document = sqlx::query!(
        r#"select id, source, external_id, content_type, title, text, url, thread, authors,
                  created_at, modified_at, meta as "meta: Jsonb<serde_json::Map<String, serde_json::Value>>"
           from documents where id = $1 and user_id = $2"#,
        id,
        caller.user_id,
    )
    .fetch_optional(&state.pool)
    .await?
    // A document outside the key's sources doesn't exist for it.
    .filter(|document| caller.may_access(&document.source))
    .ok_or_else(|| ApiError::not_found("no such document"))?;
    Ok(Json(Document {
        id: document.id,
        source: document.source,
        external_id: document.external_id,
        content_type: content_type::parse(&document.content_type),
        title: document.title,
        text: document.text,
        url: document.url,
        thread: document.thread,
        authors: document.authors,
        created_at: document.created_at,
        modified_at: document.modified_at,
        meta: document.meta.0,
    }))
}

fn check_access(caller: &ApiCaller, source: &str) -> Result<(), ApiError> {
    if caller.may_access(source) {
        Ok(())
    } else {
        Err(ApiError::forbidden(&format!(
            "this API key may not access the source {source:?}"
        )))
    }
}

/// Stores `document` unless it's unchanged, and re-chunks a changed one. Chunks whose text
/// didn't change keep their identity (and later their embeddings).
async fn upsert(
    tx: &mut PgConnection,
    user_id: Uuid,
    document: &DocumentPut,
) -> Result<PutResponse, sqlx::Error> {
    let content_hash =
        Sha256::digest(serde_json::to_vec(document).expect("a document serializes")).to_vec();
    let stored = sqlx::query!(
        "select id, content_hash from documents
         where user_id = $1 and source = $2 and external_id = $3
         for update",
        user_id,
        document.source,
        document.external_id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let (id, outcome) = match stored {
        Some(stored) if stored.content_hash == content_hash => {
            sqlx::query!(
                "update documents set seen_at = now() where id = $1",
                stored.id
            )
            .execute(&mut *tx)
            .await?;
            return Ok(PutResponse {
                id: stored.id,
                outcome: PutOutcome::Unchanged,
            });
        }
        Some(stored) => {
            sqlx::query!(
                "update documents set
                     content_type = $2, title = $3, url = $4, thread = $5, authors = $6,
                     created_at = $7, modified_at = $8, meta = $9, text = $10,
                     content_hash = $11, seen_at = now()
                 where id = $1",
                stored.id,
                content_type::as_str(document.content_type),
                document.title,
                document.url,
                document.thread,
                &document.authors,
                document.created_at,
                document.modified_at,
                Jsonb(&document.meta) as _,
                document.text,
                content_hash,
            )
            .execute(&mut *tx)
            .await?;
            (stored.id, PutOutcome::Updated)
        }
        None => {
            // ponytail: two concurrent first PUTs of the same document make one fail on the
            // unique key (500). Retry on conflict if ingesters ever race on one document.
            let id = sqlx::query_scalar!(
                "insert into documents
                     (id, user_id, source, external_id, content_type, title, url, thread,
                      authors, created_at, modified_at, meta, text, content_hash, seen_at)
                 values (uuidv7(), $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now())
                 returning id",
                user_id,
                document.source,
                document.external_id,
                content_type::as_str(document.content_type),
                document.title,
                document.url,
                document.thread,
                &document.authors,
                document.created_at,
                document.modified_at,
                Jsonb(&document.meta) as _,
                document.text,
                content_hash,
            )
            .fetch_one(&mut *tx)
            .await?;
            (id, PutOutcome::Created)
        }
    };
    sync_chunks(tx, id, user_id, document).await?;
    Ok(PutResponse { id, outcome })
}

/// Makes the document's chunks match its text: chunks with an unchanged text keep their row,
/// the others are deleted or inserted. The denormalized columns are rewritten on all of them.
async fn sync_chunks(
    tx: &mut PgConnection,
    document_id: Uuid,
    user_id: Uuid,
    document: &DocumentPut,
) -> Result<(), sqlx::Error> {
    let mut reusable: HashMap<Vec<u8>, Vec<Uuid>> = HashMap::new();
    for stored in sqlx::query!(
        "select id, text_hash from chunks where document_id = $1",
        document_id
    )
    .fetch_all(&mut *tx)
    .await?
    {
        reusable
            .entry(stored.text_hash)
            .or_default()
            .push(stored.id);
    }

    let (mut kept_ids, mut kept, mut new) =
        (Vec::new(), ChunkRows::default(), ChunkRows::default());
    for (ordinal, chunk) in chunking::chunk(document.content_type, &document.text)
        .into_iter()
        .enumerate()
    {
        let text_hash = Sha256::digest(chunk.text).to_vec();
        match reusable.get_mut(&text_hash).and_then(Vec::pop) {
            Some(id) => {
                kept_ids.push(id);
                kept.push(ordinal, &chunk, text_hash);
            }
            None => new.push(ordinal, &chunk, text_hash),
        }
    }
    let stale: Vec<Uuid> = reusable.into_values().flatten().collect();

    sqlx::query!("delete from chunks where id = any($1)", &stale)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "update chunks c set
             ordinal = k.ordinal, start_offset = k.start_offset, end_offset = k.end_offset,
             content_type = $5, ts = $6
         from unnest($1::uuid[], $2::int[], $3::int[], $4::int[])
             as k(id, ordinal, start_offset, end_offset)
         where c.id = k.id",
        &kept_ids,
        &kept.ordinals,
        &kept.start_offsets,
        &kept.end_offsets,
        content_type::as_str(document.content_type),
        document.created_at,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "insert into chunks
             (id, document_id, user_id, source, content_type, ts,
              ordinal, start_offset, end_offset, text, text_hash)
         select uuidv7(), $1, $2, $3, $4, $5,
                n.ordinal, n.start_offset, n.end_offset, n.text, n.text_hash
         from unnest($6::int[], $7::int[], $8::int[], $9::text[], $10::bytea[])
             as n(ordinal, start_offset, end_offset, text, text_hash)",
        document_id,
        user_id,
        document.source,
        content_type::as_str(document.content_type),
        document.created_at,
        &new.ordinals,
        &new.start_offsets,
        &new.end_offsets,
        &new.texts,
        &new.text_hashes,
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Chunk columns as arrays, for one `unnest` statement instead of one per chunk.
#[derive(Default)]
struct ChunkRows {
    ordinals: Vec<i32>,
    start_offsets: Vec<i32>,
    end_offsets: Vec<i32>,
    texts: Vec<String>,
    text_hashes: Vec<Vec<u8>>,
}

impl ChunkRows {
    fn push(&mut self, ordinal: usize, chunk: &chunking::Chunk, text_hash: Vec<u8>) {
        let int =
            |value: usize| i32::try_from(value).expect("documents stay below 2^31 characters");
        self.ordinals.push(int(ordinal));
        self.start_offsets.push(int(chunk.start_offset));
        self.end_offsets.push(int(chunk.end_offset));
        self.texts.push(chunk.text.to_owned());
        self.text_hashes.push(text_hash);
    }
}
