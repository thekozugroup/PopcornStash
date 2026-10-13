//! Item queries used by the API (section 8). All functions take the requesting
//! user's id where per-user playback progress is part of the result.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};

use crate::db::Db;
use crate::models::{Category, Item, ItemDetail, ItemRow, ItemStatus, Progress};
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

/// Default and maximum page sizes for [`list`].
pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 200;
/// Items per timeline page when the caller does not say (whole months are always returned).
pub const DEFAULT_TIMELINE_LIMIT: u32 = 200;
const SUMMARY_ROW: i64 = 12;
const INBOX_MAX: i64 = 500;

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
        next_episode_id: Option<String>,
        previous_episode_id: Option<String>,
    ) -> ItemDetail {
        let progress = self.progress();
        let row = self.row;
        ItemDetail {
            item: row.to_item(progress),
            media: row.media(),
            mime: row.mime.clone(),
            suggestion: row.suggestion(),
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
            episode_title, size, mtime, quick_hash, mime, duration_s, width, height, media_json, taken_at, camera,
            thumb_state, suggestion_json, favorite, source_download_id, missing_since, added_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(&row.camera)
    .bind(row.thumb_state.as_str())
    .bind(&row.suggestion_json)
    .bind(row.favorite)
    .bind(&row.source_download_id)
    .bind(row.missing_since)
    .bind(row.added_at)
    .bind(row.updated_at)
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

/// Position in the photo timeline: the last photo returned (its time key and id).
#[derive(Debug, Serialize, Deserialize)]
struct TimelineCursor {
    k: i64,
    id: String,
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
        ItemSort::Taken => "COALESCE(i.taken_at, i.added_at)",
        ItemSort::Size => "i.size",
    }
}

fn sort_key_of(row: &ItemRow, sort: ItemSort) -> CursorKey {
    match sort {
        ItemSort::Added => CursorKey::Int(row.added_at),
        ItemSort::Title => CursorKey::Text(row.sort_title.clone()),
        ItemSort::Taken => CursorKey::Int(row.taken_at.unwrap_or(row.added_at)),
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

pub async fn list(db: &Db, query: &ItemQuery, user_id: &str) -> Result<Page<Item>> {
    let limit = i64::from(query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT));
    let status = query.status.unwrap_or(ItemStatus::Ready);
    let key_expr = sort_key_expr(query.sort);
    let (dir, op) = match query.order {
        SortOrder::Desc => ("DESC", "<"),
        SortOrder::Asc => ("ASC", ">"),
    };

    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string());
    qb.push(" WHERE i.status = ").push_bind(status.as_str());
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
            (CursorKey::Text(_), ItemSort::Title)
                | (
                    CursorKey::Int(_),
                    ItemSort::Added | ItemSort::Taken | ItemSort::Size
                )
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

pub async fn detail(db: &Db, id: &str, user_id: &str) -> Result<ItemDetail> {
    let row = get_with_progress(db, id, user_id)
        .await?
        .ok_or(CoreError::NotFound)?;
    let (mut next, mut previous) = (None, None);
    if let (Category::Episode, Some(show), Some(season), Some(episode)) = (
        row.row.category,
        row.row.show_name.as_deref(),
        row.row.season,
        row.row.episode,
    ) {
        next = sqlx::query_scalar::<_, String>(
            "SELECT id FROM items WHERE category = 'episode' AND status = 'ready' AND show_name = ? AND id != ?
               AND (season > ? OR (season = ? AND episode > ?))
             ORDER BY season, episode, id LIMIT 1",
        )
        .bind(show)
        .bind(id)
        .bind(season)
        .bind(season)
        .bind(episode)
        .fetch_optional(db)
        .await?;
        previous = sqlx::query_scalar::<_, String>(
            "SELECT id FROM items WHERE category = 'episode' AND status = 'ready' AND show_name = ? AND id != ?
               AND (season < ? OR (season = ? AND episode < ?))
             ORDER BY season DESC, episode DESC, id DESC LIMIT 1",
        )
        .bind(show)
        .bind(id)
        .bind(season)
        .bind(season)
        .bind(episode)
        .fetch_optional(db)
        .await?;
    }
    Ok(row.into_detail(next, previous))
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

pub async fn summary(db: &Db, user_id: &str) -> Result<LibrarySummary> {
    let mut counts = LibraryCounts::default();
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, count(*) FROM items WHERE status = 'ready' GROUP BY category",
    )
    .fetch_all(db)
    .await?;
    for (category, n) in rows {
        match category.parse::<Category>() {
            Ok(Category::Movie) => counts.movies = n,
            Ok(Category::Episode) => counts.episodes = n,
            Ok(Category::HomeVideo) => counts.home_videos = n,
            Ok(Category::Photo) => counts.photos = n,
            Ok(Category::Other) => counts.other = n,
            Err(_) => {}
        }
    }
    counts.shows = sqlx::query_scalar(
        "SELECT count(DISTINCT show_name) FROM items
         WHERE status = 'ready' AND category = 'episode' AND show_name IS NOT NULL AND show_name != ''",
    )
    .fetch_one(db)
    .await?;
    let total_bytes: Option<i64> =
        sqlx::query_scalar("SELECT sum(size) FROM items WHERE status = 'ready'")
            .fetch_one(db)
            .await?;

    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string())
        .push(" WHERE i.status = 'ready' AND i.category != 'photo' ORDER BY i.added_at DESC, i.id DESC LIMIT ")
        .push_bind(SUMMARY_ROW);
    let recently_added = qb
        .build_query_as::<ItemWithProgress>()
        .fetch_all(db)
        .await?;

    let continue_watching = sqlx::query_as::<_, ItemWithProgress>(
        "SELECT i.*, p.position_s AS p_position_s, p.duration_s AS p_duration_s, p.finished AS p_finished
         FROM item_progress p JOIN items i ON i.id = p.item_id
         WHERE p.user_id = ? AND p.finished = 0 AND p.position_s > 30 AND i.status = 'ready'
         ORDER BY p.updated_at DESC, i.id DESC LIMIT ?",
    )
    .bind(user_id)
    .bind(SUMMARY_ROW)
    .fetch_all(db)
    .await?;

    Ok(LibrarySummary {
        counts,
        total_bytes: total_bytes.unwrap_or(0),
        needs_review: needs_review_count(db).await?,
        recently_added: recently_added
            .into_iter()
            .map(ItemWithProgress::into_item)
            .collect(),
        continue_watching: continue_watching
            .into_iter()
            .map(ItemWithProgress::into_item)
            .collect(),
    })
}

pub async fn shows(db: &Db) -> Result<Vec<ShowSummary>> {
    let rows: Vec<(String, i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT i.show_name, count(*), count(DISTINCT i.season), max(i.added_at),
           (SELECT j.id FROM items j
             WHERE j.show_name = i.show_name AND j.category = 'episode' AND j.status = 'ready'
             ORDER BY (j.thumb_state = 'ready') DESC, j.season, j.episode, j.id LIMIT 1)
         FROM items i
         WHERE i.category = 'episode' AND i.status = 'ready' AND i.show_name IS NOT NULL AND i.show_name != ''
         GROUP BY i.show_name",
    )
    .fetch_all(db)
    .await?;
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

pub async fn show(db: &Db, show_name: &str, user_id: &str) -> Result<ShowDetail> {
    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string())
        // Names typed or stored with stray spaces still match.
        .push(" WHERE i.category = 'episode' AND i.status = 'ready' AND TRIM(i.show_name) = ")
        .push_bind(show_name.trim().to_string())
        .push(" ORDER BY COALESCE(i.season, 0), COALESCE(i.episode, 0), i.sort_title, i.id");
    let rows = qb
        .build_query_as::<ItemWithProgress>()
        .fetch_all(db)
        .await?;
    if rows.is_empty() {
        return Err(CoreError::NotFound);
    }
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
        seasons,
    })
}

