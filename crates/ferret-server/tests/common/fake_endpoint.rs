//! A stand-in for an OpenAI-compatible embeddings endpoint: `POST /v1/embeddings` rejects
//! over-long inputs with a 413 like TEI with `--auto-truncate false`, and inputs it can't tokenize
//! with a 422. It can also want a bearer token, rate-limit, fail, hang
//! and answer with broken vectors.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;

pub const DIMENSION: usize = 256;
/// Words per input at most, standing in for tokens.
const MAX_INPUT_LENGTH: usize = 4096;
/// The endpoint rejects every batch with an input containing this word, like one it can't
/// tokenize.
pub const UNEMBEDDABLE: &str = "unembeddable";

/// Words of one concept share a dimension, so a query and a document about the same thing are
/// similar without sharing a word.
const CONCEPTS: &[&[&str]] = &[
    &["car", "vehicle", "automobile", "fahrzeug"],
    &["invoice", "bill", "rechnung"],
    &["holiday", "vacation", "urlaub"],
];

#[derive(Clone, Copy, Default)]
enum Health {
    #[default]
    Up,
    /// Fails every request with a 503, like a TEI that is restarting.
    Down,
    /// Never answers, like a TEI that is stuck.
    Hanging,
    /// Answers every request with a 400, like a proxy asked for a model it doesn't know.
    UnknownModel,
}

/// How answers are broken.
#[derive(Clone, Copy, Debug)]
pub enum Broken {
    /// Vectors with one value more than `DIMENSION`.
    Dimension,
    /// A value in every vector that isn't a number.
    NotFinite,
    /// Every vector all zeros.
    Zero,
    /// One vector fewer than inputs.
    MissingVector,
}

/// Texts are similar when they share words or concepts.
#[derive(Clone)]
pub struct FakeEndpoint {
    url: String,
    state: Arc<Mutex<FakeState>>,
}

#[derive(Default)]
struct FakeState {
    embedded: Vec<String>,
    /// The `model` of every request, in order.
    models: Vec<String>,
    health: Health,
    /// Wanted as `Authorization: Bearer <token>`.
    token: Option<String>,
    /// Requests until then are answered with a 429 and a `Retry-After` up to then.
    rate_limited_until: Option<Instant>,
    /// How many requests got a 429.
    rate_limited: usize,
    broken: Option<Broken>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct EmbeddingsRequest {
    model: String,
    input: Input,
    encoding_format: String,
}

impl FakeEndpoint {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let app = Router::new()
            .route("/v1/embeddings", post(embeddings))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, state }
    }

    /// What a config's `url` names: the base URL up to the version.
    pub fn api_url(&self) -> String {
        format!("{}/v1", self.url)
    }

    /// From now on, requests without `Authorization: Bearer <token>` fail with a 401.
    pub fn require_token(&self, token: &str) {
        self.state.lock().unwrap().token = Some(token.to_owned());
    }

    /// For `duration`, every request fails with a 429 whose `Retry-After` names the seconds left.
    pub fn rate_limit_for(&self, duration: Duration) {
        self.state.lock().unwrap().rate_limited_until = Some(Instant::now() + duration);
    }

    /// How many requests got a 429 so far.
    pub fn rate_limited(&self) -> usize {
        self.state.lock().unwrap().rate_limited
    }

    /// From now on, answers are broken this way.
    pub fn break_answers(&self, broken: Broken) {
        self.state.lock().unwrap().broken = Some(broken);
    }

    /// From now on, every request fails with a 503, like a TEI that is restarting.
    pub fn break_down(&self) {
        self.state.lock().unwrap().health = Health::Down;
    }

    /// From now on, requests are answered again, and correctly.
    pub fn recover(&self) {
        let mut state = self.state.lock().unwrap();
        state.health = Health::Up;
        state.broken = None;
    }

    /// From now on, every request fails with a 400, like a proxy asked for a model it doesn't
    /// know.
    pub fn forget_the_model(&self) {
        self.state.lock().unwrap().health = Health::UnknownModel;
    }

    /// From now on, no request gets an answer, like a TEI that is stuck.
    pub fn hang(&self) {
        self.state.lock().unwrap().health = Health::Hanging;
    }

    /// Every text embedded so far, in order.
    pub fn embedded(&self) -> Vec<String> {
        self.state.lock().unwrap().embedded.clone()
    }

    /// The `model` of every request so far, in order.
    pub fn models(&self) -> Vec<String> {
        self.state.lock().unwrap().models.clone()
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": {"message": message}}))).into_response()
}

