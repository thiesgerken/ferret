use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use super::*;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use subtle::ConstantTimeEq;
struct Error(StatusCode, &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(ErrorBody {
                code: self.1.to_owned(),
            }),
        )
            .into_response()
    }
}
impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "archive_unavailable")
    }
}

struct Service {
    path: PathBuf,
    token_hash: [u8; 32],
    // An idle reader keeps WAL sidecars available when the collector closes. It holds no
    // transaction/snapshot; request reads still reopen the path to detect store replacement.
    _keepalive: std::sync::Mutex<SqliteConnection>,
}

/// The secret file contains a single 64-character hexadecimal encoding of 32 random bytes.
/// Only a final newline is optional. Hashing makes the comparison fixed-length.
pub fn credential(secret: &str) -> Result<[u8; 32], &'static str> {
    let secret = secret.strip_suffix('\n').unwrap_or(secret);
    if secret.len() != 64 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("archive token must be 64 hexadecimal characters (32 random bytes)");
    }
    Ok(Sha256::digest(secret.as_bytes()).into())
}

pub async fn router(
    path: impl AsRef<Path>,
    secret: &str,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let path = path.as_ref().to_owned();
    let token_hash = credential(secret)?;
    let mut db = connect_file(&path)
        .await
        .map_err(|_| "cannot open wacli archive read-only")?;
    metadata(&mut db).await.map_err(
        |_| "cannot read supported wacli archive: verify schema 32/33 writable migrations and readable WAL/SHM sidecars",
    )?;
    let state = Arc::new(Service {
        path,
        token_hash,
        _keepalive: std::sync::Mutex::new(db),
    });
    Ok(Router::new()
        .route("/v1/archive", get(archive))
        .route("/v1/chats", get(chats))
        .route("/v1/messages", get(messages))
        .route("/v1/changes", get(changes))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .with_state(state))
}

async fn authorize(
    State(state): State<Arc<Service>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "));
    let valid = token
        .map(|token| {
            <[u8; 32]>::from(Sha256::digest(token.as_bytes()))
                .ct_eq(&state.token_hash)
                .into()
        })
        .unwrap_or(false);
    if !valid {
        return Error(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    next.run(request).await
}

impl Service {
    async fn connect(&self) -> Result<SqliteConnection, Error> {
        // Reopen per request: replacement of the mounted database must not leave us serving
        // a pool's old inode. Never use immutable=1 while a collector can write WAL pages.
        connect_file(&self.path).await
    }
}

async fn connect_file(path: &Path) -> Result<SqliteConnection, Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(5))
        .pragma("query_only", "ON");
    Ok(SqliteConnection::connect_with(&options).await?)
}

async fn metadata(db: &mut SqliteConnection) -> Result<Archive, Error> {
    let versions: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version,name FROM schema_migrations WHERE version>=30 ORDER BY version",
    )
    .fetch_all(&mut *db)
    .await?;
    let expected = [
        (30, "retrieval identity, revision and sync state"),
        (31, "content-free message change journal"),
        (32, "message receipt retrieval revisions"),
        (33, "ordered chat auto-unarchive"),
    ];
    if !matches!(versions.len(), 3 | 4)
        || !versions
            .iter()
            .zip(expected)
            .all(|((v, n), (ev, en))| *v == ev && n == en)
    {
        return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "unsupported_schema"));
    }
    let row = sqlx::query("SELECT store_id,revision,last_sync_at FROM retrieval_state WHERE id=1")
        .fetch_one(&mut *db)
        .await?;
    let store_id: String = row.try_get("store_id")?;
    if store_id.len() != 32 || !store_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "unsupported_schema"));
    }
    let floor: i64 = sqlx::query_scalar("SELECT max(floor,COALESCE((SELECT max(sequence) FROM message_changes WHERE recorded_at<=CAST(strftime('%s','now') AS INTEGER)-2592000),0)) FROM change_state WHERE id=1").fetch_one(&mut *db).await?;
    let high: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='message_changes'),0)",
    )
    .fetch_one(&mut *db)
    .await?;
    Ok(Archive {
        store_id,
        schema_version: versions.last().expect("validated migrations").0,
        revision: row.try_get("revision")?,
        last_sync_at: row.try_get("last_sync_at")?,
        collector_status: "unknown".to_owned(),
        floor,
        high,
    })
}

