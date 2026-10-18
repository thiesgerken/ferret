//! A client for OpenAI-compatible embeddings endpoints: TEI, proxies like LiteLLM, cloud APIs.
//! One request shape for all of them. Every answer is checked before anything is stored.

use std::{
    fmt,
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use reqwest::{StatusCode, header::RETRY_AFTER};
use serde::{Deserialize, Serialize};

use crate::{StartupError, config::ApiConfig};

/// How long to wait after a 429 without `Retry-After`.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(30);

/// One configured endpoint, with how its requests went lately in this process.
pub struct Endpoint {
    pub url: String,
    /// What the endpoint calls the model.
    pub model: String,
    token: Option<String>,
    health: Mutex<Health>,
}

/// How an endpoint's last request went.
#[derive(Clone, Debug, Default)]
pub struct Health {
    /// Whether it answered at all, `None` before the first request.
    pub reachable: Option<bool>,
    /// The last failure, until the next success.
    pub last_error: Option<(String, DateTime<Utc>)>,
    /// The kind of the last failure, see `EndpointError::kind`.
    failing: Option<&'static str>,
    /// Until when the endpoint asked for a pause (a 429's `Retry-After`).
    paused_until: Option<Instant>,
}

#[derive(Debug)]
pub enum EndpointError {
    /// No answer: refused, unresolvable or timed out.
    Unreachable(reqwest::Error),
    /// A 429. Nothing is wrong with the inputs, the endpoint wants a pause.
    RateLimited(Duration),
    /// A 400, 413 or 422: the endpoint refused the inputs themselves (too long, untokenizable).
    Rejected(String),
    /// Any other error status.
    Failed(String),
    /// An answer that fails the checks or doesn't parse, so nothing of it is stored.
    Invalid(String),
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(error) => write!(f, "unreachable: {error}"),
            Self::RateLimited(wait) => write!(f, "rate limited, retrying in {}s", wait.as_secs()),
            Self::Rejected(message) | Self::Failed(message) => f.write_str(message),
            Self::Invalid(message) => write!(f, "invalid answer: {message}"),
        }
    }
}

impl std::error::Error for EndpointError {}

impl EndpointError {
    /// What went wrong, without the details that differ from one retry to the next.
    fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "unreachable",
            Self::RateLimited(_) => "rate limited",
            Self::Rejected(_) => "rejected inputs",
            Self::Failed(_) => "failed",
            Self::Invalid(_) => "invalid answer",
        }
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Unreachable(error) if error.is_timeout())
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest<'a> {
    model: &'a str,
    input: &'a [String],
    encoding_format: &'static str,
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

/// A client for every endpoint. Requests set their own total timeout: an endpoint that accepts
/// connections but stops answering must not hang a search or the pump.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("an HTTP client with a connect timeout builds")
}

impl Endpoint {
    pub fn new(config: &ApiConfig) -> Result<Self, StartupError> {
        Ok(Self {
            url: config.url.trim_end_matches('/').to_owned(),
            model: config.model.clone(),
            token: config.token()?,
            health: Mutex::default(),
        })
    }

    pub fn health(&self) -> Health {
        self.health
            .lock()
            .expect("no panics while updating")
            .clone()
    }

    /// Until when the endpoint asked for a pause. The pump waits, a search tries anyway.
    pub fn paused_until(&self) -> Option<Instant> {
        self.health
            .lock()
            .expect("no panics while updating")
            .paused_until
            .filter(|until| *until > Instant::now())
    }

    /// The vectors of `texts` in their order, each `dimension` long, finite and not zero.
    /// Records how the request went.
    pub async fn embed(
        &self,
        http: &reqwest::Client,
        texts: &[String],
        dimension: usize,
        timeout: Duration,
    ) -> Result<Vec<Vec<f32>>, EndpointError> {
        let result = self.embed_unrecorded(http, texts, dimension, timeout).await;
        match &result {
            Ok(_) => self.succeeded(),
            Err(error) => self.failed(error),
        }
        result
    }

    /// Like `embed`, but leaves the endpoint's health alone: an admin's test must not change what
    /// the pump and Admin see.
    pub async fn embed_unrecorded(
        &self,
        http: &reqwest::Client,
        texts: &[String],
        dimension: usize,
        timeout: Duration,
    ) -> Result<Vec<Vec<f32>>, EndpointError> {
        self.request(http, texts, timeout)
            .await
            .and_then(|response| {
                check(response, texts.len(), dimension).map_err(EndpointError::Invalid)
            })
    }

