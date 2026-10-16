//! A client for Hugging Face text-embeddings-inference (TEI).

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

/// The longest input in tokens the TEI at `url` accepts.
pub async fn max_input_length(http: &reqwest::Client, url: &str) -> Result<usize, reqwest::Error> {
    let info: Info = http
        .get(format!("{url}/info"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(info.max_input_length)
}

/// Normalized embeddings of `texts`, in order. An over-long text fails the whole request.
pub async fn embed(
    http: &reqwest::Client,
    url: &str,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, reqwest::Error> {
    http.post(format!("{url}/embed"))
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
