//! IMAP: one document per mail, identified by its Message-ID, so a mail in several folders is one
//! document and a move is invisible. Every run is a sync over the account: it lists every folder's
//! mails to sweep deleted ones, and fetches only the new ones.

mod mail;
mod mailbox;
mod quotes;

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use ferret_api::{DocumentPut, PutResponse, ThreadQuery, internal::RunStats};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use self::mailbox::{Listed, Listing, Mailbox, folder_name};
use super::{Outcome, PluginError, Run};

pub const NAME: &str = "imap";

const DEFAULT_PORT: u16 = 993;
/// `seen` takes the ids in batches, so a large mailbox doesn't make one huge request.
const SEEN_BATCH: usize = 1000;
/// Mails are fetched this many at a time: they come with their attachments.
const FETCH_BATCH: usize = 50;
/// How quotes are stripped, part of the indexing settings: a new rule re-strips every mail.
const QUOTES: &str = "containers, markers, runs of 3 lines of earlier mails in the thread";

/// Indexes the mails of an IMAP mailbox.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "IMAP")]
pub struct ImapConfig {
    /// The IMAP server, e.g. `imap.zoho.eu`, with `:port` unless it's 993.
    server: String,
    /// Whether to connect with TLS. Without it, the password crosses the network in plain text:
    /// only for a server next to the worker.
    #[serde(default = "tls_by_default")]
    tls: bool,
    username: String,
    /// An app password where the provider offers them (Zoho: My Account → Security → App
    /// Passwords), rather than the account's own.
    #[schemars(extend("writeOnly" = true))]
    password: String,
    /// Folders not to index, by name. Folders the server marks as trash, junk or drafts are
    /// never indexed. A mail moved into an excluded folder counts as deleted.
    #[serde(default = "excluded_by_default")]
    exclude_folders: Vec<String>,
    /// Deep link to a mail, with `{message_id}`, e.g. `mid:{message_id}`. Without one, ferret's
    /// viewer is the origin.
    #[serde(default)]
    url_template: Option<String>,
}

fn tls_by_default() -> bool {
    true
}

fn excluded_by_default() -> Vec<String> {
    ["Trash", "Spam", "Junk", "Drafts"]
        .map(str::to_owned)
        .to_vec()
}

impl ImapConfig {
    fn host_and_port(&self) -> Result<(&str, u16), PluginError> {
        match self.server.rsplit_once(':') {
            Some((host, port)) => Ok((
                host,
                port.parse()
                    .map_err(|_| format!("{port:?} in {:?} is not a port", self.server))?,
            )),
            None => Ok((&self.server, DEFAULT_PORT)),
        }
    }

    /// The settings that decide which mails become which documents. When they change between
    /// runs, every mail is fetched again. Holds no secrets: it is stored in the connection's
    /// state.
    fn indexing(&self) -> Value {
        json!({
            "server": self.server,
            "username": self.username,
            "url_template": self.url_template,
            "quotes": QUOTES,
        })
    }
}

/// Where a folder's last run left off.
#[derive(Serialize, Deserialize)]
struct FolderCursor {
    uid_validity: u32,
    last_uid: u32,
}

