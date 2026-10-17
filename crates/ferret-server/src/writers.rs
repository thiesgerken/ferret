//! Writers: the connections and API keys that put documents into sources. A document's writer is
//! the connection whose root covers its source, else the API key that last put it. Revoked keys
//! stay writers.

use std::collections::HashMap;

use ferret_api::{Writer, WriterKind};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::sources;

pub struct Writers {
    /// By source root, which don't nest.
    connections: Vec<(String, Writer)>,
    api_keys: HashMap<Uuid, Writer>,
}

impl Writers {
    /// `user_id`'s connections and API keys.
    pub async fn load(db: &mut PgConnection, user_id: Uuid) -> Result<Self, sqlx::Error> {
        let connections = sqlx::query!(
            "select id, source, name, icon from connections where user_id = $1",
            user_id
        )
        .fetch_all(&mut *db)
        .await?
        .into_iter()
        .map(|row| {
            let writer = Writer {
                kind: WriterKind::Connection,
                id: row.id,
                name: row.name,
                icon: row.icon,
            };
            (row.source, writer)
        })
        .collect();
        let api_keys = sqlx::query!(
            "select id, name, icon from api_keys where user_id = $1",
            user_id
        )
        .fetch_all(&mut *db)
        .await?
        .into_iter()
        .map(|row| {
            let writer = Writer {
                kind: WriterKind::ApiKey,
                id: row.id,
                name: row.name,
                icon: row.icon,
            };
            (row.id, writer)
        })
        .collect();
        Ok(Self {
            connections,
            api_keys,
        })
    }

    /// The writer of a document in `source` that `api_key_id` put (`None`: a plugin run's, or
    /// one from before keys were recorded).
    pub fn of(&self, source: &str, api_key_id: Option<Uuid>) -> Option<Writer> {
        self.connections
            .iter()
            .find(|(root, _)| sources::covers(root, source))
            .map(|(_, writer)| writer)
            .or_else(|| api_key_id.and_then(|id| self.api_keys.get(&id)))
            .cloned()
    }

    /// The writer of a source path or selected root, for grouping facets: the connection covering
    /// it, else the one connection below it or `newest_key`, the key of its most recently
    /// modified document that has one. `None` when there's no such writer, or more than one kind
    /// (a root over a connection's documents and a key's).
    pub fn of_source(&self, source: &str, newest_key: Option<Uuid>) -> Option<Writer> {
        if let Some((_, writer)) = self
            .connections
            .iter()
            .find(|(root, _)| sources::covers(root, source))
        {
            return Some(writer.clone());
        }
        let mut below = self
            .connections
            .iter()
            .filter(|(root, _)| sources::covers(source, root));
        match (below.next(), below.next(), newest_key) {
            (None, _, key) => key.and_then(|id| self.api_keys.get(&id)).cloned(),
            (Some((_, writer)), None, None) => Some(writer.clone()),
            _ => None,
        }
    }
}
