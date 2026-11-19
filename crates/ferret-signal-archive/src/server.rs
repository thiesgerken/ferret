use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use sha2::{Digest, Sha256};
use sqlx::Row;
use subtle::ConstantTimeEq;

use crate::*;

struct Error(StatusCode, &'static str);

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            code: self.1.to_owned(),
        };
        (self.0, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        UNAVAILABLE
    }
}

const INVALID_QUERY: Error = Error(StatusCode::BAD_REQUEST, "invalid_query");
const UNAVAILABLE: Error = Error(StatusCode::SERVICE_UNAVAILABLE, "archive_unavailable");

struct Service {
    store: Store,
    token_hash: [u8; 32],
}

/// The token file holds 64 hexadecimal characters (32 random bytes); only a final newline is
/// optional. Comparing hashes makes the constant-time comparison fixed-length.
fn credential(secret: &str) -> Result<[u8; 32], &'static str> {
    let secret = secret.strip_suffix('\n').unwrap_or(secret);
    if secret.len() != 64 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("archive token must be 64 hexadecimal characters (32 random bytes)");
    }
    Ok(Sha256::digest(secret.as_bytes()).into())
}

pub fn router(store: Store, secret: &str) -> Result<Router, &'static str> {
    let state = Arc::new(Service {
        store,
        token_hash: credential(secret)?,
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
    let valid = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .is_some_and(|token| {
            <[u8; 32]>::from(Sha256::digest(token.as_bytes()))
                .ct_eq(&state.token_hash)
                .into()
        });
    if !valid {
        return Error(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    next.run(request).await
}

impl Service {
    fn same_store(&self, store_id: &str) -> Result<(), Error> {
        if self.store.store_id != store_id {
            return Err(Error(StatusCode::CONFLICT, "store_replaced"));
        }
        Ok(())
    }

    async fn high(&self) -> Result<i64, Error> {
        Ok(
            sqlx::query_scalar("SELECT coalesce(max(sequence), 0) FROM changes")
                .fetch_one(&self.store.pool)
                .await?,
        )
    }
}

async fn archive(State(state): State<Arc<Service>>) -> Result<Json<Archive>, Error> {
    let (last_received_at, unlinked): (Option<i64>, bool) =
        sqlx::query_as("SELECT last_received_at, unlinked FROM archive")
            .fetch_one(&state.store.pool)
            .await?;
    Ok(Json(Archive {
        store_id: state.store.store_id.clone(),
        status: if unlinked {
            CollectorStatus::Unlinked
        } else {
            CollectorStatus::Unknown
        },
        last_received_at,
        high: state.high().await?,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatsQuery {
    store_id: String,
    after: Option<String>,
}

async fn chats(
    State(state): State<Arc<Service>>,
    query: Result<Query<ChatsQuery>, QueryRejection>,
) -> Result<Json<ChatsPage>, Error> {
    let Query(query) = query.map_err(|_| INVALID_QUERY)?;
    state.same_store(&query.store_id)?;
    let mut chats: Vec<Chat> = sqlx::query_as(
        "SELECT chats.id, chats.kind,
                    CASE chats.kind
                        WHEN 'group' THEN chats.name
                        WHEN 'direct' THEN coalesce(recipients.name, recipients.number)
                        WHEN 'note_to_self' THEN 'Note to Self'
                    END AS name
             FROM chats LEFT JOIN recipients ON recipients.aci = chats.id
             WHERE chats.id > ? ORDER BY chats.id LIMIT ?",
    )
    .bind(query.after.unwrap_or_default())
    .bind(PAGE_SIZE + 1)
    .fetch_all(&state.store.pool)
    .await?;
    let next = next_page(&mut chats).map(|chat| chat.id.clone());
    Ok(Json(ChatsPage { chats, next }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesQuery {
    store_id: String,
    chat: String,
    after_sent_at: Option<i64>,
    after_author: Option<String>,
}

async fn messages(
    State(state): State<Arc<Service>>,
    query: Result<Query<MessagesQuery>, QueryRejection>,
) -> Result<Json<MessagesPage>, Error> {
    let Query(query) = query.map_err(|_| INVALID_QUERY)?;
    let after = match (query.after_sent_at, query.after_author) {
        (Some(sent_at), Some(author)) => (sent_at, author),
        (None, None) => (i64::MIN, String::new()),
        _ => return Err(INVALID_QUERY),
    };
    state.same_store(&query.store_id)?;
    let rows = sqlx::query(
        "SELECT sent_at, author, from_me, text, attachments, edited,
                recipients.name AS author_name, recipients.number AS author_number
         FROM messages LEFT JOIN recipients ON recipients.aci = messages.author
         WHERE chat = ? AND NOT deleted AND (sent_at, author) > (?, ?)
         ORDER BY sent_at, author LIMIT ?",
    )
    .bind(query.chat)
    .bind(after.0)
    .bind(after.1)
    .bind(PAGE_SIZE + 1)
    .fetch_all(&state.store.pool)
    .await?;
    let mut messages = rows
        .iter()
        .map(|row| {
            let attachments: &str = row.try_get("attachments")?;
            Ok(Message {
                key: MessageKey {
                    sent_at: row.try_get("sent_at")?,
                    author: row.try_get("author")?,
                },
                author_name: row.try_get("author_name")?,
                author_number: row.try_get("author_number")?,
                from_me: row.try_get("from_me")?,
                text: row.try_get("text")?,
                attachments: serde_json::from_str(attachments).map_err(|_| UNAVAILABLE)?,
                edited: row.try_get("edited")?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let next = next_page(&mut messages).map(|message| message.key.clone());
    Ok(Json(MessagesPage { messages, next }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangesQuery {
    store_id: String,
    after: i64,
    through: i64,
}

async fn changes(
    State(state): State<Arc<Service>>,
    query: Result<Query<ChangesQuery>, QueryRejection>,
) -> Result<Json<ChangesPage>, Error> {
    let Query(query) = query.map_err(|_| INVALID_QUERY)?;
    state.same_store(&query.store_id)?;
    if query.after < 0 || query.through < query.after || query.through > state.high().await? {
        return Err(Error(StatusCode::BAD_REQUEST, "invalid_checkpoint"));
    }
    let rows = sqlx::query(
        "SELECT sequence, recorded_at, operation, chat, sent_at, author FROM changes
         WHERE sequence > ? AND sequence <= ? ORDER BY sequence LIMIT ?",
    )
    .bind(query.after)
    .bind(query.through)
    .bind(PAGE_SIZE + 1)
    .fetch_all(&state.store.pool)
    .await?;
    let mut changes = rows
        .iter()
        .map(|row| {
            Ok(Change {
                sequence: row.try_get("sequence")?,
                recorded_at: row.try_get("recorded_at")?,
                operation: row.try_get("operation")?,
                chat: row.try_get("chat")?,
                key: match (row.try_get("sent_at")?, row.try_get("author")?) {
                    (Some(sent_at), Some(author)) => Some(MessageKey { sent_at, author }),
                    _ => None,
                },
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let has_more = next_page(&mut changes).is_some();
    let next = match changes.last() {
        Some(last) if has_more => last.sequence,
        _ => query.through,
    };
    Ok(Json(ChangesPage {
        changes,
        next,
        through: query.through,
        has_more,
    }))
}

/// Cuts a page fetched with one extra row back to `PAGE_SIZE` and returns its last row if
/// another page follows.
fn next_page<T>(rows: &mut Vec<T>) -> Option<&T> {
    if rows.len() > PAGE_SIZE as usize {
        rows.truncate(PAGE_SIZE as usize);
        rows.last()
    } else {
        None
    }
}
