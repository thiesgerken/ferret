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
use chrono::{DateTime, Utc};
use ferret_api::{Document, DocumentPut, PutResponse, ThreadQuery, ThreadResponse};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, types::Json as Jsonb};
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, Scope},
    chunking, connections, content_type,
    error::ApiError,
    exclusions, sources,
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
    if let Some(url) = &document.url
        && let Some(scheme) = dangerous_scheme(url)
    {
        return Err(ApiError::bad_request(&format!(
            "url must not use the {scheme}: scheme"
        )));
    }
    caller.require_source(&document.source)?;
    connections::check_api_key_may_write(&state.pool, &caller, &document.source, false).await?;

    let mut tx = state.pool.begin().await?;
    exclusions::lock_source(
        &mut tx,
        caller.user_id,
        &document.source,
        exclusions::SourceLock::Shared,
    )
    .await?;
    if exclusions::is_excluded(&mut *tx, caller.user_id, &document).await? {
        return Ok(Json(PutResponse::Excluded));
    }
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
    caller.require_source(&key.source)?;
    connections::check_api_key_may_write(&state.pool, &caller, &key.source, false).await?;
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
    find(&state.pool, caller.user_id, caller.sources(), id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("no such document"))
}

/// `user_id`'s document `id`, if it lies within the `allowed` source roots (`None`: all
/// sources). A document outside them doesn't exist for the caller. Both APIs read documents with
/// this.
pub async fn find(
    pool: &PgPool,
    user_id: Uuid,
    allowed: Option<&[String]>,
    id: Uuid,
) -> Result<Option<Document>, sqlx::Error> {
    let document = sqlx::query_as!(
        DocumentRow,
        r#"select id, source, external_id, content_type, title, text, url, thread, authors,
                  created_at, modified_at, meta as "meta: Jsonb<serde_json::Map<String, serde_json::Value>>"
           from documents
           where id = $1 and user_id = $2
             and ($3::text[] is null or exists (
                  select 1 from unnest($3::text[]) root where source_covers(root, source)))"#,
        id,
        user_id,
        allowed,
    )
    .fetch_optional(pool)
    .await?;
    Ok(document.map(Document::from))
}

/// `GET /api/documents?source=&thread=`: a thread's documents, oldest first.
pub async fn thread(
    State(state): State<AppState>,
    caller: ApiCaller,
    query: Result<Query<ThreadQuery>, QueryRejection>,
) -> Result<Json<ThreadResponse>, ApiError> {
    caller
        .require(Scope::SearchRead)
        .or_else(|_| caller.require(Scope::IngestRead))?;
    let Query(query) = query?;
    // ponytail: the whole thread in one response. A chat channel that grows for years would
    // want paging by `created_at`.
    let documents = sqlx::query_as!(
        DocumentRow,
        r#"select id, source, external_id, content_type, title, text, url, thread, authors,
                  created_at, modified_at, meta as "meta: Jsonb<serde_json::Map<String, serde_json::Value>>"
           from documents
           where user_id = $1 and source = $2 and thread = $3
             and ($4::text[] is null or exists (
                  select 1 from unnest($4::text[]) root where source_covers(root, source)))
           order by created_at, id"#,
        caller.user_id,
        query.source,
        query.thread,
        caller.sources(),
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(ThreadResponse {
        documents: documents.into_iter().map(Document::from).collect(),
    }))
}

/// The documents before and after one in its thread.
pub struct ThreadNeighbors {
    pub previous: Option<ThreadNeighbor>,
    pub next: Option<ThreadNeighbor>,
}

pub struct ThreadNeighbor {
    pub id: Uuid,
    pub title: Option<String>,
}

