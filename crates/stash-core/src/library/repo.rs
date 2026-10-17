//! Item queries used by the API (section 8). All functions take the requesting
//! user's id where per-user playback progress is part of the result.

use std::collections::{BTreeMap, HashMap};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};

use super::access::{Access, Viewer, push_visible};
use super::defs::{self, LibraryDef, LibraryView};
use crate::db::Db;
use crate::models::{
    Category, FolderState, Item, ItemDetail, ItemRow, ItemStatus, Layout, Place, Progress,
    Suggestion,
};
use crate::paths::StashLayout;
use crate::{CoreError, Result};

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
    /// Items of one library (the caller must see it; the API checks).
    pub library_id: Option<String>,
    /// Library-relative folder, including descendants. Requires library_id.
    pub folder: Option<String>,
    /// Search includes waiting Inbox files, never missing or trashed ones.
    pub include_inbox: bool,
    pub place: Option<Place>,
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

/// One library on Home (`HomeLibrary`, section 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeLibrary {
    pub library: LibraryView,
    /// Up to [`HOME_ROW`] newest ready items, newest first.
    pub recent: Vec<Item>,
    /// Poster item id for each show with an episode in `recent`, from this library,
    /// chosen the same way as in [`shows`] and [`show`]. Kept for clients that still
    /// group `recent`; a series shelf is drawn from `shows`.
    pub show_posters: BTreeMap<String, String>,
    /// A series library's shelf (layout `series`; empty for the others): up to
    /// [`HOME_ROW`] of its shows, the one with the most recently added episode first, so
    /// a library with up to 12 shows has a tile for every show its heading counts.
    pub shows: Vec<HomeShow>,
}

/// One show on a series library's Home shelf (`HomeShow`, section 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeShow {
    pub show_name: String,
    /// The show's picture in this library, the same one [`shows`] and [`show`] use.
    pub poster_item_id: Option<String>,
    /// The show's ready episodes in this library (`ShowSummary.item_count`).
    pub item_count: i64,
    /// Those added in the last [`NEW_EPISODE_DAYS`] days.
    pub new_count: i64,
    /// The show's most recently added episode.
    pub latest: Item,
    pub last_added_at: i64,
}

/// `GET /api/home`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Home {
    pub continue_watching: Vec<Item>,
    pub libraries: Vec<HomeLibrary>,
    pub other_files_count: i64,
    pub inbox_count: i64,
    pub total_bytes: i64,
}

/// `GET /api/inbox`: waiting items (oldest first) and ignored ones still in Inbox or
/// Downloads/Completed, visible ones only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxList {
    pub items: Vec<ItemDetail>,
    pub ignored: Vec<ItemDetail>,
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
    /// The episode whose picture stands for the show (the same one [`shows`] lists).
    pub poster_item_id: Option<String>,
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

/// Default and maximum page sizes for [`list`].
pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 200;
/// Photos per timeline page when the caller does not say, and the most a page holds
/// (architecture section 11).
pub const DEFAULT_TIMELINE_LIMIT: u32 = 200;
pub const MAX_TIMELINE_LIMIT: u32 = 500;
/// Items in Continue watching and in each library's row on Home.
pub const HOME_ROW: i64 = 12;
/// An episode counts as new on Home for this many days after it was added.
pub const NEW_EPISODE_DAYS: i64 = 30;
const INBOX_MAX: i64 = 500;

/// A show's poster, as one SQL subquery: its first episode in season and episode order
/// that has a thumbnail, else its first episode. Home ([`summary`]), Shows ([`shows`]) and
/// the show's page ([`show`]) all pick it this way, so a show has one picture everywhere.
/// `matches` is the condition on `j`, the candidate episode.
fn show_poster_subquery(matches: &str) -> String {
    format!(
        "(SELECT j.id FROM items j
           WHERE j.category = 'episode' AND j.status = 'ready' AND {matches}
           ORDER BY (j.thumb_state = 'ready') DESC, j.season, j.episode, j.id LIMIT 1)"
    )
}

/// The suggestion of `row` as `access` reads it, loading the library rows and folder
/// states only when the row has a suggestion.
async fn suggestion_for(
    db: &Db,
    layout: &StashLayout,
    access: &Access,
    row: &ItemRow,
    cache: &mut Option<(Vec<LibraryDef>, HashMap<String, FolderState>)>,
) -> Result<Option<Suggestion>> {
    if row.suggestion_json.is_none() {
        return Ok(None);
    }
    if cache.is_none() {
        let library_defs = defs::load_all(db).await?;
        let states = defs::folder_states(db, layout, &library_defs).await?;
        *cache = Some((library_defs, states));
    }
    let (library_defs, states) = cache.as_ref().expect("filled above");
    Ok(super::organizer::suggestion_view(
        row,
        access,
        library_defs,
        states,
    ))
}

/// Columns for an item joined with one user's progress. Bind the user id right after this.
const SELECT_ITEMS: &str = "SELECT i.*, p.position_s AS p_position_s, p.duration_s AS p_duration_s, p.finished AS p_finished \
     FROM items i LEFT JOIN item_progress p ON p.item_id = i.id AND p.user_id = ";

#[derive(sqlx::FromRow)]
struct ItemWithProgress {
    #[sqlx(flatten)]
    row: ItemRow,
    p_position_s: Option<f64>,
    p_duration_s: Option<f64>,
    p_finished: Option<bool>,
}

impl ItemWithProgress {
    fn progress(&self) -> Option<Progress> {
        match (self.p_position_s, self.p_duration_s) {
            (Some(position_s), Some(duration_s)) => Some(Progress {
                position_s,
                duration_s,
                finished: self.p_finished.unwrap_or(false),
            }),
            _ => None,
        }
    }

    fn into_item(self) -> Item {
        let progress = self.progress();
        self.row.to_item(progress)
    }

    fn into_detail(
        self,
        suggestion: Option<Suggestion>,
        next_episode_id: Option<String>,
        previous_episode_id: Option<String>,
    ) -> ItemDetail {
        let progress = self.progress();
        let row = self.row;
        ItemDetail {
            item: row.to_item(progress),
            media: row.media(),
            mime: row.mime.clone(),
            suggestion,
            source_download_id: row.source_download_id.clone(),
            next_episode_id,
            previous_episode_id,
        }
    }
}

/// Sort key for titles: lower case, leading "the ", "a " and "an " removed.
pub fn sort_title(title: &str) -> String {
    let lower = title.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            let rest = rest.trim_start();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    lower
}

/// Escape `%`, `_` and `\` for a `LIKE … ESCAPE '\'` pattern.
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub async fn get(db: &Db, id: &str) -> Result<Option<ItemRow>> {
    Ok(
        sqlx::query_as::<_, ItemRow>("SELECT * FROM items WHERE id = ?")
            .bind(id)
            .fetch_optional(db)
            .await?,
    )
}

pub async fn get_by_path(db: &Db, rel_path: &str) -> Result<Option<ItemRow>> {
    Ok(
        sqlx::query_as::<_, ItemRow>("SELECT * FROM items WHERE rel_path = ?")
            .bind(rel_path)
            .fetch_optional(db)
            .await?,
    )
}

/// Insert a new row. Returns false (and changes nothing) when `rel_path` is already taken.
pub async fn insert_item(db: &Db, row: &ItemRow) -> Result<bool> {
    let result = sqlx::query(
        "INSERT INTO items (id, rel_path, kind, category, status, title, sort_title, year, show_name, season, episode,
            episode_title, size, mtime, quick_hash, mime, duration_s, width, height, media_json, taken_at, taken_local,
            taken_offset, camera, thumb_state, suggestion_json, favorite, source_download_id, missing_since, added_at,
            updated_at, added_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(rel_path) DO NOTHING",
    )
    .bind(&row.id)
    .bind(&row.rel_path)
    .bind(row.kind.as_str())
    .bind(row.category.as_str())
    .bind(row.status.as_str())
    .bind(&row.title)
    .bind(&row.sort_title)
    .bind(row.year)
    .bind(&row.show_name)
    .bind(row.season)
    .bind(row.episode)
    .bind(&row.episode_title)
    .bind(row.size)
    .bind(row.mtime)
    .bind(&row.quick_hash)
    .bind(&row.mime)
    .bind(row.duration_s)
    .bind(row.width)
    .bind(row.height)
    .bind(&row.media_json)
    .bind(row.taken_at)
    .bind(&row.taken_local)
    .bind(row.taken_offset)
    .bind(&row.camera)
    .bind(row.thumb_state.as_str())
    .bind(&row.suggestion_json)
    .bind(row.favorite)
    .bind(&row.source_download_id)
    .bind(row.missing_since)
    .bind(row.added_at)
    .bind(row.updated_at)
    .bind(&row.added_by)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn get_with_progress(db: &Db, id: &str, user_id: &str) -> Result<Option<ItemWithProgress>> {
    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id).push(" WHERE i.id = ").push_bind(id);
    Ok(qb
        .build_query_as::<ItemWithProgress>()
        .fetch_optional(db)
        .await?)
}