    async fn request(
        &self,
        http: &reqwest::Client,
        texts: &[String],
        timeout: Duration,
    ) -> Result<EmbeddingsResponse, EndpointError> {
        let mut request = http
            .post(format!("{}/embeddings", self.url))
            .timeout(timeout)
            .json(&EmbeddingsRequest {
                model: &self.model,
                input: texts,
                encoding_format: "float",
            });
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(EndpointError::Unreachable)?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(EndpointError::RateLimited(retry_after(&response)));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = format!("{status}: {}", body.chars().take(500).collect::<String>());
            return Err(match status {
                StatusCode::BAD_REQUEST
                | StatusCode::PAYLOAD_TOO_LARGE
                | StatusCode::UNPROCESSABLE_ENTITY => EndpointError::Rejected(message),
                _ => EndpointError::Failed(message),
            });
        }
        response.json().await.map_err(|error| {
            if error.is_timeout() {
                EndpointError::Unreachable(error)
            } else {
                EndpointError::Invalid(error.to_string())
            }
        })
    }

    fn succeeded(&self) {
        let mut health = self.health.lock().expect("no panics while updating");
        if health.last_error.is_some() {
            tracing::info!(url = %self.url, model = %self.model, "the embeddings endpoint answers again");
        }
        *health = Health {
            reachable: Some(true),
            paused_until: health.paused_until,
            ..Health::default()
        };
    }

    /// Shows `error` in Admin until the next success. Logs only when the kind of failure
    /// changes: an endpoint that is down fails every retry.
    pub fn failed(&self, error: &EndpointError) {
        let mut health = self.health.lock().expect("no panics while updating");
        let kind = error.kind();
        if health.failing != Some(kind) {
            match error {
                EndpointError::RateLimited(_) => {
                    tracing::info!(url = %self.url, model = %self.model, %error, "the embeddings endpoint asks for a pause")
                }
                _ => {
                    tracing::warn!(url = %self.url, model = %self.model, %error, "the embeddings endpoint fails")
                }
            }
        }
        let paused_until = match error {
            EndpointError::RateLimited(wait) => Some(Instant::now() + *wait),
            _ => health.paused_until,
        };
        *health = Health {
            reachable: Some(!matches!(error, EndpointError::Unreachable(_))),
            last_error: Some((error.to_string(), Utc::now())),
            failing: Some(kind),
            paused_until,
        };
    }
}

/// `Retry-After` in seconds. The HTTP-date form is rare for APIs and gets the default.
fn retry_after(response: &reqwest::Response) -> Duration {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(DEFAULT_RETRY_AFTER, Duration::from_secs)
}

/// The vectors in input order, if the answer has exactly one per input, each of `dimension`
/// finite values with a norm above 0. The answer's `model` isn't checked: proxies and cloud APIs
/// name models their own way.
fn check(
    response: EmbeddingsResponse,
    inputs: usize,
    dimension: usize,
) -> Result<Vec<Vec<f32>>, String> {
    if response.data.len() != inputs {
        return Err(format!(
            "{} vectors for {inputs} inputs",
            response.data.len()
        ));
    }
    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; inputs];
    for data in response.data {
        let slot = vectors
            .get_mut(data.index)
            .ok_or_else(|| format!("index {} for {inputs} inputs", data.index))?;
        if slot.replace(data.embedding).is_some() {
            return Err(format!("index {} twice", data.index));
        }
    }
    let vectors: Vec<Vec<f32>> = vectors
        .into_iter()
        .map(|vector| vector.expect("every index once: as many entries as inputs"))
        .collect();
    for vector in &vectors {
        if vector.len() != dimension {
            return Err(format!(
                "{} dimensions instead of {dimension}",
                vector.len()
            ));
        }
        if !vector.iter().all(|value| value.is_finite()) {
            return Err("a vector with values that aren't finite".to_owned());
        }
        if vector.iter().all(|value| *value == 0.0) {
            return Err("a vector of zeros".to_owned());
        }
    }
    Ok(vectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(vectors: &[(usize, Vec<f32>)]) -> EmbeddingsResponse {
        EmbeddingsResponse {
            data: vectors
                .iter()
                .map(|(index, embedding)| EmbeddingData {
                    index: *index,
                    embedding: embedding.clone(),
                })
                .collect(),
        }
    }

    fn too_many_requests(retry_after: Option<&str>) -> reqwest::Response {
        let mut response = axum::http::Response::builder().status(429);
        if let Some(retry_after) = retry_after {
            response = response.header(RETRY_AFTER, retry_after);
        }
        response.body("").unwrap().into()
    }

    #[test]
    fn a_429_waits_for_retry_after_in_seconds_or_30_s() {
        assert_eq!(
            retry_after(&too_many_requests(Some("7"))),
            Duration::from_secs(7)
        );
        for header in [None, Some("Wed, 21 Oct 2026 07:28:00 GMT")] {
            assert_eq!(retry_after(&too_many_requests(header)), DEFAULT_RETRY_AFTER);
        }
    }

    #[test]
    fn vectors_come_back_in_input_order() {
        let checked = check(response(&[(1, vec![0.0, 1.0]), (0, vec![1.0, 0.0])]), 2, 2);

        assert_eq!(checked, Ok(vec![vec![1.0, 0.0], vec![0.0, 1.0]]));
    }

    #[test]
    fn answers_failing_a_check_are_refused() {
        for (answer, inputs, why) in [
            (response(&[(0, vec![1.0, 0.0])]), 2, "too few vectors"),
            (
                response(&[(0, vec![1.0, 0.0]), (0, vec![0.0, 1.0])]),
                2,
                "an index twice",
            ),
            (
                response(&[(0, vec![1.0, 0.0]), (2, vec![0.0, 1.0])]),
                2,
                "an index out of range",
            ),
            (
                response(&[(0, vec![1.0, 0.0, 0.0])]),
                1,
                "another dimension",
            ),
            (response(&[(0, vec![f32::NAN, 1.0])]), 1, "NaN"),
            (response(&[(0, vec![f32::INFINITY, 1.0])]), 1, "infinity"),
            (response(&[(0, vec![0.0, 0.0])]), 1, "zero norm"),
        ] {
            assert!(check(answer, inputs, 2).is_err(), "{why}");
        }
    }
}