/// A mail that no earlier run indexed.
struct NewMail<'a> {
    folder: &'a str,
    uid_validity: u32,
    listed: &'a Listed,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: ImapConfig = serde_json::from_value(run.config.clone())?;
    let indexing = config.indexing();
    let cursors = cursors(run, &indexing)?;

    let (host, port) = config.host_and_port()?;
    let mut mailbox = Mailbox::connect(host, port, &config).await?;
    let mut listings = BTreeMap::new();
    for folder in mailbox.folders(&config.exclude_folders).await? {
        let listing = mailbox.list(&folder).await?;
        if cursors
            .get(&folder)
            .is_some_and(|cursor| cursor.uid_validity != listing.uid_validity)
        {
            run.log.line(format!(
                "{} has a new UIDVALIDITY: fetching its mails again",
                folder_name(&folder)
            ));
        }
        listings.insert(folder, listing);
    }
    let ids: HashSet<&str> = listings
        .values()
        .flat_map(|listing| &listing.mails)
        .map(|listed| listed.id.as_str())
        .collect();
    let new = new_mails(&listings, &cursors);
    run.log.line(format!(
        "{} mails in {} folders ({}), {} new",
        ids.len(),
        listings.len(),
        listings
            .keys()
            .map(|folder| folder_name(folder))
            .collect::<Vec<_>>()
            .join(", "),
        new.len()
    ));

    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let ids: Vec<&str> = ids.into_iter().collect();
    for batch in ids.chunks(SEEN_BATCH) {
        run.api.sync_seen(sync, &run.source, batch).await?;
    }
    let mut stats = RunStats::default();
    let mut threads = Threads::default();
    for batch in new.chunks(FETCH_BATCH) {
        let mut mails = fetch(&mut mailbox, batch).await?;
        for new in batch {
            let listed = new.listed;
            let Some(raw) = mails.remove(&(new.folder, listed.uid)) else {
                run.log.line(format!(
                    "{}: mail {} was deleted during the run",
                    new.folder, listed.uid
                ));
                continue;
            };
            let Some(mut document) = mail::document(
                &raw,
                &listed.id,
                listed.written,
                &run.source,
                config.url_template.as_deref(),
            ) else {
                run.log.line(format!(
                    "{}: mail {} isn't a mail, skipped",
                    new.folder, listed.uid
                ));
                continue;
            };
            document.text = threads.without_earlier(run, &document).await?;
            threads.remember(&document);
            match run.api.put_document(&document).await? {
                PutResponse::Unchanged { .. } => stats.unchanged += 1,
                PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                // The user excluded it: as if the mailbox didn't have it.
                PutResponse::Excluded => {}
            }
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    if let Err(error) = mailbox.logout().await {
        run.log.line(format!("logging out failed: {error}"));
    }
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));

    let folders: BTreeMap<&str, FolderCursor> = listings
        .iter()
        .map(|(folder, listing)| {
            let last_uid = listing
                .mails
                .iter()
                .map(|listed| listed.uid)
                .chain(last_uid(&cursors, folder, listing))
                .max()
                .unwrap_or(0);
            (
                folder.as_str(),
                FolderCursor {
                    uid_validity: listing.uid_validity,
                    last_uid,
                },
            )
        })
        .collect();
    Ok(Outcome {
        state: json!({ "indexing": indexing, "folders": folders }),
        stats,
    })
}

/// The folders' cursors the last run left, none when the indexing settings changed since.
fn cursors(run: &Run, indexing: &Value) -> Result<HashMap<String, FolderCursor>, PluginError> {
    let Some(folders) = run.state.get("folders") else {
        return Ok(HashMap::new());
    };
    if run.state.get("indexing") != Some(indexing) {
        run.log
            .line("the mailbox or how mails are indexed changed: fetching every mail");
        return Ok(HashMap::new());
    }
    Ok(serde_json::from_value(folders.clone())?)
}

/// The last UID an earlier run indexed in `folder`, if its UIDs still mean the same mails.
fn last_uid(
    cursors: &HashMap<String, FolderCursor>,
    folder: &str,
    listing: &Listing,
) -> Option<u32> {
    cursors
        .get(folder)
        .filter(|cursor| cursor.uid_validity == listing.uid_validity)
        .map(|cursor| cursor.last_uid)
}

