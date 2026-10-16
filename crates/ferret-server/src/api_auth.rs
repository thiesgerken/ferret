//! Authentication and authorization on `/api`: every route takes a bearer credential and gets an
//! [`ApiCaller`], whatever kind of credential it was.

use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use uuid::Uuid;

use crate::{AppState, api_keys, error::ApiError, sources};

/// What a key may do: `<scope>:<access>`, where write implies read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, juniper::GraphQLEnum)]
#[graphql(name = "ApiKeyScope")]
pub enum Scope {
    SearchRead,
    IngestRead,
    IngestWrite,
    AdminRead,
    AdminWrite,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SearchRead => "search:read",
            Self::IngestRead => "ingest:read",
            Self::IngestWrite => "ingest:write",
            Self::AdminRead => "admin:read",
            Self::AdminWrite => "admin:write",
        }
    }

    pub fn parse(scope: &str) -> Self {
        match scope {
            "search:read" => Self::SearchRead,
            "ingest:read" => Self::IngestRead,
            "ingest:write" => Self::IngestWrite,
            "admin:read" => Self::AdminRead,
            "admin:write" => Self::AdminWrite,
            // The table's check constraint admits no other value.
            _ => panic!("unknown scope {scope:?} in the database"),
        }
    }

    fn grants(self, needed: Self) -> bool {
        self == needed
            || matches!(
                (self, needed),
                (Self::IngestWrite, Self::IngestRead) | (Self::AdminWrite, Self::AdminRead)
            )
    }

    /// Admin scopes only work while the key's owner is an admin.
    pub fn needs_admin(self) -> bool {
        matches!(self, Self::AdminRead | Self::AdminWrite)
    }
}

/// The caller of a `/api` route. It acts as its owner: it only ever sees and writes that user's
/// documents, within its scopes and sources.
pub struct ApiCaller {
    #[allow(dead_code, reason = "first read by ingest and search (#4)")]
    pub user_id: Uuid,
    scopes: Vec<Scope>,
    sources: Option<Vec<String>>,
}

impl ApiCaller {
    pub fn new(user_id: Uuid, scopes: Vec<Scope>, sources: Option<Vec<String>>) -> Self {
        Self {
            user_id,
            scopes,
            sources,
        }
    }

    /// Fails with 403 unless one of the caller's scopes grants `needed`.
    pub fn require(&self, needed: Scope) -> Result<(), ApiError> {
        if self.scopes.iter().any(|scope| scope.grants(needed)) {
            Ok(())
        } else {
            Err(ApiError::forbidden(&format!(
                "this API key lacks the {} scope",
                needed.as_str()
            )))
        }
    }

    /// The source paths every scope is restricted to (each including everything below it),
    /// `None` for all sources.
    #[allow(dead_code, reason = "first read by ingest and search (#4)")]
    pub fn sources(&self) -> Option<&[String]> {
        self.sources.as_deref()
    }

    /// Whether the caller may read or write documents of `source`.
    #[allow(dead_code, reason = "first read by ingest and search (#4)")]
    pub fn may_access(&self, source: &str) -> bool {
        self.sources
            .as_ref()
            .is_none_or(|roots| roots.iter().any(|root| sources::covers(root, source)))
    }
}

impl FromRequestParts<AppState> for ApiCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        let caller = match token {
            Some(token) => api_keys::authenticate(&state.pool, token).await?,
            None => None,
        };
        caller.ok_or_else(|| ApiError::unauthorized("missing or invalid API key"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_implies_read_of_the_same_scope_only() {
        assert!(Scope::IngestWrite.grants(Scope::IngestRead));
        assert!(Scope::AdminWrite.grants(Scope::AdminRead));
        assert!(!Scope::IngestRead.grants(Scope::IngestWrite));
        assert!(!Scope::IngestWrite.grants(Scope::SearchRead));
        assert!(!Scope::AdminWrite.grants(Scope::IngestRead));
    }
}
