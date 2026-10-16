//! Plugin credentials: the config fields a plugin's schema marks `writeOnly`. They're stored sealed
//! under `FERRET_CONFIG_KEY`, apart from the rest of the config, so the GraphQL API can't return
//! them, and only a claimed run gets them opened.

use std::fmt;

use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit},
};
use serde_json::{Map, Value};

const NONCE_LEN: usize = 24;

pub struct ConfigKey(XChaCha20Poly1305);

impl ConfigKey {
    /// `encoded` is 32 random bytes in base64, e.g. from `openssl rand -base64 32`.
    pub fn new(encoded: &str) -> Result<Self, crate::StartupError> {
        let key = STANDARD
            .decode(encoded.trim())
            .map_err(|_| "FERRET_CONFIG_KEY must be base64")?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "FERRET_CONFIG_KEY must be 32 bytes")?;
        Ok(Self(cipher))
    }

    /// The nonce, then the ciphertext.
    pub fn seal(&self, credentials: &Map<String, Value>) -> Vec<u8> {
        let nonce = XNonce::generate();
        let plaintext = serde_json::to_vec(credentials).expect("a JSON map serializes");
        let ciphertext = self
            .0
            .encrypt(&nonce, plaintext.as_slice())
            .expect("encrypting a buffer can't fail");
        [nonce.as_slice(), &ciphertext].concat()
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Map<String, Value>, OpenError> {
        let (nonce, ciphertext) = sealed.split_at_checked(NONCE_LEN).ok_or(OpenError)?;
        let nonce = XNonce::try_from(nonce).map_err(|_| OpenError)?;
        let plaintext = self.0.decrypt(&nonce, ciphertext).map_err(|_| OpenError)?;
        serde_json::from_slice(&plaintext).map_err(|_| OpenError)
    }
}

/// Sealed under another key or damaged. After restoring a database without its key, this is how
/// stored credentials show up.
#[derive(Debug)]
pub struct OpenError;

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "the stored credentials can't be decrypted with FERRET_CONFIG_KEY, enter them again",
        )
    }
}

/// Splits `config` into the fields stored as they are and the credentials.
pub fn split(schema: &Value, mut config: Value) -> (Value, Map<String, Value>) {
    let mut credentials = Map::new();
    if let (Some(properties), Some(fields)) =
        (schema["properties"].as_object(), config.as_object_mut())
    {
        for (name, property) in properties {
            if property["writeOnly"] == true
                && let Some(value) = fields.remove(name)
            {
                credentials.insert(name.clone(), value);
            }
        }
    }
    (config, credentials)
}

/// Adds the credentials to a stored config, which is always an object.
pub fn merge(mut config: Value, credentials: Map<String, Value>) -> Value {
    config
        .as_object_mut()
        .expect("a stored config is an object")
        .extend(credentials);
    config
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

    #[test]
    fn credentials_open_only_with_the_key_that_sealed_them() {
        let key = ConfigKey::new(KEY).unwrap();
        let other = ConfigKey::new(&STANDARD.encode([7; 32])).unwrap();
        let credentials = Map::from_iter([("token".to_owned(), json!("s3cret"))]);

        let sealed = key.seal(&credentials);

        assert_eq!(key.open(&sealed).unwrap(), credentials);
        assert!(other.open(&sealed).is_err());
        assert!(key.open(&sealed[..10]).is_err());
        assert!(!sealed.windows(6).any(|window| window == b"s3cret"));
    }

    #[test]
    fn the_key_must_be_32_bytes_of_base64() {
        assert!(ConfigKey::new("not base64!").is_err());
        assert!(ConfigKey::new(&STANDARD.encode([7; 16])).is_err());
    }

    #[test]
    fn split_takes_out_the_write_only_fields() {
        let schema = json!({"properties": {
            "remote": {"type": "string"},
            "token": {"type": ["string", "null"], "writeOnly": true},
        }});
        let config = json!({"remote": "https://example.test/x.git", "token": "s3cret"});

        let (stored, credentials) = split(&schema, config.clone());

        assert_eq!(stored, json!({"remote": "https://example.test/x.git"}));
        assert_eq!(merge(stored, credentials), config);
    }
}
