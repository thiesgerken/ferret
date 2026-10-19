//! Home Assistant inventory and loaded configured behavior, reconciled without runtime telemetry.

mod inventory;
mod protocol;
mod render;

use ferret_api::{PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "homeassistant";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Home Assistant")]
pub struct HomeAssistantConfig {
    /// HTTP(S) instance root reachable by the worker, without `/api` or a path prefix.
    url: String,
    /// Browser-facing HTTP(S) instance root. Uses `url` when omitted.
    #[serde(default)]
    public_url: Option<String>,
    /// Dedicated administrator account token. The plugin only reads, but the token can control
    /// the origin. Faithful automation/script definitions can contain private or secret content.
    #[schemars(extend("writeOnly" = true))]
    token: String,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: HomeAssistantConfig = serde_json::from_value(run.config.clone())
        .map_err(|_| "Home Assistant configuration is invalid")?;
    let url = protocol::instance_url(&config.url)?;
    let links = protocol::instance_url(config.public_url.as_deref().unwrap_or(&config.url))?;
    if config.token.trim().is_empty() {
        return Err("Home Assistant requires an administrator token".into());
    }
    let mut session = protocol::Session::connect(url, &config.token).await?;
    let snapshot = inventory::Snapshot::load(&mut session).await?;
    let documents = render::documents(&snapshot, &run.source, &links)?;
    // A disconnect while completing the required reads must not authorize a sweep.
    session.close().await?;
    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let mut stats = RunStats::default();
    for document in documents {
        match run.api.put_document(&document).await? {
            PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
            PutResponse::Unchanged { .. } => stats.unchanged += 1,
            PutResponse::Excluded => {}
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    Ok(Outcome {
        state: json!({}),
        stats,
    })
}