/// Earliest and latest time keys the timeline can place (years 0001 to 9999, UTC).
const TIMELINE_MIN_KEY: i64 = -62_135_596_800;
const TIMELINE_MAX_KEY: i64 = 253_402_300_799;

/// Photos grouped by month (of `taken_at`, else `added_at`, UTC), newest first. A page
/// holds at most `limit` photos (`0` means [`DEFAULT_TIMELINE_LIMIT`]), so one busy month
/// can span several pages: `count` is always the month's full total and the next page
/// continues the same month where this one stopped (clients merge consecutive entries
/// with the same `month`).
pub async fn photo_timeline(
    db: &Db,
    cursor: Option<&str>,
    limit: u32,
    user_id: &str,
) -> Result<PhotoTimeline> {
    let limit = i64::from(if limit == 0 {
        DEFAULT_TIMELINE_LIMIT
    } else {
        limit.min(MAX_LIMIT * 5)
    });
    let after: Option<TimelineCursor> = cursor
        .filter(|c| !c.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;
    let key = "COALESCE(i.taken_at, i.added_at)";
    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string())
        .push(format!(
            " WHERE i.category = 'photo' AND i.status = 'ready' AND {key} BETWEEN "
        ))
        .push_bind(TIMELINE_MIN_KEY)
        .push(" AND ")
        .push_bind(TIMELINE_MAX_KEY);
    if let Some(after) = &after {
        qb.push(format!(" AND ({key} < "))
            .push_bind(after.k)
            .push(format!(" OR ({key} = "))
            .push_bind(after.k)
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
                k: last.row.taken_at.unwrap_or(last.row.added_at),
                id: last.row.id.clone(),
            })?),
            None => None,
        }
    } else {
        None
    };

    let mut months: Vec<PhotoMonth> = Vec::new();
    for row in rows {
        let Some(month) = month_of(row.row.taken_at.unwrap_or(row.row.added_at)) else {
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
        let totals: Vec<(Option<String>, i64)> = sqlx::query_as(
            "SELECT strftime('%Y-%m', COALESCE(taken_at, added_at), 'unixepoch') AS month, count(*)
             FROM items
             WHERE category = 'photo' AND status = 'ready'
               AND COALESCE(taken_at, added_at) >= ? AND COALESCE(taken_at, added_at) < ?
             GROUP BY month",
        )
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

/// `YYYY-MM` for a unix time (UTC), matching SQLite's `strftime('%Y-%m', t, 'unixepoch')`.
fn month_of(ts: i64) -> Option<String> {
    let dt = time::OffsetDateTime::from_unix_timestamp(ts).ok()?;
    Some(format!("{:04}-{:02}", dt.year(), u8::from(dt.month())))
}

/// Unix start of `YYYY-MM` and of the month after it.
fn month_bounds(month: &str) -> Option<(i64, i64)> {
    let (y, m) = month.split_once('-')?;
    let (y, m): (i32, u8) = (y.parse().ok()?, m.parse().ok()?);
    let start_month = time::Month::try_from(m).ok()?;
    let start = time::Date::from_calendar_date(y, start_month, 1).ok()?;
    let (ny, nm) = if m == 12 {
        (y + 1, time::Month::January)
    } else {
        (y, start_month.next())
    };
    let end = time::Date::from_calendar_date(ny, nm, 1).ok()?;
    let ts = |d: time::Date| d.midnight().assume_utc().unix_timestamp();
    Some((ts(start), ts(end)))
}

pub async fn inbox(db: &Db, user_id: &str) -> Result<Vec<ItemDetail>> {
    let mut qb = QueryBuilder::<Sqlite>::new(SELECT_ITEMS);
    qb.push_bind(user_id.to_string())
        .push(" WHERE i.status = 'needs_review' ORDER BY i.added_at DESC, i.id DESC LIMIT ")
        .push_bind(INBOX_MAX);
    let rows = qb
        .build_query_as::<ItemWithProgress>()
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| r.into_detail(None, None))
        .collect())
}

