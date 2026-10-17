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
    /// The TEI pool: every TEI works for whichever model it serves, which the server discovers.
    pub teis: Vec<TeiConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeiConfig {
    pub url: String,
    /// Not part of the deployment, e.g. a laptop in the LAN: it starts with queries off, so
    /// searches don't depend on it. Admins switch that in the UI.
    #[serde(default)]
    pub external: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// Stored with every embedding and part of its index name: `[a-z0-9][a-z0-9._-]*`, ending in
    /// a letter or digit, at most 40 characters.
    pub name: String,
    /// The Hugging Face model id a TEI reports in `/info` when it serves this model.
    pub model_id: String,
    /// Asymmetric models expect an instruction or marker in front of queries and documents.
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default)]
    pub document_prefix: String,
    /// Semantic candidates less similar to the query (cosine) are dropped. Where noise begins
    /// depends on the model, see embeddings.md for how to calibrate it.
    pub min_similarity: f64,
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
        let mut names = HashSet::new();
        let mut model_ids = HashSet::new();
        let mut slugs = HashSet::new();
        for model in &self.models {
            if !is_valid_name(&model.name) {
                return Err(format!(
                    "embedding model name {:?} must match [a-z0-9][a-z0-9._-]*, end in a letter \
                     or digit and have at most {MAX_NAME_CHARS} characters",
                    model.name
                )
                .into());
            }
            if !(0.0..1.0).contains(&model.min_similarity) {
                return Err(format!(
                    "embedding model {:?}: min_similarity must be at least 0 and below 1",
                    model.name
                )
                .into());
            }
            if !names.insert(model.name.as_str()) {
                return Err(format!("embedding model {:?} is configured twice", model.name).into());
            }
            if !model_ids.insert(model.model_id.as_str()) {
                return Err(format!(
                    "embedding models share the model id {:?}: a TEI serving it couldn't tell \
                     which one it works for",
                    model.model_id
                )
                .into());
            }
            if !slugs.insert(model.slug()) {
                return Err(format!(
                    "embedding model {:?} differs from another one only in `.`, `-` or `_`, \
                     which its index name can't tell apart",
                    model.name
                )
                .into());
            }
        }
        if !names.contains(self.default.as_str()) {
            return Err(format!(
                "the default embedding model {:?} isn't configured",
                self.default
            )
            .into());
        }
        if self.teis.is_empty() {
            return Err("no TEI is configured (embedding.teis)".into());
        }
        let mut urls = HashSet::new();
        if let Some(tei) = self.teis.iter().find(|tei| !urls.insert(tei.url.as_str())) {
            return Err(format!("the TEI {:?} is configured twice", tei.url).into());
        }
        Ok(())
    }
}

/// Index names have room for this much of a model name: Postgres cuts identifiers at 63 bytes.
const MAX_NAME_CHARS: usize = 40;

impl ModelConfig {
    /// The name as part of an identifier: `.` and `-` become `_`.
    pub fn slug(&self) -> String {
        self.name.replace(['.', '-'], "_")
    }
}

/// Names end up in SQL identifiers and literals (index names, partial index predicates), so they
/// must not need quoting.
fn is_valid_name(name: &str) -> bool {
    let alphanumeric = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    name.len() <= MAX_NAME_CHARS
        && name.starts_with(alphanumeric)
        && name.ends_with(alphanumeric)
        && name
            .chars()
            .all(|c| alphanumeric(c) || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(names: &[&str], default: &str) -> EmbeddingConfig {
        EmbeddingConfig {
            default: default.to_owned(),
            models: names
                .iter()
                .map(|name| ModelConfig {
                    name: name.to_string(),
                    model_id: format!("org/{name}"),
                    query_prefix: String::new(),
                    document_prefix: String::new(),
                    min_similarity: 0.5,
                })
                .collect(),
            teis: vec![TeiConfig {
                url: "http://tei".to_owned(),
                external: false,
            }],
        }
    }

    #[test]
    fn parses_the_documented_shape() {
        let config: Config = serde_saphyr::from_str(
            r#"
embedding:
  default: qwen3-embedding-0.6b
  models:
    - name: qwen3-embedding-0.6b
      model_id: Qwen/Qwen3-Embedding-0.6B
      query_prefix: "Instruct: Given a search query\nQuery: "
      min_similarity: 0.4
  teis:
    - url: http://ferret-tei-qwen3:80
    - url: http://192.168.1.20:8080
      external: true
"#,
        )
        .unwrap();

        config.embedding.validate().unwrap();
        let model = &config.embedding.models[0];
        assert_eq!(model.model_id, "Qwen/Qwen3-Embedding-0.6B");
        let teis = &config.embedding.teis;
        assert!(!teis[0].external);
        assert!(teis[1].external);
        assert_eq!(
            model.query_prefix,
            "Instruct: Given a search query\nQuery: "
        );
        assert_eq!(model.document_prefix, "");
        assert_eq!(model.min_similarity, 0.4);
    }

    #[test]
    fn rejects_bad_names_duplicates_and_an_unknown_default() {
        assert!(
            models(&["bge-m3", "qwen3-embedding-0.6b"], "bge-m3")
                .validate()
                .is_ok()
        );
        for (names, default) in [
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
                models(names, default).validate().is_err(),
                "{names:?} {default}"
            );
        }
    }

    #[test]
    fn rejects_no_teis_a_tei_twice_and_a_model_id_twice() {
        let mut config = models(&["a"], "a");
        config.teis.clear();
        assert!(config.validate().is_err());

        let mut config = models(&["a"], "a");
        config.teis.push(config.teis[0].clone());
        assert!(config.validate().is_err());

        let mut config = models(&["a", "b"], "a");
        config.models[1].model_id = config.models[0].model_id.clone();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_a_min_similarity_outside_0_to_1() {
        for min_similarity in [-0.1, 1.0] {
            let mut config = models(&["a"], "a");
            config.models[0].min_similarity = min_similarity;
            assert!(config.validate().is_err(), "{min_similarity}");
        }
    }
}