// ---------------------------------------------------------------------------
// Cursors

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum CursorKey {
    Int(i64),
    Text(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct ListCursor {
    s: ItemSort,
    k: CursorKey,
    id: String,
}

/// Position in the photo timeline: the last photo returned (its [`TAKEN_KEY`] and id).
#[derive(Debug, Serialize, Deserialize)]
struct TimelineCursor {
    k: String,
    id: String,
}

/// What `sort=taken` and the photo timeline order by (section 4, Capture time): the
/// camera's clock where it was taken (`taken_local`, `YYYY-MM-DDTHH:MM:SS`, which sorts
/// as text), else, for an item with no capture time, when it was added, read in UTC.
/// Ordering by the clock keeps every month of the timeline in one piece.
const TAKEN_KEY: &str =
    "COALESCE(i.taken_local, strftime('%Y-%m-%dT%H:%M:%S', i.added_at, 'unixepoch'))";

/// [`TAKEN_KEY`] of a row, worked out the same way.
fn taken_key(row: &ItemRow) -> String {
    row.taken_local.clone().unwrap_or_else(|| {
        crate::capture::CaptureTime::from_stored(Some(row.added_at), None, None)
            .map(|t| t.local_text())
            .unwrap_or_default()
    })
}

fn encode_cursor<T: Serialize>(value: &T) -> Result<String> {
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(value)?))
}

fn decode_cursor<T: for<'de> Deserialize<'de>>(cursor: &str) -> Result<T> {
    let invalid =
        || CoreError::invalid("The page cursor is not valid. Start again from the first page.");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor.trim())
        .map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn sort_key_expr(sort: ItemSort) -> &'static str {
    match sort {
        ItemSort::Added => "i.added_at",
        ItemSort::Title => "i.sort_title",
        ItemSort::Taken => TAKEN_KEY,
        ItemSort::Size => "i.size",
    }
}

fn sort_key_of(row: &ItemRow, sort: ItemSort) -> CursorKey {
    match sort {
        ItemSort::Added => CursorKey::Int(row.added_at),
        ItemSort::Title => CursorKey::Text(row.sort_title.clone()),
        ItemSort::Taken => CursorKey::Text(taken_key(row)),
        ItemSort::Size => CursorKey::Int(row.size),
    }
}

fn push_key(qb: &mut QueryBuilder<'_, Sqlite>, key: &CursorKey) {
    match key {
        CursorKey::Int(v) => qb.push_bind(*v),
        CursorKey::Text(v) => qb.push_bind(v.clone()),
    };
}

// ---------------------------------------------------------------------------
// Queries

/// A page of the items `viewer` sees.
pub async fn list(db: &Db, query: &ItemQuery, viewer: &Viewer) -> Result<Page<Item>> {
    let user_id = viewer.user_id.as_str();
    let limit = i64::from(query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT));
    let status = query.status.unwrap_or(ItemStatus::Ready);
    let key_expr = sort_key_expr(query.sort);
    let (dir, op) = match query.order {
        SortOrder::Desc => ("DESC", "<"),
        SortOrder::Asc => ("ASC", ">"),
    };

    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string());
    if query.include_inbox && query.status.is_none() {
        qb.push(" WHERE i.status IN ('ready', 'needs_review')");
    } else {
        qb.push(" WHERE i.status = ").push_bind(status.as_str());
    }
    push_visible(&mut qb, viewer);
    if let Some(library_id) = &query.library_id {
        qb.push(" AND i.library_id = ")
            .push_bind(library_id.clone());
    }
    if let Some(folder) = &query.folder {
        super::folders::validate_folder(folder)?;
        let id = query
            .library_id
            .as_deref()
            .ok_or_else(|| CoreError::invalid("Choose a library before filtering by folder."))?;
        let def = defs::load_one(db, id).await?.ok_or(CoreError::NotFound)?;
        let prefix = if folder.is_empty() {
            format!("{}/", def.folder())
        } else {
            format!("{}/{folder}/", def.folder())
        };
        qb.push(" AND substr(i.rel_path, 1, ")
            .push_bind(prefix.chars().count() as i64)
            .push(") = ")
            .push_bind(prefix);
    }
    if let Some(place) = query.place {
        qb.push(" AND i.place = ").push_bind(place.as_str());
    }
    if let Some(category) = query.category {
        qb.push(" AND i.category = ").push_bind(category.as_str());
    }
    if let Some(favorite) = query.favorite {
        qb.push(" AND i.favorite = ").push_bind(favorite);
    }
    if let Some(year) = query.year {
        qb.push(" AND i.year = ").push_bind(year);
    }
    if let Some(q) = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        let pattern = format!("%{}%", escape_like(q));
        qb.push(" AND (i.title LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR i.show_name LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR i.rel_path LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
    if let Some(cursor) = query.cursor.as_deref().filter(|c| !c.trim().is_empty()) {
        let cursor: ListCursor = decode_cursor(cursor)?;
        let key_matches_sort = matches!(
            (&cursor.k, query.sort),
            (CursorKey::Text(_), ItemSort::Title | ItemSort::Taken)
                | (CursorKey::Int(_), ItemSort::Added | ItemSort::Size)
        );
        if cursor.s != query.sort || !key_matches_sort {
            return Err(CoreError::invalid(
                "The page cursor belongs to a different sort order. Start again from the first page.",
            ));
        }
        qb.push(format!(" AND ({key_expr} {op} "));
        push_key(&mut qb, &cursor.k);
        qb.push(format!(" OR ({key_expr} = "));
        push_key(&mut qb, &cursor.k);
        qb.push(format!(" AND i.id {op} "))
            .push_bind(cursor.id)
            .push("))");
    }
    qb.push(format!(" ORDER BY {key_expr} {dir}, i.id {dir} LIMIT "))
        .push_bind(limit + 1);

    let mut rows = qb
        .build_query_as::<ItemWithProgress>()
        .fetch_all(db)
        .await?;
    let next_cursor = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        match rows.last() {
            Some(last) => Some(encode_cursor(&ListCursor {
                s: query.sort,
                k: sort_key_of(&last.row, query.sort),
                id: last.row.id.clone(),
            })?),
            None => None,
        }
    } else {
        None
    };
    Ok(Page {
        items: rows.into_iter().map(ItemWithProgress::into_item).collect(),
        next_cursor,
    })
}

/// One item as `access` sees it (not found when they cannot). Next and previous episode
/// come from the same library only.
pub async fn detail(
    db: &Db,
    layout: &StashLayout,
    access: &Access,
    id: &str,
) -> Result<ItemDetail> {
    let row = get_with_progress(db, id, &access.viewer.user_id)
        .await?
        .filter(|r| access.sees_row(&r.row))
        .ok_or(CoreError::NotFound)?;
    let (mut next, mut previous) = (None, None);
    if let (Category::Episode, Some(library_id), Some(show), Some(season), Some(episode)) = (
        row.row.category,
        row.row.library_id.as_deref(),
        row.row.show_name.as_deref(),
        row.row.season,
        row.row.episode,
    ) {
        next = sqlx::query_scalar::<_, String>(
            "SELECT id FROM items WHERE library_id = ? AND category = 'episode' AND status = 'ready'
               AND show_name = ? AND id != ?
               AND (season > ? OR (season = ? AND episode > ?))
             ORDER BY season, episode, id LIMIT 1",
        )
        .bind(library_id)
        .bind(show)
        .bind(id)
        .bind(season)
        .bind(season)
        .bind(episode)
        .fetch_optional(db)
        .await?;
        previous = sqlx::query_scalar::<_, String>(
            "SELECT id FROM items WHERE library_id = ? AND category = 'episode' AND status = 'ready'
               AND show_name = ? AND id != ?
               AND (season < ? OR (season = ? AND episode < ?))
             ORDER BY season DESC, episode DESC, id DESC LIMIT 1",
        )
        .bind(library_id)
        .bind(show)
        .bind(id)
        .bind(season)
        .bind(season)
        .bind(episode)
        .fetch_optional(db)
        .await?;
    }
    let suggestion = suggestion_for(db, layout, access, &row.row, &mut None).await?;
    Ok(row.into_detail(suggestion, next, previous))
}

