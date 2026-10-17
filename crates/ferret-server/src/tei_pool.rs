//! The TEI pool: the configured TEIs, which model each one serves (discovered from its `/info`),
//! and the switches admins flip in the UI (stored in `teis`).

use std::{
    sync::{
        Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use sqlx::PgPool;

use crate::{
    config::{ModelConfig, TeiConfig},
    tei::{self, Info},
};

/// Chunks per pump batch at most, TEI's default client batch size.
pub const MAX_BATCH_SIZE: usize = 32;
/// A pump batch may take this long, see `BatchSize` for a TEI that is slower.
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(60);
/// What a pump batch should take on a TEI that also answers queries: a query waits behind it.
const SHARED_BATCH_TARGET: Duration = Duration::from_secs(2);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Tei {
    pub url: String,
    pub external: bool,
    state: RwLock<TeiState>,
    /// The switches as the database had them at the last look: only a change there overrides
    /// the state, so a look that started before an admin's change doesn't undo it.
    stored_switches: Mutex<Option<Switches>>,
    batch_size: BatchSize,
    /// A pump batch of this process is in flight.
    busy: AtomicBool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TeiState {
    pub switches: Switches,
    pub discovery: Discovery,
}

/// What an admin allows a TEI to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Switches {
    pub enabled: bool,
    pub queries: bool,
    pub embedding: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Discovery {
    /// Not asked yet.
    Pending,
    /// Its `/info` doesn't answer.
    Offline(String),
    /// Serves a model no configured one has as its `model_id`.
    Unused(Info),
    /// Serves a configured model (by index), but can't be used for it.
    Unsuitable {
        info: Info,
        model: usize,
        problem: String,
    },
    Serving {
        info: Info,
        model: usize,
    },
}

impl Tei {
    pub fn new(config: &TeiConfig) -> Self {
        Self {
            url: config.url.clone(),
            external: config.external,
            state: RwLock::new(TeiState {
                switches: Switches::defaults(config.external),
                discovery: Discovery::Pending,
            }),
            stored_switches: Mutex::new(None),
            batch_size: BatchSize::new(),
            busy: AtomicBool::new(false),
        }
    }

    pub fn state(&self) -> TeiState {
        self.state.read().expect("no panics while updating").clone()
    }

    /// Changes the state in place, and returns whether it changed.
    pub fn update(&self, change: impl FnOnce(&mut TeiState)) -> bool {
        let mut state = self.state.write().expect("no panics while updating");
        let before = state.clone();
        change(&mut state);
        *state != before
    }

    /// Takes the switches the database has, if they changed there since the last look.
    pub fn apply_stored_switches(&self, stored: Switches) -> bool {
        let mut last = self
            .stored_switches
            .lock()
            .expect("no panics while updating");
        if *last == Some(stored) {
            return false;
        }
        *last = Some(stored);
        self.update(|state| state.switches = stored)
    }

    /// The model index and served model name, if the TEI serves a model and is enabled for `use`.
    pub fn serving(&self, use_for: Use) -> Option<(usize, String)> {
        let state = self.state();
        let switches = state.switches;
        let allowed = switches.enabled
            && match use_for {
                Use::Queries => switches.queries,
                Use::Embedding => switches.embedding,
            };
        match state.discovery {
            Discovery::Serving { info, model } if allowed => {
                Some((model, info.served_model_name().to_owned()))
            }
            _ => None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    /// Marks the TEI busy until the guard drops.
    pub fn busy(&self) -> Busy<'_> {
        self.busy.store(true, Ordering::Relaxed);
        Busy(&self.busy)
    }

    /// Chunks per pump batch in this process: at most what the TEI takes per request.
    pub fn batch_size(&self) -> usize {
        let limit = match self.state().discovery {
            Discovery::Serving { info, .. } => info.max_client_batch_size,
            _ => MAX_BATCH_SIZE,
        };
        self.batch_size.get().min(limit).max(1)
    }

    /// Adapts the batch size to how long a batch took: short batches on a TEI that also answers
    /// queries, so a query never waits long behind one.
    pub fn batch_succeeded(&self, took: Duration) {
        let target = if self.state().switches.queries {
            SHARED_BATCH_TARGET
        } else {
            BATCH_TIMEOUT
        };
        self.batch_size.succeeded(took, target);
    }

    pub fn batch_timed_out(&self) {
        self.batch_size.timed_out();
    }
}

impl Discovery {
    /// Why a TEI is of no use, `None` while it serves.
    pub fn problem(&self) -> Option<String> {
        match self {
            Self::Serving { .. } => None,
            Self::Pending => Some("not asked yet".to_owned()),
            Self::Offline(error) => Some(error.clone()),
            Self::Unused(info) => Some(format!(
                "serves {}, which no configured model uses",
                info.model_id
            )),
            Self::Unsuitable { problem, .. } => Some(problem.clone()),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Use {
    Queries,
    Embedding,
}

pub struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

impl Switches {
    /// A TEI of the deployment does everything. An external one only embeds: searches shouldn't
    /// depend on a laptop that may go to sleep.
    fn defaults(external: bool) -> Self {
        Self {
            enabled: true,
            queries: !external,
            embedding: true,
        }
    }
}

/// Asks the TEI at `url` which model it serves, and matches it to the configured `models`.
/// `min_input_length` is the longest input the pump sends, in characters.
pub async fn discover(
    http: &reqwest::Client,
    url: &str,
    models: &[ModelConfig],
    min_input_length: usize,
) -> Discovery {
    let info = match tei::info(http, url, DISCOVERY_TIMEOUT).await {
        Ok(info) => info,
        Err(error) => return Discovery::Offline(error.to_string()),
    };
    let Some(model) = models
        .iter()
        .position(|model| model.model_id == info.model_id)
    else {
        return Discovery::Unused(info);
    };
    // Answers are checked against the name they come back under. Only the model id tells the
    // models apart: under a fixed name, a TEI could switch models unnoticed.
    if info.served_model_name() != info.model_id {
        let problem = format!(
            "serves its model as {:?}, so its answers can't be checked: start it without \
             --served-model-name",
            info.served_model_name()
        );
        return Discovery::Unsuitable {
            info,
            model,
            problem,
        };
    }
    // Tokens against characters, one to one: Latin text practically never takes more than one
    // token per character.
    if info.max_input_length < min_input_length {
        let problem = format!(
            "takes at most {} tokens, embedded inputs (title and chunk) have up to \
             {min_input_length} characters",
            info.max_input_length
        );
        return Discovery::Unsuitable {
            info,
            model,
            problem,
        };
    }
    Discovery::Serving { info, model }
}

/// Makes `teis` match the config: new TEIs get their default switches, removed ones go.
pub async fn sync_config(pool: &PgPool, teis: &[TeiConfig]) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let urls: Vec<String> = teis.iter().map(|tei| tei.url.clone()).collect();
    sqlx::query!("delete from teis where url <> all($1)", &urls)
        .execute(&mut *tx)
        .await?;
    for tei in teis {
        let switches = Switches::defaults(tei.external);
        sqlx::query!(
            "insert into teis (url, enabled, queries, embedding) values ($1, $2, $3, $4)
             on conflict (url) do nothing",
            tei.url,
            switches.enabled,
            switches.queries,
            switches.embedding,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Every TEI's switches by URL.
pub async fn switches(pool: &PgPool) -> Result<Vec<(String, Switches)>, sqlx::Error> {
    let rows = sqlx::query!("select url, enabled, queries, embedding from teis")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.url,
                Switches {
                    enabled: row.enabled,
                    queries: row.queries,
                    embedding: row.embedding,
                },
            )
        })
        .collect())
}

/// Changes the switches given, returns `None` for a TEI that isn't configured.
pub async fn update_switches(
    pool: &PgPool,
    url: &str,
    enabled: Option<bool>,
    queries: Option<bool>,
    embedding: Option<bool>,
) -> Result<Option<Switches>, sqlx::Error> {
    let row = sqlx::query!(
        "update teis set enabled = coalesce($2, enabled), queries = coalesce($3, queries),
                         embedding = coalesce($4, embedding)
         where url = $1
         returning enabled, queries, embedding",
        url,
        enabled,
        queries,
        embedding,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| Switches {
        enabled: row.enabled,
        queries: row.queries,
        embedding: row.embedding,
    }))
}

/// A TEI's last failure, until its next success.
pub async fn last_error(
    pool: &PgPool,
    url: &str,
) -> Result<Option<(String, chrono::DateTime<chrono::Utc>)>, sqlx::Error> {
    let row = sqlx::query!(
        "select last_error, last_error_at from teis where url = $1",
        url
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|row| row.last_error.zip(row.last_error_at)))
}

/// Shown in Admin until the TEI's next success.
pub async fn record_error(pool: &PgPool, url: &str, error: &str) {
    let recorded = sqlx::query!(
        "update teis set last_error = $2, last_error_at = now() where url = $1",
        url,
        error,
    )
    .execute(pool)
    .await;
    if let Err(error) = recorded {
        tracing::error!(%error, "can't record a TEI's error");
    }
}

pub async fn clear_error(pool: &PgPool, url: &str) {
    let cleared = sqlx::query!(
        "update teis set last_error = null, last_error_at = null
         where url = $1 and last_error is not null",
        url
    )
    .execute(pool)
    .await;
    if let Err(error) = cleared {
        tracing::error!(%error, "can't clear a TEI's error");
    }
}

/// How many chunks the pump claims per batch on one TEI. TEI doesn't cancel a request the pump
/// gave up on, so on a CPU that needs longer than `BATCH_TIMEOUT` for a full batch, retrying the
/// same size queues behind the abandoned one and times out again, forever. Halving on a timeout
/// lets the abandoned work drain. Growing only after a batch that took less than half its target
/// keeps the size below the next timeout, and a GPU TEI at the maximum. A batch that took longer
/// than its target halves the size too, which keeps batches short on a TEI that answers queries.
struct BatchSize(AtomicUsize);

impl BatchSize {
    fn new() -> Self {
        Self(AtomicUsize::new(MAX_BATCH_SIZE))
    }

    fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    fn timed_out(&self) {
        self.0.update(Ordering::Relaxed, Ordering::Relaxed, |size| {
            (size / 2).max(1)
        });
    }

    fn succeeded(&self, took: Duration, target: Duration) {
        if took > target {
            self.timed_out();
        } else if took < target / 2 {
            self.0.update(Ordering::Relaxed, Ordering::Relaxed, |size| {
                (size + 1).min(MAX_BATCH_SIZE)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TEI that computes every request to the end, one after another, even the ones the pump
    /// gave up on. Returns how many chunks the pump embedded within `within`, and the longest
    /// successful batch.
    fn pump_against(
        size: &BatchSize,
        per_chunk: Duration,
        within: Duration,
        target: Duration,
    ) -> (usize, Duration) {
        let (mut now, mut tei_busy_until, mut embedded) = (Duration::ZERO, Duration::ZERO, 0);
        let mut longest = Duration::ZERO;
        while now < within {
            let batch = size.get();
            tei_busy_until = now.max(tei_busy_until) + per_chunk * batch as u32;
            let took = tei_busy_until - now;
            if took > BATCH_TIMEOUT {
                size.timed_out();
                now += BATCH_TIMEOUT;
            } else {
                size.succeeded(took, target);
                embedded += batch;
                longest = longest.max(took);
                now = tei_busy_until;
            }
        }
        (embedded, longest)
    }

    #[test]
    fn a_tei_too_slow_for_a_full_batch_still_makes_progress_and_a_fast_one_gets_full_batches() {
        let size = BatchSize::new();
        // A CPU that needs 70 s for 32 long chunks. A fixed batch of 32 embeds nothing.
        let slow = Duration::from_secs(70) / 32;
        let ten_minutes = Duration::from_secs(600);
        let (embedded, _) = pump_against(&size, slow, ten_minutes, BATCH_TIMEOUT);
        // At least three quarters of what the TEI could compute in that time.
        assert!(
            embedded * 4 >= (ten_minutes.as_secs_f64() / slow.as_secs_f64()) as usize * 3,
            "{embedded}"
        );

        pump_against(
            &size,
            Duration::from_millis(10),
            Duration::from_secs(60),
            BATCH_TIMEOUT,
        );
        assert_eq!(size.get(), MAX_BATCH_SIZE);
    }

    #[test]
    fn batches_on_a_tei_that_answers_queries_settle_below_their_target() {
        let size = BatchSize::new();
        let per_chunk = Duration::from_millis(400);

        // The first batches are too long, after that they stay at the target or below.
        pump_against(
            &size,
            per_chunk,
            Duration::from_secs(60),
            SHARED_BATCH_TARGET,
        );
        let (_, longest) = pump_against(
            &size,
            per_chunk,
            Duration::from_secs(120),
            SHARED_BATCH_TARGET,
        );

        assert!(longest <= SHARED_BATCH_TARGET, "{longest:?}");
        assert!(size.get() >= 2, "{}", size.get());
    }
}
