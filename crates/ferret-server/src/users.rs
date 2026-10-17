use std::sync::LazyLock;

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    AppState, StartupError,
    api_auth::{ApiCaller, Scope},
    error::ApiError,
};

pub const BOOTSTRAP_USERNAME: &str = "admin";
pub const BOOTSTRAP_PASSWORD_MIN_CHARS: usize = 16;
/// For passwords an admin sets. The bootstrap's is longer, since nobody types it.
pub const PASSWORD_MIN_CHARS: usize = 12;

/// A signed-in user, as the UI sees it.
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    /// The embedding model the user's searches use, `None` for the configured default.
    pub default_embedding_model: Option<String>,
}

/// Sets the embedding model `user_id`'s searches use, `None` for the configured default. The
/// caller checks that the model is configured.
pub async fn set_default_embedding_model(
    pool: &PgPool,
    user_id: Uuid,
    model: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "update users set default_embedding_model = $2 where id = $1",
        user_id,
        model,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// A user as admins manage them.
pub struct Account {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

pub struct NewUser {
    pub username: String,
    pub display_name: Option<String>,
    pub password: String,
    pub is_admin: bool,
}

pub struct UserUpdate {
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub enabled: bool,
}

pub enum UserError {
    /// Rejected input, worded for the admin.
    Invalid(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for UserError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, UserError> {
    Err(UserError::Invalid(message.into()))
}

pub async fn accounts(pool: &PgPool) -> Result<Vec<Account>, sqlx::Error> {
    sqlx::query_as!(
        Account,
        "select id, username, display_name, is_admin, enabled, created_at, last_login_at
         from users order by username"
    )
    .fetch_all(pool)
    .await
}

pub async fn create(pool: &PgPool, new: NewUser) -> Result<Account, UserError> {
    // As the login reads usernames, so the new user can sign in with what was typed.
    let username = new.username.trim().to_lowercase();
    let valid = username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if username.is_empty() || !valid {
        return invalid(format!(
            "{:?} isn't a username: letters, digits, `.`, `_` and `-` only",
            new.username
        ));
    }
    check_password(&new.password)?;
    let password_hash = hash_password(new.password).await;
    sqlx::query_as!(
        Account,
        "insert into users
             (id, username, display_name, password_hash, is_admin, created_at, password_changed_at)
         values (uuidv7(), $1, $2, $3, $4, now(), now())
         on conflict (username) do nothing
         returning id, username, display_name, is_admin, enabled, created_at, last_login_at",
        username,
        display_name(new.display_name),
        password_hash,
        new.is_admin,
    )
    .fetch_optional(pool)
    .await?
    .map_or_else(
        || invalid(format!("the username {username:?} is taken")),
        Ok,
    )
}

/// Sets a user's password and ends their sessions. Returns the new token version, `None` if
/// there's no such user.
pub async fn set_password(
    pool: &PgPool,
    id: Uuid,
    password: String,
) -> Result<Option<i32>, UserError> {
    store_password(pool, id, password, None).await
}

pub enum OwnPasswordChange {
    Changed {
        token_version: i32,
    },
    WrongPassword,
    /// Another password change (or an admin's) ended the session while this one hashed.
    SessionEnded,
}

/// Changes `id`'s own password once `current` matches, as long as the user's token version is
/// still `session_version`. The check costs a hash either way, like a login.
pub async fn change_own_password(
    pool: &PgPool,
    id: Uuid,
    session_version: i32,
    current: String,
    new: String,
) -> Result<OwnPasswordChange, UserError> {
    let stored_hash = sqlx::query_scalar!("select password_hash from users where id = $1", id)
        .fetch_one(pool)
        .await?;
    if !password_matches(current, Some(stored_hash)).await {
        return Ok(OwnPasswordChange::WrongPassword);
    }
    Ok(
        match store_password(pool, id, new, Some(session_version)).await? {
            Some(token_version) => OwnPasswordChange::Changed { token_version },
            None => OwnPasswordChange::SessionEnded,
        },
    )
}

/// Stores a new password and bumps the token version, only from `expected_version` if given, so
/// two changes can't silently overwrite each other. The new version, `None` if nothing changed.
async fn store_password(
    pool: &PgPool,
    id: Uuid,
    password: String,
    expected_version: Option<i32>,
) -> Result<Option<i32>, UserError> {
    check_password(&password)?;
    let password_hash = hash_password(password).await;
    let version = sqlx::query_scalar!(
        "update users
         set password_hash = $2, token_version = token_version + 1, password_changed_at = now()
         where id = $1 and ($3::int is null or token_version = $3)
         returning token_version",
        id,
        password_hash,
        expected_version,
    )
    .fetch_optional(pool)
    .await?;
    Ok(version)
}

/// Changes a user's name, admin role and whether they can sign in. Refuses to leave no enabled
/// admin: the bootstrap would bring `admin` back on the next start, but only where the bootstrap
/// password is still set, so the UI never relies on it.
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    update: UserUpdate,
) -> Result<Option<Account>, UserError> {
    let mut tx = pool.begin().await?;
    // Locks every enabled admin, so two admins demoting each other can't both succeed.
    let admins = sqlx::query_scalar!("select id from users where is_admin and enabled for update")
        .fetch_all(&mut *tx)
        .await?;
    let stays_admin = update.is_admin && update.enabled;
    if !stays_admin && admins == [id] {
        return invalid("this is the last enabled admin: make someone else an admin first");
    }
    let account = sqlx::query_as!(
        Account,
        "update users set display_name = $2, is_admin = $3, enabled = $4 where id = $1
         returning id, username, display_name, is_admin, enabled, created_at, last_login_at",
        id,
        display_name(update.display_name),
        update.is_admin,
        update.enabled,
    )
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(account)
}

fn check_password(password: &str) -> Result<(), UserError> {
    if password.chars().count() < PASSWORD_MIN_CHARS {
        return invalid(format!(
            "a password needs at least {PASSWORD_MIN_CHARS} characters"
        ));
    }
    Ok(())
}

/// A blank display name is no display name.
fn display_name(name: Option<String>) -> Option<String> {
    name.map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// `GET /api/users`: every user, for admin tooling.
pub async fn list(
    State(state): State<AppState>,
    caller: ApiCaller,
) -> Result<Json<Vec<ferret_api::User>>, ApiError> {
    caller.require(Scope::AdminRead)?;
    let users = sqlx::query_as!(
        ferret_api::User,
        "select id, username, display_name, is_admin, enabled from users order by username"
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

/// Makes sure an enabled admin exists, creating `admin` with `password` if none does.
///
/// A user named `admin` that exists without being an enabled admin is turned back into one with
/// the new password, so setting the bootstrap password is also the way back in after locking
/// every admin out.
pub async fn bootstrap_admin(pool: &PgPool, password: Option<&str>) -> Result<(), StartupError> {
    let admin_exists = sqlx::query_scalar!(
        r#"select exists (select 1 from users where is_admin and enabled) as "exists!""#
    )
    .fetch_one(pool)
    .await?;
    if admin_exists {
        return Ok(());
    }

    let password = password
        .filter(|password| password.chars().count() >= BOOTSTRAP_PASSWORD_MIN_CHARS)
        .ok_or_else(|| {
            format!(
                "no enabled admin exists, and FERRET_BOOTSTRAP_PASSWORD is missing or shorter \
                 than {BOOTSTRAP_PASSWORD_MIN_CHARS} characters"
            )
        })?;
    let password_hash = hash_password(password.to_owned()).await;
    // Replicas starting together may both get here, the upsert lets both succeed.
    sqlx::query!(
        "insert into users
             (id, username, password_hash, is_admin, created_at, password_changed_at)
         values (uuidv7(), $1, $2, true, now(), now())
         on conflict (username) do update set
             password_hash = excluded.password_hash,
             is_admin = true,
             enabled = true,
             token_version = users.token_version + 1,
             password_changed_at = now()",
        BOOTSTRAP_USERNAME,
        password_hash,
    )
    .execute(pool)
    .await?;
    tracing::info!(username = BOOTSTRAP_USERNAME, "created the bootstrap admin");
    Ok(())
}

async fn hash_password(password: String) -> String {
    run_argon2(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .expect("hashing with a fresh salt succeeds")
            .to_string()
    })
    .await
}

/// Checks `password` against a stored hash. Without a stored hash (unknown user) it checks
/// against a dummy, so the response time doesn't reveal which usernames exist.
pub async fn password_matches(password: String, stored_hash: Option<String>) -> bool {
    static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
        Argon2::default()
            .hash_password(b"dummy password for unknown users")
            .expect("hashing with a fresh salt succeeds")
            .to_string()
    });
    let known_user = stored_hash.is_some();
    run_argon2(move || {
        let hash = stored_hash.unwrap_or_else(|| DUMMY_HASH.clone());
        let hash = PasswordHash::new(&hash).expect("stored password hashes are PHC strings");
        let matches = Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok();
        known_user && matches
    })
    .await
}

/// Runs Argon2 off the async runtime (it takes tens of milliseconds of CPU), a few at a time.
/// Each run allocates about 19 MiB, and logins are public, so an unbounded burst of them would
/// allocate until the pod is OOM-killed. Excess runs queue instead.
// ponytail: one global bound. Per-IP or per-username rate limiting if logins get hammered.
async fn run_argon2<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    // Fixed rather than per core: the pod has no CPU limit, and a big node mustn't mean more memory.
    static PERMITS: Semaphore = Semaphore::const_new(4);
    let permit = PERMITS
        .acquire()
        .await
        .expect("the semaphore is never closed");
    tokio::task::spawn_blocking(move || {
        // Held by the blocking task, not the request: a client that disconnects drops the
        // request's future, but the hash keeps running and must keep its permit.
        let _permit = permit;
        work()
    })
    .await
    .expect("Argon2 doesn't panic")
}
