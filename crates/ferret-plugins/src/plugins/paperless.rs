//! Paperless-ngx: one document per Paperless document, with its OCR text and metadata. Every run
//! lists all ids to sweep deleted documents, and fetches only those modified since the last run.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, json};

use super::{Log, Outcome, PluginError, Run};

pub const NAME: &str = "paperless";

/// Pinned, so a newer Paperless keeps answering in the shape this plugin parses. Version 10 came
/// with Paperless-ngx 3.0 and is the newest that 3.3 accepts (it also takes 9).
const ACCEPT: &str = "application/json; version=10";
const LIST_PAGE_SIZE: usize = 1000;
/// Documents carry their whole OCR text, so they come in smaller pages.
const DOCUMENT_PAGE_SIZE: usize = 100;
const DOCUMENT_FIELDS: &str =
    "id,title,content,created,modified,correspondent,document_type,tags,archive_serial_number";
/// `seen` takes the ids in batches, so a large archive doesn't make one huge request.
const SEEN_BATCH: usize = 1000;
/// Paperless sets `modified` inside the transaction that saves a document, so a save that commits
/// after a listing can carry a `modified` older than the newest one listed. Fetching this window
/// before the cursor again catches it. Unchanged documents only cost a no-op `PUT`.
const CURSOR_OVERLAP: TimeDelta = TimeDelta::minutes(10);

/// Indexes the documents of a Paperless-ngx instance (3.0 or later).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Paperless")]
pub struct PaperlessConfig {
    /// The instance's address as the worker reaches it, e.g.
    /// `http://paperless.apps-paperless.svc:8000`. Not an address behind a login proxy: the API
    /// token only works on requests that reach Paperless directly.
    url: String,
    /// The address people open Paperless at, which hits link to, e.g.
    /// `https://paperless.example.com`. `url` when left out.
    #[serde(default)]
    public_url: Option<String>,
    /// An API token of the Paperless user whose documents to index (Paperless: My Profile → API
    /// Auth Token).
    #[schemars(extend("writeOnly" = true))]
    token: String,
}

#[derive(Deserialize)]
struct Page<T> {
    count: usize,
    next: Option<String>,
    results: Vec<T>,
}

#[derive(Deserialize)]
struct Listed {
    id: u64,
    modified: DateTime<Utc>,
}

#[derive(Deserialize)]
struct PaperlessDocument {
    id: u64,
    title: String,
    content: String,
    created: NaiveDate,
    modified: DateTime<Utc>,
    correspondent: Option<u64>,
    document_type: Option<u64>,
    tags: Vec<u64>,
    archive_serial_number: Option<u64>,
}

#[derive(Deserialize)]
struct Named {
    id: u64,
    name: String,
}

/// Names of correspondents, document types and tags by id: documents only carry the ids.
#[derive(Default)]
struct Names {
    correspondents: HashMap<u64, String>,
    document_types: HashMap<u64, String>,
    tags: HashMap<u64, String>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: PaperlessConfig = serde_json::from_value(run.config.clone())?;
    let paperless = Paperless::new(&config)?;
    let links = config
        .public_url
        .as_deref()
        .unwrap_or(&config.url)
        .trim_end_matches('/');
    // Documents link to the instance, so they are all fetched again when its addresses change.
    let addresses = json!({"url": config.url, "public_url": config.public_url});
    let cursor = match run.state.get("modified") {
        Some(modified) if run.state.get("addresses") == Some(&addresses) => {
            Some(serde_json::from_value::<DateTime<Utc>>(modified.clone())?)
        }
        Some(_) => {
            run.log
                .line("the addresses of Paperless changed: fetching every document");
            None
        }
        None => None,
    };

    let listed: Vec<Listed> = paperless
        .all(&format!(
            "/api/documents/?fields=id,modified&page_size={LIST_PAGE_SIZE}"
        ))
        .await?;
    // A document missing from the listing would be swept, and the cursor would never fetch it
    // again.
    let distinct: HashSet<u64> = listed.iter().map(|document| document.id).collect();
    if distinct.len() != listed.len() {
        return Err(changed_during_listing("/api/documents/"));
    }
    let changed = changed_since(&listed, cursor);
    run.log.line(format!(
        "{} documents in Paperless, {} modified since the last run",
        listed.len(),
        changed.len()
    ));

    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let ids: Vec<String> = listed
        .iter()
        .map(|document| document.id.to_string())
        .collect();
    for batch in ids.chunks(SEEN_BATCH) {
        let batch: Vec<&str> = batch.iter().map(String::as_str).collect();
        run.api.sync_seen(sync, &run.source, &batch).await?;
    }
    let mut stats = RunStats::default();
    if !changed.is_empty() {
        let names = paperless.names().await?;
        for batch in changed.chunks(DOCUMENT_PAGE_SIZE) {
            let ids: Vec<String> = batch.iter().map(u64::to_string).collect();
            let documents: Vec<PaperlessDocument> = paperless
                .all(&format!(
                    "/api/documents/?id__in={}&fields={DOCUMENT_FIELDS}&page_size={DOCUMENT_PAGE_SIZE}",
                    ids.join(",")
                ))
                .await?;
            for document in documents {
                let put = to_document(document, &names, links, &run.source, &run.log);
                match run.api.put_document(&put).await? {
                    PutResponse::Unchanged { .. } => stats.unchanged += 1,
                    PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                    // The user excluded it: as if Paperless didn't have it.
                    PutResponse::Excluded => {}
                }
            }
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));

