//! The server's configuration file (`FERRET_CONFIG_FILE`). Secrets stay in environment variables.

use std::{collections::HashSet, path::Path};

use serde::Deserialize;

use crate::StartupError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub embedding: EmbeddingConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    /// The model searches use unless they ask for another one.
    pub default: String,
    pub models: Vec<ModelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// Stored with every embedding and part of its index name: `[a-z0-9][a-z0-9._-]*`, ending in
    /// a letter or digit, at most 40 characters.
    pub id: String,
    /// Of its vectors. The model's index needs it, and every answer is checked against it.
    pub dimension: usize,
    /// Asymmetric models expect an instruction or marker in front of queries and documents.
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default)]
    pub document_prefix: String,
    /// Semantic candidates less similar to the query (cosine) are dropped. Where noise begins
    /// depends on the model, see embeddings.md for how to calibrate it.
    pub min_similarity: f64,
    /// What computes the embeddings of chunks. `query_api` when absent.
    pub embedding_api: Option<ApiConfig>,
    /// What computes the embeddings of search queries. `embedding_api` when absent.
    pub query_api: Option<ApiConfig>,
}

/// An OpenAI-compatible embeddings endpoint: a TEI, a proxy like LiteLLM, or a cloud API.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    /// Up to the version, e.g. `http://tei:80/v1`: requests go to `{url}/embeddings`.
    pub url: String,
    /// What the endpoint calls the model.
    pub model: String,
    /// The environment variable holding a bearer token, for endpoints that want one.
    pub token_env: Option<String>,
    /// Embedding requests at once per server replica, 1 when absent. Only for the endpoint that
    /// embeds chunks.
    pub concurrency: Option<usize>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, StartupError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("can't read {}: {error}", path.display()))?;
        let config: Self = serde_saphyr::from_str(&text)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        config.embedding.validate()?;
        Ok(config)
    }
}

impl EmbeddingConfig {
    pub fn validate(&self) -> Result<(), StartupError> {
        let mut ids = HashSet::new();
        let mut slugs = HashSet::new();
        for model in &self.models {
            let id = &model.id;
            if !is_valid_id(id) {
                return Err(format!(
                    "embedding model id {id:?} must match [a-z0-9][a-z0-9._-]*, end in a letter \
                     or digit and have at most {MAX_ID_CHARS} characters"
                )
                .into());
            }
            if !(0.0..1.0).contains(&model.min_similarity) {
                return Err(format!(
                    "embedding model {id:?}: min_similarity must be at least 0 and below 1"
                )
                .into());
            }
            if !ids.insert(id.as_str()) {
                return Err(format!("embedding model {id:?} is configured twice").into());
            }
            if model.dimension == 0 {
                return Err(format!("embedding model {id:?}: dimension must be positive").into());
            }
            if !slugs.insert(model.slug()) {
                return Err(format!(
                    "embedding model {id:?} differs from another one only in `.`, `-` or `_`, \
                     which its index name can't tell apart"
                )
                .into());
            }
            model.validate_apis()?;
        }
        if !ids.contains(self.default.as_str()) {
            return Err(format!(
                "the default embedding model {:?} isn't configured",
                self.default
            )
            .into());
        }
        Ok(())
    }
}

/// Index names have room for this much of a model id: Postgres cuts identifiers at 63 bytes.
const MAX_ID_CHARS: usize = 40;

impl ModelConfig {
    /// The id as part of an identifier: `.` and `-` become `_`.
    pub fn slug(&self) -> String {
        self.id.replace(['.', '-'], "_")
    }

    /// The endpoint that embeds chunks.
    pub fn embedding_api(&self) -> &ApiConfig {
        self.embedding_api
            .as_ref()
            .or(self.query_api.as_ref())
            .expect("validated: a model has at least one endpoint")
    }

    /// The endpoint that embeds search queries.
    pub fn query_api(&self) -> &ApiConfig {
        self.query_api
            .as_ref()
            .or(self.embedding_api.as_ref())
            .expect("validated: a model has at least one endpoint")
    }

    fn validate_apis(&self) -> Result<(), StartupError> {
        let id = &self.id;
        if self.embedding_api.is_none() && self.query_api.is_none() {
            return Err(
                format!("embedding model {id:?} needs an embedding_api or a query_api").into(),
            );
        }
        if self.embedding_api.is_some()
            && self
                .query_api
                .as_ref()
                .is_some_and(|api| api.concurrency.is_some())
        {
            return Err(format!(
                "embedding model {id:?}: concurrency belongs on the embedding_api, the query_api \
                 doesn't embed chunks"
            )
            .into());
        }
        for api in [&self.embedding_api, &self.query_api].into_iter().flatten() {
            if api.concurrency == Some(0) {
                return Err(
                    format!("embedding model {id:?}: concurrency must be at least 1").into(),
                );
            }
            if api.url.trim().is_empty() || api.model.trim().is_empty() {
                return Err(
                    format!("embedding model {id:?}: an endpoint needs a url and a model").into(),
                );
            }
        }
        Ok(())
    }
}

impl ApiConfig {
    /// The bearer token from `token_env`'s variable. Fails when it's unset or empty: a typo
    /// would otherwise send requests without one.
    pub fn token(&self) -> Result<Option<String>, StartupError> {
        self.token_env
            .as_deref()
            .map(|name| token_from(name, std::env::var(name).ok()))
            .transpose()
    }
}

