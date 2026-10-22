//! The harness every black-box server test uses.
//!
//! Tests take their pool from `#[sqlx::test(migrations = false)]`, which gives each test a fresh
//! database in the ParadeDB that `DATABASE_URL` points at, and leaves migrating to the server's
//! own startup.

pub mod fake_endpoint;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, TimeZone, Utc};
use fake_endpoint::FakeEndpoint;
use ferret_api::internal::InternalClient;
use ferret_api::{Client, ContentType, DocumentPut};
use ferret_server::{
    AppState, ConfigKey, SessionKey, WorkerSecret,
    config::{ApiConfig, EmbeddingConfig, ModelConfig},
    embeddings,
};
use reqwest::{Response, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;

pub const ADMIN_PASSWORD: &str = "the bootstrap admin's password";
const SESSION_SECRET: &str = "a session secret of at least 32 bytes";
/// 32 bytes in base64.
const CONFIG_KEY: &str = "dGVzdCBjb25maWcga2V5IG9mIHRoaXJ0eS10d28gYnk=";
pub const MODEL: &str = "fake";
pub const QUERY_PREFIX: &str = "query: ";
pub const DOCUMENT_PREFIX: &str = "passage: ";
/// What the test models call their model at the endpoint.
pub const ENDPOINT_MODEL: &str = "fake/fake-embedding";
/// Generous, since a busy CI runner embeds slowly.
const POLL_BUDGET: Duration = Duration::from_secs(30);
pub const WORKER_SECRET: &str = "the workers' pre-shared key, 32+ bytes";

pub struct TestServer {
    /// The machine API's client.
    pub api: Client,
    /// The default model's endpoint.
    pub endpoint: FakeEndpoint,
    pool: PgPool,
    url: String,
    /// The plugin workers' API, on its own listener like in production.
    pub internal_url: String,
    http: reqwest::Client,
    /// Servers, pump and run wakeups. They hold pool connections (LISTEN ones for good), which
    /// must be back before `#[sqlx::test]` drops the test database.
    tasks: Vec<tokio::task::AbortHandle>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.tasks.iter().for_each(tokio::task::AbortHandle::abort);
    }
}

/// Starts the server in-process on a random port, the way `ferret` starts: migrations, the
/// bootstrap admin (`admin` / [`ADMIN_PASSWORD`]) and the embedding model, but without the
/// pump. Nothing gets embedded, so searches behave like keyword searches.
pub async fn start_server(pool: PgPool) -> TestServer {
    let endpoint = FakeEndpoint::start().await;
    let config = embedding_config(model(MODEL, &endpoint));
    start(pool, endpoint, config, false).await
}

/// Like [`start_server`], with the pump.
pub async fn start_semantic_server(pool: PgPool) -> TestServer {
    let endpoint = FakeEndpoint::start().await;
    let config = embedding_config(model(MODEL, &endpoint));
    start_server_with(pool, endpoint, config).await
}

/// Like [`start_semantic_server`], with the given models. `endpoint` is the default model's.
pub async fn start_server_with(
    pool: PgPool,
    endpoint: FakeEndpoint,
    embedding: EmbeddingConfig,
) -> TestServer {
    start(pool, endpoint, embedding, true).await
}

async fn start(
    pool: PgPool,
    endpoint: FakeEndpoint,
    embedding: EmbeddingConfig,
    pump: bool,
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
    let mut tasks = Vec::new();
    if pump {
        let pump = tokio::spawn(embeddings::run_pump(pool.clone(), embedding.clone()));
        tasks.push(pump.abort_handle());
    }
    let session_key = SessionKey::new(SESSION_SECRET).unwrap();
    let worker_secret = WorkerSecret::new(WORKER_SECRET.to_owned()).unwrap();
    let config_key = ConfigKey::new(CONFIG_KEY).unwrap();
    let state = AppState::new(
        pool.clone(),
        session_key,
        worker_secret,
        embedding,
        config_key,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let internal_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let internal_url = format!("http://{}", internal_listener.local_addr().unwrap());
    let wakeups = tokio::spawn(ferret_server::listen_for_due_runs(state.clone()));
    let internal = ferret_server::internal_router(state.clone());
    let internal =
        tokio::spawn(async move { axum::serve(internal_listener, internal).await.unwrap() });
    let app = ferret_server::router(state);
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tasks.extend([
        wakeups.abort_handle(),
        internal.abort_handle(),
        server.abort_handle(),
    ]);
    TestServer {
        api: Client::new(url.clone()),
        endpoint,
        pool,
        url,
        internal_url,
        http: reqwest::Client::new(),
        tasks,
    }
}

/// Polls `check` until it returns `Some`, and panics if that takes too long.
pub async fn eventually<T>(what: &str, mut check: impl AsyncFnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + POLL_BUDGET;
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "expected {what} within {POLL_BUDGET:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A model computed by `endpoint` for chunks and queries, with the test prefixes.
pub fn model(id: &str, endpoint: &FakeEndpoint) -> ModelConfig {
    ModelConfig {
        id: id.to_owned(),
        dimension: fake_endpoint::DIMENSION,
        query_prefix: QUERY_PREFIX.to_owned(),
        document_prefix: DOCUMENT_PREFIX.to_owned(),
        // The fake's vectors are never negative, so no candidate is dropped.
        min_similarity: 0.0,
        embedding_api: Some(api(endpoint)),
        query_api: None,
    }
}

/// `endpoint` as a config's endpoint.
pub fn api(endpoint: &FakeEndpoint) -> ApiConfig {
    ApiConfig {
        url: endpoint.api_url(),
        model: ENDPOINT_MODEL.to_owned(),
        token_env: None,
        concurrency: None,
    }
}

/// `model` as the only and default model.
pub fn embedding_config(model: ModelConfig) -> EmbeddingConfig {
    EmbeddingConfig {
        default: model.id.clone(),
        models: vec![model],
    }
}

impl TestServer {
    /// Waits until the pump has embedded `count` chunks (all models together).
    pub async fn wait_for_embeddings(&self, count: i64) {
        eventually(&format!("{count} embedded chunks"), async || {
            let embedded: i64 = sqlx::query_scalar("select count(*) from chunk_embeddings")
                .fetch_one(&self.pool)
                .await
                .unwrap();
            (embedded >= count).then_some(())
        })
        .await
    }

    /// The public API's origin.
    pub fn public_url(&self) -> String {
        self.url.clone()
    }

    /// A plugin worker's client for the internal API.
    pub fn internal(&self) -> InternalClient {
        InternalClient::new(self.internal_url.clone(), WORKER_SECRET)
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
                json!({"input": {"name": "test key", "icon": "key-round", "scopes": scopes, "sources": sources}}),
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
        created_at: Some(day(1)),
        modified_at: Some(day(1)),
        meta: Default::default(),
    }
}

/// Noon on the given day of January 2026.
pub fn day(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, day, 12, 0, 0).unwrap()
}
