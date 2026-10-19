//! Replica-local database snapshots. Endpoint health is deliberately not part of a snapshot.

use crate::{
    embeddings::{self, Embedding},
    model_registry::{self, StoredModel},
    writer_stats,
    writers::Writers,
};
use parking_lot::Mutex;
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::Hash,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::time::Instant;
use uuid::Uuid;

const PROGRESS_TTL: Duration = Duration::from_secs(5);
const TOTALS_TTL: Duration = Duration::from_secs(60);
const MAX_SCOPES: usize = 128;

type CachedResult<T> = Result<Arc<T>, Arc<sqlx::Error>>;
type Snapshot<T> = Option<(Instant, CachedResult<T>)>;
struct Slot<T> {
    value: AsyncMutex<Snapshot<T>>,
}

struct Cache<K, T> {
    slots: Mutex<HashMap<K, Arc<Slot<T>>>>,
    available: Notify,
    ttl: Duration,
}

// A cancelled loader must also wake callers waiting for bounded cache capacity.
struct WakeOnDrop<'a>(&'a Notify);
impl Drop for WakeOnDrop<'_> {
    fn drop(&mut self) {
        self.0.notify_waiters();
    }
}

impl<K: Eq + Hash + Clone, T> Cache<K, T> {
    fn new(ttl: Duration) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            available: Notify::new(),
            ttl,
        }
    }

    async fn get<F, Fut>(&self, key: K, load: F) -> CachedResult<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, sqlx::Error>>,
    {
        let _wake = WakeOnDrop(&self.available);
        let active = loop {
            let notified = self.available.notified();
            // Register before checking capacity so a completing loader cannot lose the wakeup.
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut slots = self.slots.lock();
                if !slots.contains_key(&key) && slots.len() == MAX_SCOPES {
                    let idle = slots
                        .iter()
                        .find(|(_, slot)| Arc::strong_count(slot) == 1)
                        .map(|(key, _)| key.clone());
                    if let Some(idle) = idle {
                        slots.remove(&idle);
                    }
                }
                if slots.contains_key(&key) || slots.len() < MAX_SCOPES {
                    let slot = slots
                        .entry(key.clone())
                        .or_insert_with(|| {
                            Arc::new(Slot {
                                value: AsyncMutex::new(None),
                            })
                        })
                        .clone();
                    break slot;
                }
            }
            notified.await;
        };
        let mut snapshot = active.value.lock().await;
        if let Some((created, value)) = snapshot.as_ref() {
            let ttl = if value.is_ok() {
                self.ttl
            } else {
                PROGRESS_TTL
            };
            if created.elapsed() < ttl {
                return value.clone();
            }
        }
        // Share failed refreshes too, rather than queueing every waiter behind a database outage.
        let value = load().await.map(Arc::new).map_err(Arc::new);
        *snapshot = Some((Instant::now(), value.clone()));
        value
    }
}

pub struct Totals {
    pub documents: i64,
    pub chunks: i64,
    pub sources: i64,
}
pub struct Progress {
    pub embedded: i64,
    pub pending: i64,
    pub failed: i64,
}

impl Progress {
    pub fn chunks(&self) -> i64 {
        self.embedded + self.pending + self.failed
    }
}

pub struct GlobalProgress {
    pub stored: Vec<StoredModel>,
    pub backlogs: HashMap<String, Arc<Vec<embeddings::SourceBacklog>>>,
}

pub struct Statistics {
    totals: Cache<Uuid, Totals>,
    document_scopes: Cache<(Uuid, Option<Vec<String>>), i64>,
    progress: Cache<(Uuid, String), Progress>,
    rates: Cache<String, f64>,
    global: Cache<(), GlobalProgress>,
    writers: Cache<Uuid, writer_stats::Statistics>,
}

impl Statistics {
    pub fn new() -> Self {
        Self {
            totals: Cache::new(TOTALS_TTL),
            document_scopes: Cache::new(TOTALS_TTL),
            progress: Cache::new(PROGRESS_TTL),
            rates: Cache::new(PROGRESS_TTL),
            global: Cache::new(PROGRESS_TTL),
            writers: Cache::new(PROGRESS_TTL),
        }
    }

