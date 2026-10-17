//! AFFiNE (self-hosted): one document per doc of a workspace, as Markdown. Every run is a sync
//! over the docs the workspace's root doc lists outside the trash, and fetches only those the
//! server saw change since the last run.

mod markdown;
mod workspace;

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, TimeDelta, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Outcome, PluginError, Run};
use markdown::Links;
use workspace::DocMeta;

pub const NAME: &str = "affine";

/// The most docs AFFiNE returns per page.
const PAGE_SIZE: usize = 100;
/// `seen` takes the ids in batches, so a large workspace doesn't make one huge request.
const SEEN_BATCH: usize = 1000;
/// Fetching this window before the cursor again catches a doc saved while the last run listed.
/// Unchanged docs only cost a no-op `PUT`.
const CURSOR_OVERLAP: TimeDelta = TimeDelta::minutes(10);

/// Indexes the docs of a workspace on a self-hosted AFFiNE.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "AFFiNE")]
pub struct AffineConfig {
    /// The instance's address as the worker reaches it, e.g. `http://affine.apps-affine.svc:3010`.
    /// Not an address behind a login proxy: the sign-in only works on requests that reach AFFiNE
    /// directly.
    url: String,
    /// The address people open AFFiNE at, which hits link to, e.g. `https://affine.example.com`.
    /// `url` when left out.
    #[serde(default)]
    public_url: Option<String>,
    /// The workspace to index: the id in its address, `/workspace/<id>/…`.
    workspace_id: String,
    /// The email of the AFFiNE user ferret signs in as. Invite a ferret user with read access to
    /// the workspace rather than signing in as yourself.
    email: String,
    /// That user's password.
    #[schemars(extend("writeOnly" = true))]
    password: String,
}