    let newest = listed
        .iter()
        .map(|document| document.modified)
        .max()
        .max(cursor);
    let mut state = json!({ "addresses": addresses });
    if let Some(modified) = newest {
        state["modified"] = json!(modified);
    }
    Ok(Outcome { state, stats })
}

/// The ids modified after `cursor` (less the overlap), all of them on the first run.
fn changed_since(listed: &[Listed], cursor: Option<DateTime<Utc>>) -> Vec<u64> {
    listed
        .iter()
        .filter(|document| cursor.is_none_or(|cursor| document.modified > cursor - CURSOR_OVERLAP))
        .map(|document| document.id)
        .collect()
}

fn to_document(
    document: PaperlessDocument,
    names: &Names,
    base_url: &str,
    source: &str,
    log: &Log,
) -> DocumentPut {
    let name = |names: &HashMap<u64, String>, kind: &str, id: u64| {
        let found = names.get(&id).cloned();
        if found.is_none() {
            // The token's user may see a document but not every object it refers to.
            log.line(format!(
                "document {}: {kind} {id} isn't visible to the token",
                document.id
            ));
        }
        found
    };
    let correspondent = document
        .correspondent
        .and_then(|id| name(&names.correspondents, "correspondent", id));
    let mut meta = Map::new();
    if let Some(correspondent) = &correspondent {
        meta.insert("correspondent".to_owned(), json!(correspondent));
    }
    if let Some(document_type) = document
        .document_type
        .and_then(|id| name(&names.document_types, "document type", id))
    {
        meta.insert("document_type".to_owned(), json!(document_type));
    }
    let tags: Vec<String> = document
        .tags
        .iter()
        .filter_map(|id| name(&names.tags, "tag", *id))
        .collect();
    if !tags.is_empty() {
        meta.insert("tags".to_owned(), json!(tags));
    }
    if let Some(asn) = document.archive_serial_number {
        meta.insert("archive_serial_number".to_owned(), json!(asn));
    }
    DocumentPut {
        source: source.to_owned(),
        external_id: document.id.to_string(),
        content_type: ContentType::Plain,
        title: Some(document.title),
        text: document.content,
        url: Some(format!("{base_url}/documents/{}/details", document.id)),
        thread: None,
        authors: correspondent.into_iter().collect(),
        // Paperless keeps only the date. Noon UTC keeps that date in every time zone from UTC-11
        // to UTC+11.
        created_at: Some(
            document
                .created
                .and_time(NaiveTime::from_hms_opt(12, 0, 0).expect("noon exists"))
                .and_utc(),
        ),
        modified_at: Some(document.modified),
        meta,
    }
}

fn changed_during_listing(path: &str) -> PluginError {
    let endpoint = path.split('?').next().unwrap_or(path);
    format!("Paperless changed while listing {endpoint}, the next run retries").into()
}