    pub async fn totals(&self, pool: &PgPool, user: Uuid) -> CachedResult<Totals> {
        self.totals
            .get(user, || async {
                let mut tx = pool.begin().await?;
                sqlx::query!("set transaction isolation level repeatable read, read only")
                    .execute(&mut *tx)
                    .await?;
                let writers = Writers::load(&mut tx, user).await?;
                let sources = sqlx::query!(
                    r#"select source, count(*) as "documents!",
                              (array_agg(api_key_id order by modified_at desc)
                                   filter (where api_key_id is not null))[1] as api_key_id
                       from documents where user_id = $1 group by source"#,
                    user,
                )
                .fetch_all(&mut *tx)
                .await?;
                let chunks = sqlx::query_scalar!(
                    r#"select count(*) as "count!" from chunks where user_id = $1"#,
                    user,
                )
                .fetch_one(&mut *tx)
                .await?;
                tx.commit().await?;
                let documents = sources.iter().map(|source| source.documents).sum();
                let sources = sources
                    .iter()
                    .map(|source| {
                        writers
                            .of_source(&source.source, source.api_key_id)
                            .map(|writer| writer.id)
                    })
                    .collect::<HashSet<_>>()
                    .len() as i64;
                Ok(Totals {
                    documents,
                    chunks,
                    sources,
                })
            })
            .await
    }

    pub async fn documents_in_scope(
        &self,
        pool: &PgPool,
        user: Uuid,
        roots: Option<&[String]>,
    ) -> Result<i64, Arc<sqlx::Error>> {
        let normalized = roots.map(|roots| {
            let mut roots = roots.to_vec();
            roots.sort_unstable();
            roots.dedup();
            roots
        });
        self.document_scopes
            .get((user, normalized), || async {
                sqlx::query_scalar!(
                    r#"select count(*) as "count!" from documents d
                       where d.user_id = $1
                       and ($2::text[] is null
                            or exists (select 1 from unnest($2::text[]) root where source_covers(root, d.source)))"#,
                    user,
                    roots,
                )
                .fetch_one(pool)
                .await
            })
            .await
            .map(|count| *count)
    }

    pub async fn progress(&self, pool: &PgPool, user: Uuid, model: &str) -> CachedResult<Progress> {
        self.progress
            .get((user, model.to_owned()), || async {
                sqlx::query_as!(
                    Progress,
                    r#"select count(e.chunk_id) as "embedded!",
                              count(*) filter (where e.chunk_id is null and f.chunk_id is null) as "pending!",
                              count(*) filter (where e.chunk_id is null and f.chunk_id is not null) as "failed!"
                       from chunks c
                       left join chunk_embeddings e on e.chunk_id = c.id and e.model = $2 and e.user_id = $1
                       left join embedding_failures f on f.chunk_id = c.id and f.model = $2
                       where c.user_id = $1"#,
                    user,
                    model,
                )
                .fetch_one(pool)
                .await
            })
            .await
    }

    pub async fn rate(&self, pool: &PgPool, model: &str) -> Result<f64, Arc<sqlx::Error>> {
        self.rates
            .get(model.to_owned(), || {
                embeddings::rate_per_minute(pool, model)
            })
            .await
            .map(|rate| *rate)
    }

    pub async fn global(
        &self,
        pool: &PgPool,
        embedding: &Embedding,
    ) -> CachedResult<GlobalProgress> {
        self.global
            .get((), || async {
                let mut tx = pool.begin().await?;
                sqlx::query!("set transaction isolation level repeatable read, read only")
                    .execute(&mut *tx)
                    .await?;
                let stored = model_registry::list(&mut tx, embedding).await?;
                let mut backlogs = HashMap::new();
                for model in embedding.models() {
                    let backlog = embeddings::source_backlog(&mut tx, model.id()).await?;
                    backlogs.insert(model.id().to_owned(), Arc::new(backlog));
                }
                tx.commit().await?;
                Ok(GlobalProgress { stored, backlogs })
            })
            .await
    }

    pub async fn writers(
        &self,
        pool: &PgPool,
        user: Uuid,
    ) -> CachedResult<writer_stats::Statistics> {
        self.writers
            .get(user, || writer_stats::load(pool, user))
            .await
    }

