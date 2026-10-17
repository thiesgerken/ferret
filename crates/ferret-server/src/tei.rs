//! A client for Hugging Face text-embeddings-inference (TEI).

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Info {
    max_input_length: usize,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
    normalize: bool,
    /// TEI truncates over-long inputs by default, which would embed a chunk's beginning only.
    truncate: bool,
}

/// A client for every TEI. Requests set their own total timeout: a TEI that accepts connections
/// but stops answering must not hang a search or the pump.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("an HTTP client with a connect timeout builds")
}

/// The longest input in tokens the TEI at `url` accepts.
pub async fn max_input_length(
    http: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<usize, reqwest::Error> {
    let info: Info = http
        .get(format!("{url}/info"))
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(info.max_input_length)
}

/// Normalized embeddings of `texts`, in order. An over-long text fails the whole request with a
/// 4xx.
pub async fn embed(
    http: &reqwest::Client,
    url: &str,
    texts: &[String],
    timeout: Duration,
) -> Result<Vec<Vec<f32>>, reqwest::Error> {
    http.post(format!("{url}/embed"))
        .timeout(timeout)
        .json(&EmbedRequest {
            inputs: texts,
            normalize: true,
            truncate: false,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

/// Whether the TEI at `url` answers its health check within `timeout`.
pub async fn healthy(http: &reqwest::Client, url: &str, timeout: Duration) -> bool {
    http.get(format!("{url}/health"))
        .timeout(timeout)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}