async fn archive(State(state): State<Arc<Service>>) -> Result<Json<Archive>, Error> {
    let mut db = state.connect().await?;
    let mut tx = db.begin().await?;
    let mut result = metadata(&mut tx).await?;
    result.collector_status = match std::fs::metadata(state.path.with_file_name("SESSION_REVOKED"))
    {
        Ok(_) => "logged_out",
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "unknown",
        Err(_) => {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "collector_status_unavailable",
            ));
        }
    }
    .to_owned();
    tx.commit().await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatsQuery {
    store_id: String,
    after: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesQuery {
    store_id: String,
    chat: String,
    after_ts: Option<i64>,
    after_rowid: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangesQuery {
    store_id: String,
    after: i64,
    through: i64,
}

fn same_store(archive: &Archive, store_id: &str) -> Result<(), Error> {
    if archive.store_id != store_id {
        return Err(Error(StatusCode::CONFLICT, "store_replaced"));
    }
    Ok(())
}

async fn chats(
    State(state): State<Arc<Service>>,
    query: Result<Query<ChatsQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<ChatsPage>, Error> {
    let Query(query) = query.map_err(|_| Error(StatusCode::BAD_REQUEST, "invalid_query"))?;
    let mut db = state.connect().await?;
    let mut tx = db.begin().await?;
    same_store(&metadata(&mut tx).await?, &query.store_id)?;
    let mut chats: Vec<Chat> =
        sqlx::query_as("SELECT jid,kind,name FROM chats WHERE jid>? ORDER BY jid LIMIT ?")
            .bind(query.after.unwrap_or_default())
            .bind(PAGE_SIZE + 1)
            .fetch_all(&mut *tx)
            .await?;
    let next = if chats.len() > PAGE_SIZE as usize {
        chats.truncate(PAGE_SIZE as usize);
        chats.last().map(|chat| chat.jid.clone())
    } else {
        None
    };
    tx.commit().await?;
    Ok(Json(ChatsPage { chats, next }))
}

async fn messages(
    State(state): State<Arc<Service>>,
    query: Result<Query<MessagesQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<MessagesPage>, Error> {
    let Query(query) = query.map_err(|_| Error(StatusCode::BAD_REQUEST, "invalid_query"))?;
    if query.after_ts.is_some() != query.after_rowid.is_some() {
        return Err(Error(StatusCode::BAD_REQUEST, "invalid_query"));
    }
    let mut db = state.connect().await?;
    let mut tx = db.begin().await?;
    same_store(&metadata(&mut tx).await?, &query.store_id)?;
    let mut messages: Vec<Message> = sqlx::query_as("SELECT rowid,msg_id,sender_jid,sender_name,ts,from_me,text,display_text,media_type,media_caption,filename,mime_type,file_length,edited FROM messages WHERE chat_jid=? AND deleted_at IS NULL AND revoked=0 AND deleted_for_me=0 AND payload_purged_at IS NULL AND (ts,rowid)>(?,?) ORDER BY ts,rowid LIMIT ?")
        .bind(query.chat).bind(query.after_ts.unwrap_or(i64::MIN)).bind(query.after_rowid.unwrap_or(i64::MIN)).bind(PAGE_SIZE+1).fetch_all(&mut *tx).await?;
    let next = if messages.len() > PAGE_SIZE as usize {
        messages.truncate(PAGE_SIZE as usize);
        messages.last().map(|message| MessageKey {
            ts: message.ts,
            rowid: message.rowid,
        })
    } else {
        None
    };
    tx.commit().await?;
    Ok(Json(MessagesPage { messages, next }))
}

async fn changes(
    State(state): State<Arc<Service>>,
    query: Result<Query<ChangesQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<ChangesPage>, Error> {
    let Query(query) = query.map_err(|_| Error(StatusCode::BAD_REQUEST, "invalid_query"))?;
    let mut db = state.connect().await?;
    let mut tx = db.begin().await?;
    let archive = metadata(&mut tx).await?;
    same_store(&archive, &query.store_id)?;
    if query.after < archive.floor {
        return Err(Error(StatusCode::GONE, "checkpoint_expired"));
    }
    if query.after < 0 || query.through < query.after || query.through > archive.high {
        return Err(Error(StatusCode::BAD_REQUEST, "invalid_checkpoint"));
    }
    let mut changes: Vec<Change> = sqlx::query_as("SELECT sequence,recorded_at,operation,chat_jid,msg_id,new_chat_jid,new_msg_id,reason,message_ts FROM message_changes WHERE sequence>? AND sequence<=? ORDER BY sequence LIMIT ?").bind(query.after).bind(query.through).bind(PAGE_SIZE+1).fetch_all(&mut *tx).await?;
    let has_more = changes.len() > PAGE_SIZE as usize;
    changes.truncate(PAGE_SIZE as usize);
    let next = if has_more {
        changes.last().expect("continuation row").sequence
    } else {
        query.through
    };
    tx.commit().await?;
    Ok(Json(ChangesPage {
        changes,
        next,
        through: query.through,
        has_more,
    }))
}
