//! Durable background job queue backed by the `jobs` table.
//!
//! Producers call [`JobQueue::enqueue`]. The server registers one handler per
//! [`JobKind`] at startup with [`JobQueue::start`]; each kind gets its own
//! worker pool. Jobs survive restarts; failed jobs retry with backoff.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::db::Db;
use crate::events::{Event, EventBus};
use crate::{now, Result};

pub const MAX_ATTEMPTS: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Full library reconciliation.
    Scan,
    /// ffprobe / EXIF for one item.
    Probe,
    /// Thumbnails (and photo renditions) for one item.
    Thumb,
    /// Classify and move one completed or inbox path. Payload: `{"rel_path": "...", "download_id": "..."|null}`.
    Sort,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Scan => "scan",
            JobKind::Probe => "probe",
            JobKind::Thumb => "thumb",
            JobKind::Sort => "sort",
        }
    }

    pub const ALL: [JobKind; 4] = [JobKind::Scan, JobKind::Probe, JobKind::Thumb, JobKind::Sort];

    /// Default worker count per kind (section 5).
    pub fn default_concurrency(self) -> usize {
        match self {
            JobKind::Probe | JobKind::Thumb => 2,
            JobKind::Scan | JobKind::Sort => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub kind: JobKind,
    pub item_id: Option<String>,
    pub payload: serde_json::Value,
    pub attempts: i64,
}

pub type Handler = Arc<dyn Fn(Job) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

/// Wrap an async closure as a [`Handler`].
pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Job) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    Arc::new(move |job| Box::pin(f(job)))
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobCounts {
    pub queued: i64,
    pub running: i64,
    pub failed: i64,
}

#[derive(Clone)]
pub struct JobQueue {
    db: Db,
    events: EventBus,
    notify: Arc<Notify>,
}

impl JobQueue {
    pub fn new(db: Db, events: EventBus) -> Self {
        Self { db, events, notify: Arc::new(Notify::new()) }
    }

