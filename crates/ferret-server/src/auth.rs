//! Browser sessions: password login, the session cookie and its check on every request.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppState, StartupError, error::ApiError, users};

const COOKIE_NAME: &str = "ferret_session";
const SESSION_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const ISSUER: &str = "ferret";
const AUDIENCE: &str = "ferret-web";
const TOKEN_TYPE: &str = "session";
const SECRET_MIN_BYTES: usize = 32;

/// Signs and verifies session JWTs (HS256 under `FERRET_SESSION_SECRET`).
pub struct SessionKey {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    typ: String,
    sub: Uuid,
    sid: Uuid,
    iat: u64,
    exp: u64,
    ver: i32,
}

impl SessionKey {
    pub fn new(secret: &str) -> Result<Self, StartupError> {
        if secret.len() < SECRET_MIN_BYTES {
            return Err(
                format!("FERRET_SESSION_SECRET must be at least {SECRET_MIN_BYTES} bytes").into(),
            );
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        Ok(Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            validation,
        })
    }

    fn sign(&self, claims: &Claims) -> String {
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), claims, &self.encoding)
            .expect("HS256 signing of plain claims succeeds")
    }

    /// The claims of a correctly signed, unexpired session token, `None` for anything else.
    fn verify(&self, token: &str) -> Option<Claims> {
        let claims = jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .ok()?
            .claims;
        (claims.typ == TOKEN_TYPE).then_some(claims)
    }
}

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    credentials: Result<Json<Credentials>, JsonRejection>,
) -> Result<(StatusCode, [(axum::http::HeaderName, HeaderValue); 1]), ApiError> {
    let Json(credentials) = credentials?;
    let username = credentials.username.trim().to_lowercase();
    let user = sqlx::query!(
        "select id, password_hash, enabled, token_version from users where username = $1",
        username,
    )
    .fetch_optional(&state.pool)
    .await?;
    let stored_hash = user.as_ref().map(|user| user.password_hash.clone());
    let password_matches = users::password_matches(credentials.password, stored_hash).await;
    let user = match user {
        Some(user) if password_matches && user.enabled => user,
        // One answer for all three, so a login attempt doesn't reveal which usernames exist.
        _ => return Err(ApiError::unauthorized("invalid username or password")),
    };

    let mut tx = state.pool.begin().await?;
    let session_id = sqlx::query_scalar!(
        "insert into sessions (id, user_id, created_at, expires_at)
         values (gen_random_uuid(), $1, now(), now() + make_interval(secs => $2))
         returning id",
        user.id,
        SESSION_TTL.as_secs_f64(),
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "update users set last_login_at = now() where id = $1",
        user.id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let issued_at = unix_now();
    let token = state.session_key.sign(&Claims {
        iss: ISSUER.to_owned(),
        aud: AUDIENCE.to_owned(),
        typ: TOKEN_TYPE.to_owned(),
        sub: user.id,
        sid: session_id,
        iat: issued_at,
        exp: issued_at + SESSION_TTL.as_secs(),
        ver: user.token_version,
    });
    Ok((
        StatusCode::NO_CONTENT,
        [(SET_COOKIE, session_cookie(&token, SESSION_TTL))],
    ))
}

/// Always succeeds: whatever the cookie, the browser ends up without a session.
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, [(axum::http::HeaderName, HeaderValue); 1]), ApiError> {
    if let Some(claims) = session_token(&headers).and_then(|token| state.session_key.verify(token))
    {
        sqlx::query!("delete from sessions where id = $1", claims.sid)
            .execute(&state.pool)
            .await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(SET_COOKIE, session_cookie("", Duration::ZERO))],
    ))
}

/// The user behind the request's session cookie, if the session is still valid: the row exists
/// and hasn't expired, the user is enabled, and no password change happened since the login.
pub async fn signed_in_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<users::User>, sqlx::Error> {
    let Some(claims) = session_token(headers).and_then(|token| state.session_key.verify(token))
    else {
        return Ok(None);
    };
    sqlx::query_as!(
        users::User,
        "select u.id, u.username, u.display_name, u.is_admin
         from sessions s join users u on u.id = s.user_id
         where s.id = $1 and u.id = $2 and s.expires_at > now()
           and u.enabled and u.token_version = $3",
        claims.sid,
        claims.sub,
        claims.ver,
    )
    .fetch_optional(&state.pool)
    .await
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(COOKIE_NAME)?.strip_prefix('='))
}

fn session_cookie(value: &str, max_age: Duration) -> HeaderValue {
    let max_age = max_age.as_secs();
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={value}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={max_age}"
    ))
    .expect("a JWT is a valid header value")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs()
}
