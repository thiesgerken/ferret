# Plugin implementation contract

For configuring built-ins use [files](files.md), [git](git.md), [Gitea](gitea.md), [Home Assistant](homeassistant.md), [Paperless](paperless.md), [carapace](carapace.md), [IMAP](imap.md), [AFFiNE](affine.md), [Vikunja](vikunja.md), [Karakeep](karakeep.md), [Mealie](mealie.md), [Moneta](moneta.md). This page is for adding a worker plugin. If your producer already runs independently, [HTTP ingestion](../ingestion.md) is simpler and does not require worker registration.

A plugin is code in a worker, not code dynamically loaded into the server. Registration advertises a name and JSON Schema; the server stores schemas for validation/UI forms. Workers claim only names they advertised. Third-party workers can implement the same protocol without linking the built-in Rust binary, but the PSK is a trusted cross-user credential.

## Built-in Rust interface

[plugins/mod.rs](../../crates/ferret-plugins/src/plugins/mod.rs) declares modules, `registrations()` and dispatch in `run()`. Each plugin provides a serde/schemars config struct and `async fn run(&Run) -> Result<Outcome, PluginError>`. `Run` contains:

- an authenticated `ferret_api::Client` limited to the connection root;
- `source`, decrypted `config`, reusable successful `state` (initially `{}`, also `{}` after a failed/lost run);
- a run log and operator-fixed options (currently Git transport allowlist).

`Outcome` returns JSON state and `RunStats { put, unchanged, deleted }`. The server saves state only on successful finish. Returning an error does not roll back earlier puts or deletes. After a failed/lost run, the next claim supplies `{}` instead of the stored cursor: those partial writes may no longer match it, so the plugin must fully reconcile. A subsequent successful run restores cursor reuse. Never use a new cursor until all origin pages and required writes have completed.

