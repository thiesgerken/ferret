//! A client for Hugging Face text-embeddings-inference (TEI).

use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

/// What a TEI says about itself.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Info {
    /// The Hugging Face model it serves.
    pub model_id: String,
    /// The name its embeddings come back under, the model id unless TEI was started with
    /// `--served-model-name`. TEIs before 1.7 don't report it and answer with the model id.
    #[serde(default)]
    served_model_name: Option<String>,
    /// In tokens.
    pub max_input_length: usize,
    /// Inputs per request at most.
    pub max_client_batch_size: usize,
}

impl Info {
    pub fn served_model_name(&self) -> &str {
        self.served_model_name.as_deref().unwrap_or(&self.model_id)
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest<'a> {
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingData>,
    model: String,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

/// Embeddings as TEI answered them, and the model it says it computed them with.
pub struct Embedded {
    pub model: String,
    /// In the order of the inputs.
    pub vectors: Vec<Vec<f32>>,
}

/// A client for every TEI. Requests set their own total timeout: a TEI that accepts connections
/// but stops answering must not hang a search or the pump.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("an HTTP client with a connect timeout builds")
}

pub async fn info(
    http: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<Info, reqwest::Error> {
    http.get(format!("{url}/info"))
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

/// Normalized embeddings of `texts` from the OpenAI-compatible route, whose answer names the
/// model that computed them. TEI started without `--auto-truncate` rejects an over-long text with
/// a 413, failing the whole request.
pub async fn embed(
    http: &reqwest::Client,
    url: &str,
    texts: &[String],
    timeout: Duration,
) -> Result<Embedded, reqwest::Error> {
    let response: EmbeddingsResponse = http
        .post(format!("{url}/v1/embeddings"))
        .timeout(timeout)
        .json(&EmbeddingsRequest { input: texts })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut data = response.data;
    data.sort_by_key(|data| data.index);
    Ok(Embedded {
        model: response.model,
        vectors: data.into_iter().map(|data| data.embedding).collect(),
    })
}

/// TEI refused the inputs themselves (too long, untokenizable), as opposed to failing, being
/// overloaded or serving no embedding model.
pub fn is_rejection(error: &reqwest::Error) -> bool {
    error.status().is_some_and(|status| {
        status == StatusCode::PAYLOAD_TOO_LARGE || status == StatusCode::UNPROCESSABLE_ENTITY
    })
}

/// The counters and gauges of TEI's Prometheus metrics that its load is read from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    /// `te_queue_size`: inputs waiting.
    pub queue_size: f64,
    /// `te_embed_success`: inputs embedded since TEI started.
    pub embedded: f64,
    /// `te_request_failure`, all reasons: failed requests since TEI started.
    pub failures: f64,
    /// `te_embed_inference_duration_sum`: seconds, recorded for every input with its batch's
    /// inference time.
    pub inference_seconds: f64,
    /// `te_batch_next_size_sum` and `_count`: inputs and inference batches since TEI started.
    pub batched_inputs: f64,
    pub batches: f64,
}

/// TEI's `/metrics` at `url`, `None` if it doesn't answer within `timeout` or lacks them.
pub async fn metrics(http: &reqwest::Client, url: &str, timeout: Duration) -> Option<Metrics> {
    let text = http
        .get(format!("{url}/metrics"))
        .timeout(timeout)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    parse_metrics(&text)
}

/// Prometheus' text format, summed over each metric's labels.
fn parse_metrics(text: &str) -> Option<Metrics> {
    let mut values = std::collections::HashMap::<&str, f64>::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let Some((series, value)) = line.rsplit_once(' ') else {
            continue;
        };
        let name = series.split('{').next().unwrap_or(series);
        if let Ok(value) = value.parse::<f64>() {
            *values.entry(name).or_default() += value;
        }
    }
    Some(Metrics {
        queue_size: *values.get("te_queue_size")?,
        embedded: *values.get("te_embed_success")?,
        failures: values.get("te_request_failure").copied().unwrap_or(0.0),
        inference_seconds: *values.get("te_embed_inference_duration_sum")?,
        batched_inputs: *values.get("te_batch_next_size_sum")?,
        batches: *values.get("te_batch_next_size_count")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_are_summed_over_labels() {
        let text = "# HELP te_queue_size x\n\
                    te_request_failure{err=\"dropped\"} 29\n\
                    te_request_failure{err=\"validation\"} 1\n\
                    te_embed_success 513\n\
                    te_queue_size 4\n\
                    te_embed_inference_duration_sum 4497.0\n\
                    te_embed_inference_duration_bucket{le=\"1\"} 3\n\
                    te_batch_next_size_sum 540\n\
                    te_batch_next_size_count 317\n";
        assert_eq!(
            parse_metrics(text),
            Some(Metrics {
                queue_size: 4.0,
                embedded: 513.0,
                failures: 30.0,
                inference_seconds: 4497.0,
                batched_inputs: 540.0,
                batches: 317.0,
            })
        );
        assert_eq!(parse_metrics("te_queue_size 0\n"), None);
    }
}
