use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use crate::{ChatKind, signal_cli::Received};

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

    /// Records the account's own ACI on first use. A daemon serving another account is not this
    /// chat archive's collector.
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
        Ok(())
    }

    pub(crate) async fn record_arrival(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE archive SET last_received_at = ?")
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Stores a message unless its identity is already present, and journals the insert.
    pub(crate) async fn insert(&self, message: Received) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO chats (id, kind) VALUES (?, ?) ON CONFLICT (id) DO NOTHING")
            .bind(&message.chat)
            .bind(kind_name(message.kind))
            .execute(&mut *tx)
            .await?;
        for (aci, number) in &message.numbers {
            sqlx::query(
                "INSERT INTO recipients (aci, number) VALUES (?, ?)
                 ON CONFLICT (aci) DO UPDATE SET number = excluded.number",
            )
            .bind(aci)
            .bind(number)
            .execute(&mut *tx)
            .await?;
        }
        let attachments =
            serde_json::to_string(&message.attachments).expect("attachments serialize");
        let inserted = sqlx::query(
            "INSERT INTO messages (chat, sent_at, author, from_me, text, attachments)
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(&message.chat)
        .bind(message.sent_at)
        .bind(&message.author)
        .bind(message.from_me)
        .bind(&message.text)
        .bind(attachments)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if inserted {
            sqlx::query(
                "INSERT INTO changes (recorded_at, operation, chat, sent_at, author)
                 VALUES (?, 'insert', ?, ?, ?)",
            )
            .bind(now())
            .bind(&message.chat)
            .bind(message.sent_at)
            .bind(&message.author)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
}

fn kind_name(kind: ChatKind) -> &'static str {
    match kind {
        ChatKind::Direct => "direct",
        ChatKind::Group => "group",
        ChatKind::NoteToSelf => "note_to_self",
    }
}

fn now() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970");
    i64::try_from(elapsed.as_millis()).expect("milliseconds fit i64")
}
