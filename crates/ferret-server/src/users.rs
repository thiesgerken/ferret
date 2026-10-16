use std::{num::NonZero, sync::LazyLock, thread};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::StartupError;

pub const BOOTSTRAP_USERNAME: &str = "admin";
pub const BOOTSTRAP_PASSWORD_MIN_CHARS: usize = 16;

/// A signed-in user, as the UI sees it.
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
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

/// Runs Argon2 off the async runtime (it takes tens of milliseconds of CPU), at most one per core
/// at a time. Each run allocates about 19 MiB, and logins are public, so an unbounded burst of
/// them would allocate until the pod is OOM-killed. Excess runs queue instead.
// ponytail: one global bound. Per-IP or per-username rate limiting if logins get hammered.
async fn run_argon2<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    static PERMITS: LazyLock<Semaphore> =
        LazyLock::new(|| Semaphore::new(thread::available_parallelism().map_or(1, NonZero::get)));
    let _permit = PERMITS
        .acquire()
        .await
        .expect("the semaphore is never closed");
    tokio::task::spawn_blocking(work)
        .await
        .expect("Argon2 doesn't panic")
}