pub async fn update_metadata(db: &Db, id: &str, patch: &ItemPatch) -> Result<ItemRow> {
    let current = get(db, id).await?.ok_or(CoreError::NotFound)?;
    let mut qb = QueryBuilder::<Sqlite>::new("UPDATE items SET updated_at = ");
    qb.push_bind(crate::now());
    if let Some(title) = &patch.title {
        let title = title.trim();
        if title.is_empty() {
            return Err(CoreError::invalid("The title cannot be empty."));
        }
        if title.chars().count() > 500 {
            return Err(CoreError::invalid(
                "The title is too long. Keep it under 500 characters.",
            ));
        }
        qb.push(", title = ").push_bind(title.to_string());
        qb.push(", sort_title = ").push_bind(sort_title(title));
    }
    if let Some(year) = patch.year {
        if year.is_some_and(|y| !(1800..=2200).contains(&y)) {
            return Err(CoreError::invalid(
                "The year must be between 1800 and 2200.",
            ));
        }
        qb.push(", year = ").push_bind(year);
    }
    if let Some(favorite) = patch.favorite {
        qb.push(", favorite = ").push_bind(favorite);
    }
    if let Some(show_name) = &patch.show_name {
        let show_name = show_name.trim();
        qb.push(", show_name = ")
            .push_bind((!show_name.is_empty()).then(|| show_name.to_string()));
    }
    for (column, value, label) in [
        ("season", patch.season, "season"),
        ("episode", patch.episode, "episode"),
    ] {
        if let Some(value) = value {
            if value.is_some_and(|v| !(0..=10_000).contains(&v)) {
                return Err(CoreError::invalid(format!(
                    "The {label} number must be between 0 and 10000."
                )));
            }
            qb.push(format!(", {column} = ")).push_bind(value);
        }
    }
    if let Some(episode_title) = &patch.episode_title {
        let value = episode_title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        qb.push(", episode_title = ").push_bind(value);
    }
    qb.push(" WHERE id = ").push_bind(current.id.clone());
    qb.build().execute(db).await?;
    get(db, id).await?.ok_or(CoreError::NotFound)
}

pub async fn set_progress(
    db: &Db,
    user_id: &str,
    item_id: &str,
    position_s: f64,
    duration_s: f64,
) -> Result<()> {
    if !position_s.is_finite() || !duration_s.is_finite() || position_s < 0.0 || duration_s < 0.0 {
        return Err(CoreError::invalid(
            "Playback position and duration must be zero or more seconds.",
        ));
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(db)
        .await?;
    if exists.is_none() {
        return Err(CoreError::NotFound);
    }
    let position_s = if duration_s > 0.0 {
        position_s.min(duration_s)
    } else {
        position_s
    };
    let finished = duration_s > 0.0 && position_s >= 0.9 * duration_s;
    sqlx::query(
        "INSERT INTO item_progress (user_id, item_id, position_s, duration_s, finished, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, item_id) DO UPDATE SET
           position_s = excluded.position_s, duration_s = excluded.duration_s,
           finished = excluded.finished, updated_at = excluded.updated_at",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(position_s)
    .bind(duration_s)
    .bind(finished)
    .bind(crate::now())
    .execute(db)
    .await?;
    Ok(())
}

/// `GET /api/home` for `access`.
pub async fn home(db: &Db, layout: &StashLayout, access: &Access) -> Result<Home> {
    let viewer = &access.viewer;
    let user_id = viewer.user_id.clone();
    let mut libraries = Vec::new();
    for library in defs::views(db, layout, access).await? {
        let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
        qb.push_bind(user_id.clone())
            .push(" WHERE i.library_id = ")
            .push_bind(library.id.clone())
            .push(" AND i.status = 'ready' ORDER BY i.added_at DESC, i.id DESC LIMIT ")
            .push_bind(HOME_ROW);
        let recent: Vec<ItemWithProgress> = qb.build_query_as().fetch_all(db).await?;
        let mut names: Vec<String> = recent
            .iter()
            .filter(|r| r.row.category == Category::Episode)
            .filter_map(|r| r.row.show_name.clone())
            .filter(|name| !name.is_empty())
            .collect();
        names.sort();
        names.dedup();
        let mut show_posters = BTreeMap::new();
        if !names.is_empty() {
            let mut qb = QueryBuilder::<Sqlite>::new("SELECT s.show_name, ");
            qb.push(show_poster_subquery(
                "j.library_id = s.library_id AND j.show_name = s.show_name",
            ))
            .push(" FROM (SELECT DISTINCT library_id, show_name FROM items WHERE library_id = ")
            .push_bind(library.id.clone())
            .push(" AND show_name IN (");
            let mut list = qb.separated(", ");
            for name in &names {
                list.push_bind(name.clone());
            }
            qb.push(")) s");
            let rows: Vec<(String, Option<String>)> = qb.build_query_as().fetch_all(db).await?;
            show_posters.extend(rows.into_iter().filter_map(|(name, id)| Some((name, id?))));
        }
        let shows = if library.layout == Layout::Series {
            home_shows(db, &library.id, &user_id).await?
        } else {
            Vec::new()
        };
        libraries.push(HomeLibrary {
            library,
            recent: recent
                .into_iter()
                .map(ItemWithProgress::into_item)
                .collect(),
            show_posters,
            shows,
        });
    }

    // Continue watching: visible items in a library, most recently watched first.
    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT i.*, p.position_s AS p_position_s, p.duration_s AS p_duration_s, p.finished AS p_finished
         FROM item_progress p JOIN items i ON i.id = p.item_id
         WHERE p.user_id = ",
    );
    qb.push_bind(user_id.clone()).push(
        " AND p.finished = 0 AND p.position_s > 30 AND i.status = 'ready' AND i.place = 'library'",
    );
    push_visible(&mut qb, viewer);
    qb.push(" ORDER BY p.updated_at DESC, i.id DESC LIMIT ")
        .push_bind(HOME_ROW);
    let continue_watching: Vec<ItemWithProgress> = qb.build_query_as().fetch_all(db).await?;

    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT coalesce(sum(i.size), 0), coalesce(sum(i.place = 'other_files'), 0) FROM items i WHERE i.status = 'ready'",
    );
    push_visible(&mut qb, viewer);
    let (total_bytes, other_files_count): (i64, i64) = qb.build_query_as().fetch_one(db).await?;
    Ok(Home {
        continue_watching: continue_watching
            .into_iter()
            .map(ItemWithProgress::into_item)
            .collect(),
        libraries,
        other_files_count,
        inbox_count: inbox_count(db, viewer).await?,
        total_bytes,
    })
}

/// A series library's Home shelf: up to [`HOME_ROW`] shows, the one with the most recently
/// added episode first.
async fn home_shows(db: &Db, library_id: &str, user_id: &str) -> Result<Vec<HomeShow>> {
    let since = crate::now() - NEW_EPISODE_DAYS * 86_400;
    let sql = format!(
        "SELECT i.show_name, count(*), coalesce(sum(i.added_at >= ?), 0), max(i.added_at), {}
         FROM items i
         WHERE i.library_id = ? AND i.category = 'episode' AND i.status = 'ready'
           AND i.show_name IS NOT NULL AND i.show_name != ''
         GROUP BY i.show_name
         ORDER BY max(i.added_at) DESC, max(i.id) DESC
         LIMIT ?",
        show_poster_subquery("j.library_id = i.library_id AND j.show_name = i.show_name")
    );
    let rows: Vec<(String, i64, i64, i64, Option<String>)> = sqlx::query_as(&sql)
        .bind(since)
        .bind(library_id)
        .bind(HOME_ROW)
        .fetch_all(db)
        .await?;
    let mut shows = Vec::with_capacity(rows.len());
    for (show_name, item_count, new_count, last_added_at, poster_item_id) in rows {
        let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
        qb.push_bind(user_id.to_string())
            .push(" WHERE i.library_id = ")
            .push_bind(library_id.to_string())
            .push(" AND i.category = 'episode' AND i.status = 'ready' AND i.show_name = ")
            .push_bind(show_name.clone())
            .push(" ORDER BY i.added_at DESC, i.id DESC LIMIT 1");
        let Some(latest) = qb
            .build_query_as::<ItemWithProgress>()
            .fetch_optional(db)
            .await?
        else {
            continue;
        };
        shows.push(HomeShow {
            show_name,
            poster_item_id,
            item_count,
            new_count,
            latest: latest.into_item(),
            last_added_at,
        });
    }
    Ok(shows)
}

/// The shows of an Episodes library, by title.
pub async fn shows(db: &Db, library_id: &str) -> Result<Vec<ShowSummary>> {
    let sql = format!(
        "SELECT i.show_name, count(*), count(DISTINCT i.season), max(i.added_at), {}
         FROM items i
         WHERE i.library_id = ? AND i.category = 'episode' AND i.status = 'ready'
           AND i.show_name IS NOT NULL AND i.show_name != ''
         GROUP BY i.show_name",
        show_poster_subquery("j.library_id = i.library_id AND j.show_name = i.show_name")
    );
    let rows: Vec<(String, i64, i64, i64, Option<String>)> =
        sqlx::query_as(&sql).bind(library_id).fetch_all(db).await?;
    let mut shows: Vec<ShowSummary> = rows
        .into_iter()
        .map(
            |(show_name, item_count, season_count, last_added_at, poster_item_id)| ShowSummary {
                show_name,
                item_count,
                season_count,
                poster_item_id,
                last_added_at,
            },
        )
        .collect();
    shows.sort_by_cached_key(|s| (sort_title(&s.show_name), s.show_name.clone()));
    Ok(shows)
}

