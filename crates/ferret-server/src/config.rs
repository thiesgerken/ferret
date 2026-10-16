//! The server's configuration file (`FERRET_CONFIG`). Secrets stay in environment variables.

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
    /// Stored with every embedding and part of its index name: `[a-z0-9][a-z0-9._-]*`.
    pub name: String,
    /// TEI for queries, and for the embed pump unless `bulk_url` is set.
    pub url: String,
    /// TEI for the embed pump only, e.g. a GPU node that joins for backfills.
    pub bulk_url: Option<String>,
    /// Asymmetric models expect an instruction or marker in front of queries and documents.
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default)]
    pub document_prefix: String,
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
        for model in &self.models {
            if !is_valid_name(&model.name) {
                return Err(format!(
                    "embedding model name {:?} must match [a-z0-9][a-z0-9._-]*",
                    model.name
                )
                .into());
            }
            if !names.insert(model.name.as_str()) {
                return Err(format!("embedding model {:?} is configured twice", model.name).into());
            }
        }
        if !names.contains(self.default.as_str()) {
            return Err(format!(
                "the default embedding model {:?} isn't configured",
                self.default
            )
            .into());
        }
        Ok(())
    }
}

/// Names end up in SQL identifiers and literals (index names, partial index predicates), so they
/// must not need quoting.
fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
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
                    url: "http://tei".to_owned(),
                    bulk_url: None,
                    query_prefix: String::new(),
                    document_prefix: String::new(),
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
    - name: qwen3-embedding-0.6b
      url: http://ferret-tei-qwen3:80
      bulk_url: http://ferret-tei-qwen3-gpu:80
      query_prefix: "Instruct: Given a search query\nQuery: "
"#,
        )
        .unwrap();

        config.embedding.validate().unwrap();
        let model = &config.embedding.models[0];
        assert_eq!(
            model.bulk_url.as_deref(),
            Some("http://ferret-tei-qwen3-gpu:80")
        );
        assert_eq!(
            model.query_prefix,
            "Instruct: Given a search query\nQuery: "
        );
        assert_eq!(model.document_prefix, "");
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
            (&["a"][..], "b"),
        ] {
            assert!(
                models(names, default).validate().is_err(),
                "{names:?} {default}"
            );
        }
    }
}
