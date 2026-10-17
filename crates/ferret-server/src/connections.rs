//! Connections: a plugin configured by a user for one origin, with its schedule and the source
//! root it owns. Workers run them (`runs`), the UI manages them over GraphQL.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use croner::Cron;
use serde_json::{Map, Value};
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::{
    api_auth::ApiCaller,
    credentials::{self, ConfigKey, Owner},
    error::ApiError,
    icons, sources,
};

/// The channel that wakes waiting claims: a connection became due (run now, a new or changed
/// connection). Every server replica listens, so a claim waiting on any of them wakes.
pub const RUNS_CHANNEL: &str = "ferret_runs";

pub struct PluginType {
    pub name: String,
    pub config_schema: Value,
}

pub struct Connection {
    pub id: Uuid,
    pub name: String,
    /// A lucide icon name, see `icons`.
    pub icon: String,
    pub plugin: String,
    pub source: String,
    /// Without the credentials, which never leave the server except to a claimed run.
    pub config: Value,
    pub schedule: String,
    pub enabled: bool,
    pub next_run_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

pub struct PluginRun {
    pub id: Uuid,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub stats: Option<Value>,
    pub log: String,
    pub error: Option<String>,
}

pub struct NewConnection {
    pub name: String,
    pub icon: String,
    pub plugin: String,
    pub source: String,
    pub config: Value,
    pub schedule: String,
}

pub struct ConnectionUpdate {
    pub name: String,
    pub icon: String,
    pub config: Value,
    pub schedule: String,
    pub enabled: bool,
}

pub enum ConnectionError {
    /// Rejected input, worded for the user.
    Invalid(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ConnectionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ConnectionError> {
    Err(ConnectionError::Invalid(message.into()))
}

fn validate_label(name: &str, icon: &str) -> Result<(), ConnectionError> {
    if name.trim().is_empty() {
        return invalid("a connection needs a name");
    }
    if !icons::is_valid(icon) {
        return invalid(format!("{icon:?} is not an icon name like `git-branch`"));
    }
    Ok(())
}

pub async fn plugin_types(pool: &PgPool) -> Result<Vec<PluginType>, sqlx::Error> {
    sqlx::query_as!(
        PluginType,
        "select name, config_schema from plugin_types order by name"
    )
    .fetch_all(pool)
    .await
}

pub async fn list(pool: &PgPool, user_id: Uuid) -> Result<Vec<Connection>, sqlx::Error> {
    sqlx::query_as!(
        Connection,
        "select id, name, icon, plugin, source, config, schedule, enabled, next_run_at, created_at
         from connections where user_id = $1 order by source",
        user_id,
    )
    .fetch_all(pool)
    .await
}

pub async fn find(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Connection>, sqlx::Error> {
    sqlx::query_as!(
        Connection,
        "select id, name, icon, plugin, source, config, schedule, enabled, next_run_at, created_at
         from connections where id = $1 and user_id = $2",
        id,
        user_id,
    )
    .fetch_optional(pool)
    .await
}

/// Creates a connection that runs right away, then on its schedule.
pub async fn create(
    pool: &PgPool,
    config_key: &ConfigKey,
    user_id: Uuid,
    new: NewConnection,
) -> Result<Connection, ConnectionError> {
    validate_label(&new.name, &new.icon)?;
    if !sources::is_valid(&new.source) {
        return invalid(format!(
            "{:?} is not a source path like `notes` or `mail/zoho`",
            new.source
        ));
    }
    next_slot(&new.schedule, Utc::now())?;
    let schema = config_schema(pool, &new.plugin).await?;
    validate_config(&schema, &new.config)?;
    let (config, mut credentials) = credentials::split(&schema, new.config);
    credentials.retain(|_, value| !value.is_null());
    // From here rather than the database: the credentials are sealed for this id.
    let owner = Owner {
        user_id,
        connection_id: Uuid::now_v7(),
    };
    let mut tx = pool.begin().await?;
    // Serializes creations per user, so two overlapping roots can't slip in side by side.
    sqlx::query!("select id from users where id = $1 for update", user_id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(root) = overlapping_root(&mut *tx, user_id, &new.source).await? {
        return invalid(format!(
            "the source {:?} overlaps the connection on {root:?}: connection roots can't contain each other",
            new.source
        ));
    }
    // The connection's first sync would sweep documents that others wrote below its root.
    // ponytail: an API key writing between this count and the commit slips through; a lock on
    // the user's documents would close that, unlikely enough to leave for now.
    let existing = sqlx::query_scalar!(
        r#"select count(*) as "count!" from documents where user_id = $1 and source_covers($2, source)"#,
        user_id,
        new.source,
    )
    .fetch_one(&mut *tx)
    .await?;
    if existing > 0 {
        let documents = if existing == 1 {
            "1 document already lives".to_owned()
        } else {
            format!("{existing} documents already live")
        };
        return invalid(format!(
            "{documents} below {:?}: delete them or pick another root",
            new.source
        ));
    }
    let connection = sqlx::query_as!(
        Connection,
        "insert into connections
             (id, user_id, plugin, source, config, credentials, schedule, next_run_at, created_at,
              name, icon)
         values ($1, $2, $3, $4, $5, $6, $7, now(), now(), $8, $9)
         returning id, name, icon, plugin, source, config, schedule, enabled, next_run_at, created_at",
        owner.connection_id,
        user_id,
        new.plugin,
        new.source,
        config,
        seal(config_key, &credentials, owner),
        new.schedule,
        new.name.trim(),
        new.icon,
    )
    .fetch_one(&mut *tx)
    .await?;
    wake_claims(&mut *tx).await?;
    tx.commit().await?;
    Ok(connection)
}

/// Changes name, icon, config, schedule and whether the connection runs. A new schedule takes
/// effect from now.
///
/// Stored credentials survive only an update that leaves the rest of the config as it is
/// (schedule and enabled may change). Any other change needs every stored credential entered
/// again: otherwise whoever holds a session could point a connection elsewhere, say at their own
/// git server, and its next run would send the stored token there. A credential set to `null`
/// is removed.
pub async fn update(
    pool: &PgPool,
    config_key: &ConfigKey,
    user_id: Uuid,
    id: Uuid,
    update: ConnectionUpdate,
) -> Result<Option<Connection>, ConnectionError> {
    validate_label(&update.name, &update.icon)?;
    let Some(current) = find(pool, user_id, id).await? else {
        return Ok(None);
    };
    let next_run_at = if update.schedule == current.schedule {
        current.next_run_at
    } else {
        next_slot(&update.schedule, Utc::now())?
    };
    let schema = config_schema(pool, &current.plugin).await?;
    let owner = Owner {
        user_id,
        connection_id: id,
    };
    let (config, entered) = credentials::split(&schema, update.config);
    let sealed = sqlx::query_scalar!("select credentials from connections where id = $1", id)
        .fetch_one(pool)
        .await?;
    let credentials = updated_credentials(
        config_key,
        owner,
        &schema,
        sealed.as_deref(),
        config == current.config,
        entered,
    )?;
    validate_config(
        &schema,
        &credentials::merge(config.clone(), credentials.clone()),
    )?;
    let mut tx = pool.begin().await?;
    let connection = sqlx::query_as!(
        Connection,
        "update connections
         set config = $3, credentials = $4, schedule = $5, enabled = $6, next_run_at = $7,
             name = $8, icon = $9
         where id = $1 and user_id = $2
         returning id, name, icon, plugin, source, config, schedule, enabled, next_run_at, created_at",
        id,
        user_id,
        config,
        seal(config_key, &credentials, owner),
        update.schedule,
        update.enabled,
        next_run_at,
        update.name.trim(),
        update.icon,
    )
    .fetch_optional(&mut *tx)
    .await?;
    wake_claims(&mut *tx).await?;
    tx.commit().await?;
    Ok(connection)
}

/// Makes an enabled connection due now. False if there's no such enabled connection.
pub async fn run_now(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let updated = sqlx::query!(
        "update connections set next_run_at = now() where id = $1 and user_id = $2 and enabled",
        id,
        user_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    wake_claims(&mut *tx).await?;
    tx.commit().await?;
    Ok(updated == 1)
}

/// Deletes the connection and every document below its source root, once `confirm_source`
/// repeats the root. Returns how many documents went, `None` if there's no such connection.
pub async fn delete(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    confirm_source: &str,
) -> Result<Option<u64>, ConnectionError> {
    let mut tx = pool.begin().await?;
    let Some(source) = sqlx::query_scalar!(
        "select source from connections where id = $1 and user_id = $2 for update",
        id,
        user_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    if confirm_source != source {
        return invalid(format!("type the source {source:?} to confirm"));
    }
    let deleted = sqlx::query!(
        "delete from documents where user_id = $1 and source_covers($2, source)",
        user_id,
        source,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    // Its runs go with it, which ends a live run's token.
    sqlx::query!("delete from connections where id = $1", id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(deleted))
}

/// The connection's runs, newest first.
pub async fn runs(
    pool: &PgPool,
    connection_id: Uuid,
    limit: i64,
) -> Result<Vec<PluginRun>, sqlx::Error> {
    sqlx::query_as!(
        PluginRun,
        "select id, status, started_at, finished_at, stats, log, error
         from plugin_runs where connection_id = $1
         order by started_at desc limit $2",
        connection_id,
        limit,
    )
    .fetch_all(pool)
    .await
}

/// The first slot of the cron `schedule` after `after`.
pub fn next_slot(schedule: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>, ConnectionError> {
    let cron = Cron::from_str(schedule).map_err(|error| {
        ConnectionError::Invalid(format!("{schedule:?} is not a cron schedule: {error}"))
    })?;
    cron.find_next_occurrence(&after, false)
        .map_err(|error| ConnectionError::Invalid(format!("{schedule:?} never runs: {error}")))
}

async fn config_schema(pool: &PgPool, plugin: &str) -> Result<Value, ConnectionError> {
    match sqlx::query_scalar!(
        "select config_schema from plugin_types where name = $1",
        plugin
    )
    .fetch_optional(pool)
    .await?
    {
        Some(schema) => Ok(schema),
        None => invalid(format!("no worker offers the plugin {plugin:?}")),
    }
}

fn validate_config(schema: &Value, config: &Value) -> Result<(), ConnectionError> {
    let validator = jsonschema::validator_for(schema).map_err(|error| {
        ConnectionError::Invalid(format!("the plugin's config schema is broken: {error}"))
    })?;
    let Some(error) = validator.iter_errors(config).next() else {
        return Ok(());
    };
    let path = error.instance_path().to_string();
    let at = if path.is_empty() {
        String::new()
    } else {
        format!(" at {path}")
    };
    // jsonschema's messages quote the values they reject, and the config may hold stored
    // credentials, which the API never returns. Then only the path is said.
    let secret_fields = credentials::secret_fields(schema);
    let under_secret = secret_fields
        .iter()
        .any(|name| path == format!("/{name}") || path.starts_with(&format!("/{name}/")));
    let message = error.to_string();
    let quotes_secret = secret_fields
        .iter()
        .filter_map(|name| config.get(*name))
        .map(|value| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        })
        .any(|secret| !secret.is_empty() && message.contains(&secret));
    if under_secret || quotes_secret {
        return invalid(format!("invalid config{at}"));
    }
    invalid(format!("invalid config{at}: {message}"))
}

fn seal(config_key: &ConfigKey, credentials: &Map<String, Value>, owner: Owner) -> Option<Vec<u8>> {
    (!credentials.is_empty()).then(|| config_key.seal(credentials, owner))
}

/// The credentials an update stores: the `entered` ones, on top of the stored ones while the
/// config stays the same. Fails when a credential has to be entered again but wasn't: after a
/// config change, or when the stored ones can't be opened (the database was restored without
/// its key). `null` removes a credential.
fn updated_credentials(
    config_key: &ConfigKey,
    owner: Owner,
    schema: &Value,
    sealed: Option<&[u8]>,
    config_unchanged: bool,
    entered: Map<String, Value>,
) -> Result<Map<String, Value>, ConnectionError> {
    let mut credentials = Map::new();
    if let Some(sealed) = sealed {
        let (required, reason): (Vec<String>, _) = match config_key.open(sealed, owner) {
            Ok(stored) if config_unchanged => {
                credentials = stored;
                (Vec::new(), "")
            }
            Ok(stored) => (
                stored.into_iter().map(|(name, _)| name).collect(),
                "a changed configuration needs its credentials again",
            ),
            // Which ones were stored is sealed too, so all of them.
            Err(_) => (
                credentials::secret_fields(schema)
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                "the stored credentials can't be decrypted with FERRET_CONFIG_KEY",
            ),
        };
        let missing: Vec<&str> = required
            .iter()
            .filter(|name| !entered.contains_key(*name))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return invalid(format!("{reason}: enter {}", missing.join(", ")));
        }
    }
    credentials.extend(entered);
    credentials.retain(|_, value| !value.is_null());
    Ok(credentials)
}

/// Wakes waiting claims on every server replica once the transaction commits.
async fn wake_claims(executor: impl PgExecutor<'_>) -> Result<(), sqlx::Error> {
    sqlx::query!("select pg_notify($1, '')", RUNS_CHANNEL)
        .execute(executor)
        .await?;
    Ok(())
}

/// The root of a connection of `user_id` that contains `source` or lies below it.
async fn overlapping_root(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    source: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        "select source from connections
         where user_id = $1 and (source_covers(source, $2) or source_covers($2, source))
         limit 1",
        user_id,
        source,
    )
    .fetch_optional(executor)
    .await
}

/// Every source below a connection's root belongs to that connection: an API key writing there
/// would be swept by the connection's next sync, or would sweep the connection's documents.
/// Plugin runs only write below their own root, which no other root overlaps.
///
/// `sweeps` is for syncs, which delete below their source: then a connection root below the
/// source is just as off-limits as one above it.
pub async fn check_api_key_may_write(
    pool: &PgPool,
    caller: &ApiCaller,
    source: &str,
    sweeps: bool,
) -> Result<(), ApiError> {
    if caller.is_plugin_run() {
        return Ok(());
    }
    let root = if sweeps {
        overlapping_root(pool, caller.user_id, source).await?
    } else {
        sqlx::query_scalar!(
            "select source from connections where user_id = $1 and source_covers(source, $2) limit 1",
            caller.user_id,
            source,
        )
        .fetch_optional(pool)
        .await?
    };
    match root {
        Some(root) => Err(ApiError::forbidden(&format!(
            "the source {source:?} belongs to the connection on {root:?}: API keys can't write there"
        ))),
        None => Ok(()),
    }
}
