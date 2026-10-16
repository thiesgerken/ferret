//! Shared request and response types of ferret's REST and internal APIs, plus a small HTTP
//! client for them.

/// A client for ferret's public REST API.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
}

impl Client {
    /// `base_url` is the server's origin without a trailing slash, e.g. `http://localhost:8080`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    /// Succeeds when the server is up and reaches its database.
    pub async fn health(&self) -> Result<(), reqwest::Error> {
        self.http
            .get(format!("{}/api/health", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}
