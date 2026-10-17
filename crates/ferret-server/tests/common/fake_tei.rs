//! A stand-in for text-embeddings-inference with real TEI's API shape: `/info` reports the
//! maximum input length but no dimension, `/embed` takes `inputs`, `normalize` and `truncate`,
//! and rejects over-long inputs and oversized batches with a 422.

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
    /// Fails every embed request with a 503, like a TEI that is restarting.
    Down,
    /// Never answers an embed request, like a TEI that is stuck.
    Hanging,
}

/// Texts are similar when they share words or concepts.
#[derive(Clone)]
pub struct FakeTei {
    pub url: String,
    embedded: Arc<Mutex<Vec<String>>>,
    health: Arc<Mutex<Health>>,
}

#[derive(Clone)]
struct Fake {
    max_input_length: usize,
    embedded: Arc<Mutex<Vec<String>>>,
    health: Arc<Mutex<Health>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Inputs {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct EmbedRequest {
    inputs: Inputs,
    #[serde(default)]
    truncate: Option<bool>,
}

impl FakeTei {
    pub async fn start(max_input_length: usize) -> Self {
        let embedded = Arc::new(Mutex::new(Vec::new()));
        let health = Arc::new(Mutex::new(Health::Up));
        let fake = Fake {
            max_input_length,
            embedded: embedded.clone(),
            health: health.clone(),
        };
        let app = Router::new()
            .route("/info", get(info))
            .route("/embed", post(embed))
            .with_state(fake);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            embedded,
            health,
        }
    }

    /// From now on, every embed request fails with a 503, like a TEI that is restarting.
    pub fn break_down(&self) {
        *self.health.lock().unwrap() = Health::Down;
    }

    /// From now on, no embed request gets an answer, like a TEI that is stuck.
    pub fn hang(&self) {
        *self.health.lock().unwrap() = Health::Hanging;
    }

    /// Every text embedded so far, in order.
    pub fn embedded(&self) -> Vec<String> {
        self.embedded.lock().unwrap().clone()
    }
}

/// Like the embed requests: fine, a 503, or no answer.
async fn info(State(fake): State<Fake>) -> Result<Json<Value>, StatusCode> {
    let health = *fake.health.lock().unwrap();
    match health {
        Health::Up => {}
        Health::Down => return Err(StatusCode::SERVICE_UNAVAILABLE),
        Health::Hanging => std::future::pending().await,
    }
    Ok(Json(json!({
        "model_id": "fake/fake-embedding",
        "model_type": {"embedding": {"pooling": "last_token"}},
        "max_input_length": fake.max_input_length,
        "max_client_batch_size": MAX_CLIENT_BATCH_SIZE,
        "auto_truncate": true,
    })))
}

async fn embed(
    State(fake): State<Fake>,
    Json(request): Json<EmbedRequest>,
) -> Result<Json<Vec<Vec<f32>>>, (StatusCode, Json<Value>)> {
    let health = *fake.health.lock().unwrap();
    match health {
        Health::Up => {}
        Health::Down => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "model is overloaded", "error_type": "Overloaded"})),
            ));
        }
        Health::Hanging => std::future::pending().await,
    }
    let inputs = match request.inputs {
        Inputs::One(input) => vec![input],
        Inputs::Many(inputs) => inputs,
    };
    // Words for tokens, close enough for a fake.
    let too_long = |input: &String| words(input).count() > fake.max_input_length;
    if inputs.len() > MAX_CLIENT_BATCH_SIZE
        || inputs.iter().any(|input| input.contains(UNEMBEDDABLE))
        || (request.truncate == Some(false) && inputs.iter().any(too_long))
    {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "Input validation error", "error_type": "Validation"})),
        ));
    }
    fake.embedded.lock().unwrap().extend(inputs.iter().cloned());
    Ok(Json(inputs.iter().map(|input| vector(input)).collect()))
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
