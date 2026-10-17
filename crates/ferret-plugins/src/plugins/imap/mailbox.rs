//! The IMAP side: connecting, the folders to index, what each holds, and the mails themselves.

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::Arc,
    time::Duration,
};

use async_imap::{
    Client, Session,
    imap_proto::{Address, NameAttribute},
    types::Fetch,
};
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use rustls_platform_verifier::ConfigVerifierExt;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, pki_types::ServerName},
};

use super::{ImapConfig, mail::message_id};
use crate::plugins::PluginError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// For one command and its whole answer. Listing 50k mails is a few MB.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

trait Connection: AsyncRead + AsyncWrite + Unpin + Send + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> Connection for T {}

pub struct Mailbox {
    session: Session<Box<dyn Connection>>,
}

/// What a folder holds, as of this run.
pub struct Listing {
    pub uid_validity: u32,
    pub mails: Vec<Listed>,
}

pub struct Listed {
    pub uid: u32,
    /// The Message-ID, or for a mail without one, a stand-in made of its date, sender and subject.
    pub id: String,
    /// When the mail arrived in the mailbox (IMAP's `INTERNALDATE`).
    pub received: DateTime<Utc>,
}

impl Mailbox {
    pub async fn connect(host: &str, port: u16, config: &ImapConfig) -> Result<Self, PluginError> {
        let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
            .await
            .map_err(|_| format!("connecting to {host}:{port} timed out"))??;
        let connection: Box<dyn Connection> = if config.tls {
            let tls = ClientConfig::with_platform_verifier()?;
            let name = ServerName::try_from(host.to_owned())?;
            Box::new(TlsConnector::from(Arc::new(tls)).connect(name, tcp).await?)
        } else {
            Box::new(tcp)
        };
        let mut client = Client::new(connection);
        within("the greeting", client.read_response())
            .await??
            .ok_or("the server closed the connection before greeting")?;
        // Only the server's answer: the error never repeats the password.
        let session = within(
            "logging in",
            client.login(&config.username, &config.password),
        )
        .await?
        .map_err(|(error, _)| format!("logging in as {} failed: {error}", config.username))?;
        Ok(Self { session })
    }

    /// The folders that hold mail, without the excluded ones: by name, and Trash, Junk and
    /// Drafts by their special use (RFC 6154), whatever they are called.
    pub async fn folders(&mut self, excluded: &[String]) -> Result<Vec<String>, PluginError> {
        let names: Vec<_> = within("listing the folders", async {
            self.session
                .list(Some(""), Some("*"))
                .await?
                .try_collect()
                .await
        })
        .await??;
        let excluded: HashSet<String> = excluded.iter().map(|name| name.to_lowercase()).collect();
        let mut folders: Vec<String> = names
            .iter()
            .filter(|name| {
                !name.attributes().iter().any(|attribute| {
                    matches!(
                        attribute,
                        NameAttribute::NoSelect
                            | NameAttribute::Trash
                            | NameAttribute::Junk
                            | NameAttribute::Drafts
                    )
                })
            })
            .map(|name| name.name().to_owned())
            .filter(|name| !excluded.contains(&name.to_lowercase()))
            .collect();
        folders.sort();
        Ok(folders)
    }

    pub async fn list(&mut self, folder: &str) -> Result<Listing, PluginError> {
        let (uid_validity, exists) = self.examine(folder).await?;
        // Some servers answer `1:*` in an empty folder with BAD.
        if exists == 0 {
            return Ok(Listing {
                uid_validity,
                mails: Vec::new(),
            });
        }
        let fetches: Vec<Fetch> = within(&format!("listing {folder}"), async {
            self.session
                .uid_fetch("1:*", "(UID INTERNALDATE ENVELOPE)")
                .await?
                .try_collect()
                .await
        })
        .await??;
        let mails = fetches
            .iter()
            .map(|fetch| {
                let uid = fetch
                    .uid
                    .ok_or("the server listed a mail without its UID")?;
                Ok(Listed {
                    uid,
                    id: listed_id(fetch)
                        .ok_or_else(|| format!("{folder}: mail {uid} has no envelope"))?,
                    received: fetch
                        .internal_date()
                        .ok_or_else(|| format!("{folder}: mail {uid} has no arrival date"))?
                        .to_utc(),
                })
            })
            .collect::<Result<_, PluginError>>()?;
        Ok(Listing {
            uid_validity,
            mails,
        })
    }

    /// The raw mails of `uids` in `folder`, by UID. A mail deleted since the listing is missing.
    /// The folder's UIDs must still be the listing's.
    pub async fn mails(
        &mut self,
        folder: &str,
        uid_validity: u32,
        uids: &[u32],
    ) -> Result<HashMap<u32, Vec<u8>>, PluginError> {
        if self.examine(folder).await?.0 != uid_validity {
            return Err(format!(
                "{folder} changed its UIDVALIDITY during the run, the next run retries"
            )
            .into());
        }
        let set: Vec<String> = uids.iter().map(u32::to_string).collect();
        let fetches: Vec<Fetch> = within(&format!("fetching mails of {folder}"), async {
            self.session
                // PEEK: indexing a mail doesn't mark it read.
                .uid_fetch(set.join(","), "(UID BODY.PEEK[])")
                .await?
                .try_collect()
                .await
        })
        .await??;
        Ok(fetches
            .iter()
            .filter_map(|fetch| Some((fetch.uid?, fetch.body()?.to_vec())))
            .collect())
    }

    pub async fn logout(mut self) -> Result<(), PluginError> {
        within("logging out", self.session.logout()).await??;
        Ok(())
    }

    /// Opens `folder` read-only and returns its UIDVALIDITY and how many mails it holds.
    async fn examine(&mut self, folder: &str) -> Result<(u32, u32), PluginError> {
        let mailbox = within(&format!("opening {folder}"), self.session.examine(folder)).await??;
        let uid_validity = mailbox
            .uid_validity
            .ok_or_else(|| format!("{folder} has no UIDVALIDITY"))?;
        Ok((uid_validity, mailbox.exists))
    }
}

/// The Message-ID from the envelope, or for a mail without one, a stand-in that stays the same
/// as long as the mail does, also in another folder.
fn listed_id(fetch: &Fetch) -> Option<String> {
    let envelope = fetch.envelope()?;
    if let Some(id) = envelope.message_id.as_deref() {
        let id = message_id(&String::from_utf8_lossy(id));
        if !id.is_empty() {
            return Some(id);
        }
    }
    let mut hash = Sha256::new();
    for part in [envelope.date.as_deref(), envelope.subject.as_deref()] {
        hash.update(part.unwrap_or_default());
        hash.update([0]);
    }
    for address in envelope.from.iter().flatten() {
        hash.update(address_bytes(address));
    }
    let hex: String = hash.finalize()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(format!("no-message-id-{hex}"))
}

fn address_bytes(address: &Address) -> Vec<u8> {
    [&address.mailbox, &address.host]
        .into_iter()
        .flat_map(|part| {
            part.as_deref()
                .unwrap_or_default()
                .iter()
                .copied()
                .chain([0])
        })
        .collect()
}

async fn within<T>(what: &str, future: impl Future<Output = T>) -> Result<T, PluginError> {
    timeout(COMMAND_TIMEOUT, future)
        .await
        .map_err(|_| format!("{what} timed out").into())
}