/// A doc whose content the server changed, as `recentlyUpdatedDocs` lists them.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Updated {
    id: String,
    updated_at: DateTime<Utc>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: AffineConfig = serde_json::from_value(run.config.clone())?;
    let affine = Affine::sign_in(&config).await?;
    let workspace = &config.workspace_id;
    let public_url = config
        .public_url
        .as_deref()
        .unwrap_or(&config.url)
        .trim_end_matches('/');
    // Documents link to the instance, so they are all fetched again when its addresses or the
    // workspace change.
    let settings =
        json!({"url": config.url, "public_url": config.public_url, "workspace_id": workspace});
    let mut cursor = match run.state.get("updated") {
        Some(updated) if run.state.get("settings") == Some(&settings) => {
            Some(serde_json::from_value::<DateTime<Utc>>(updated.clone())?)
        }
        Some(_) => {
            run.log
                .line("the AFFiNE settings changed: fetching every doc");
            None
        }
        None => None,
    };

    let root = affine.doc(workspace, workspace).await?.ok_or_else(|| {
        format!(
            "AFFiNE has no workspace {workspace} that {} can read",
            config.email
        )
    })?;
    let docs = workspace::docs(&root)?;
    let live: Vec<&DocMeta> = docs.iter().filter(|doc| !doc.trashed).collect();
    // Leaving the trash doesn't change a doc on the server, so a restored doc is only found by
    // fetching everything once.
    let trashed_before: HashSet<String> = match run.state.get("trashed") {
        Some(trashed) => serde_json::from_value(trashed.clone())?,
        None => HashSet::new(),
    };
    if cursor.is_some() && live.iter().any(|doc| trashed_before.contains(&doc.id)) {
        run.log.line("a doc left the trash: fetching every doc");
        cursor = None;
    }

    let updated = affine
        .updated_since(workspace, cursor.map(|cursor| cursor - CURSOR_OVERLAP))
        .await?;
    let changed = changed(&live, &updated, cursor);
    run.log.line(format!(
        "{} docs in the workspace, {} in the trash, {} changed since the last run",
        live.len(),
        docs.len() - live.len(),
        changed.len()
    ));

    let titles: HashMap<String, String> = docs
        .iter()
        .map(|doc| (doc.id.clone(), doc.title.clone()))
        .collect();
    let base = format!("{public_url}/workspace/{workspace}");
    let links = Links {
        titles: &titles,
        base: &base,
    };
    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let ids: Vec<&str> = live.iter().map(|doc| doc.id.as_str()).collect();
    for batch in ids.chunks(SEEN_BATCH) {
        run.api.sync_seen(sync, &run.source, batch).await?;
    }
    let mut stats = RunStats::default();
    for (doc, updated_at) in changed {
        // The root doc can list a doc whose content no client has synced to the server yet.
        let Some(update) = affine.doc(workspace, &doc.id).await? else {
            run.log
                .line(format!("doc {} has no content on the server yet", doc.id));
            continue;
        };
        let page = markdown::to_markdown(&update, &links)
            .map_err(|error| format!("doc {}: {error}", doc.id))?;
        if !page.unknown.is_empty() {
            run.log.line(format!(
                "doc {}: block types this plugin doesn't know, kept as their text: {}",
                doc.id,
                page.unknown.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        let title = [page.title.trim(), doc.title.as_str()]
            .into_iter()
            .find(|title| !title.is_empty())
            .map(str::to_owned);
        let put = DocumentPut {
            source: run.source.clone(),
            external_id: doc.id.clone(),
            content_type: ContentType::Markdown,
            title,
            text: page.markdown,
            url: Some(links.url(&doc.id)),
            thread: None,
            authors: Vec::new(),
            created_at: doc.created_at.unwrap_or(updated_at),
            modified_at: updated_at,
            meta: Map::new(),
        };
        match run.api.put_document(&put).await? {
            PutResponse::Unchanged { .. } => stats.unchanged += 1,
            PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
            // The user excluded it: as if AFFiNE didn't have it.
            PutResponse::Excluded => {}
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));

    let mut state = json!({
        "settings": settings,
        "trashed": docs.iter().filter(|doc| doc.trashed).map(|doc| &doc.id).collect::<Vec<_>>(),
    });
    if let Some(newest) = updated.iter().map(|doc| doc.updated_at).max().max(cursor) {
        state["updated"] = json!(newest);
    }
    Ok(Outcome { state, stats })
}

/// The live docs to fetch, with when the server last changed them: all on the first run, else
/// those changed after the cursor (less the overlap). A doc the server has no content for isn't
/// listed and can't be fetched.
fn changed<'a>(
    live: &[&'a DocMeta],
    updated: &[Updated],
    cursor: Option<DateTime<Utc>>,
) -> Vec<(&'a DocMeta, DateTime<Utc>)> {
    let updated: HashMap<&str, DateTime<Utc>> = updated
        .iter()
        .filter(|doc| cursor.is_none_or(|cursor| doc.updated_at > cursor - CURSOR_OVERLAP))
        .map(|doc| (doc.id.as_str(), doc.updated_at))
        .collect();
    live.iter()
        .filter_map(|doc| Some((*doc, *updated.get(doc.id.as_str())?)))
        .collect()
}

struct Affine {
    http: reqwest::Client,
    base_url: String,
    /// The session cookies of the sign-in.
    cookies: String,
}

impl Affine {
    async fn sign_in(config: &AffineConfig) -> Result<Self, PluginError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(120))
            // AFFiNE's API doesn't redirect. A redirect comes from something in front of it,
            // typically a login proxy.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let base_url = config.url.trim_end_matches('/').to_owned();
        let response = http
            .post(format!("{base_url}/api/auth/sign-in"))
            .json(&json!({"email": config.email, "password": config.password}))
            .send()
            .await?;
        let status = response.status();
        if status.is_redirection() {
            return Err(format!(
                "{base_url} redirected the sign-in ({status}), probably to a login: use an address \
                 that reaches AFFiNE directly"
            )
            .into());
        }
        if !status.is_success() {
            // Only the status: the run log is shown to the user, and the body could be anything.
            return Err(
                format!("AFFiNE refused the sign-in of {} ({status})", config.email).into(),
            );
        }
        let cookies: Vec<&str> = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|cookie| cookie.to_str().ok()?.split(';').next())
            .collect();
        if cookies.is_empty() {
            return Err("AFFiNE signed in without a session cookie".into());
        }
        Ok(Self {
            cookies: cookies.join("; "),
            http,
            base_url,
        })
    }

    /// A doc's Yjs state, `None` when the server has none.
    async fn doc(&self, workspace: &str, doc: &str) -> Result<Option<Vec<u8>>, PluginError> {
        let path = format!("/api/workspaces/{workspace}/docs/{doc}");
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .header(reqwest::header::COOKIE, &self.cookies)
            .send()
            .await?;
        match response.status() {
            status if status.is_success() => Ok(Some(response.bytes().await?.to_vec())),
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => Err(format!("AFFiNE answered {path} with {status}").into()),
        }
    }

    /// The docs whose content changed after `since`, newest first, or every doc without it.
    /// AFFiNE pages with `updatedAt < after`, so ties at a page's end could fall between two
    /// pages. Each page asks for everything up to and including the last one's time instead,
    /// and repeats are dropped.
    async fn updated_since(
        &self,
        workspace: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Updated>, PluginError> {
        let mut seen = HashSet::new();
        let mut docs = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self.updated_page(workspace, after.as_deref()).await?;
            let Some(oldest) = page.last().map(|doc| doc.updated_at) else {
                break;
            };
            let full = page.len() == PAGE_SIZE;
            let before = docs.len();
            docs.extend(page.into_iter().filter(|doc| seen.insert(doc.id.clone())));
            if !full || since.is_some_and(|since| oldest <= since) {
                break;
            }
            if docs.len() == before {
                return Err(format!(
                    "more than {PAGE_SIZE} docs share the modification time {oldest}"
                )
                .into());
            }
            after = Some(STANDARD.encode((oldest + TimeDelta::milliseconds(1)).to_rfc3339()));
        }
        Ok(docs)
    }

    async fn updated_page(
        &self,
        workspace: &str,
        after: Option<&str>,
    ) -> Result<Vec<Updated>, PluginError> {
        let query = "query($workspace: String!, $after: String) {
            workspace(id: $workspace) {
                recentlyUpdatedDocs(pagination: {first: 100, after: $after}) {
                    edges { node { id updatedAt } }
                }
            }
        }";
        let response = self
            .http
            .post(format!("{}/graphql", self.base_url))
            .header(reqwest::header::COOKIE, &self.cookies)
            .json(&json!({"query": query, "variables": {"workspace": workspace, "after": after}}))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("AFFiNE answered its GraphQL API with {status}").into());
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| "AFFiNE's GraphQL API answered with something that isn't JSON")?;
        parse_updated_page(&body)
    }
}

