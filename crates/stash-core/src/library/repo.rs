//! Item queries used by the API (section 8). All functions take the requesting
//! user's id where per-user playback progress is part of the result.

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::models::{Category, Item, ItemDetail, ItemRow, ItemStatus};
use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSort {
    #[default]
    Added,
    Title,
    Taken,
    Size,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

/// `GET /api/items` query string.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemQuery {
    pub category: Option<Category>,
    pub q: Option<String>,
    pub sort: ItemSort,
    pub order: SortOrder,
    pub favorite: Option<bool>,
    pub year: Option<i64>,
    /// Defaults to `ready` when absent.
    pub status: Option<ItemStatus>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// `PATCH /api/items/:id` body. Metadata only; never moves files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemPatch {
    pub title: Option<String>,
    pub year: Option<Option<i64>>,
    pub favorite: Option<bool>,
    pub show_name: Option<String>,
    pub season: Option<Option<i64>>,
    pub episode: Option<Option<i64>>,
    pub episode_title: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryCounts {
    pub movies: i64,
    pub shows: i64,
    pub episodes: i64,
    pub home_videos: i64,
    pub photos: i64,
    pub other: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub counts: LibraryCounts,
    pub total_bytes: i64,
    pub needs_review: i64,
    pub recently_added: Vec<Item>,
    pub continue_watching: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowSummary {
    pub show_name: String,
    pub item_count: i64,
    pub season_count: i64,
    pub poster_item_id: Option<String>,
    pub last_added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Season {
    pub season: i64,
    pub episodes: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowDetail {
    pub show_name: String,
    pub seasons: Vec<Season>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoMonth {
    /// `YYYY-MM`
    pub month: String,
    pub count: i64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoTimeline {
    pub months: Vec<PhotoMonth>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub item: Item,
    pub original_rel_path: String,
    pub trashed_at: i64,
    pub trashed_by: Option<String>,
}

pub async fn get(db: &Db, id: &str) -> Result<Option<ItemRow>> {
    let _ = (db, id);
    todo!("library agent")
}

pub async fn get_by_path(db: &Db, rel_path: &str) -> Result<Option<ItemRow>> {
    let _ = (db, rel_path);
    todo!("library agent")
}

pub async fn list(db: &Db, query: &ItemQuery, user_id: &str) -> Result<Page<Item>> {
    let _ = (db, query, user_id);
    todo!("library agent: cursor pagination, search on title/show/rel_path")
}

pub async fn detail(db: &Db, id: &str, user_id: &str) -> Result<ItemDetail> {
    let _ = (db, id, user_id);
    todo!("library agent: includes next/previous episode")
}

pub async fn update_metadata(db: &Db, id: &str, patch: &ItemPatch) -> Result<ItemRow> {
    let _ = (db, id, patch);
    todo!("library agent")
}

pub async fn set_progress(db: &Db, user_id: &str, item_id: &str, position_s: f64, duration_s: f64) -> Result<()> {
    let _ = (db, user_id, item_id, position_s, duration_s);
    todo!("library agent: finished when position >= 90% of duration")
}

pub async fn summary(db: &Db, user_id: &str) -> Result<LibrarySummary> {
    let _ = (db, user_id);
    todo!("library agent")
}

pub async fn shows(db: &Db) -> Result<Vec<ShowSummary>> {
    let _ = db;
    todo!("library agent")
}

pub async fn show(db: &Db, show_name: &str, user_id: &str) -> Result<ShowDetail> {
    let _ = (db, show_name, user_id);
    todo!("library agent")
}

pub async fn photo_timeline(db: &Db, cursor: Option<&str>, limit: u32, user_id: &str) -> Result<PhotoTimeline> {
    let _ = (db, cursor, limit, user_id);
    todo!("library agent")
}

pub async fn inbox(db: &Db, user_id: &str) -> Result<Vec<ItemDetail>> {
    let _ = (db, user_id);
    todo!("library agent: status needs_review, newest first")
}

pub async fn needs_review_count(db: &Db) -> Result<i64> {
    let _ = db;
    todo!("library agent")
}

pub async fn trash_list(db: &Db) -> Result<Vec<TrashEntry>> {
    let _ = db;
    todo!("library agent")
}
