//! Plugin credentials: the config fields a plugin's schema marks `writeOnly`. They're stored sealed
//! under `FERRET_CONFIG_KEY`, apart from the rest of the config, so the GraphQL API can't return
//! them, and only a claimed run gets them opened.

use std::fmt;

use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use serde_json::{Map, Value};
use uuid::Uuid;

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

    /// The nonce, then the ciphertext. Sealed for one connection of one user: copied to another
    /// row, it doesn't open. The field names are part of what's sealed.
    pub fn seal(&self, credentials: &Map<String, Value>, owner: Owner) -> Vec<u8> {
        let nonce = XNonce::generate();
        let plaintext = serde_json::to_vec(credentials).expect("a JSON map serializes");
        let ciphertext = self
            .0
            .encrypt(
                &nonce,
                Payload {
                    msg: &plaintext,
                    aad: &owner.bytes(),
                },
            )
            .expect("encrypting a buffer can't fail");
        [nonce.as_slice(), &ciphertext].concat()
    }

    pub fn open(&self, sealed: &[u8], owner: Owner) -> Result<Map<String, Value>, OpenError> {
        let (nonce, ciphertext) = sealed.split_at_checked(NONCE_LEN).ok_or(OpenError)?;
        let nonce = XNonce::try_from(nonce).map_err(|_| OpenError)?;
        let plaintext = self
            .0
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad: &owner.bytes(),
                },
            )
            .map_err(|_| OpenError)?;
        serde_json::from_slice(&plaintext).map_err(|_| OpenError)
    }
}

/// The connection credentials are sealed for.
#[derive(Clone, Copy)]
pub struct Owner {
    pub user_id: Uuid,
    pub connection_id: Uuid,
}

impl Owner {
    fn bytes(self) -> [u8; 32] {
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(self.user_id.as_bytes());
        bytes[16..].copy_from_slice(self.connection_id.as_bytes());
        bytes
    }
}

/// Sealed under another key, for another connection, or damaged. After restoring a database
/// without its key, this is how stored credentials show up.
#[derive(Debug)]
pub struct OpenError;

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "the stored credentials can't be decrypted with FERRET_CONFIG_KEY, enter them again",
        )
    }
}

/// The config fields the schema marks `writeOnly`.
pub fn secret_fields(schema: &Value) -> Vec<&str> {
    schema["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, property)| property["writeOnly"] == true)
        .map(|(name, _)| name.as_str())
        .collect()
}

/// Splits `config` into the fields stored as they are and the credentials.
pub fn split(schema: &Value, mut config: Value) -> (Value, Map<String, Value>) {
    let mut credentials = Map::new();
    if let Some(fields) = config.as_object_mut() {
        for name in secret_fields(schema) {
            if let Some(value) = fields.remove(name) {
                credentials.insert(name.to_owned(), value);
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
        let owner = Owner {
            user_id: Uuid::now_v7(),
            connection_id: Uuid::now_v7(),
        };
        let elsewhere = Owner {
            connection_id: Uuid::now_v7(),
            ..owner
        };
        let credentials = Map::from_iter([("token".to_owned(), json!("s3cret"))]);

        let sealed = key.seal(&credentials, owner);

        assert_eq!(key.open(&sealed, owner).unwrap(), credentials);
        assert!(other.open(&sealed, owner).is_err());
        assert!(
            key.open(&sealed, elsewhere).is_err(),
            "bound to its connection"
        );
        assert!(key.open(&sealed[..10], owner).is_err());
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