    pub async fn invalidate_model_statistics(&self) {
        // Wait out an older refresh before invalidating, so it cannot publish deleted state.
        let slot = self.global.slots.lock().get(&()).cloned();
        if let Some(slot) = slot {
            *slot.value.lock().await = None;
        }
        let slots: Vec<_> = self.writers.slots.lock().values().cloned().collect();
        for slot in slots {
            *slot.value.lock().await = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn concurrent_reads_share_refresh_and_expire_from_completion() {
        let cache = Arc::new(Cache::new(PROGRESS_TTL));
        let loads = Arc::new(AtomicUsize::new(0));
        for generation in 1..=2 {
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..32 {
                let cache = cache.clone();
                let loads = loads.clone();
                tasks.spawn(async move {
                    *cache
                        .get((), || async {
                            let value = loads.fetch_add(1, Ordering::SeqCst) + 1;
                            tokio::task::yield_now().await;
                            Ok(value)
                        })
                        .await
                        .unwrap()
                });
            }
            while let Some(value) = tasks.join_next().await {
                assert_eq!(value.unwrap(), generation);
            }
            assert_eq!(loads.load(Ordering::SeqCst), generation);
            let slot = cache.slots.lock().get(&()).unwrap().clone();
            slot.value.lock().await.as_mut().unwrap().0 -= PROGRESS_TTL;
        }
    }

    #[tokio::test]
    async fn failed_refresh_is_not_zero_or_stale_success() {
        let cache = Cache::new(PROGRESS_TTL);
        assert_eq!(*cache.get((), || async { Ok(7) }).await.unwrap(), 7);
        let slot = cache.slots.lock().get(&()).unwrap().clone();
        slot.value.lock().await.as_mut().unwrap().0 -= PROGRESS_TTL;
        assert!(
            cache
                .get((), || async { Err(sqlx::Error::PoolClosed) })
                .await
                .is_err()
        );
        let error = cache.get((), || async { Ok(9) }).await.unwrap_err();
        assert!(matches!(*error, sqlx::Error::PoolClosed));
        slot.value.lock().await.as_mut().unwrap().0 -= PROGRESS_TTL;
        assert_eq!(*cache.get((), || async { Ok(9) }).await.unwrap(), 9);
    }

    #[tokio::test]
    async fn a_blocked_user_does_not_block_another_and_retention_is_bounded() {
        let cache = Arc::new(Cache::new(TOTALS_TTL));
        let (started, waiting) = tokio::sync::oneshot::channel();
        let blocked = {
            let cache = cache.clone();
            tokio::spawn(async move {
                cache
                    .get(0, || async {
                        started.send(()).unwrap();
                        std::future::pending::<Result<i32, sqlx::Error>>().await
                    })
                    .await
            })
        };
        waiting.await.unwrap();
        for key in 1..=MAX_SCOPES * 2 {
            assert_eq!(
                *tokio::time::timeout(
                    Duration::from_secs(1),
                    cache.get(key, || async { Ok(key as i32) })
                )
                .await
                .unwrap()
                .unwrap(),
                key as i32
            );
        }
        assert_eq!(cache.slots.lock().len(), MAX_SCOPES);
        blocked.abort();
        let _ = blocked.await;
        assert_eq!(*cache.get(0, || async { Ok(42) }).await.unwrap(), 42);
    }
}

#[cfg(test)]
mod capacity_tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_a_refresh_releases_capacity_without_duplicate_inflight_loads() {
        let cache = Arc::new(Cache::new(PROGRESS_TTL));
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let mut blocked = Vec::new();
        for key in 0..MAX_SCOPES {
            let cache = cache.clone();
            let started = started.clone();
            blocked.push(tokio::spawn(async move {
                cache
                    .get(key, || async {
                        started.send(()).unwrap();
                        std::future::pending::<Result<usize, sqlx::Error>>().await
                    })
                    .await
            }));
            starts.recv().await.unwrap();
        }
        let extra = {
            let cache = cache.clone();
            tokio::spawn(async move { cache.get(MAX_SCOPES, || async { Ok(MAX_SCOPES) }).await })
        };
        tokio::task::yield_now().await;
        assert!(!extra.is_finished());
        blocked[0].abort();
        assert_eq!(
            *tokio::time::timeout(Duration::from_secs(1), extra)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            MAX_SCOPES
        );
        assert_eq!(cache.slots.lock().len(), MAX_SCOPES);
        for task in blocked {
            task.abort();
            let _ = task.await;
        }
    }
}
