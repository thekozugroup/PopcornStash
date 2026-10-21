//! The library: indexing files in place, sorting incoming files, explicit
//! organize/review actions, trash, and the filesystem watcher.
//! Rules: docs/architecture.md sections 3–5.

pub mod access;
pub mod classify;
pub mod cleanup;
pub mod conversion;
pub mod defs;
pub mod destinations;
pub mod download_folders;
pub mod folders;
pub mod organizer;
pub mod parse;
pub mod repo;
pub mod scanner;
pub mod watcher;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::db::Db;
use crate::events::EventBus;
use crate::jobs::JobQueue;
use crate::media::MediaService;
use crate::models::{Category, ItemRow, Place};
use crate::paths::StashLayout;
use crate::settings::SettingsStore;

/// Outcome of a full rescan.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanReport {
    pub added: u64,
    pub updated: u64,
    pub moved: u64,
    pub missing: u64,
    pub removed: u64,
    /// Inbox / Completed paths queued for sorting.
    pub queued_for_sort: u64,
}

/// Target for an explicit organize or an accepted review. Missing fields are
/// taken from the item's current metadata or suggestion. `place` and `library_id` name
/// where the file goes (section 8, Inbox accept and Organize); without them the
/// suggestion decides as it reads now.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OrganizeRequest {
    pub place: Option<Place>,
    pub library_id: Option<String>,
    pub category: Option<Category>,
    pub title: Option<String>,
    pub year: Option<i64>,
    pub show_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum ReviewAction {
    /// Move to the (possibly edited) suggested destination.
    Accept(OrganizeRequest),
    /// Mark reviewed and leave the file where it is.
    Ignore,
}

pub struct Library {
    pub db: Db,
    pub layout: Arc<StashLayout>,
    pub events: EventBus,
    pub jobs: JobQueue,
    pub settings: SettingsStore,
    pub media: Arc<MediaService>,
    /// Serializes every step that moves a file or rewrites which file an item points at
    /// (sorting, organize, review, trash, restore, indexing, marking missing), so the
    /// sorter, the watcher, rescans and API actions never act on stale state. Library
    /// writes that change where files belong hold it too (section 5, Move lock).
    moves: Arc<tokio::sync::Mutex<()>>,
    /// Goes up with every library write that can change where a file goes (create,
    /// delete, claims, type, structure). A sort job that sees it change stops moving files.
    generation: AtomicU64,
}

impl Library {
    pub fn new(
        db: Db,
        layout: Arc<StashLayout>,
        events: EventBus,
        jobs: JobQueue,
        settings: SettingsStore,
        media: Arc<MediaService>,
    ) -> Self {
        Self {
            db,
            layout,
            events,
            jobs,
            settings,
            moves: media.publication.clone(),
            media,
            generation: AtomicU64::new(0),
        }
    }

    /// See [`Library::generation`].
    pub fn claims_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub(crate) fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Take the library's move lock (see [`Library`]).
    pub(crate) async fn lock_moves(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.moves.lock().await
    }

    /// Reconcile the database with the filesystem (Scan job handler).
    pub async fn rescan(&self) -> Result<ScanReport> {
        scanner::rescan(self).await
    }

    /// Index (or refresh) one file in place. Returns None for ignored or non-indexed paths.
    pub async fn index_path(&self, rel_path: &str) -> Result<Option<ItemRow>> {
        scanner::index_path(self, rel_path).await
    }

    /// Classify and sort a file or folder under `Inbox/` or `Downloads/Completed/`
    /// (Sort job handler). Returns the ids of items created or updated.
    pub async fn sort_path(
        &self,
        rel_path: &str,
        download_id: Option<&str>,
    ) -> Result<Vec<String>> {
        organizer::sort_path(self, rel_path, download_id).await
    }

    /// Queue a Sort job for a path under `Inbox/` or `Downloads/Completed/`, unless one is
    /// already queued or running for the same path. Used by the watcher, rescans and the
    /// download manager. When `download_id` is set, the sorter marks that download `sorted`
    /// and links the resulting items in `download_items`.
    pub async fn enqueue_sort(&self, rel_path: &str, download_id: Option<&str>) -> Result<()> {
        organizer::enqueue_sort(self, rel_path, download_id).await
    }

    /// Explicit user action: move an item into the library structure.
    pub async fn organize(&self, item_id: &str, request: OrganizeRequest) -> Result<ItemRow> {
        organizer::organize(self, item_id, request).await
    }

    /// Resolve an Inbox item that needs review.
    pub async fn review(&self, item_id: &str, action: ReviewAction) -> Result<ItemRow> {
        organizer::review(self, item_id, action).await
    }

    /// Move an item to `.popcornstash/Trash`.
    pub async fn trash_item(&self, item_id: &str, user_id: Option<&str>) -> Result<()> {
        organizer::trash_item(self, item_id, user_id).await
    }

    /// Restore an item from the trash to its original path (or a free name next to it).
    pub async fn restore_item(&self, item_id: &str) -> Result<ItemRow> {
        organizer::restore_item(self, item_id).await
    }

    /// Why a trashed item cannot be restored now, if it cannot (section 11).
    pub async fn restore_refusal(&self, item_id: &str) -> Result<Option<String>> {
        organizer::restore_refusal(self, item_id).await
    }

    /// Permanently delete trashed items. `older_than_days = None` empties everything.
    pub async fn empty_trash(&self, older_than_days: Option<u32>) -> Result<u64> {
        organizer::empty_trash(self, older_than_days).await
    }

    /// Watch the stash for changes and index/sort them once stable.
    pub fn spawn_watcher(self: &Arc<Self>, cancel: CancellationToken) -> JoinHandle<()> {
        watcher::spawn(self.clone(), cancel)
    }
}
