//! API keys: bearer credentials for the REST API, with scopes and an optional source restriction.

use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{AppState, error::ApiError, sources, users::User};

const KEY_TAG: &str = "fk_";
const PREFIX_BYTES: usize = 4;
const SECRET_BYTES: usize = 32;

/// What a key may do: `<scope>:<access>`, where write implies read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, juniper::GraphQLEnum)]
#[graphql(name = "ApiKeyScope")]
pub enum Scope {
    SearchRead,
    IngestRead,
    IngestWrite,
    AdminRead,
    AdminWrite,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Self::SearchRead => "search:read",
            Self::IngestRead => "ingest:read",
            Self::IngestWrite => "ingest:write",
            Self::AdminRead => "admin:read",
            Self::AdminWrite => "admin:write",
        }
    }

    fn parse(scope: &str) -> Self {
        match scope {
            "search:read" => Self::SearchRead,
            "ingest:read" => Self::IngestRead,
            "ingest:write" => Self::IngestWrite,
            "admin:read" => Self::AdminRead,
            "admin:write" => Self::AdminWrite,
            // The table's check constraint admits no other value.
            _ => panic!("unknown scope {scope:?} in the database"),
        }
    }

    fn grants(self, needed: Self) -> bool {
        self == needed
            || matches!(
                (self, needed),
                (Self::IngestWrite, Self::IngestRead) | (Self::AdminWrite, Self::AdminRead)
            )
    }

    /// Admin scopes only work while the key's owner is an admin.
    fn needs_admin(self) -> bool {
        matches!(self, Self::AdminRead | Self::AdminWrite)
    }
}

