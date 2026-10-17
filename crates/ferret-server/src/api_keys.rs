//! API keys: long-lived bearer credentials for the REST API, created and revoked by their owner.

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    api_auth::{ApiCaller, CallerKind, Scope},
    icons, sources,
    users::User,
};

pub const KEY_TAG: &str = "fk_";
const PREFIX_BYTES: usize = 4;
const SECRET_BYTES: usize = 32;

/// A key as its owner sees it in Settings. The secret is gone after creation.
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    /// A lucide icon name, see `icons`.
    pub icon: String,
    pub prefix: String,
    pub scopes: Vec<Scope>,
    pub sources: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

pub struct NewApiKey {
    pub name: String,
    pub icon: String,
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
        "select id, name, icon, prefix, scopes, sources, created_at, last_used_at, expires_at
         from api_keys where user_id = $1 and revoked_at is null order by created_at desc",
        user_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ApiKey {
            id: row.id,
            name: row.name,
            icon: row.icon,
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
             (id, user_id, name, icon, prefix, secret_hash, scopes, sources, created_at, expires_at)
         values (uuidv7(), $1, $2, $3, $4, $5, $6, $7, now(), $8)
         returning id, created_at",
        owner.id,
        new.name.trim(),
        new.icon,
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
        icon: new.icon,
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
    validate_label(&new.name, &new.icon)?;
    if new.scopes.is_empty() {
        return invalid("a key needs at least one scope");
    }
    if !owner.is_admin && new.scopes.iter().any(|scope| scope.needs_admin()) {
        return invalid("only admins can create keys with an admin scope");
    }
    if new.sources.is_some() && new.scopes.iter().any(|scope| scope.needs_admin()) {
        // Users and the other admin objects have no source, so the restriction couldn't apply.
        return invalid("admin scopes can't be restricted to sources");
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

fn validate_label(name: &str, icon: &str) -> Result<(), CreateError> {
    if name.trim().is_empty() {
        return Err(CreateError::Invalid("a key needs a name".to_owned()));
    }
    if !icons::is_valid(icon) {
        return Err(CreateError::Invalid(format!(
            "{icon:?} is not an icon name like `key-round`"
        )));
    }
    Ok(())
}

/// Renames one of `user_id`'s active keys or changes its icon. `None` if there's no such key.
pub async fn update(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    name: &str,
    icon: &str,
) -> Result<Option<ApiKey>, CreateError> {
    validate_label(name, icon)?;
    let updated = sqlx::query!(
        "update api_keys set name = $3, icon = $4
         where id = $1 and user_id = $2 and revoked_at is null",
        id,
        user_id,
        name.trim(),
        icon,
    )
    .execute(pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(list(pool, user_id)
        .await?
        .into_iter()
        .find(|key| key.id == id))
}

/// Revokes one of `user_id`'s keys: it stops working with its next request, and stays the origin
/// of the documents it put.
pub async fn revoke(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let revoked = sqlx::query!(
        "update api_keys set revoked_at = now()
         where id = $1 and user_id = $2 and revoked_at is null",
        id,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(revoked.rows_affected() == 1)
}

/// The caller behind an API key (`fk_<prefix>.<secret>`), `None` for a malformed, unknown, revoked
/// or expired key or a disabled owner.
pub async fn authenticate(pool: &PgPool, key: &str) -> Result<Option<ApiCaller>, sqlx::Error> {
    let Some((prefix, secret)) = key
        .strip_prefix(KEY_TAG)
        .and_then(|key| key.split_once('.'))
    else {
        return Ok(None);
    };
    // Hashed before the lookup, so an unknown prefix costs the same as a wrong secret.
    let presented_hash = secret_hash(secret);

    let key = sqlx::query!(
        r#"select k.id, k.secret_hash, k.scopes, k.sources, u.id as user_id, u.is_admin,
               u.enabled and k.revoked_at is null and (k.expires_at is null or k.expires_at > now())
                   as "usable!"
           from api_keys k join users u on u.id = k.user_id
           where k.prefix = $1"#,
        prefix,
    )
    .fetch_optional(pool)
    .await?;
    let stored_hash = key
        .as_ref()
        .map_or([0; 32].as_slice(), |key| key.secret_hash.as_slice());
    let secret_matches: bool = presented_hash.as_slice().ct_eq(stored_hash).into();
    let key = match key {
        Some(key) if secret_matches && key.usable => key,
        _ => return Ok(None),
    };

    // At most one write per key and minute, however busy the key is.
    sqlx::query!(
        "update api_keys set last_used_at = now()
         where id = $1 and (last_used_at is null or last_used_at < now() - interval '1 minute')",
        key.id,
    )
    .execute(pool)
    .await?;

    let scopes = key
        .scopes
        .iter()
        .map(|scope| Scope::parse(scope))
        // Admin scopes stop working the moment the owner stops being an admin.
        .filter(|scope| key.is_admin || !scope.needs_admin())
        .collect();
    Ok(Some(ApiCaller::new(
        key.user_id,
        scopes,
        key.sources,
        CallerKind::ApiKey(key.id),
    )))
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