fn parse_updated_page(body: &Value) -> Result<Vec<Updated>, PluginError> {
    if let Some(message) = body.pointer("/errors/0/message").and_then(Value::as_str) {
        return Err(format!("AFFiNE's GraphQL API: {message}").into());
    }
    let edges = body
        .pointer("/data/workspace/recentlyUpdatedDocs/edges")
        .and_then(Value::as_array)
        .ok_or("AFFiNE's GraphQL API answered without the workspace's docs")?;
    edges
        .iter()
        .map(|edge| Ok(Updated::deserialize(&edge["node"])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, trashed: bool) -> DocMeta {
        DocMeta {
            id: id.to_owned(),
            title: id.to_owned(),
            created_at: None,
            trashed,
        }
    }

    fn updated(id: &str, at: &str) -> Updated {
        Updated {
            id: id.to_owned(),
            updated_at: at.parse().unwrap(),
        }
    }

    #[test]
    fn a_listing_page_parses_and_errors_surface() {
        let page = parse_updated_page(
            &json!({"data": {"workspace": {"recentlyUpdatedDocs": {"edges": [
                {"node": {"id": "a", "updatedAt": "2026-10-17T18:09:34.345Z"}},
            ]}}}}),
        )
        .unwrap();
        assert_eq!(page, [updated("a", "2026-10-17T18:09:34.345Z")]);

        let error = parse_updated_page(
            &json!({"errors": [{"message": "You do not have permission to access Space w."}]}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("permission"));
    }

    #[test]
    fn only_live_docs_changed_after_the_cursor_are_fetched() {
        let (a, b, gone) = (meta("a", false), meta("b", false), meta("gone", false));
        let live = [&a, &b, &gone];
        let listed = [
            updated("a", "2026-10-17T12:00:00Z"),
            updated("b", "2026-10-17T10:00:00Z"),
            updated("trashed", "2026-10-17T12:00:00Z"),
        ];
        let ids = |changed: Vec<(&DocMeta, DateTime<Utc>)>| -> Vec<String> {
            changed.into_iter().map(|(doc, _)| doc.id.clone()).collect()
        };

        assert_eq!(
            ids(changed(&live, &listed, None)),
            ["a", "b"],
            "no content, nothing to fetch"
        );
        let cursor = "2026-10-17T11:00:00Z".parse().unwrap();
        assert_eq!(ids(changed(&live, &listed, Some(cursor))), ["a"]);
        let within_overlap = "2026-10-17T12:05:00Z".parse().unwrap();
        assert_eq!(ids(changed(&live, &listed, Some(within_overlap))), ["a"]);
    }
}