/// A key as its owner sees it in Settings. The secret is gone after creation.
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<Scope>,
    pub sources: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub struct NewApiKey {
    pub name: String,
    pub scopes: Vec<Scope>,
    pub sources: Option<Vec<String>>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub enum CreateError {
    /// Rejected input, worded for the user.
    Invalid(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for CreateError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub async fn list(pool: &PgPool, user_id: Uuid) -> Result<Vec<ApiKey>, sqlx::Error> {
    let rows = sqlx::query!(
        "select id, name, prefix, scopes, sources, created_at, last_used_at, expires_at
         from api_keys where user_id = $1 order by created_at desc",
        user_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ApiKey {
            id: row.id,
            name: row.name,
            prefix: row.prefix,
            scopes: row.scopes.iter().map(|scope| Scope::parse(scope)).collect(),
            sources: row.sources,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
            expires_at: row.expires_at,
        })
        .collect())
}

/// Creates a key for `owner` and returns it with the full key string, which exists only here.
pub async fn create(
    pool: &PgPool,
    owner: &User,
    new: NewApiKey,
) -> Result<(ApiKey, String), CreateError> {
    validate(owner, &new)?;
    let prefix = hex(&random_bytes::<PREFIX_BYTES>());
    let secret = BASE64_URL_SAFE_NO_PAD.encode(random_bytes::<SECRET_BYTES>());
    let scopes: Vec<&str> = new.scopes.iter().map(|scope| scope.as_str()).collect();
    let secret_hash = secret_hash(&secret);
    // ponytail: a prefix collision (32 random bits) fails the insert on the unique constraint.
    // Retry on conflict if anyone ever holds tens of thousands of keys.
    let row = sqlx::query!(
        "insert into api_keys
             (id, user_id, name, prefix, secret_hash, scopes, sources, created_at, expires_at)
         values (uuidv7(), $1, $2, $3, $4, $5, $6, now(), $7)
         returning id, created_at",
        owner.id,
        new.name.trim(),
        prefix,
        secret_hash.as_slice(),
        &scopes as &[&str],
        new.sources.as_deref(),
        new.expires_at,
    )
    .fetch_one(pool)
    .await?;
    let key = format!("{KEY_TAG}{prefix}.{secret}");
    let api_key = ApiKey {
        id: row.id,
        name: new.name.trim().to_owned(),
        prefix,
        scopes: new.scopes,
        sources: new.sources,
        created_at: row.created_at,
        last_used_at: None,
        expires_at: new.expires_at,
    };
    Ok((api_key, key))
}

fn validate(owner: &User, new: &NewApiKey) -> Result<(), CreateError> {
    let invalid = |message: &str| Err(CreateError::Invalid(message.to_owned()));
    if new.name.trim().is_empty() {
        return invalid("a key needs a name");
    }
    if new.scopes.is_empty() {
        return invalid("a key needs at least one scope");
    }
    if !owner.is_admin && new.scopes.iter().any(|scope| scope.needs_admin()) {
        return invalid("only admins can create keys with an admin scope");
    }
    if let Some(sources) = &new.sources {
        if sources.is_empty() {
            return invalid("leave sources out for all sources, or name at least one");
        }
        if let Some(source) = sources.iter().find(|source| !sources::is_valid(source)) {
            return Err(CreateError::Invalid(format!(
                "{source:?} is not a source path"
            )));
        }
    }
    if new
        .expires_at
        .is_some_and(|expires_at| expires_at <= Utc::now())
    {
        return invalid("the expiry date must lie in the future");
    }
    Ok(())
}

/// Deletes one of `user_id`'s keys. A revoked key stops working with its next request.
pub async fn revoke(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let deleted = sqlx::query!(
        "delete from api_keys where id = $1 and user_id = $2",
        id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected() == 1)
}

/// The caller of a `/api` route, authenticated by an API key. A key acts as its owner: it only
/// ever sees and writes that user's documents, within its scopes and sources.
pub struct ApiCaller {
    pub user_id: Uuid,
    scopes: Vec<Scope>,
    sources: Option<Vec<String>>,
}

impl ApiCaller {
    /// Fails with 403 unless one of the caller's scopes grants `needed`.
    pub fn require(&self, needed: Scope) -> Result<(), ApiError> {
        if self.scopes.iter().any(|scope| scope.grants(needed)) {
            Ok(())
        } else {
            Err(ApiError::forbidden(&format!(
                "this API key lacks the {} scope",
                needed.as_str()
            )))
        }
    }

    /// The source roots the key is restricted to, `None` for all sources.
    pub fn sources(&self) -> Option<&[String]> {
        self.sources.as_deref()
    }

    /// Whether the caller may read or write documents of `source`.
    pub fn may_access(&self, source: &str) -> bool {
        self.sources
            .as_ref()
            .is_none_or(|roots| roots.iter().any(|root| sources::covers(root, source)))
    }
}

impl FromRequestParts<AppState> for ApiCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let invalid = || ApiError::unauthorized("missing or invalid API key");
        let key = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(invalid)?;
        let (prefix, secret) = key
            .strip_prefix(KEY_TAG)
            .and_then(|key| key.split_once('.'))
            .ok_or_else(invalid)?;
        // Hashed before the lookup, so an unknown prefix costs the same as a wrong secret.
        let presented_hash = secret_hash(secret);

        let key = sqlx::query!(
            r#"select k.id, k.secret_hash, k.scopes, k.sources, u.id as user_id, u.is_admin,
                   u.enabled and (k.expires_at is null or k.expires_at > now()) as "usable!"
               from api_keys k join users u on u.id = k.user_id
               where k.prefix = $1"#,
            prefix,
        )
        .fetch_optional(&state.pool)
        .await?;
        let stored_hash = key
            .as_ref()
            .map_or([0; 32].as_slice(), |key| key.secret_hash.as_slice());
        let secret_matches: bool = presented_hash.as_slice().ct_eq(stored_hash).into();
        let key = match key {
            Some(key) if secret_matches && key.usable => key,
            _ => return Err(invalid()),
        };

        // At most one write per key and minute, however busy the key is.
        sqlx::query!(
            "update api_keys set last_used_at = now()
             where id = $1 and (last_used_at is null or last_used_at < now() - interval '1 minute')",
            key.id,
        )
        .execute(&state.pool)
        .await?;

        Ok(Self {
            user_id: key.user_id,
            scopes: key
                .scopes
                .iter()
                .map(|scope| Scope::parse(scope))
                .filter(|scope| key.is_admin || !scope.needs_admin())
                .collect(),
            sources: key.sources,
        })
    }
}

fn secret_hash(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the OS provides randomness");
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_implies_read_of_the_same_scope_only() {
        assert!(Scope::IngestWrite.grants(Scope::IngestRead));
        assert!(Scope::AdminWrite.grants(Scope::AdminRead));
        assert!(!Scope::IngestRead.grants(Scope::IngestWrite));
        assert!(!Scope::IngestWrite.grants(Scope::SearchRead));
        assert!(!Scope::AdminWrite.grants(Scope::IngestRead));
    }
}