/// The mails no earlier run indexed, each once, in the order they arrived: a thread's earlier
/// mails are then indexed before its later ones, which the thread diff relies on.
fn new_mails<'a>(
    listings: &'a BTreeMap<String, Listing>,
    cursors: &HashMap<String, FolderCursor>,
) -> Vec<NewMail<'a>> {
    let is_indexed = |folder: &str, listing: &Listing, listed: &Listed| {
        last_uid(cursors, folder, listing).is_some_and(|last_uid| listed.uid <= last_uid)
    };
    // A new UID with an indexed Message-ID is a move or a copy: nothing to fetch.
    let indexed: HashSet<&str> = listings
        .iter()
        .flat_map(|(folder, listing)| {
            listing
                .mails
                .iter()
                .filter(|listed| is_indexed(folder, listing, listed))
                .map(|listed| listed.id.as_str())
        })
        .collect();
    let mut taken = HashSet::new();
    let mut new: Vec<NewMail> = listings
        .iter()
        .flat_map(|(folder, listing)| {
            listing.mails.iter().map(move |listed| NewMail {
                folder,
                uid_validity: listing.uid_validity,
                listed,
            })
        })
        .filter(|new| {
            !indexed.contains(new.listed.id.as_str()) && taken.insert(new.listed.id.as_str())
        })
        .collect();
    new.sort_by(|a, b| (a.listed.written, &a.listed.id).cmp(&(b.listed.written, &b.listed.id)));
    new
}

/// The raw mails of `batch`, by folder and UID.
async fn fetch<'a>(
    mailbox: &mut Mailbox,
    batch: &[NewMail<'a>],
) -> Result<HashMap<(&'a str, u32), Vec<u8>>, PluginError> {
    let mut by_folder: BTreeMap<(&str, u32), Vec<u32>> = BTreeMap::new();
    for new in batch {
        by_folder
            .entry((new.folder, new.uid_validity))
            .or_default()
            .push(new.listed.uid);
    }
    let mut mails = HashMap::new();
    for ((folder, uid_validity), uids) in by_folder {
        let fetched = mailbox.mails(folder, uid_validity, &uids).await?;
        mails.extend(fetched.into_iter().map(|(uid, raw)| ((folder, uid), raw)));
    }
    Ok(mails)
}

/// The mails of the threads this run touched, for the thread diff: each thread is asked from
/// ferret once, and the mails this run puts are added as they go.
#[derive(Default)]
struct Threads(HashMap<String, Thread>);

#[derive(Default)]
struct Thread {
    /// Whether the mails ferret held before are in `mails`.
    asked: bool,
    mails: Vec<ThreadMail>,
}

struct ThreadMail {
    id: String,
    written: DateTime<Utc>,
    text: String,
}

impl Threads {
    /// The mail's text without the runs of lines that earlier mails of its thread already have,
    /// as ferret holds them: their own text, already without what they quoted.
    async fn without_earlier(
        &mut self,
        run: &Run,
        document: &DocumentPut,
    ) -> Result<String, PluginError> {
        let thread_id = document.thread.as_deref().expect("every mail has a thread");
        // The thread's first mail has nothing before it.
        if thread_id == document.external_id {
            return Ok(document.text.clone());
        }
        let thread = self.0.entry(thread_id.to_owned()).or_default();
        if !thread.asked {
            // Ferret holds this run's mails of the thread too.
            let held = run
                .api
                .thread(&ThreadQuery {
                    source: run.source.clone(),
                    thread: thread_id.to_owned(),
                })
                .await?;
            thread.mails = held
                .documents
                .into_iter()
                .map(|document| ThreadMail {
                    id: document.external_id,
                    written: document.created_at,
                    text: document.text,
                })
                .collect();
            thread.asked = true;
        }
        let earlier: Vec<&str> = thread
            .mails
            .iter()
            .filter(|earlier| {
                earlier.id != document.external_id && earlier.written < document.created_at
            })
            .map(|earlier| earlier.text.as_str())
            .collect();
        Ok(quotes::without_repeated_runs(&document.text, &earlier))
    }

    fn remember(&mut self, document: &DocumentPut) {
        let thread = document.thread.clone().expect("every mail has a thread");
        let mails = &mut self.0.entry(thread).or_default().mails;
        mails.retain(|mail| mail.id != document.external_id);
        mails.push(ThreadMail {
            id: document.external_id.clone(),
            written: document.created_at,
            text: document.text.clone(),
        });
    }
}