Large metadata inventories belong in server-owned Postgres, not cursor JSON or worker-local files. The [files plugin](files.md#inventory-recovery-and-diagnostics) uses bounded run-token inventory endpoints with per-run staged generations. Only successful finish publishes a reusable generation. Empty working state and settings changes must invalidate reuse even when bulk inventory still exists.

## A complete minimal plugin

This small plugin indexes one configured note. It illustrates the actual interface without inventing a runtime plugin loader. Add a module file in your feature branch, add its schema to `registrations()`, and dispatch its name as the built-ins do. It requires no external account, and a complete run reconciles its root, so give it a dedicated root with no other producer.

```rust
use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "configured-note";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteConfig {
    pub title: String,
    pub text: String,
    pub url: Option<String>,
    #[schemars(with = "String")]
    pub created_at: DateTime<Utc>,
    #[schemars(with = "String")]
    pub modified_at: DateTime<Utc>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: NoteConfig = serde_json::from_value(run.config.clone())?;
    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let document = DocumentPut {
        source: run.source.clone(),
        external_id: "note".to_owned(),
        content_type: ContentType::Plain,
        title: Some(config.title),
        text: config.text,
        url: config.url,
        thread: None,
        authors: Vec::new(),
        created_at: Some(config.created_at),
        modified_at: Some(config.modified_at),
        meta: Default::default(),
    };
    let mut stats = RunStats::default();
    match run.api.put_document(&document).await? {
        PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put = 1,
        PutResponse::Unchanged { .. } => stats.unchanged = 1,
        PutResponse::Excluded => {}
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    Ok(Outcome { state: json!({}), stats })
}
```

Register `schemars::schema_for!(NoteConfig)` under `configured-note`, then add a `NAME => module::run(run).await` dispatcher arm. A connection config is:

```json
{
  "title": "Workshop checklist",
  "text": "Bring the cobalt torque wrench.",
  "url": null,
  "created_at": "2026-01-02T10:00:00Z",
  "modified_at": "2026-01-02T10:00:00Z"
}
```

Creation should produce one document; repeating without configuration changes should count one unchanged. Test these outcomes and source ownership using the existing [plugin integration tests](../../crates/ferret-server/tests/git_plugin.rs) as a pattern. This is documentation sample code, not a built-in shipped plugin.

## Configuration schema and secrets

Use `#[serde(deny_unknown_fields)]` and `JsonSchema`, with real defaults declared on the config fields. The UI supports the shapes described by [config-schema.ts](../../frontend/src/lib/config-schema.ts); do not assume arbitrary nested JSON Schema constructs automatically render. Keep credential fields top-level and mark them `#[schemars(extend("writeOnly" = true))]`. The server splits/encrypts those fields; an unmarked token is ordinary visible configuration.

A top-level non-secret field may opt in to credential preservation with `#[schemars(extend("x-ferret-preserve-credentials" = true))]`. Audit every use first: changing it must not redirect stored credentials, change their transport, or select another login identity or credential-bearing resource. Browser-only links and local indexing filters are examples; a non-secret field or a URL is not automatically safe. Only boolean `true` opts in, and `writeOnly` always takes precedence. Unknown/future fields, missing/false/non-boolean annotations, and any mixed safe/protected edit require every stored credential again. Adding or removing a field counts as changing it. If stored credentials cannot be decrypted, even safe edits require re-entry.

Schema registration is an upsert by name. Deploying incompatible workers under the same name can overwrite schemas while old workers still claim runs; coordinate schema/worker changes rather than treating registration as version negotiation. Saved state is plugin-owned JSON. Include the non-secret settings that affect identity/rendering in state so changing them can trigger a full reconciliation. Never put credentials or raw sensitive responses in state or logs.

## Worker wire lifecycle

All internal calls use `Authorization: Bearer <FERRET_WORKER_SECRET>` against the private server port 8081, **not** the reverse-proxied public origin. The canonical types/client are [internal.rs](../../crates/ferret-api/src/internal.rs).

| Operation | Request | Response |
| --- | --- | --- |
| `POST /internal/workers/register` | `{name,plugins:[{name,config_schema}]}` | 200 `{worker_id}` |
| `POST /internal/runs/claim?wait=30` | `{worker_id}` | 200 claimed run, or 204 no due work |
| `POST /internal/runs/{id}/heartbeat` | `{log_lines:[...]}` | 204, or 409 ended run |
| `POST /internal/runs/{id}/finish` | succeeded/failed result plus `log_lines` | 204, or 409 ended run |

Claim waits at most 60 seconds even if more is requested. Unknown worker ids return 404: register again. A claimed response includes `run_id`, `connection_id`, `plugin`, `source`, merged `config`, reusable `state` and `token`. Its state is `{}` initially or when the previous run failed/was lost; otherwise it is the previous successful state. Use only `token` as bearer for public document/sync calls. Do not send the worker PSK to an origin or use it as a public key.

Successful finish body:

```json
{"result":"succeeded","state":{"cursor":"origin-version-42"},"stats":{"put":3,"unchanged":7,"deleted":1},"log_lines":["complete listing processed"]}
```

Failure body:

```json
{"result":"failed","error":"origin authentication failed","log_lines":[]}
```

Heartbeats keep the two-minute lease fresh; built-in workers send every 30 seconds, including newly buffered log lines. A late run token fails public authentication once the lease is stale. Claims mark expired running runs lost. Heartbeat/finish reject a run already ended with 409; stop ingestion instead of pretending to succeed. Run JWTs also expire after 24 hours regardless of heartbeats.

The server retains the tail of run logs (64 Ki characters). A worker should reconnect/retry transient transport errors without duplicating origin side effects. Built-in workers retry registration/claims after five seconds, heartbeat delivery preserves undelivered log lines, and finish retries are bounded to stay within the lease. See [worker.rs](../../crates/ferret-plugins/src/worker.rs) for cancellation/finish behavior.

## Safe origin synchronization

Use stable `(source, external_id)` identity and explicit content type. Respect exclusions (`PutResponse::Excluded` is not an error). Enumerate the full owned root before committing a sweep; mark unchanged ids seen. An incremental cursor may instead apply explicit origin deletions, provided missed history triggers a full sync. Failure to enumerate is never evidence of an empty origin.

Use independent nullable origin timestamps when dates are unknown; never use the run clock, state-change time or the other origin date as a substitute. Composite documents may have a known creation time but no reliable modification time covering their definitions and membership. Their full serialized content, not a modified-since shortcut, still detects changes.

Allowlist transports where origins can point at worker resources. A plugin worker has origin credentials and network access; an arbitrary URL fetcher can reach internal services unless the operator constrains egress. Avoid logging tokens even on error, including HTTP bodies that might echo secrets. The run's config/token are intentionally omitted from its Debug output.

Implementation: [server registration/claims/leases](../../crates/ferret-server/src/runs.rs), [connection schema validation](../../crates/ferret-server/src/connections.rs), [credential sealing](../../crates/ferret-server/src/credentials.rs), [public client](../../crates/ferret-api/src/lib.rs). See [development](../development.md) for building and testing a worker change.