struct Paperless {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl Paperless {
    fn new(config: &PaperlessConfig) -> Result<Self, PluginError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(120))
                // Paperless's API doesn't redirect. A redirect comes from something in front of
                // it, typically a login proxy, whose page would only fail to parse.
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base_url: config.url.trim_end_matches('/').to_owned(),
            token: config.token.clone(),
        })
    }

    /// Every page of a list endpoint, ordered by id: Paperless's default order (`-created`, a
    /// date) has ties, and paging over ties isn't stable. Pages are counted here rather than
    /// followed through `next`, which Paperless builds from the host it believes it's on, wrong
    /// behind some proxies.
    async fn all<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, PluginError> {
        let mut results = Vec::new();
        let mut count = None;
        for page in 1.. {
            let response = self
                .http
                .get(format!("{}{path}&ordering=id&page={page}", self.base_url))
                .header("Accept", ACCEPT)
                .header("Authorization", format!("Token {}", self.token))
                .send()
                .await?;
            let status = response.status();
            if status.is_redirection() {
                return Err(format!(
                    "{} redirected {path} ({status}), probably to a login: use an address that \
                     reaches Paperless directly",
                    self.base_url
                )
                .into());
            }
            if !status.is_success() {
                // Only the status: the body of whatever answered could be anything, and the run
                // log is shown to the user.
                return Err(format!("Paperless answered {path} with {status}").into());
            }
            // Not serde's message, which quotes the body: whatever answered could be anything.
            let page: Page<T> = response.json().await.map_err(|_| {
                format!("Paperless answered {path} with something that isn't a version 10 list")
            })?;
            if *count.get_or_insert(page.count) != page.count {
                return Err(changed_during_listing(path));
            }
            results.extend(page.results);
            if page.next.is_none() {
                break;
            }
        }
        if Some(results.len()) != count {
            return Err(changed_during_listing(path));
        }
        Ok(results)
    }

    async fn names(&self) -> Result<Names, PluginError> {
        let named = |kind: &'static str| async move {
            let named: Vec<Named> = self
                .all(&format!("/api/{kind}/?page_size={LIST_PAGE_SIZE}"))
                .await?;
            Ok::<_, PluginError>(
                named
                    .into_iter()
                    .map(|named| (named.id, named.name))
                    .collect(),
            )
        };
        Ok(Names {
            correspondents: named("correspondents").await?,
            document_types: named("document_types").await?,
            tags: named("tags").await?,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn fixture<T: DeserializeOwned>(json: &str) -> Vec<T> {
        serde_json::from_str::<Page<T>>(json).unwrap().results
    }

    fn names() -> Names {
        let named = |json| {
            fixture::<Named>(json)
                .into_iter()
                .map(|named| (named.id, named.name))
                .collect()
        };
        Names {
            correspondents: named(include_str!("../../fixtures/paperless/correspondents.json")),
            document_types: named(include_str!("../../fixtures/paperless/document_types.json")),
            tags: named(include_str!("../../fixtures/paperless/tags.json")),
        }
    }

    #[test]
    fn a_document_carries_its_text_metadata_and_a_link_back() {
        let documents: Vec<PaperlessDocument> =
            fixture(include_str!("../../fixtures/paperless/documents.json"));
        let log = Log::default();

        let puts: Vec<DocumentPut> = documents
            .into_iter()
            .map(|document| {
                to_document(
                    document,
                    &names(),
                    "https://paperless.example",
                    "paperless",
                    &log,
                )
            })
            .collect();

        let insurance = &puts[0];
        assert_eq!(insurance.external_id, "412");
        assert_eq!(insurance.title.as_deref(), Some("Kfz-Versicherung 2025"));
        assert!(insurance.text.contains("412,80 EUR"));
        assert_eq!(
            insurance.url.as_deref(),
            Some("https://paperless.example/documents/412/details")
        );
        assert_eq!(insurance.authors, ["Allianz"]);
        assert_eq!(
            insurance.created_at.unwrap().to_rfc3339(),
            "2025-01-14T12:00:00+00:00"
        );
        assert_eq!(
            insurance.modified_at.unwrap().to_rfc3339(),
            "2025-01-16T07:41:07.518236+00:00"
        );
        assert_eq!(
            Value::Object(insurance.meta.clone()),
            json!({
                "correspondent": "Allianz",
                "document_type": "Rechnung",
                "tags": ["Versicherung", "Auto"],
                "archive_serial_number": 1041,
            })
        );
        let note = &puts[1];
        assert!(note.authors.is_empty());
        assert!(note.meta.is_empty(), "no correspondent, type, tags or ASN");
        assert!(log.take().is_empty());
    }

    #[test]
    fn an_object_the_token_cant_see_is_left_out_with_a_log_line() {
        let mut documents: Vec<PaperlessDocument> =
            fixture(include_str!("../../fixtures/paperless/documents.json"));
        let log = Log::default();

        let put = to_document(
            documents.remove(0),
            &Names::default(),
            "https://p",
            "paperless",
            &log,
        );

        assert!(put.authors.is_empty());
        assert!(put.meta.contains_key("archive_serial_number"));
        assert_eq!(
            log.take().len(),
            4,
            "correspondent, document type and two tags"
        );
    }

    #[test]
    fn only_documents_modified_after_the_cursor_change() {
        let listed: Vec<Listed> = fixture(include_str!("../../fixtures/paperless/listing.json"));
        let cursor = "2025-02-01T00:00:00Z".parse().unwrap();

        assert_eq!(changed_since(&listed, None), [412, 977]);
        assert_eq!(changed_since(&listed, Some(cursor)), [977]);
        let just_after_977 = "2025-03-03T17:25:00Z".parse().unwrap();
        assert_eq!(
            changed_since(&listed, Some(just_after_977)),
            [977],
            "within the overlap"
        );
    }
}