async fn embeddings(
    State(state): State<Arc<Mutex<FakeState>>>,
    headers: HeaderMap,
    Json(request): Json<EmbeddingsRequest>,
) -> Response {
    let health = state.lock().unwrap().health;
    match health {
        Health::Up => {}
        Health::Down => return error(StatusCode::SERVICE_UNAVAILABLE, "model is overloaded"),
        Health::Hanging => std::future::pending().await,
        Health::UnknownModel => {
            return error(
                StatusCode::BAD_REQUEST,
                "Invalid model name passed in model=fake/fake-embedding",
            );
        }
    }
    let mut state = state.lock().unwrap();
    state.models.push(request.model);
    if let Some(token) = &state.token {
        let sent = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if sent != Some(format!("Bearer {token}").as_str()) {
            return error(StatusCode::UNAUTHORIZED, "invalid token");
        }
    }
    if let Some(left) = state
        .rate_limited_until
        .and_then(|until| until.checked_duration_since(Instant::now()))
    {
        state.rate_limited += 1;
        let mut response = error(StatusCode::TOO_MANY_REQUESTS, "slow down");
        response.headers_mut().insert(
            header::RETRY_AFTER,
            left.as_millis().div_ceil(1000).to_string().parse().unwrap(),
        );
        return response;
    }
    assert_eq!(request.encoding_format, "float");
    let inputs = match request.input {
        Input::One(input) => vec![input],
        Input::Many(inputs) => inputs,
    };
    // Words for tokens, close enough for a fake.
    if inputs
        .iter()
        .any(|input| words(input).count() > MAX_INPUT_LENGTH)
    {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "Input validation error");
    }
    if inputs.iter().any(|input| input.contains(UNEMBEDDABLE)) {
        return error(StatusCode::UNPROCESSABLE_ENTITY, "Tokenization error");
    }
    state.embedded.extend(inputs.iter().cloned());
    let broken = state.broken;
    let mut data: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let mut vector = vector(input);
            match broken {
                Some(Broken::Dimension) => vector.push(0.5),
                Some(Broken::Zero) => vector.iter_mut().for_each(|value| *value = 0.0),
                Some(Broken::NotFinite | Broken::MissingVector) | None => {}
            }
            let mut embedding = json!(vector);
            // JSON has no NaN: a broken proxy's `"NaN"` instead of a number.
            if matches!(broken, Some(Broken::NotFinite)) {
                embedding[0] = json!("NaN");
            }
            json!({"object": "embedding", "embedding": embedding, "index": index})
        })
        .collect();
    if matches!(broken, Some(Broken::MissingVector)) {
        data.pop();
    }
    // Reversed: endpoints may answer out of order, `index` says which input a vector is for.
    data.reverse();
    Json(json!({
        "object": "list",
        "data": data,
        "model": "whatever-the-proxy-calls-it",
        "usage": {"prompt_tokens": 0, "total_tokens": 0},
    }))
    .into_response()
}

fn vector(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; DIMENSION];
    for word in words(text) {
        let word = word.to_lowercase();
        let axis = CONCEPTS
            .iter()
            .position(|concept| concept.contains(&word.as_str()))
            .unwrap_or_else(|| CONCEPTS.len() + fnv(&word) % (DIMENSION - CONCEPTS.len()));
        vector[axis] += 1.0;
    }
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    vector.iter_mut().for_each(|value| *value /= norm);
    vector
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
}

/// A stable hash, unlike std's, so similarities don't change with the toolchain.
fn fnv(word: &str) -> usize {
    word.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    }) as usize
}
