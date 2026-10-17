//! A stand-in for text-embeddings-inference with real TEI's API shape: `/info` reports the
//! maximum input length but no dimension, `/embed` takes `inputs`, `normalize` and `truncate`,
//! and rejects over-long inputs with a 422 unless truncation is allowed.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

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

/// Words of one concept share a dimension, so a query and a document about the same thing are
/// similar without sharing a word.
const CONCEPTS: &[&[&str]] = &[
    &["car", "vehicle", "automobile", "fahrzeug"],
    &["invoice", "bill", "rechnung"],
    &["holiday", "vacation", "urlaub"],
];

#[derive(Clone, Copy)]
pub enum Similarity {
    /// No query is similar to any document: the vector half finds nothing, so keyword behaviour
    /// shows unaltered.
    None,
    /// Texts are similar when they share words or concepts.
    Words,
}

#[derive(Clone)]
pub struct FakeTei {
    pub url: String,
    embedded: Arc<Mutex<Vec<String>>>,
    broken: Arc<AtomicBool>,
}

#[derive(Clone)]
struct Fake {
    similarity: Similarity,
    max_input_length: usize,
    embedded: Arc<Mutex<Vec<String>>>,
    broken: Arc<AtomicBool>,
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
    pub async fn start(similarity: Similarity, max_input_length: usize) -> Self {
        let embedded = Arc::new(Mutex::new(Vec::new()));
        let broken = Arc::new(AtomicBool::new(false));
        let fake = Fake {
            similarity,
            max_input_length,
            embedded: embedded.clone(),
            broken: broken.clone(),
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
            broken,
        }
    }

    /// From now on, every embed request fails with a 503, like a TEI that is restarting.
    pub fn break_down(&self) {
        self.broken.store(true, Ordering::SeqCst);
    }

    /// Every text embedded so far, in order.
    pub fn embedded(&self) -> Vec<String> {
        self.embedded.lock().unwrap().clone()
    }
}

async fn info(State(fake): State<Fake>) -> Json<Value> {
    Json(json!({
        "model_id": "fake/fake-embedding",
        "model_type": {"embedding": {"pooling": "last_token"}},
        "max_input_length": fake.max_input_length,
        "max_client_batch_size": 32,
        "auto_truncate": true,
    }))
}

async fn embed(
    State(fake): State<Fake>,
    Json(request): Json<EmbedRequest>,
) -> Result<Json<Vec<Vec<f32>>>, (StatusCode, Json<Value>)> {
    if fake.broken.load(Ordering::SeqCst) {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "model is overloaded", "error_type": "Overloaded"})),
        ));
    }
    let inputs = match request.inputs {
        Inputs::One(input) => vec![input],
        Inputs::Many(inputs) => inputs,
    };
    // Words for tokens, close enough for a fake.
    if request.truncate == Some(false)
        && inputs
            .iter()
            .any(|input| words(input).count() > fake.max_input_length)
    {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(
                json!({"error": "Input validation error: too many tokens", "error_type": "Validation"}),
            ),
        ));
    }
    fake.embedded.lock().unwrap().extend(inputs.iter().cloned());
    Ok(Json(
        inputs
            .iter()
            .map(|input| vector(fake.similarity, input))
            .collect(),
    ))
}

fn vector(similarity: Similarity, text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; DIMENSION];
    match similarity {
        Similarity::None => {
            let axis = usize::from(text.starts_with(super::QUERY_PREFIX));
            vector[axis] = 1.0;
        }
        Similarity::Words => {
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
        }
    }
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
