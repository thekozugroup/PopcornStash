//! Live events with what decides who may see them (section 7, Live events).
//!
//! One task reads the in-process event bus, adds what each event needs for access
//! checks (an item's place and library, a download's linked items), reading the
//! database once per event, and passes it on. Every event stream then decides for its
//! own caller in memory (`api::events`). Downloads are shown here exactly as
//! `GET /api/downloads` shows them.

use std::sync::Arc;

use stash_core::db::Db;
use stash_core::events::{Event, EventBus, LibraryChange};
use stash_core::library::access::{Access, facts_of, push_visible};
use stash_core::models::{
    Destination, Download, DownloadRow, DownloadState, ItemFacts, Place, VpnStatus,
};
use tokio::sync::broadcast;

use crate::downloads::DownloadFilter;
use crate::error::ApiResult;

/// An event with what decides who may see it.
#[derive(Debug, Clone)]
pub enum Live {
    Item {
        change: LibraryChange,
        item_id: String,
        /// Who could see it before the change (None: the same as now).
        before: Option<ItemFacts>,
        /// Who can see it now (None: the item is gone).
        after: Option<ItemFacts>,
    },
    Regrouped {
        item_ids: Vec<String>,
        from: Option<String>,
        to: Option<String>,
    },
    Job {
        kind: String,
        state: String,
        item_id: Option<String>,
        facts: Option<ItemFacts>,
    },
    Inbox,
    Download {
        download: Download,
        /// Linked items, in the order they were linked, with their facts.
        links: Vec<(String, ItemFacts)>,
    },
    Libraries,
    Vpn(VpnStatus),
    AccessChanged {
        user_id: String,
    },
    UserRemoved {
        user_id: String,
    },
}

#[derive(Clone)]
pub struct LiveBus {
    tx: broadcast::Sender<Arc<Live>>,
}

impl LiveBus {
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Live>> {
        self.tx.subscribe()
    }

    /// Start passing events from `events` on (subscribed before this returns, so nothing
    /// published afterwards is missed).
    pub fn start(db: Db, events: &EventBus) -> Self {
        let (tx, _) = broadcast::channel(2048);
        let mut rx = events.subscribe();
        let bus = Self { tx: tx.clone() };
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => match enrich(&db, event).await {
                        Ok(Some(live)) => {
                            let _ = tx.send(Arc::new(live));
                        }
                        Ok(None) => {}
                        Err(e) => tracing::warn!(error = %e, "could not prepare a live event"),
                    },
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!(skipped, "live events fell behind; continuing");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        bus
    }
}

async fn one_fact(db: &Db, id: &str) -> stash_core::Result<Option<ItemFacts>> {
    Ok(facts_of(db, &[id.to_string()]).await?.remove(id))
}

async fn enrich(db: &Db, event: Event) -> stash_core::Result<Option<Live>> {
    Ok(Some(match event {
        Event::Library {
            r#type,
            item_id,
            before,
        } => {
            let after = one_fact(db, &item_id).await?;
            Live::Item {
                change: r#type,
                item_id,
                before,
                after,
            }
        }
        Event::Regrouped { item_ids, from, to } => Live::Regrouped { item_ids, from, to },
        Event::Job {
            kind,
            state,
            item_id,
        } => {
            let facts = match &item_id {
                Some(id) => one_fact(db, id).await?,
                None => None,
            };
            Live::Job {
                kind,
                state,
                item_id,
                facts,
            }
        }
        Event::Inbox { .. } => Live::Inbox,
        Event::Download(download) => {
            let links = links_of(db, &download.id).await?;
            Live::Download { download, links }
        }
        Event::Libraries => Live::Libraries,
        Event::Vpn(status) => Live::Vpn(status),
        Event::AccessChanged { user_id } => Live::AccessChanged { user_id },
        Event::UserRemoved { user_id } => Live::UserRemoved { user_id },
    }))
}

/// A download's linked items that still exist, in the order they were linked.
pub async fn links_of(db: &Db, download_id: &str) -> stash_core::Result<Vec<(String, ItemFacts)>> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT item_id FROM download_items WHERE download_id = ? ORDER BY rowid",
    )
    .bind(download_id)
    .fetch_all(db)
    .await?;
    let mut facts = facts_of(db, &ids).await?;
    Ok(ids
        .into_iter()
        .filter_map(|id| facts.remove(&id).map(|f| (id, f)))
        .collect())
}

/// A download as `access` sees it, or None when they do not see it (section 7,
/// Downloads): its creator and full owners always; anyone else only while it is sorted
/// and at least one linked item is visible to them. `item_ids` and `destinations`
/// cover visible items only.
pub fn download_view(
    access: &Access,
    download: &Download,
    links: &[(String, ItemFacts)],
) -> Option<Download> {
    let mine = download.created_by.as_deref() == Some(access.viewer.user_id.as_str());
    let visible: Vec<&(String, ItemFacts)> = links.iter().filter(|(_, f)| access.sees(f)).collect();
    let shown = access.full_owner()
        || mine
        || (download.state == DownloadState::Sorted && !visible.is_empty());
    if !shown {
        return None;
    }
    let mut view = download.clone();
    view.item_ids = visible.iter().map(|(id, _)| id.clone()).collect();
    let mut destinations: Vec<Destination> = Vec::new();
    for (_, facts) in &visible {
        if !matches!(
            facts.place,
            Place::Library | Place::OtherFiles | Place::Incoming
        ) {
            continue;
        }
        match destinations
            .iter_mut()
            .find(|d| d.place == facts.place && d.library_id == facts.library_id)
        {
            Some(d) => d.count += 1,
            None => destinations.push(Destination {
                place: facts.place,
                library_id: facts.library_id.clone(),
                count: 1,
            }),
        }
    }
    view.destinations = destinations;
    Some(view)
}

/// The downloads `access` sees, newest first (`GET /api/downloads`).
pub async fn visible_downloads(
    db: &Db,
    access: &Access,
    filter: DownloadFilter,
    limit: u32,
) -> ApiResult<Vec<Download>> {
    let limit = i64::from(limit.clamp(1, 500));
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT * FROM downloads d WHERE 1 = 1");
    match filter {
        DownloadFilter::All => {}
        DownloadFilter::Active => {
            qb.push(" AND d.state IN ('queued', 'downloading', 'paused', 'seeding', 'sorting')");
        }
        DownloadFilter::Finished => {
            qb.push(" AND d.state IN ('completed', 'sorted', 'failed', 'cancelled')");
        }
    }
    if !access.full_owner() {
        qb.push(" AND (d.created_by = ")
            .push_bind(access.viewer.user_id.clone())
            .push(
                " OR (d.state = 'sorted' AND EXISTS (SELECT 1 FROM download_items di \
                 JOIN items i ON i.id = di.item_id WHERE di.download_id = d.id",
            );
        push_visible(&mut qb, &access.viewer);
        qb.push(")))");
    }
    qb.push(" ORDER BY d.created_at DESC, d.id DESC LIMIT ")
        .push_bind(limit);
    let rows: Vec<DownloadRow> = qb.build_query_as().fetch_all(db).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let links = links_of(db, &row.id).await?;
        if let Some(view) = download_view(access, &row.to_download(Vec::new()), &links) {
            out.push(view);
        }
    }
    Ok(out)
}

/// One download as `access` sees it, None when they do not.
pub async fn visible_download(db: &Db, access: &Access, id: &str) -> ApiResult<Option<Download>> {
    let row: Option<DownloadRow> = sqlx::query_as("SELECT * FROM downloads WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let links = links_of(db, id).await?;
    Ok(download_view(access, &row.to_download(Vec::new()), &links))
}
