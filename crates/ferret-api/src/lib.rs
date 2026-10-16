//! Shared request and response types of ferret's REST and internal APIs, plus a small HTTP
//! client for them.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user, as `GET /api/users` lists them.
#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub enabled: bool,
}

/// A client for ferret's public REST API.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl Client {
    /// `base_url` is the server's origin without a trailing slash, e.g. `http://localhost:8380`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: None,
        }
    }

    /// Authenticates every `/api` call with `api_key` (`fk_...`).
    pub fn with_api_key(self, api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            ..self
        }
    }

    /// Succeeds when the server is up and reaches its database.
    pub async fn health(&self) -> Result<(), reqwest::Error> {
        self.http
            .get(format!("{}/healthz", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Every user. Needs the `admin:read` scope.
    pub async fn users(&self) -> Result<Vec<User>, reqwest::Error> {
        self.api(reqwest::Method::GET, "/api/users")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    fn api(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let request = self
            .http
            .request(method, format!("{}{path}", self.base_url));
        match &self.api_key {
            Some(api_key) => request.bearer_auth(api_key),
            None => request,
        }
    }
}