    /// Queue a job. A queued job of the same kind for the same item is reused.
    pub async fn enqueue(&self, kind: JobKind, item_id: Option<&str>, payload: serde_json::Value) -> Result<i64> {
        if let Some(item_id) = item_id {
            let existing: Option<(i64,)> =
                sqlx::query_as("SELECT id FROM jobs WHERE kind = ? AND item_id = ? AND state = 'queued' LIMIT 1")
                    .bind(kind.as_str())
                    .bind(item_id)
                    .fetch_optional(&self.db)
                    .await?;
            if let Some((id,)) = existing {
                return Ok(id);
            }
        } else if kind == JobKind::Scan {
            let existing: Option<(i64,)> =
                sqlx::query_as("SELECT id FROM jobs WHERE kind = 'scan' AND state IN ('queued', 'running') LIMIT 1")
                    .fetch_optional(&self.db)
                    .await?;
            if let Some((id,)) = existing {
                return Ok(id);
            }
        }
        let t = now();
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO jobs (kind, item_id, payload, state, attempts, run_after, created_at, updated_at)
             VALUES (?, ?, ?, 'queued', 0, ?, ?, ?) RETURNING id",
        )
        .bind(kind.as_str())
        .bind(item_id)
        .bind(payload.to_string())
        .bind(t)
        .bind(t)
        .bind(t)
        .fetch_one(&self.db)
        .await?;
        self.notify.notify_waiters();
        Ok(id)
    }

    /// Return jobs interrupted by a restart to the queue.
    pub async fn recover(&self) -> Result<()> {
        sqlx::query("UPDATE jobs SET state = 'queued', updated_at = ? WHERE state = 'running'")
            .bind(now())
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn counts(&self) -> Result<JobCounts> {
        let rows: Vec<(String, i64)> = sqlx::query_as("SELECT state, count(*) FROM jobs GROUP BY state")
            .fetch_all(&self.db)
            .await?;
        let mut counts = JobCounts::default();
        for (state, n) in rows {
            match state.as_str() {
                "queued" => counts.queued = n,
                "running" => counts.running = n,
                "failed" => counts.failed = n,
                _ => {}
            }
        }
        Ok(counts)
    }

    /// Spawn worker pools. `handlers` maps each kind to its handler and worker count.
    pub fn start(&self, handlers: HashMap<JobKind, (Handler, usize)>, cancel: CancellationToken) {
        for (kind, (handler, workers)) in handlers {
            for _ in 0..workers.max(1) {
                let queue = self.clone();
                let handler = handler.clone();
                let cancel = cancel.clone();
                tokio::spawn(async move { queue.worker(kind, handler, cancel).await });
            }
        }
        // Housekeeping: forget finished jobs after a day, failed after a week.
        let db = self.db.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
                }
                let t = now();
                let _ = sqlx::query("DELETE FROM jobs WHERE (state = 'done' AND updated_at < ?) OR (state = 'failed' AND updated_at < ?)")
                    .bind(t - 86_400)
                    .bind(t - 7 * 86_400)
                    .execute(&db)
                    .await;
            }
        });
    }

    async fn claim(&self, kind: JobKind) -> Result<Option<Job>> {
        let t = now();
        let row: Option<(i64, Option<String>, String, i64)> = sqlx::query_as(
            "UPDATE jobs SET state = 'running', attempts = attempts + 1, updated_at = ?
             WHERE id = (SELECT id FROM jobs WHERE state = 'queued' AND kind = ? AND run_after <= ? ORDER BY id LIMIT 1)
             RETURNING id, item_id, payload, attempts",
        )
        .bind(t)
        .bind(kind.as_str())
        .bind(t)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|(id, item_id, payload, attempts)| Job {
            id,
            kind,
            item_id,
            payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
            attempts,
        }))
    }

    async fn worker(&self, kind: JobKind, handler: Handler, cancel: CancellationToken) {
        loop {
            if cancel.is_cancelled() {
                return;
            }
            let job = match self.claim(kind).await {
                Ok(Some(job)) => job,
                Ok(None) => {
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        _ = self.notify.notified() => {}
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    }
                    continue;
                }
                Err(e) => {
                    tracing::warn!(error = %e, kind = kind.as_str(), "job claim failed");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            let (id, item_id, attempts) = (job.id, job.item_id.clone(), job.attempts);
            self.publish(kind, "running", item_id.clone());
            let result = handler(job).await;
            let t = now();
            let outcome = match result {
                Ok(()) => sqlx::query("UPDATE jobs SET state = 'done', error = NULL, updated_at = ? WHERE id = ?")
                    .bind(t)
                    .bind(id)
                    .execute(&self.db)
                    .await
                    .map(|_| "done"),
                Err(e) if attempts >= MAX_ATTEMPTS => {
                    tracing::warn!(error = %e, job = id, kind = kind.as_str(), "job failed permanently");
                    sqlx::query("UPDATE jobs SET state = 'failed', error = ?, updated_at = ? WHERE id = ?")
                        .bind(format!("{e:#}"))
                        .bind(t)
                        .bind(id)
                        .execute(&self.db)
                        .await
                        .map(|_| "failed")
                }
                Err(e) => {
                    let delay = 30 * (1 << attempts.clamp(0, 6));
                    tracing::info!(error = %e, job = id, kind = kind.as_str(), retry_in_s = delay, "job failed, retrying");
                    sqlx::query("UPDATE jobs SET state = 'queued', error = ?, run_after = ?, updated_at = ? WHERE id = ?")
                        .bind(format!("{e:#}"))
                        .bind(t + delay)
                        .bind(t)
                        .bind(id)
                        .execute(&self.db)
                        .await
                        .map(|_| "queued")
                }
            };
            match outcome {
                Ok(state) => self.publish(kind, state, item_id),
                Err(e) => tracing::warn!(error = %e, job = id, "could not record job result"),
            }
        }
    }

    fn publish(&self, kind: JobKind, state: &str, item_id: Option<String>) {
        self.events.publish(Event::Job { kind: kind.as_str().to_string(), state: state.to_string(), item_id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn runs_and_retries() {
        let db = crate::db::open_memory().await.unwrap();
        let queue = JobQueue::new(db.clone(), EventBus::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let h = handler(move |job: Job| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                if job.item_id.as_deref() == Some("bad") {
                    anyhow::bail!("boom")
                }
                Ok(())
            }
        });
        queue.enqueue(JobKind::Probe, Some("good"), serde_json::json!({})).await.unwrap();
        // Same kind + item is deduplicated while queued.
        queue.enqueue(JobKind::Probe, Some("good"), serde_json::json!({})).await.unwrap();
        queue.enqueue(JobKind::Probe, Some("bad"), serde_json::json!({})).await.unwrap();
        let cancel = CancellationToken::new();
        queue.start(HashMap::from([(JobKind::Probe, (h, 1))]), cancel.clone());
        for _ in 0..50 {
            if calls.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        cancel.cancel();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let (done,): (i64,) = sqlx::query_as("SELECT count(*) FROM jobs WHERE state = 'done'").fetch_one(&db).await.unwrap();
        assert_eq!(done, 1);
        let (retry,): (i64,) = sqlx::query_as("SELECT count(*) FROM jobs WHERE state = 'queued' AND error IS NOT NULL")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(retry, 1);
    }
}