pub async fn needs_review_count(db: &Db) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM items WHERE status = 'needs_review'")
            .fetch_one(db)
            .await?,
    )
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
    use crate::models::{MediaKind, ThumbState};

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
            camera: None,
            thumb_state: ThumbState::Pending,
            suggestion_json: None,
            favorite: false,
            source_download_id: None,
            missing_since: None,
            added_at: 1_700_000_000,
            updated_at: 1_700_000_000,
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
                let page = list(&db, &q, "u").await.unwrap();
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
                "u",
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
            "u",
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
            list(&db, &bad, "u").await,
            Err(CoreError::Invalid(_))
        ));
        let bad = ItemQuery {
            cursor: Some("!!!".into()),
            ..Default::default()
        };
        assert!(matches!(
            list(&db, &bad, "u").await,
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
        assert_eq!(ids(list(&db, &q("100%"), "u").await.unwrap()), vec!["a"]);
        assert_eq!(
            ids(list(&db, &q("snake_case"), "u").await.unwrap()),
            vec!["c"]
        );
        assert_eq!(
            ids(list(&db, &q("WOLF"), "u").await.unwrap()),
            vec!["a", "e"]
        );
        assert_eq!(
            ids(list(&db, &q("Season 01"), "u").await.unwrap()),
            vec!["e"],
            "rel_path is searched"
        );
        let fav = ItemQuery {
            favorite: Some(true),
            ..Default::default()
        };
        assert_eq!(ids(list(&db, &fav, "u").await.unwrap()), vec!["a"]);
        let year = ItemQuery {
            year: Some(2019),
            ..Default::default()
        };
        assert_eq!(ids(list(&db, &year, "u").await.unwrap()), vec!["b"]);
        let cat = ItemQuery {
            category: Some(Category::Other),
            ..Default::default()
        };
        assert_eq!(ids(list(&db, &cat, "u").await.unwrap()), vec!["c", "d"]);
        let review = ItemQuery {
            status: Some(ItemStatus::NeedsReview),
            ..Default::default()
        };
        assert_eq!(ids(list(&db, &review, "u").await.unwrap()), vec!["h"]);
    }

    #[tokio::test]
    async fn progress_and_summary() {
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

        let s = summary(&db, "u1").await.unwrap();
        assert_eq!(s.counts.movies, 15);
        assert_eq!(s.counts.episodes, 3);
        assert_eq!(s.counts.shows, 2);
        assert_eq!(s.counts.photos, 1);
        assert_eq!(s.counts.home_videos, 0);
        assert_eq!(s.total_bytes, 15 * 10 + 5 + 3 * 1000);
        assert_eq!(s.needs_review, 1);
        assert_eq!(s.recently_added.len(), 12);
        assert!(
            s.recently_added
                .iter()
                .all(|i| i.category != Category::Photo)
        );
        assert_eq!(s.recently_added[0].id, "m14");
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

        let d = detail(&db, "m02", "u1").await.unwrap();
        assert!(d.item.progress.unwrap().finished);
        assert!(
            detail(&db, "m02", "u2")
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
            "u2",
        )
        .await
        .unwrap();
        assert_eq!(page.items[0].progress.unwrap().position_s, 300.0);
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

        let list = shows(&db).await.unwrap();
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

        let detail_show = show(&db, "The Office", "u").await.unwrap();
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
            show(&db, "Nope", "u").await,
            Err(CoreError::NotFound)
        ));

        let d = detail(&db, "o2", "u").await.unwrap();
        assert_eq!(d.next_episode_id.as_deref(), Some("o3"));
        assert_eq!(d.previous_episode_id.as_deref(), Some("o1"));
        let d = detail(&db, "o1", "u").await.unwrap();
        assert_eq!(d.previous_episode_id, None);
        let d = detail(&db, "o4", "u").await.unwrap();
        assert_eq!(d.next_episode_id, None, "missing episodes are skipped");
        assert!(matches!(
            detail(&db, "zzz", "u").await,
            Err(CoreError::NotFound)
        ));
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
            r.taken_at = taken;
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
        let page = photo_timeline(&db, None, 3, "u").await.unwrap();
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
        let page2 = photo_timeline(&db, page.next_cursor.as_deref(), 3, "u")
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
        let page3 = photo_timeline(&db, page2.next_cursor.as_deref(), 3, "u")
            .await
            .unwrap();
        assert_eq!(page3.months.len(), 1);
        assert_eq!(page3.months[0].month, "2023-11");
        assert!(page3.next_cursor.is_none());
        let everything = photo_timeline(&db, None, 0, "u").await.unwrap();
        assert_eq!(everything.months.len(), 4);
        assert!(everything.next_cursor.is_none());
        assert!(photo_timeline(&db, Some("garbage"), 3, "u").await.is_err());
        assert_eq!(
            month_bounds("2024-12"),
            Some((1_733_011_200, 1_735_689_600))
        );
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
            r.taken_at = Some(june + 100 * (n / 2));
            insert(&db, &r).await;
        }
        let mut m = row("m0", "May");
        m.category = Category::Photo;
        m.kind = crate::models::MediaKind::Photo;
        m.rel_path = "Photos/2024/05/m0.jpg".into();
        m.taken_at = Some(may + 1);
        insert(&db, &m).await;

        let mut cursor: Option<String> = None;
        let mut pages = Vec::new();
        loop {
            let page = photo_timeline(&db, cursor.as_deref(), 2, "u")
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
        let inbox_items = inbox(&db, "owner").await.unwrap();
        assert_eq!(
            inbox_items
                .iter()
                .map(|d| d.item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["r2", "r1"]
        );
        assert_eq!(
            inbox_items[1].suggestion.as_ref().unwrap().destination,
            "Movies/Some Movie/Some Movie.mkv"
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