/// One show of an Episodes library: this library's episodes of it, by season.
pub async fn show(db: &Db, library_id: &str, show_name: &str, user_id: &str) -> Result<ShowDetail> {
    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string())
        .push(" WHERE i.library_id = ")
        .push_bind(library_id.to_string())
        // Names typed or stored with stray spaces still match.
        .push(" AND i.category = 'episode' AND i.status = 'ready' AND TRIM(i.show_name) = ")
        .push_bind(show_name.trim().to_string())
        .push(" ORDER BY COALESCE(i.season, 0), COALESCE(i.episode, 0), i.sort_title, i.id");
    let rows = qb
        .build_query_as::<ItemWithProgress>()
        .fetch_all(db)
        .await?;
    if rows.is_empty() {
        return Err(CoreError::NotFound);
    }
    let poster_item_id: Option<String> = sqlx::query_scalar(&format!(
        "SELECT {}",
        show_poster_subquery("j.library_id = ? AND TRIM(j.show_name) = ?")
    ))
    .bind(library_id)
    .bind(show_name.trim())
    .fetch_one(db)
    .await?;
    let mut seasons: Vec<Season> = Vec::new();
    for row in rows {
        let number = row.row.season.unwrap_or(0);
        let item = row.into_item();
        match seasons.last_mut() {
            Some(season) if season.season == number => season.episodes.push(item),
            _ => seasons.push(Season {
                season: number,
                episodes: vec![item],
            }),
        }
    }
    Ok(ShowDetail {
        show_name: show_name.to_string(),
        poster_item_id,
        seasons,
    })
}

/// Photos grouped by the month on the camera's clock (section 4, Capture time; of
/// `added_at` in UTC for a photo with no capture time), newest first, in [`TAKEN_KEY`]
/// order. A page holds at most `limit` photos (`0` means [`DEFAULT_TIMELINE_LIMIT`]), so
/// one busy month can span several pages: `count` is always the month's full total and
/// the next page continues the same month where this one stopped (clients merge
/// consecutive entries with the same `month`).
pub async fn photo_timeline(
    db: &Db,
    library_id: &str,
    cursor: Option<&str>,
    limit: u32,
    user_id: &str,
) -> Result<PhotoTimeline> {
    let limit = i64::from(if limit == 0 {
        DEFAULT_TIMELINE_LIMIT
    } else {
        limit.min(MAX_TIMELINE_LIMIT)
    });
    let after: Option<TimelineCursor> = cursor
        .filter(|c| !c.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;
    let key = TAKEN_KEY;
    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string())
        .push(" WHERE i.library_id = ")
        .push_bind(library_id.to_string())
        .push(format!(" AND i.status = 'ready' AND {key} IS NOT NULL"));
    if let Some(after) = &after {
        qb.push(format!(" AND ({key} < "))
            .push_bind(after.k.clone())
            .push(format!(" OR ({key} = "))
            .push_bind(after.k.clone())
            .push(" AND i.id < ")
            .push_bind(after.id.clone())
            .push("))");
    }
    qb.push(format!(" ORDER BY {key} DESC, i.id DESC LIMIT "))
        .push_bind(limit + 1);
    let mut rows = qb
        .build_query_as::<ItemWithProgress>()
        .fetch_all(db)
        .await?;
    let next_cursor = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        match rows.last() {
            Some(last) => Some(encode_cursor(&TimelineCursor {
                k: taken_key(&last.row),
                id: last.row.id.clone(),
            })?),
            None => None,
        }
    } else {
        None
    };

    let mut months: Vec<PhotoMonth> = Vec::new();
    for row in rows {
        let Some(month) = taken_key(&row.row).get(..7).map(str::to_string) else {
            continue;
        };
        let item = row.into_item();
        match months.last_mut() {
            Some(last) if last.month == month => {
                last.items.push(item);
                last.count += 1;
            }
            _ => months.push(PhotoMonth {
                month,
                count: 1,
                items: vec![item],
            }),
        }
    }
    // Full totals for the months on this page (a month may continue on other pages).
    let bounds = (
        months.last().and_then(|m| month_bounds(&m.month)),
        months.first().and_then(|m| month_bounds(&m.month)),
    );
    if let (Some((start, _)), Some((_, end))) = bounds {
        let sql = format!(
            "SELECT substr({key}, 1, 7) AS month, count(*)
             FROM items i
             WHERE i.library_id = ? AND i.status = 'ready' AND {key} >= ? AND {key} < ?
             GROUP BY month"
        );
        let totals: Vec<(Option<String>, i64)> = sqlx::query_as(&sql)
            .bind(library_id)
            .bind(start)
            .bind(end)
            .fetch_all(db)
            .await?;
        for month in &mut months {
            if let Some((_, n)) = totals
                .iter()
                .find(|(m, _)| m.as_deref() == Some(month.month.as_str()))
            {
                month.count = *n;
            }
        }
    }
    Ok(PhotoTimeline {
        months,
        next_cursor,
    })
}

/// `YYYY-MM` and the month after it: the bounds of the month's [`TAKEN_KEY`]s as text.
fn month_bounds(month: &str) -> Option<(String, String)> {
    let (y, m) = month.split_once('-')?;
    let (y, m): (i32, u32) = (y.parse().ok()?, m.parse().ok()?);
    if !(1..=12).contains(&m) || !(0..=9998).contains(&y) {
        return None;
    }
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    Some((format!("{y:04}-{m:02}"), format!("{ny:04}-{nm:02}")))
}

/// The Inbox as `access` sees it.
pub async fn inbox(db: &Db, layout: &StashLayout, access: &Access) -> Result<InboxList> {
    let viewer = &access.viewer;
    let mut cache = None;
    let mut lists = Vec::new();
    for (filter, order) in [
        ("i.status = 'needs_review'", "ASC"),
        ("i.status = 'ready' AND i.place = 'incoming'", "DESC"),
    ] {
        let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
        qb.push_bind(viewer.user_id.clone())
            .push(format!(" WHERE {filter}"));
        push_visible(&mut qb, viewer);
        qb.push(format!(" ORDER BY i.added_at {order}, i.id {order} LIMIT "))
            .push_bind(INBOX_MAX);
        let rows: Vec<ItemWithProgress> = qb.build_query_as().fetch_all(db).await?;
        let mut details = Vec::with_capacity(rows.len());
        for row in rows {
            let suggestion = suggestion_for(db, layout, access, &row.row, &mut cache).await?;
            details.push(row.into_detail(suggestion, None, None));
        }
        lists.push(details);
    }
    let ignored = lists.pop().unwrap_or_default();
    let items = lists.pop().unwrap_or_default();
    Ok(InboxList { items, ignored })
}

/// Waiting items in the whole stash (what the sorter compares to tell streams to recount).
pub async fn needs_review_count(db: &Db) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM items WHERE status = 'needs_review'")
            .fetch_one(db)
            .await?,
    )
}

/// Waiting items `viewer` sees.
pub async fn inbox_count(db: &Db, viewer: &Viewer) -> Result<i64> {
    let mut qb =
        QueryBuilder::<Sqlite>::new("SELECT count(*) FROM items i WHERE i.status = 'needs_review'");
    push_visible(&mut qb, viewer);
    Ok(qb.build_query_scalar().fetch_one(db).await?)
}

#[derive(sqlx::FromRow)]
struct TrashRow {
    #[sqlx(flatten)]
    row: ItemRow,
    t_original_rel_path: String,
    t_trashed_at: i64,
    t_trashed_by: Option<String>,
}

