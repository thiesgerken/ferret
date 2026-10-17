//! Where documents come from: the connection whose root covers their source, else the API key
//! that last put them. Revoked keys stay origins, a deleted user takes everything along.

use std::collections::HashMap;

use ferret_api::{Origin, OriginKind};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::sources;

pub struct Origins {
    /// By source root, which don't nest.
    connections: Vec<(String, Origin)>,
    api_keys: HashMap<Uuid, Origin>,
}

impl Origins {
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
            let origin = Origin {
                kind: OriginKind::Connection,
                id: row.id,
                name: row.name,
                icon: row.icon,
            };
            (row.source, origin)
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
            let origin = Origin {
                kind: OriginKind::ApiKey,
                id: row.id,
                name: row.name,
                icon: row.icon,
            };
            (row.id, origin)
        })
        .collect();
        Ok(Self {
            connections,
            api_keys,
        })
    }

    /// The origin of a document in `source` that `api_key_id` put (`None`: a plugin run's, or
    /// one from before keys were recorded).
    pub fn of(&self, source: &str, api_key_id: Option<Uuid>) -> Option<Origin> {
        self.connections
            .iter()
            .find(|(root, _)| sources::covers(root, source))
            .map(|(_, origin)| origin)
            .or_else(|| api_key_id.and_then(|id| self.api_keys.get(&id)))
            .cloned()
    }
}