fn token_from(name: &str, value: Option<String>) -> Result<String, StartupError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            format!("the environment variable {name} (a token_env) is unset or empty").into()
        })
}

/// Ids end up in SQL identifiers and literals (index names, partial index predicates), so they
/// must not need quoting.
fn is_valid_id(id: &str) -> bool {
    let alphanumeric = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    id.len() <= MAX_ID_CHARS
        && id.starts_with(alphanumeric)
        && id.ends_with(alphanumeric)
        && id
            .chars()
            .all(|c| alphanumeric(c) || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(url: &str) -> ApiConfig {
        ApiConfig {
            url: url.to_owned(),
            model: "org/model".to_owned(),
            token_env: None,
            concurrency: None,
        }
    }

    fn models(ids: &[&str], default: &str) -> EmbeddingConfig {
        EmbeddingConfig {
            default: default.to_owned(),
            models: ids
                .iter()
                .map(|id| ModelConfig {
                    id: id.to_string(),
                    dimension: 1024,
                    query_prefix: String::new(),
                    document_prefix: String::new(),
                    min_similarity: 0.5,
                    embedding_api: Some(api("http://tei/v1")),
                    query_api: None,
                })
                .collect(),
        }
    }

    #[test]
    fn parses_the_documented_shape() {
        let config: Config = serde_saphyr::from_str(
            r#"
embedding:
  default: qwen3-embedding-0.6b
  models:
    - id: qwen3-embedding-0.6b
      dimension: 1024
      query_prefix: "Instruct: Given a search query\nQuery: "
      min_similarity: 0.4
      embedding_api:
        url: http://litellm:4000/v1
        model: qwen3-embed
        token_env: FERRET_LITELLM_TOKEN
        concurrency: 4
      query_api:
        url: http://tei:80/v1
        model: Qwen/Qwen3-Embedding-0.6B
    - id: bge-m3
      dimension: 1024
      min_similarity: 0.5
      query_api:
        url: http://tei-bge:80/v1
        model: BAAI/bge-m3
"#,
        )
        .unwrap();

        config.embedding.validate().unwrap();
        let qwen3 = &config.embedding.models[0];
        assert_eq!(qwen3.embedding_api().url, "http://litellm:4000/v1");
        assert_eq!(qwen3.embedding_api().concurrency, Some(4));
        assert_eq!(
            qwen3.embedding_api().token_env.as_deref(),
            Some("FERRET_LITELLM_TOKEN")
        );
        assert_eq!(qwen3.query_api().model, "Qwen/Qwen3-Embedding-0.6B");
        assert_eq!(
            qwen3.query_prefix,
            "Instruct: Given a search query\nQuery: "
        );
        assert_eq!(qwen3.document_prefix, "");
        assert_eq!(qwen3.min_similarity, 0.4);
        let bge = &config.embedding.models[1];
        assert_eq!(bge.embedding_api(), bge.query_api());
    }

    #[test]
    fn rejects_bad_ids_duplicates_and_an_unknown_default() {
        assert!(
            models(&["bge-m3", "qwen3-embedding-0.6b"], "bge-m3")
                .validate()
                .is_ok()
        );
        for (ids, default) in [
            (&["Qwen3"][..], "Qwen3"),
            (&["a'b"][..], "a'b"),
            (&["-a"][..], "-a"),
            (&["a", "a"][..], "a"),
            (&["a.b", "a_b"][..], "a.b"),
            (&["a-"][..], "a-"),
            (&["a".repeat(41).as_str()][..], "a"),
            (&["a"][..], "b"),
        ] {
            assert!(
                models(ids, default).validate().is_err(),
                "{ids:?} {default}"
            );
        }
    }

    #[test]
    fn a_model_needs_an_endpoint_and_concurrency_only_where_it_embeds() {
        let mut config = models(&["a"], "a");
        config.models[0].embedding_api = None;
        assert!(config.validate().is_err(), "no endpoint");

        let mut config = models(&["a"], "a");
        config.models[0].query_api = Some(ApiConfig {
            concurrency: Some(2),
            ..api("http://query/v1")
        });
        assert!(config.validate().is_err(), "concurrency on the query_api");

        let mut config = models(&["a"], "a");
        config.models[0].embedding_api = None;
        config.models[0].query_api = Some(ApiConfig {
            concurrency: Some(2),
            ..api("http://both/v1")
        });
        assert!(config.validate().is_ok(), "the query_api embeds chunks too");

        let mut config = models(&["a"], "a");
        config.models[0].embedding_api.as_mut().unwrap().concurrency = Some(0);
        assert!(config.validate().is_err(), "concurrency 0");
    }

    #[test]
    fn rejects_a_min_similarity_outside_0_to_1() {
        for min_similarity in [-0.1, 1.0] {
            let mut config = models(&["a"], "a");
            config.models[0].min_similarity = min_similarity;
            assert!(config.validate().is_err(), "{min_similarity}");
        }
    }

    #[test]
    fn a_token_variable_must_be_set_and_not_empty() {
        assert_eq!(
            token_from("T", Some("secret".to_owned())).unwrap(),
            "secret"
        );
        for value in [None, Some(String::new()), Some("  ".to_owned())] {
            let error = token_from("FERRET_TOKEN", value).unwrap_err();
            assert!(error.to_string().contains("FERRET_TOKEN"), "{error}");
        }
    }
}