pub async fn trash_list(db: &Db) -> Result<Vec<TrashEntry>> {
    let rows = sqlx::query_as::<_, TrashRow>(
        "SELECT i.*, t.original_rel_path AS t_original_rel_path, t.trashed_at AS t_trashed_at,
                t.trashed_by AS t_trashed_by
         FROM trash t JOIN items i ON i.id = t.item_id
         ORDER BY t.trashed_at DESC, i.id DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| TrashEntry {
            item: r.row.to_item(None),
            original_rel_path: r.t_original_rel_path,
            trashed_at: r.t_trashed_at,
            trashed_by: r.t_trashed_by,
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::library::defs::{STARTING_PHOTOS, STARTING_SHOWS};
    use crate::models::{MediaKind, Role, ThumbState};

    /// A full owner: sees everything.
    pub(crate) fn owner(id: &str) -> Viewer {
        Viewer {
            user_id: id.to_string(),
            role: Role::Owner,
            full: true,
        }
    }

    async fn detail_as(db: &Db, id: &str, user: &str) -> Result<ItemDetail> {
        let access = Access::load(db, &owner(user)).await?;
        detail(db, &StashLayout::new("/nonexistent"), &access, id).await
    }

    async fn home_as(db: &Db, user: &str) -> Home {
        let access = Access::load(db, &owner(user)).await.unwrap();
        home(db, &StashLayout::new("/nonexistent"), &access)
            .await
            .unwrap()
    }

    /// A ready movie row with sensible defaults, for tests.
    pub(crate) fn row(id: &str, title: &str) -> ItemRow {
        ItemRow {
            id: id.to_string(),
            rel_path: format!("Movies/{title}/{title}.mkv"),
            kind: MediaKind::Video,
            category: Category::Movie,
            status: ItemStatus::Ready,
            title: title.to_string(),
            sort_title: sort_title(title),
            year: None,
            show_name: None,
            season: None,
            episode: None,
            episode_title: None,
            size: 1000,
            mtime: 1_700_000_000,
            quick_hash: None,
            mime: Some("video/x-matroska".into()),
            duration_s: Some(3600.0),
            width: None,
            height: None,
            media_json: None,
            taken_at: None,
            taken_local: None,
            taken_offset: None,
            camera: None,
            thumb_state: ThumbState::Pending,
            suggestion_json: None,
            favorite: false,
            source_download_id: None,
            missing_since: None,
            added_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            library_id: None,
            place: crate::models::Place::Folder,
            added_by: None,
        }
    }

    pub(crate) async fn add_user(db: &Db, id: &str) {
        sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES (?, ?, 'owner', ?, 0)")
            .bind(id)
            .bind(id)
            .bind(format!("w-{id}"))
            .execute(db)
            .await
            .unwrap();
    }

    async fn insert(db: &Db, row: &ItemRow) {
        assert!(insert_item(db, row).await.unwrap(), "insert {}", row.id);
    }

    fn episode(id: &str, show: &str, season: i64, ep: i64, added_at: i64) -> ItemRow {
        let mut r = row(id, &format!("{show} S{season:02}E{ep:02}"));
        r.category = Category::Episode;
        r.show_name = Some(show.to_string());
        r.season = Some(season);
        r.episode = Some(ep);
        r.rel_path = format!("Shows/{show}/Season {season:02}/{show} - S{season:02}E{ep:02}.mkv");
        r.added_at = added_at;
        r
    }

    #[test]
    fn sort_titles() {
        assert_eq!(sort_title("The Office"), "office");
        assert_eq!(sort_title("A Quiet Place"), "quiet place");
        assert_eq!(sort_title("An Education"), "education");
        assert_eq!(sort_title("Theodore"), "theodore");
        assert_eq!(sort_title("A"), "a");
        assert_eq!(sort_title("The "), "the");
        assert_eq!(escape_like("100%_a\\b"), "100\\%\\_a\\\\b");
    }

    #[tokio::test]
    async fn get_insert_and_conflict() {
        let db = crate::db::open_memory().await.unwrap();
        let r = row("a", "Heat");
        insert(&db, &r).await;
        let mut dup = row("b", "Heat");
        dup.rel_path = r.rel_path.clone();
        assert!(!insert_item(&db, &dup).await.unwrap());
        assert_eq!(get(&db, "a").await.unwrap().unwrap().title, "Heat");
        assert_eq!(
            get_by_path(&db, &r.rel_path).await.unwrap().unwrap().id,
            "a"
        );
        assert!(get(&db, "missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn pagination_is_stable_across_pages() {
        let db = crate::db::open_memory().await.unwrap();
        // 7 items, several sharing the same added_at so the id tiebreak matters.
        for i in 0..7 {
            let mut r = row(&format!("id{i}"), &format!("Movie {i}"));
            r.added_at = 1000 + (i / 3);
            r.size = 10 * (7 - i);
            insert(&db, &r).await;
        }
        for (sort, order) in [
            (ItemSort::Added, SortOrder::Desc),
            (ItemSort::Added, SortOrder::Asc),
            (ItemSort::Title, SortOrder::Asc),
            (ItemSort::Size, SortOrder::Desc),
            (ItemSort::Taken, SortOrder::Desc),
        ] {
            let mut seen = Vec::new();
            let mut cursor = None;
            let mut pages = 0;
            loop {
                let q = ItemQuery {
                    sort,
                    order,
                    cursor: cursor.clone(),
                    limit: Some(3),
                    ..Default::default()
                };
                let page = list(&db, &q, &owner("u")).await.unwrap();
                pages += 1;
                seen.extend(page.items.iter().map(|i| i.id.clone()));
                match page.next_cursor {
                    Some(c) => cursor = Some(c),
                    None => break,
                }
            }
            assert_eq!(pages, 3, "{sort:?} {order:?}");
            let mut unique = seen.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), 7, "{sort:?} {order:?}: {seen:?}");
            // Same order as one big page.
            let all = list(
                &db,
                &ItemQuery {
                    sort,
                    order,
                    limit: Some(50),
                    ..Default::default()
                },
                &owner("u"),
            )
            .await
            .unwrap();
            assert_eq!(
                all.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(),
                seen
            );
            assert!(all.next_cursor.is_none());
        }
        let page = list(
            &db,
            &ItemQuery {
                sort: ItemSort::Added,
                order: SortOrder::Desc,
                limit: Some(2),
                ..Default::default()
            },
            &owner("u"),
        )
        .await
        .unwrap();
        assert_eq!(page.items[0].id, "id6");
        // A cursor from another sort order is refused; garbage is refused.
        let bad = ItemQuery {
            sort: ItemSort::Title,
            cursor: page.next_cursor.clone(),
            ..Default::default()
        };
        assert!(matches!(
            list(&db, &bad, &owner("u")).await,
            Err(CoreError::Invalid(_))
        ));
        let bad = ItemQuery {
            cursor: Some("!!!".into()),
            ..Default::default()
        };
        assert!(matches!(
            list(&db, &bad, &owner("u")).await,
            Err(CoreError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn filters_and_search() {
        let db = crate::db::open_memory().await.unwrap();
        let mut a = row("a", "100% Wolf");
        a.year = Some(2020);
        a.favorite = true;
        insert(&db, &a).await;
        let mut b = row("b", "1000 Wolves");
        b.year = Some(2019);
        insert(&db, &b).await;
        let mut c = row("c", "snake_case");
        c.category = Category::Other;
        c.rel_path = "Other/snake_case.mkv".into();
        insert(&db, &c).await;
        let mut d = row("d", "snakeXcase");
        d.category = Category::Other;
        d.rel_path = "Other/snakeXcase.mkv".into();
        insert(&db, &d).await;
        let mut e = episode("e", "Wolf Hall", 1, 1, 5);
        e.title = "Episode One".into();
        insert(&db, &e).await;
        let mut hidden = row("h", "Wolf Review");
        hidden.status = ItemStatus::NeedsReview;
        insert(&db, &hidden).await;

        let ids = |p: Page<Item>| {
            let mut v: Vec<String> = p.items.into_iter().map(|i| i.id).collect();
            v.sort();
            v
        };
        let q = |s: &str| ItemQuery {
            q: Some(s.into()),
            ..Default::default()
        };
        assert_eq!(
            ids(list(&db, &q("100%"), &owner("u")).await.unwrap()),
            vec!["a"]
        );
        assert_eq!(
            ids(list(&db, &q("snake_case"), &owner("u")).await.unwrap()),
            vec!["c"]
        );
        assert_eq!(
            ids(list(&db, &q("WOLF"), &owner("u")).await.unwrap()),
            vec!["a", "e"]
        );
        assert_eq!(
            ids(list(&db, &q("Season 01"), &owner("u")).await.unwrap()),
            vec!["e"],
            "rel_path is searched"
        );
        let fav = ItemQuery {
            favorite: Some(true),
            ..Default::default()
        };
        assert_eq!(ids(list(&db, &fav, &owner("u")).await.unwrap()), vec!["a"]);
        let year = ItemQuery {
            year: Some(2019),
            ..Default::default()
        };
        assert_eq!(ids(list(&db, &year, &owner("u")).await.unwrap()), vec!["b"]);
        let cat = ItemQuery {
            category: Some(Category::Other),
            ..Default::default()
        };
        assert_eq!(
            ids(list(&db, &cat, &owner("u")).await.unwrap()),
            vec!["c", "d"]
        );
        let review = ItemQuery {
            status: Some(ItemStatus::NeedsReview),
            ..Default::default()
        };
        assert_eq!(
            ids(list(&db, &review, &owner("u")).await.unwrap()),
            vec!["h"]
        );
    }

    #[tokio::test]
    async fn progress_and_home() {
        let db = crate::db::open_memory().await.unwrap();
        add_user(&db, "u1").await;
        add_user(&db, "u2").await;
        for i in 0..15 {
            let mut r = row(&format!("m{i:02}"), &format!("Movie {i:02}"));
            r.added_at = 100 + i;
            r.size = 10;
            insert(&db, &r).await;
        }
        let mut photo = row("p1", "IMG_1");
        photo.category = Category::Photo;
        photo.kind = crate::models::MediaKind::Photo;
        photo.rel_path = "Photos/2024/06/IMG_1.jpg".into();
        photo.added_at = 10_000;
        photo.size = 5;
        insert(&db, &photo).await;
        insert(&db, &episode("e1", "The Office", 1, 1, 50)).await;
        insert(&db, &episode("e2", "The Office", 1, 2, 51)).await;
        insert(&db, &episode("e3", "Severance", 1, 1, 52)).await;
        let mut review = row("r1", "Unknown");
        review.status = ItemStatus::NeedsReview;
        review.size = 1_000_000;
        insert(&db, &review).await;

        set_progress(&db, "u1", "m01", 120.0, 3600.0).await.unwrap();
        set_progress(&db, "u1", "m02", 3500.0, 3600.0)
            .await
            .unwrap(); // finished
        set_progress(&db, "u1", "m03", 10.0, 3600.0).await.unwrap(); // too early
        set_progress(&db, "u2", "m04", 300.0, 3600.0).await.unwrap(); // other user
        // Upsert keeps one row and recomputes `finished`.
        set_progress(&db, "u1", "m05", 3300.0, 3600.0)
            .await
            .unwrap();
        set_progress(&db, "u1", "m05", 600.0, 3600.0).await.unwrap();
        assert!(matches!(
            set_progress(&db, "u1", "nope", 1.0, 2.0).await,
            Err(CoreError::NotFound)
        ));
        assert!(set_progress(&db, "u1", "m01", f64::NAN, 2.0).await.is_err());
        assert!(set_progress(&db, "u1", "m01", -1.0, 2.0).await.is_err());

        let s = home_as(&db, "u1").await;
        let library = |id: &str| s.libraries.iter().find(|l| l.library.id == id).unwrap();
        let movies = library(crate::library::defs::STARTING_MOVIES);
        assert_eq!(movies.library.item_count, 15);
        let shows_row = library(STARTING_SHOWS);
        assert_eq!(shows_row.library.item_count, 3);
        assert_eq!(shows_row.library.show_count, Some(2));
        assert_eq!(library(STARTING_PHOTOS).library.item_count, 1);
        assert_eq!(s.total_bytes, 15 * 10 + 5 + 3 * 1000);
        assert_eq!(s.inbox_count, 1);
        assert_eq!(s.other_files_count, 0);
        // Up to twelve per library, newest first.
        assert_eq!(movies.recent.len(), 12);
        assert_eq!(movies.recent[0].id, "m14");
        assert!(
            !movies.recent.iter().any(|i| i.id == "m02"),
            "the oldest movies drop off"
        );
        assert_eq!(library(STARTING_PHOTOS).recent[0].id, "p1");
        assert_eq!(
            shows_row.show_posters,
            BTreeMap::from([
                ("Severance".to_string(), "e3".to_string()),
                ("The Office".to_string(), "e1".to_string()),
            ])
        );
        let cw: Vec<&str> = s.continue_watching.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(cw.len(), 2);
        assert!(cw.contains(&"m01") && cw.contains(&"m05"));
        let p = s
            .continue_watching
            .iter()
            .find(|i| i.id == "m05")
            .unwrap()
            .progress
            .unwrap();
        assert_eq!(
            p,
            Progress {
                position_s: 600.0,
                duration_s: 3600.0,
                finished: false
            }
        );

        let d = detail_as(&db, "m02", "u1").await.unwrap();
        assert!(d.item.progress.unwrap().finished);
        assert!(
            detail_as(&db, "m02", "u2")
                .await
                .unwrap()
                .item
                .progress
                .is_none()
        );
        let page = list(
            &db,
            &ItemQuery {
                q: Some("Movie 04".into()),
                ..Default::default()
            },
            &owner("u2"),
        )
        .await
        .unwrap();
        assert_eq!(page.items[0].progress.unwrap().position_s, 300.0);
    }

    #[tokio::test]
    async fn home_rows_are_per_library_with_one_poster_per_show() {
        let db = crate::db::open_memory().await.unwrap();
        add_user(&db, "u").await;
        // A burst of new movies and photos, and older home videos and other files.
        for i in 0..14 {
            let mut r = row(&format!("m{i:02}"), &format!("Movie {i:02}"));
            r.added_at = 1_000 + i;
            insert(&db, &r).await;
        }
        for i in 0..13 {
            let mut r = row(&format!("p{i:02}"), &format!("IMG_{i:02}"));
            r.category = Category::Photo;
            r.kind = MediaKind::Photo;
            r.rel_path = format!("Photos/2024/06/IMG_{i:02}.jpg");
            r.added_at = 500 + i;
            insert(&db, &r).await;
        }
        for i in 0..2 {
            let mut r = row(&format!("h{i}"), &format!("VID_{i}"));
            r.category = Category::HomeVideo;
            r.rel_path = format!("Home Videos/2023/07/VID_{i}.mp4");
            r.added_at = 10 + i;
            insert(&db, &r).await;
        }
        let mut doc = row("o1", "Recipe Card");
        doc.category = Category::Other;
        doc.kind = MediaKind::Other;
        doc.rel_path = "Other/Recipe Card.pdf".into();
        doc.added_at = 5;
        insert(&db, &doc).await;
        // Only the second episode has a picture yet; the newest one has none.
        insert(&db, &episode("a1", "Andor", 1, 1, 20)).await;
        let mut pictured = episode("a2", "Andor", 1, 2, 21);
        pictured.thumb_state = ThumbState::Ready;
        insert(&db, &pictured).await;
        insert(&db, &episode("a3", "Andor", 1, 3, 22)).await;
        let mut gone = row("hx", "VID_gone");
        gone.category = Category::HomeVideo;
        gone.status = ItemStatus::Missing;
        gone.added_at = 2_000;
        insert(&db, &gone).await;

        let s = home_as(&db, "u").await;
        let recent = |id: &str| {
            s.libraries
                .iter()
                .find(|l| l.library.id == id)
                .unwrap()
                .recent
                .clone()
        };
        assert_eq!(
            recent(crate::library::defs::STARTING_MOVIES).len(),
            12,
            "twelve newest movies"
        );
        assert_eq!(recent(STARTING_PHOTOS).len(), 12, "photos are on Home too");
        assert_eq!(
            recent(crate::library::defs::STARTING_HOME_VIDEOS).len(),
            2,
            "new movies do not push home videos off Home"
        );
        assert_eq!(s.other_files_count, 1, "Other files are not a library");
        assert_eq!(recent(STARTING_SHOWS).len(), 3);
        let movies = recent(crate::library::defs::STARTING_MOVIES);
        assert!(
            movies
                .windows(2)
                .all(|w| (w[0].added_at, &w[0].id) >= (w[1].added_at, &w[1].id)),
            "newest first"
        );
        assert!(!movies.iter().any(|i| i.id == "hx" || i.id == "m00"));
        let shows_row = s
            .libraries
            .iter()
            .find(|l| l.library.id == STARTING_SHOWS)
            .unwrap();

        // Home, Shows and the show's page agree on the poster.
        let listed = shows(&db, STARTING_SHOWS).await.unwrap();
        assert_eq!(listed[0].poster_item_id.as_deref(), Some("a2"));
        assert_eq!(
            shows_row.show_posters.get("Andor").map(String::as_str),
            Some("a2")
        );
        let detail = show(&db, STARTING_SHOWS, "Andor", "u").await.unwrap();
        assert_eq!(detail.poster_item_id.as_deref(), Some("a2"));
        let typed = show(&db, STARTING_SHOWS, " Andor ", "u").await.unwrap();
        assert_eq!(typed.poster_item_id.as_deref(), Some("a2"));
    }

    /// Issue 3 of the design review: "3 shows, 18 episodes" must have three show tiles,
    /// even when the twelve newest episodes belong to two of them.
    #[tokio::test]
    async fn a_series_shelf_has_a_tile_for_every_show() {
        let db = crate::db::open_memory().await.unwrap();
        add_user(&db, "u").await;
        let now = crate::now();
        let day = 86_400;
        // An older show, added two months ago, with a poster.
        for ep in 1..=6 {
            let mut e = episode(
                &format!("c{ep}"),
                "Captain Paws",
                1,
                ep,
                now - 60 * day + ep,
            );
            e.thumb_state = ThumbState::Ready;
            insert(&db, &e).await;
        }
        // Two shows whose twelve episodes all came in this week.
        for ep in 1..=5 {
            insert(
                &db,
                &episode(
                    &format!("s{ep}"),
                    "Starlight Bakery",
                    1,
                    ep,
                    now - 3 * day + ep,
                ),
            )
            .await;
        }
        for ep in 1..=7 {
            insert(
                &db,
                &episode(
                    &format!("l{ep}"),
                    "The Lantern Keepers",
                    1,
                    ep,
                    now - 2 * day + ep,
                ),
            )
            .await;
        }
        // One more episode of the old show, added 10 days ago: new, but older than the rest.
        insert(&db, &episode("c7", "Captain Paws", 1, 7, now - 10 * day)).await;
        let mut gone = episode("c8", "Captain Paws", 1, 8, now);
        gone.status = ItemStatus::Missing;
        insert(&db, &gone).await;

        let home = home_as(&db, "u").await;
        let entry = home
            .libraries
            .iter()
            .find(|l| l.library.id == STARTING_SHOWS)
            .unwrap();
        assert_eq!(entry.library.show_count, Some(3));
        assert_eq!(entry.library.item_count, 19);
        let tiles: Vec<(&str, i64, i64, &str)> = entry
            .shows
            .iter()
            .map(|s| {
                (
                    s.show_name.as_str(),
                    s.item_count,
                    s.new_count,
                    s.latest.id.as_str(),
                )
            })
            .collect();
        assert_eq!(
            tiles,
            vec![
                ("The Lantern Keepers", 7, 7, "l7"),
                ("Starlight Bakery", 5, 5, "s5"),
                ("Captain Paws", 7, 1, "c7"),
            ],
            "every show, the most recently added first, with its new episodes counted"
        );
        assert_eq!(
            entry.shows.iter().map(|s| s.item_count).sum::<i64>(),
            entry.library.item_count,
            "the tiles add up to the heading"
        );
        assert_eq!(entry.shows[2].poster_item_id.as_deref(), Some("c1"));
        assert_eq!(
            entry.shows[0].poster_item_id.as_deref(),
            Some("l1"),
            "without a picture yet, the first episode"
        );
        assert_eq!(entry.shows[0].last_added_at, now - 2 * day + 7);
        // The twelve newest episodes alone would have shown two shows.
        let grouped: std::collections::BTreeSet<_> = entry
            .recent
            .iter()
            .filter_map(|i| i.show_name.clone())
            .collect();
        assert_eq!(grouped.len(), 2);
        // Other layouts have no shows.
        for other in home
            .libraries
            .iter()
            .filter(|l| l.library.id != STARTING_SHOWS)
        {
            assert!(other.shows.is_empty(), "{}", other.library.name);
        }

        // More than twelve shows: the twelve with the newest episodes.
        for n in 0..12 {
            insert(
                &db,
                &episode(
                    &format!("x{n:02}"),
                    &format!("Show {n:02}"),
                    1,
                    1,
                    now - 100 + n,
                ),
            )
            .await;
        }
        let home = home_as(&db, "u").await;
        let entry = home
            .libraries
            .iter()
            .find(|l| l.library.id == STARTING_SHOWS)
            .unwrap();
        let names: Vec<&str> = entry.shows.iter().map(|s| s.show_name.as_str()).collect();
        assert_eq!(names.len(), 12);
        assert_eq!(names[0], "Show 11");
        assert_eq!(names[11], "Show 00");
    }

    #[tokio::test]
    async fn shows_and_episode_navigation() {
        let db = crate::db::open_memory().await.unwrap();
        insert(&db, &episode("o2", "The Office", 1, 2, 2)).await;
        insert(&db, &episode("o1", "The Office", 1, 1, 1)).await;
        insert(&db, &episode("o3", "The Office", 2, 1, 3)).await;
        let mut thumb = episode("o4", "The Office", 2, 2, 4);
        thumb.thumb_state = ThumbState::Ready;
        insert(&db, &thumb).await;
        insert(&db, &episode("a1", "Andor", 1, 1, 9)).await;
        let mut gone = episode("o5", "The Office", 2, 3, 5);
        gone.status = ItemStatus::Missing;
        insert(&db, &gone).await;

        let list = shows(&db, STARTING_SHOWS).await.unwrap();
        assert_eq!(
            list.iter()
                .map(|s| s.show_name.as_str())
                .collect::<Vec<_>>(),
            vec!["Andor", "The Office"]
        );
        let office = &list[1];
        assert_eq!(
            (office.item_count, office.season_count, office.last_added_at),
            (4, 2, 4)
        );
        assert_eq!(
            office.poster_item_id.as_deref(),
            Some("o4"),
            "an episode with a thumbnail is the poster"
        );
        assert_eq!(list[0].poster_item_id.as_deref(), Some("a1"));

        let detail_show = show(&db, STARTING_SHOWS, "The Office", "u").await.unwrap();
        assert_eq!(
            detail_show.poster_item_id, office.poster_item_id,
            "the show's page uses the poster the Shows grid uses"
        );
        assert_eq!(detail_show.seasons.len(), 2);
        assert_eq!(
            detail_show.seasons[0]
                .episodes
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            vec!["o1", "o2"]
        );
        assert_eq!(
            detail_show.seasons[1]
                .episodes
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            vec!["o3", "o4"]
        );
        assert!(matches!(
            show(&db, STARTING_SHOWS, "Nope", "u").await,
            Err(CoreError::NotFound)
        ));

        let d = detail_as(&db, "o2", "u").await.unwrap();
        assert_eq!(d.next_episode_id.as_deref(), Some("o3"));
        assert_eq!(d.previous_episode_id.as_deref(), Some("o1"));
        let d = detail_as(&db, "o1", "u").await.unwrap();
        assert_eq!(d.previous_episode_id, None);
        let d = detail_as(&db, "o4", "u").await.unwrap();
        assert_eq!(d.next_episode_id, None, "missing episodes are skipped");
        assert!(matches!(
            detail_as(&db, "zzz", "u").await,
            Err(CoreError::NotFound)
        ));
    }

    /// A moment read on a server in UTC.
    fn utc_moment(at: i64) -> Option<crate::capture::CaptureTime> {
        crate::capture::Recorded::instant(at)?.resolve(&jiff::tz::TimeZone::UTC)
    }

    /// Photos near a month boundary taken in different time zones group under the month
    /// on the camera's clock, whatever that is in UTC or wherever the viewer is, and every
    /// month stays in one piece because the timeline is ordered by that clock.
    #[tokio::test]
    async fn the_timeline_groups_by_the_month_on_the_camera_clock() {
        use crate::capture::Recorded;
        let db = crate::db::open_memory().await.unwrap();
        let ny = jiff::tz::TimeZone::get("America/New_York").unwrap();
        let photos = [
            // June 30, 20:00 in Los Angeles: July 1, 03:00 UTC.
            ("la", Recorded::clock(2024, 6, 30, 20, 0, 0, Some(-420))),
            // July 1, 08:00 in Tokyo: June 30, 23:00 UTC.
            ("tokyo", Recorded::clock(2024, 7, 1, 8, 0, 0, Some(540))),
            // June 30, 22:30 on a camera that wrote no offset; the server is in New York.
            ("bare", Recorded::clock(2024, 6, 30, 22, 30, 0, None)),
            // A file time of July 1, 02:00 UTC on that New York server: June 30, 22:00.
            ("mtime", Recorded::instant(1_719_799_200)),
            // July 1, 00:30 on a camera without an offset.
            ("july", Recorded::clock(2024, 7, 1, 0, 30, 0, None)),
        ];
        for (id, recorded) in photos {
            let mut r = row(id, id);
            r.category = Category::Photo;
            r.kind = crate::models::MediaKind::Photo;
            r.rel_path = format!("Photos/x/{id}.jpg");
            r.set_taken(recorded.unwrap().resolve(&ny));
            insert(&db, &r).await;
        }
        let page = photo_timeline(&db, STARTING_PHOTOS, None, 0, "u")
            .await
            .unwrap();
        let months: Vec<(&str, i64, Vec<&str>)> = page
            .months
            .iter()
            .map(|m| {
                (
                    m.month.as_str(),
                    m.count,
                    m.items.iter().map(|i| i.id.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            months,
            vec![
                ("2024-07", 2, vec!["tokyo", "july"]),
                ("2024-06", 3, vec!["bare", "mtime", "la"]),
            ]
        );
        let la = &page.months[1].items[2];
        assert_eq!(la.taken_local.as_deref(), Some("2024-06-30T20:00:00"));
        assert_eq!(la.taken_offset, Some(-420));
        assert_eq!(la.taken_at, Some(1_719_802_800));
        // Paged one photo at a time, the months still come out whole and in order.
        let mut cursor: Option<String> = None;
        let mut seen = Vec::new();
        loop {
            let page = photo_timeline(&db, STARTING_PHOTOS, cursor.as_deref(), 1, "u")
                .await
                .unwrap();
            for m in &page.months {
                for i in &m.items {
                    seen.push((m.month.clone(), m.count, i.id.clone()));
                }
            }
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        let expected: Vec<(String, i64, String)> = [
            ("2024-07", 2, "tokyo"),
            ("2024-07", 2, "july"),
            ("2024-06", 3, "bare"),
            ("2024-06", 3, "mtime"),
            ("2024-06", 3, "la"),
        ]
        .into_iter()
        .map(|(m, c, id)| (m.to_string(), c, id.to_string()))
        .collect();
        assert_eq!(seen, expected);
        // `sort=taken` uses the same order.
        let q = ItemQuery {
            library_id: Some(STARTING_PHOTOS.to_string()),
            sort: ItemSort::Taken,
            order: SortOrder::Desc,
            ..Default::default()
        };
        let listed: Vec<String> = list(&db, &q, &owner("u"))
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(listed, ["tokyo", "july", "bare", "mtime", "la"]);
    }

    #[tokio::test]
    async fn photo_timeline_pages_by_month() {
        let db = crate::db::open_memory().await.unwrap();
        let june = 1_717_200_000; // 2024-06-01 00:00 UTC
        let may = 1_714_521_600; // 2024-05-01 00:00 UTC
        let dec = 1_701_388_800; // 2023-12-01 00:00 UTC
        let mut n = 0;
        let mut add = |taken: Option<i64>, added: i64| {
            n += 1;
            let mut r = row(&format!("p{n:02}"), &format!("IMG_{n}"));
            r.category = Category::Photo;
            r.kind = crate::models::MediaKind::Photo;
            r.rel_path = format!("Photos/x/IMG_{n}.jpg");
            r.set_taken(taken.and_then(utc_moment));
            r.added_at = added;
            r
        };
        let rows = vec![
            add(Some(june + 10), 1),
            add(Some(june + 20), 1),
            add(Some(june + 30), 1),
            add(Some(may + 5), 1),
            add(None, may + 100), // falls back to added_at
            add(Some(dec + 1), 1),
            add(Some(dec - 1), 1), // 2023-11-30
        ];
        for r in &rows {
            insert(&db, r).await;
        }
        let page = photo_timeline(&db, STARTING_PHOTOS, None, 3, "u")
            .await
            .unwrap();
        assert_eq!(page.months.len(), 1);
        assert_eq!(page.months[0].month, "2024-06");
        assert_eq!(page.months[0].count, 3);
        assert_eq!(
            page.months[0]
                .items
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            vec!["p03", "p02", "p01"]
        );
        let page2 = photo_timeline(&db, STARTING_PHOTOS, page.next_cursor.as_deref(), 3, "u")
            .await
            .unwrap();
        assert_eq!(
            page2
                .months
                .iter()
                .map(|m| m.month.as_str())
                .collect::<Vec<_>>(),
            vec!["2024-05", "2023-12"]
        );
        assert_eq!(page2.months[0].items.len(), 2);
        let page3 = photo_timeline(&db, STARTING_PHOTOS, page2.next_cursor.as_deref(), 3, "u")
            .await
            .unwrap();
        assert_eq!(page3.months.len(), 1);
        assert_eq!(page3.months[0].month, "2023-11");
        assert!(page3.next_cursor.is_none());
        let everything = photo_timeline(&db, STARTING_PHOTOS, None, 0, "u")
            .await
            .unwrap();
        assert_eq!(everything.months.len(), 4);
        assert!(everything.next_cursor.is_none());
        assert!(
            photo_timeline(&db, STARTING_PHOTOS, Some("garbage"), 3, "u")
                .await
                .is_err()
        );
        assert_eq!(
            month_bounds("2024-12"),
            Some(("2024-12".to_string(), "2025-01".to_string()))
        );
        assert_eq!(month_bounds("2024-13"), None);
    }

    #[tokio::test]
    async fn a_busy_month_is_split_across_pages() {
        let db = crate::db::open_memory().await.unwrap();
        let june = 1_717_200_000; // 2024-06-01 00:00 UTC
        let may = 1_714_521_600; // 2024-05-01 00:00 UTC
        for n in 0..5 {
            let mut r = row(&format!("j{n}"), &format!("June {n}"));
            r.category = Category::Photo;
            r.kind = crate::models::MediaKind::Photo;
            r.rel_path = format!("Photos/2024/06/j{n}.jpg");
            // Two photos share a timestamp so the id tiebreak is exercised.
            r.set_taken(utc_moment(june + 100 * (n / 2)));
            insert(&db, &r).await;
        }
        let mut m = row("m0", "May");
        m.category = Category::Photo;
        m.kind = crate::models::MediaKind::Photo;
        m.rel_path = "Photos/2024/05/m0.jpg".into();
        m.set_taken(utc_moment(may + 1));
        insert(&db, &m).await;

        let mut cursor: Option<String> = None;
        let mut pages = Vec::new();
        loop {
            let page = photo_timeline(&db, STARTING_PHOTOS, cursor.as_deref(), 2, "u")
                .await
                .unwrap();
            let n: usize = page.months.iter().map(|m| m.items.len()).sum();
            assert!(n <= 2, "a page never holds more than the limit");
            pages.push(page.months.clone());
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        assert_eq!(pages.len(), 3);
        let flat: Vec<(String, i64, String)> = pages
            .iter()
            .flatten()
            .flat_map(|m| {
                m.items
                    .iter()
                    .map(|i| (m.month.clone(), m.count, i.id.clone()))
            })
            .collect();
        assert_eq!(
            flat,
            vec![
                ("2024-06".into(), 5, "j4".into()),
                ("2024-06".into(), 5, "j3".into()),
                ("2024-06".into(), 5, "j2".into()),
                ("2024-06".into(), 5, "j1".into()),
                ("2024-06".into(), 5, "j0".into()),
                ("2024-05".into(), 1, "m0".into()),
            ],
            "every month carries its full count; nothing is repeated or skipped"
        );
    }

    #[tokio::test]
    async fn metadata_inbox_and_trash() {
        let db = crate::db::open_memory().await.unwrap();
        add_user(&db, "owner").await;
        insert(&db, &row("a", "Heat")).await;
        let patched = update_metadata(
            &db,
            "a",
            &ItemPatch {
                title: Some("  The Heat  ".into()),
                year: Some(Some(1995)),
                favorite: Some(true),
                episode_title: Some(Some("   ".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(patched.title, "The Heat");
        assert_eq!(patched.sort_title, "heat");
        assert_eq!(patched.year, Some(1995));
        assert!(patched.favorite);
        assert_eq!(patched.episode_title, None);
        let cleared = update_metadata(
            &db,
            "a",
            &ItemPatch {
                year: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.year, None);
        assert_eq!(cleared.title, "The Heat");
        assert!(
            update_metadata(
                &db,
                "a",
                &ItemPatch {
                    title: Some(" ".into()),
                    ..Default::default()
                }
            )
            .await
            .is_err()
        );
        assert!(
            update_metadata(
                &db,
                "a",
                &ItemPatch {
                    year: Some(Some(3)),
                    ..Default::default()
                }
            )
            .await
            .is_err()
        );
        assert!(matches!(
            update_metadata(&db, "zz", &ItemPatch::default()).await,
            Err(CoreError::NotFound)
        ));

        let mut r1 = row("r1", "Review one");
        r1.status = ItemStatus::NeedsReview;
        r1.added_at = 10;
        r1.suggestion_json = Some(
            serde_json::to_string(&crate::library::classify::classify(
                "Inbox/Some.Movie.1080p.mkv",
                None,
                None,
                0,
            ))
            .unwrap(),
        );
        insert(&db, &r1).await;
        let mut r2 = row("r2", "Review two");
        r2.status = ItemStatus::NeedsReview;
        r2.added_at = 20;
        insert(&db, &r2).await;
        let access = Access::load(&db, &owner("owner")).await.unwrap();
        let inbox_items = inbox(&db, &StashLayout::new("/nonexistent"), &access)
            .await
            .unwrap()
            .items;
        assert_eq!(
            inbox_items
                .iter()
                .map(|d| d.item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["r1", "r2"],
            "oldest first"
        );
        assert_eq!(
            inbox_items[0]
                .suggestion
                .as_ref()
                .unwrap()
                .destination
                .as_deref(),
            Some("Movies/Some Movie/Some Movie.mkv")
        );
        assert_eq!(needs_review_count(&db).await.unwrap(), 2);

        let mut t = row("t", "Trashed");
        t.status = ItemStatus::Trashed;
        t.rel_path = ".popcornstash/Trash/t/Trashed.mkv".into();
        insert(&db, &t).await;
        sqlx::query("INSERT INTO trash (item_id, original_rel_path, trashed_rel_path, trashed_at, trashed_by) VALUES ('t', 'Movies/Trashed.mkv', ?, 42, 'owner')")
            .bind(&t.rel_path)
            .execute(&db)
            .await
            .unwrap();
        let trash = trash_list(&db).await.unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].original_rel_path, "Movies/Trashed.mkv");
        assert_eq!(trash[0].trashed_at, 42);
        assert_eq!(trash[0].trashed_by.as_deref(), Some("owner"));
        assert_eq!(trash[0].item.status, ItemStatus::Trashed);
    }
}