/// `document`'s neighbors in its thread, ordered like the thread listing.
pub async fn thread_neighbors(
    pool: &PgPool,
    user_id: Uuid,
    document: &Document,
) -> Result<ThreadNeighbors, sqlx::Error> {
    let Some(thread) = &document.thread else {
        return Ok(ThreadNeighbors {
            previous: None,
            next: None,
        });
    };
    let previous = sqlx::query_as!(
        ThreadNeighbor,
        "select id, title from documents
         where user_id = $1 and source = $2 and thread = $3 and (created_at, id) < ($4, $5)
         order by created_at desc, id desc limit 1",
        user_id,
        document.source,
        thread,
        document.created_at,
        document.id,
    )
    .fetch_optional(pool)
    .await?;
    let next = sqlx::query_as!(
        ThreadNeighbor,
        "select id, title from documents
         where user_id = $1 and source = $2 and thread = $3 and (created_at, id) > ($4, $5)
         order by created_at, id limit 1",
        user_id,
        document.source,
        thread,
        document.created_at,
        document.id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(ThreadNeighbors { previous, next })
}

struct DocumentRow {
    id: Uuid,
    source: String,
    external_id: String,
    content_type: String,
    title: Option<String>,
    text: String,
    url: Option<String>,
    thread: Option<String>,
    authors: Vec<String>,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
    meta: Jsonb<serde_json::Map<String, serde_json::Value>>,
}

impl From<DocumentRow> for Document {
    fn from(row: DocumentRow) -> Self {
        Self {
            id: row.id,
            source: row.source,
            external_id: row.external_id,
            content_type: content_type::parse(&row.content_type),
            title: row.title,
            text: row.text,
            url: row.url,
            thread: row.thread,
            authors: row.authors,
            created_at: row.created_at,
            modified_at: row.modified_at,
            meta: row.meta.0,
        }
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

    let (id, response) = match stored {
        Some(stored) if stored.content_hash == content_hash => {
            sqlx::query!(
                "update documents set seen_at = now() where id = $1",
                stored.id
            )
            .execute(&mut *tx)
            .await?;
            return Ok(PutResponse::Unchanged { id: stored.id });
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
            (stored.id, PutResponse::Updated { id: stored.id })
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
            (id, PutResponse::Created { id })
        }
    };
    sync_chunks(tx, id, user_id, document).await?;
    Ok(response)
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
    // The title is part of the embedded input: a new title is a new input, embedded or rejected
    // afresh.
    sqlx::query!(
        "delete from chunk_embeddings e using chunks c
         where c.id = e.chunk_id and c.id = any($1) and c.title is distinct from $2",
        &kept_ids,
        document.title,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "delete from embedding_failures f using chunks c
         where c.id = f.chunk_id and c.id = any($1) and c.title is distinct from $2",
        &kept_ids,
        document.title,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "update chunks c set
             ordinal = k.ordinal, start_offset = k.start_offset, end_offset = k.end_offset,
             content_type = $5, ts = $6, title = $7
         from unnest($1::uuid[], $2::int[], $3::int[], $4::int[])
             as k(id, ordinal, start_offset, end_offset)
         where c.id = k.id",
        &kept_ids,
        &kept.ordinals,
        &kept.start_offsets,
        &kept.end_offsets,
        content_type::as_str(document.content_type),
        document.created_at,
        document.title,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "update chunk_embeddings set content_type = $2, ts = $3 where chunk_id = any($1)",
        &kept_ids,
        content_type::as_str(document.content_type),
        document.created_at,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "insert into chunks
             (id, document_id, user_id, source, content_type, ts, title,
              ordinal, start_offset, end_offset, text, text_hash)
         select uuidv7(), $1, $2, $3, $4, $5, $6,
                n.ordinal, n.start_offset, n.end_offset, n.text, n.text_hash
         from unnest($7::int[], $8::int[], $9::int[], $10::text[], $11::bytea[])
             as n(ordinal, start_offset, end_offset, text, text_hash)",
        document_id,
        user_id,
        document.source,
        content_type::as_str(document.content_type),
        document.created_at,
        document.title,
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

/// Schemes that run code or smuggle content when a browser follows the link. A denylist, because
/// origins legitimately use custom schemes (`obsidian:`, `mid:`, `zotero:`). The frontend applies
/// the same rule before rendering a link (`frontend/src/lib/safe-url.ts`).
const DANGEROUS_SCHEMES: [&str; 3] = ["javascript", "data", "vbscript"];

/// The dangerous scheme `url` uses, read the way browsers do: leading control characters and
/// spaces don't count, tabs and line breaks inside are dropped, case doesn't matter.
fn dangerous_scheme(url: &str) -> Option<&'static str> {
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let (scheme, _) = url.split_once(':')?;
    DANGEROUS_SCHEMES
        .into_iter()
        .find(|dangerous| scheme.eq_ignore_ascii_case(dangerous))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dangerous_schemes_the_way_browsers_read_them() {
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "  \u{1}javascript:alert(1)",
            "java\tscript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "VBScript:msgbox",
        ] {
            assert!(dangerous_scheme(url).is_some(), "{url:?}");
        }
        for url in [
            "https://example.com",
            "obsidian://open?vault=main&file=a",
            "mid:1234@example.com",
            "relative/javascript:path",
            "no scheme at all",
        ] {
            assert_eq!(dangerous_scheme(url), None, "{url:?}");
        }
    }
}
