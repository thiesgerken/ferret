//! The harness every black-box server test uses.
//!
//! Tests take their pool from `#[sqlx::test(migrations = false)]`, which gives each test a fresh
//! database in the ParadeDB that `DATABASE_URL` points at, and leaves migrating to the server's
//! own startup.

// Every test file compiles its own copy of this module and uses only part of it.
#![allow(dead_code)]

use ferret_api::Client;
use ferret_server::{AppState, SessionKey};
use reqwest::{Response, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;

pub const ADMIN_PASSWORD: &str = "the bootstrap admin's password";
const SESSION_SECRET: &str = "a session secret of at least 32 bytes";

pub struct TestServer {
    /// The machine API's client.
    pub api: Client,
    url: String,
    http: reqwest::Client,
}

/// Starts the server in-process on a random port, the way `ferret` starts: migrations, then the
/// bootstrap admin (`admin` / [`ADMIN_PASSWORD`]).
pub async fn start_server(pool: PgPool) -> TestServer {
    ferret_server::migrate(&pool)
        .await
        .expect("migrations apply");
    ferret_server::bootstrap_admin(&pool, Some(ADMIN_PASSWORD))
        .await
        .expect("the admin is bootstrapped");
    let session_key = SessionKey::new(SESSION_SECRET).unwrap();
    let app = ferret_server::router(AppState::new(pool, session_key));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestServer {
        api: Client::new(url.clone()),
        url,
        http: reqwest::Client::new(),
    }
}

impl TestServer {
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}{path}", self.url))
    }

    pub async fn login(&self, username: &str, password: &str) -> Response {
        self.request(reqwest::Method::POST, "/auth/login")
            .json(&json!({"username": username, "password": password}))
            .send()
            .await
            .unwrap()
    }

    /// Signs in as the bootstrap admin and returns the `Cookie` header value for the session.
    pub async fn sign_in(&self) -> String {
        let response = self.login("admin", ADMIN_PASSWORD).await;
        assert_eq!(response.status(), 204);
        session_cookie(&response)
    }

    /// Asks GraphQL for `me`, with the given `Cookie` header value.
    pub async fn me(&self, cookie: &str) -> Response {
        self.request(reqwest::Method::POST, "/graphql")
            .header(header::COOKIE, cookie)
            .json(&json!({"query": "{ me { id username displayName isAdmin } }"}))
            .send()
            .await
            .unwrap()
    }
}

/// The `name=value` part of the response's `Set-Cookie`, ready for a `Cookie` header.
pub fn session_cookie(response: &Response) -> String {
    set_cookie(response).split(';').next().unwrap().to_owned()
}

pub fn set_cookie(response: &Response) -> &str {
    response.headers()[header::SET_COOKIE].to_str().unwrap()
}

pub async fn json_body(response: Response) -> Value {
    response.json().await.unwrap()
}
