use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use crate::{
    ChangeOperation,
    signal_cli::{Group, Incoming, Named, Received, Target},
};

/// The chat archive's SQLite database. The receiver is its only writer.
#[derive(Clone)]
pub struct Store {
    pub(crate) pool: SqlitePool,
    pub(crate) store_id: String,
}

impl Store {
    /// Opens the database, creating and migrating it as needed.
    pub async fn open(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            // The history import writes while the receiver does.
            .busy_timeout(Duration::from_secs(30))
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::migrate!().run(&pool).await?;
        let store_id = sqlx::query_scalar("SELECT store_id FROM archive")
            .fetch_one(&pool)
            .await?;
        Ok(Self { pool, store_id })
    }

    /// Records the account's own ACI on first use, and that its device is linked. A daemon
    /// serving another account is not this chat archive's collector.
    pub(crate) async fn adopt_self_aci(&self, aci: &str) -> Result<(), crate::receiver::Fatal> {
        sqlx::query("UPDATE archive SET self_aci = ? WHERE self_aci IS NULL")
            .bind(aci)
            .execute(&self.pool)
            .await?;
        let stored: String = sqlx::query_scalar("SELECT self_aci FROM archive")
            .fetch_one(&self.pool)
            .await?;
        if stored != aci {
            return Err("signal-cli serves another account than the chat archive holds".into());
        }
        sqlx::query("UPDATE archive SET unlinked = 0")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records that signal-cli lists no account although this chat archive adopted one. signal-cli
    /// v0.14.9 drops an account whose device was removed: on the server's authentication failure
    /// `ReceiveHelper` marks it unregistered and closes its manager, which
    /// `MultiAccountManagerImpl` then removes, and on restart `SignalAccountFiles` skips it.
    /// Before the first link, no account is just not linked yet.
    pub(crate) async fn mark_unlinked(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE archive SET unlinked = 1 WHERE self_aci IS NOT NULL")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Takes over current contact and group names and journals a rename for every chat whose
    /// title or authors' names changed, so the plugin re-renders exactly those chats.
    pub(crate) async fn refresh_names(
        &self,
        contacts: &[Named],
        groups: &[Group],
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for contact in contacts {
            // A number signal-cli no longer reports is kept: the messages still came from it.
            let changed = sqlx::query(
                "INSERT INTO recipients (aci, number, name) VALUES (?1, ?2, ?3)
                 ON CONFLICT (aci) DO UPDATE SET
                     number = coalesce(excluded.number, number), name = excluded.name
                 WHERE number IS NOT coalesce(excluded.number, number)
                     OR name IS NOT excluded.name",
            )
            .bind(&contact.aci)
            .bind(&contact.number)
            .bind(&contact.name)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                == 1;
            if changed {
                sqlx::query(
                    "INSERT INTO changes (recorded_at, operation, chat)
                     SELECT ?1, 'rename', chat FROM (
                         SELECT id AS chat FROM chats WHERE id = ?2 AND kind = 'direct'
                         UNION SELECT DISTINCT chat FROM messages WHERE author = ?2
                     ) ORDER BY chat",
                )
                .bind(now())
                .bind(&contact.aci)
                .execute(&mut *tx)
                .await?;
            }
        }
        for group in groups {
            let renamed = sqlx::query(
                "UPDATE chats SET name = ?1 WHERE id = ?2 AND kind = 'group' AND name IS NOT ?1",
            )
            .bind(group.name())
            .bind(&group.id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                == 1;
            if renamed {
                sqlx::query(
                    "INSERT INTO changes (recorded_at, operation, chat) VALUES (?, 'rename', ?)",
                )
                .bind(now())
                .bind(&group.id)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await
    }

    pub(crate) async fn record_arrival(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE archive SET last_received_at = ?")
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn apply(&self, incoming: Incoming) -> Result<(), sqlx::Error> {
        match incoming {
            Incoming::Message(message) => self.insert(message).await.map(drop),
            Incoming::Edit {
                target,
                edited_at,
                text,
            } => self.edit(target, edited_at, text).await,
            Incoming::Delete(target) => self.delete(target).await,
        }
    }

    /// Stores a message unless its identity is already present, and journals the insert.
    /// Records its revisions either way. Returns whether it was absent.
    pub(crate) async fn insert(&self, message: Received) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        // A group's later renames come from the name refresh, which journals them.
        sqlx::query(
            "INSERT INTO chats (id, kind, name) VALUES (?, ?, ?) ON CONFLICT (id) DO NOTHING",
        )
        .bind(&message.chat)
        .bind(message.kind)
        .bind(&message.group_name)
        .execute(&mut *tx)
        .await?;
        for (aci, number) in &message.numbers {
            sqlx::query(
                // Envelopes only fill in unknown numbers; a changed one comes with the name
                // refresh, which journals it.
                "INSERT INTO recipients (aci, number) VALUES (?, ?)
                 ON CONFLICT (aci) DO UPDATE SET number = excluded.number WHERE number IS NULL",
            )
            .bind(aci)
            .bind(number)
            .execute(&mut *tx)
            .await?;
        }
        let attachments =
            serde_json::to_string(&message.attachments).expect("attachments serialize");
        let inserted = sqlx::query(
            "INSERT INTO messages (chat, sent_at, author, from_me, text, attachments, edited)
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(&message.chat)
        .bind(message.sent_at)
        .bind(&message.author)
        .bind(message.from_me)
        .bind(&message.text)
        .bind(attachments)
        .bind(!message.revisions.is_empty())
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if inserted {
            journal(
                &mut tx,
                ChangeOperation::Insert,
                &message.chat,
                message.sent_at,
                &message.author,
            )
            .await?;
        }
        for edited_at in message.revisions {
            record_revision(
                &mut tx,
                &message.chat,
                &message.author,
                edited_at,
                message.sent_at,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(inserted)
    }

    pub(crate) async fn self_aci(&self) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT self_aci FROM archive")
            .fetch_one(&self.pool)
            .await
    }

    /// Replaces the text of the message `target` names and records the revision. Edits of
    /// unknown or deleted messages and redelivered edits change nothing, and an edit arriving
    /// after a newer one only records its revision.
    async fn edit(
        &self,
        target: Target,
        edited_at: i64,
        text: Option<String>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let Some((sent_at, false)) = resolve(&mut tx, &target).await? else {
            return Ok(());
        };
        if !record_revision(&mut tx, &target.chat, &target.author, edited_at, sent_at).await? {
            return Ok(());
        }
        let latest = sqlx::query(
            "UPDATE messages SET text = ?1, edited = 1
             WHERE chat = ?2 AND sent_at = ?3 AND author = ?4 AND NOT EXISTS (SELECT 1 FROM edits
                 WHERE chat = ?2 AND author = ?4 AND sent_at = ?3 AND edited_at > ?5)",
        )
        .bind(text)
        .bind(&target.chat)
        .bind(sent_at)
        .bind(&target.author)
        .bind(edited_at)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if latest {
            journal(
                &mut tx,
                ChangeOperation::Edit,
                &target.chat,
                sent_at,
                &target.author,
            )
            .await?;
        }
        tx.commit().await
    }

    /// Turns the message `target` names into a tombstone, see the `deleted` column. A delete
    /// that arrives before its message leaves a tombstone too, because the message can still
    /// come: redelivered by signal-cli, or from a history import of an older export. That
    /// tombstone sits at the timestamp the delete names, so it misses an original that arrives
    /// later only if the delete named a later revision. Its chat row is created as well, so an
    /// otherwise unknown chat is listed without visible messages.
    async fn delete(&self, target: Target) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        match resolve(&mut tx, &target).await? {
            Some((_, true)) => return Ok(()),
            Some((sent_at, false)) => {
                sqlx::query(
                    "UPDATE messages SET deleted = 1, text = NULL, attachments = '[]'
                     WHERE chat = ? AND sent_at = ? AND author = ?",
                )
                .bind(&target.chat)
                .bind(sent_at)
                .bind(&target.author)
                .execute(&mut *tx)
                .await?;
                journal(
                    &mut tx,
                    ChangeOperation::Delete,
                    &target.chat,
                    sent_at,
                    &target.author,
                )
                .await?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO chats (id, kind) VALUES (?, ?) ON CONFLICT (id) DO NOTHING",
                )
                .bind(&target.chat)
                .bind(target.kind)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO messages (chat, sent_at, author, from_me, attachments, deleted)
                     VALUES (?1, ?2, ?3, IFNULL(?3 = (SELECT self_aci FROM archive), 0), '[]', 1)",
                )
                .bind(&target.chat)
                .bind(target.sent_at)
                .bind(&target.author)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await
    }
}

/// The original send and tombstone flag of the message `target` names by any of its revision
/// timestamps. Signal-Desktop v8.32.0 addresses an edit to the latest revision
/// (`ts/util/sendEditedMessage.preload.ts`, `getMessageSentTimestamp` includes edits) and matches
/// a delete against every revision (`ts/messageModifiers/Deletes.preload.ts`).
async fn resolve(
    conn: &mut SqliteConnection,
    target: &Target,
) -> Result<Option<(i64, bool)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT sent_at, deleted FROM messages WHERE chat = ?1 AND author = ?2 AND sent_at IN
             (?3, (SELECT sent_at FROM edits WHERE chat = ?1 AND author = ?2 AND edited_at = ?3))",
    )
    .bind(&target.chat)
    .bind(&target.author)
    .bind(target.sent_at)
    .fetch_optional(conn)
    .await
}

/// Records that revision `edited_at` of the message `(chat, sent_at, author)` exists, so edits
/// and deletes addressed to it resolve. Returns false if it was already recorded.
async fn record_revision(
    conn: &mut SqliteConnection,
    chat: &str,
    author: &str,
    edited_at: i64,
    sent_at: i64,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "INSERT INTO edits (chat, author, edited_at, sent_at) VALUES (?, ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(chat)
    .bind(author)
    .bind(edited_at)
    .bind(sent_at)
    .execute(conn)
    .await?
    .rows_affected()
        == 1)
}

async fn journal(
    conn: &mut SqliteConnection,
    operation: ChangeOperation,
    chat: &str,
    sent_at: i64,
    author: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO changes (recorded_at, operation, chat, sent_at, author)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(now())
    .bind(operation)
    .bind(chat)
    .bind(sent_at)
    .bind(author)
    .execute(conn)
    .await?;
    Ok(())
}

fn now() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970");
    i64::try_from(elapsed.as_millis()).expect("milliseconds fit i64")
}
