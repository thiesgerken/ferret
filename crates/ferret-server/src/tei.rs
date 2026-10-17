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

/// Whether the TEI at `url` answers within `timeout`. Asks `/info`, not `/health`: TEI runs its
/// health check on the thread that runs the embedding batches, so a TEI busy with a backfill
/// would look offline. TEI only listens once its model is loaded, and a stuck one shows up as
/// the embed pump's last error.
pub async fn healthy(http: &reqwest::Client, url: &str, timeout: Duration) -> bool {
    http.get(format!("{url}/info"))
        .timeout(timeout)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
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
