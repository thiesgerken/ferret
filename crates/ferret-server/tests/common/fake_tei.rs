//! A stand-in for text-embeddings-inference with real TEI's API shape: `/info` reports the model
//! and the maximum input length but no dimension, `/v1/embeddings` (OpenAI's shape) names the model
//! in its answer, rejects over-long inputs with a 413 like TEI without `--auto-truncate`, and
//! inputs it can't tokenize with a 422.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;

pub const DIMENSION: usize = 256;
pub const MAX_INPUT_LENGTH: usize = 4096;
/// What [`FakeTei::start`] serves.
pub const FAKE_MODEL_ID: &str = "fake/fake-embedding";
const MAX_CLIENT_BATCH_SIZE: usize = 32;
/// TEI rejects every batch with an input containing this word, like one it can't tokenize.
pub const UNEMBEDDABLE: &str = "unembeddable";

/// Words of one concept share a dimension, so a query and a document about the same thing are
/// similar without sharing a word.
const CONCEPTS: &[&[&str]] = &[
    &["car", "vehicle", "automobile", "fahrzeug"],
    &["invoice", "bill", "rechnung"],
    &["holiday", "vacation", "urlaub"],
];

#[derive(Clone, Copy)]
enum Health {
    Up,
    /// Fails every request with a 503, like a TEI that is restarting.
    Down,
    /// Never answers, like a TEI that is stuck.
    Hanging,
}

/// Texts are similar when they share words or concepts.
#[derive(Clone)]
pub struct FakeTei {
    pub url: String,
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    model_id: String,
    /// The model embeddings come back under, when it isn't the one `/info` reports.
    answers_as: Option<String>,
    max_input_length: usize,
    embedded: Vec<String>,
    health: Health,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct EmbeddingsRequest {
    input: Input,
}

impl FakeTei {
    /// A TEI serving [`FAKE_MODEL_ID`].
    pub async fn start(max_input_length: usize) -> Self {
        Self::serving(FAKE_MODEL_ID, max_input_length).await
    }

    pub async fn serving(model_id: &str, max_input_length: usize) -> Self {
        let state = Arc::new(Mutex::new(FakeState {
            model_id: model_id.to_owned(),
            answers_as: None,
            max_input_length,
            embedded: Vec::new(),
            health: Health::Up,
        }));
        let app = Router::new()
            .route("/info", get(info))
            .route("/v1/embeddings", post(embeddings))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, state }
    }

    pub fn model_id(&self) -> String {
        self.state.lock().unwrap().model_id.clone()
    }

    /// Loads another model, like a laptop's TEI restarted with it.
    pub fn serve(&self, model_id: &str) {
        self.state.lock().unwrap().model_id = model_id.to_owned();
    }

    /// From now on, embeddings come back under `model`, while `/info` still reports the old one:
    /// like a TEI that switched models between two of the server's looks.
    pub fn answer_as(&self, model: &str) {
        self.state.lock().unwrap().answers_as = Some(model.to_owned());
    }

    /// From now on, every request fails with a 503, like a TEI that is restarting.
    pub fn break_down(&self) {
        self.state.lock().unwrap().health = Health::Down;
    }

    /// From now on, no request gets an answer, like a TEI that is stuck.
    pub fn hang(&self) {
        self.state.lock().unwrap().health = Health::Hanging;
    }

    /// Every text embedded so far, in order.
    pub fn embedded(&self) -> Vec<String> {
        self.state.lock().unwrap().embedded.clone()
    }
}

async fn wait_unless_up(state: &Mutex<FakeState>) -> Result<(), StatusCode> {
    let health = state.lock().unwrap().health;
    match health {
        Health::Up => Ok(()),
        Health::Down => Err(StatusCode::SERVICE_UNAVAILABLE),
        Health::Hanging => std::future::pending().await,
    }
}

async fn info(State(state): State<Arc<Mutex<FakeState>>>) -> Result<Json<Value>, StatusCode> {
    wait_unless_up(&state).await?;
    let state = state.lock().unwrap();
    Ok(Json(json!({
        "model_id": state.model_id,
        "served_model_name": state.model_id,
        "model_type": {"embedding": {"pooling": "last_token"}},
        "max_input_length": state.max_input_length,
        "max_client_batch_size": MAX_CLIENT_BATCH_SIZE,
        "auto_truncate": false,
    })))
}

async fn embeddings(
    State(state): State<Arc<Mutex<FakeState>>>,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    wait_unless_up(&state).await.map_err(|status| {
        (
            status,
            Json(json!({"message": "model is overloaded", "type": "unhealthy"})),
        )
    })?;
    let inputs = match request.input {
        Input::One(input) => vec![input],
        Input::Many(inputs) => inputs,
    };
    let mut state = state.lock().unwrap();
    // Words for tokens, close enough for a fake.
    let too_long = |input: &String| words(input).count() > state.max_input_length;
    if inputs.len() > MAX_CLIENT_BATCH_SIZE || inputs.iter().any(too_long) {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"message": "Input validation error", "type": "validation"})),
        ));
    }
    if inputs.iter().any(|input| input.contains(UNEMBEDDABLE)) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"message": "Tokenization error", "type": "tokenizer"})),
        ));
    }
    state.embedded.extend(inputs.iter().cloned());
    let model = state.answers_as.clone().unwrap_or(state.model_id.clone());
    let data: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| json!({"object": "embedding", "embedding": vector(input), "index": index}))
        .collect();
    Ok(Json(json!({
        "object": "list",
        "data": data,
        "model": model,
        "usage": {"prompt_tokens": 0, "total_tokens": 0},
    })))
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
