//! The harness every black-box server test uses.
//!
//! Tests take their pool from `#[sqlx::test(migrations = false)]`, which gives each test a fresh
//! database in the ParadeDB that `DATABASE_URL` points at, and leaves migrating to the server's
//! own startup.

// Every test file compiles its own copy of this module and uses only part of it.
#![allow(dead_code)]

pub mod fake_tei;

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, TimeZone, Utc};
use fake_tei::{FakeTei, Similarity};
use ferret_api::{Client, ContentType, DocumentPut};
use ferret_server::{
    AppState, SessionKey,
    config::{EmbeddingConfig, ModelConfig},
    embeddings,
};
use reqwest::{Response, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;

pub const ADMIN_PASSWORD: &str = "the bootstrap admin's password";
const SESSION_SECRET: &str = "a session secret of at least 32 bytes";
pub const MODEL: &str = "fake";
pub const QUERY_PREFIX: &str = "query: ";
pub const DOCUMENT_PREFIX: &str = "passage: ";

pub struct TestServer {
    /// The machine API's client.
    pub api: Client,
    /// The default model's TEI.
    pub tei: FakeTei,
    pool: PgPool,
    url: String,
    http: reqwest::Client,
    /// Server and pump. They hold pool connections (the pump's LISTEN one for good), which
    /// must be back before `#[sqlx::test]` drops the test database.
    tasks: Vec<tokio::task::AbortHandle>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.tasks.iter().for_each(tokio::task::AbortHandle::abort);
    }
}

/// Starts the server in-process on a random port, the way `ferret` starts: migrations, the
/// bootstrap admin (`admin` / [`ADMIN_PASSWORD`]), the embedding model and its pump. The model's
/// fake TEI finds nothing similar, so searches behave like keyword searches.
pub async fn start_server(pool: PgPool) -> TestServer {
    let tei = FakeTei::start(Similarity::None, fake_tei::MAX_INPUT_LENGTH).await;
    start_server_with(pool, tei.clone(), embedding_config(model(MODEL, &tei))).await
}

/// Like [`start_server`], with a TEI whose vectors are similar for texts about the same thing.
pub async fn start_semantic_server(pool: PgPool) -> TestServer {
    let tei = FakeTei::start(Similarity::Words, fake_tei::MAX_INPUT_LENGTH).await;
    start_server_with(pool, tei.clone(), embedding_config(model(MODEL, &tei))).await
}

pub async fn start_server_with(
    pool: PgPool,
    tei: FakeTei,
    embedding: EmbeddingConfig,
) -> TestServer {
    ferret_server::migrate(&pool)
        .await
        .expect("migrations apply");
    ferret_server::bootstrap_admin(&pool, Some(ADMIN_PASSWORD))
        .await
        .expect("the admin is bootstrapped");
    let embedding = Arc::new(
        embeddings::prepare(&pool, embedding)
            .await
            .expect("the embedding models are ready"),
    );
    let pump = tokio::spawn(embeddings::run_pump(pool.clone(), embedding.clone()));
    let session_key = SessionKey::new(SESSION_SECRET).unwrap();
    let app = ferret_server::router(AppState::new(pool.clone(), session_key, embedding));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestServer {
        api: Client::new(url.clone()),
        tei,
        pool,
        url,
        http: reqwest::Client::new(),
        tasks: vec![pump.abort_handle(), server.abort_handle()],
    }
}

/// A model on `tei` with the test prefixes.
pub fn model(name: &str, tei: &FakeTei) -> ModelConfig {
    ModelConfig {
        name: name.to_owned(),
        url: tei.url.clone(),
        bulk_url: None,
        query_prefix: QUERY_PREFIX.to_owned(),
        document_prefix: DOCUMENT_PREFIX.to_owned(),
    }
}

/// `model` as the only and default model.
pub fn embedding_config(model: ModelConfig) -> EmbeddingConfig {
    EmbeddingConfig {
        default: model.name.clone(),
        models: vec![model],
    }
}

impl TestServer {
    /// Waits until the pump has embedded `count` chunks (all models together).
    pub async fn wait_for_embeddings(&self, count: i64) {
        for _ in 0..200 {
            let embedded: i64 = sqlx::query_scalar("select count(*) from chunk_embeddings")
                .fetch_one(&self.pool)
                .await
                .unwrap();
            if embedded >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the pump didn't embed {count} chunks within 10 s");
    }

    /// A machine API client that authenticates with `api_key`.
    pub fn api_with_key(&self, api_key: &str) -> Client {
        Client::new(self.url.clone()).with_api_key(api_key)
    }

    pub fn api_without_key(&self) -> Client {
        Client::new(self.url.clone())
    }

    /// A client for the bootstrap admin with a key that may ingest and search everywhere.
    pub async fn admin_api(&self) -> Client {
        let key = self
            .create_api_key(&["INGEST_WRITE", "SEARCH_READ"], None)
            .await;
        self.api_with_key(&key)
    }

    /// Creates an API key for the bootstrap admin and returns it (`fk_...`). `scopes` are the
    /// GraphQL names (`"INGEST_WRITE"`, `"SEARCH_READ"`, ...), `sources` restricts every scope,
    /// `None` for all sources.
    pub async fn create_api_key(&self, scopes: &[&str], sources: Option<&[&str]>) -> String {
        let cookie = self.sign_in().await;
        let body = self
            .graphql(
                &cookie,
                "mutation ($input: NewApiKey!) { createApiKey(input: $input) { key } }",
                json!({"input": {"name": "test key", "scopes": scopes, "sources": sources}}),
            )
            .await;
        assert_eq!(body["errors"], Value::Null, "{body}");
        body["data"]["createApiKey"]["key"]
            .as_str()
            .unwrap()
            .to_owned()
    }

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

    /// Runs a GraphQL operation with the given `Cookie` header value and returns the response
    /// body.
    pub async fn graphql(&self, cookie: &str, query: &str, variables: Value) -> Value {
        let response = self
            .request(reqwest::Method::POST, "/graphql")
            .header(header::COOKIE, cookie)
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await
            .unwrap();
        json_body(response).await
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

/// A plain-text document created at [`day`]`(1)`, to adjust per test.
pub fn document(source: &str, external_id: &str, text: &str) -> DocumentPut {
    DocumentPut {
        source: source.to_owned(),
        external_id: external_id.to_owned(),
        content_type: ContentType::Plain,
        title: Some(external_id.to_owned()),
        text: text.to_owned(),
        url: None,
        thread: None,
        authors: vec!["someone".to_owned()],
        created_at: day(1),
        modified_at: day(1),
        meta: Default::default(),
    }
}

/// Noon on the given day of January 2026.
pub fn day(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, day, 12, 0, 0).unwrap()
}
