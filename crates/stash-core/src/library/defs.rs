//! Libraries the owner defines (sections 3.1, 4 and 8): their rows, claims and people,
//! the icon list, folder rules and state, how each is shown, and the writes that create,
//! change, reorder and delete them. None of these writes moves, renames or deletes a file.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};
use sqlx::SqliteConnection;

use super::access::Access;
use super::scanner::{in_place_category, is_junk_name, library_metadata};
use super::{Library, repo};
use crate::db::Db;
use crate::events::{Event, LibraryChange};
use crate::jobs::JobKind;
use crate::models::{
    Category, ContentType, FolderState, ItemFacts, ItemRow, ItemStatus, Layout, MediaKind, Place,
    Role, Structure, Visibility,
};
use crate::paths::{self, StashLayout};
use crate::{CoreError, Rejection, Result, new_id, now};

/// The curated icon names, in picker order (section 8, Icons). Lucide names (kebab case)
/// from the release the web app pins; web and Android draw the same glyphs.
pub const LIBRARY_ICONS: &[&str] = &[
    "film",
    "clapperboard",
    "tv",
    "popcorn",
    "projector",
    "video",
    "drama",
    "image",
    "images",
    "camera",
    "aperture",
    "users",
    "baby",
    "heart",
    "cake",
    "party-popper",
    "graduation-cap",
    "gift",
    "plane",
    "tree-palm",
    "mountain-snow",
    "tent",
    "sun",
    "snowflake",
    "gamepad-2",
    "music",
    "book-open",
    "trophy",
    "paw-print",
    "bike",
    "star",
    "folder",
];

/// The most libraries a stash can have.
pub const MAX_LIBRARIES: usize = 24;
pub const MAX_NAME_CHARS: usize = 40;
pub const MAX_FOLDER_CHARS: usize = 100;
pub const LIBRARY_NOT_FOUND: &str = "That library no longer exists.";
/// What a library may not be called: the places in the sidebar that are not libraries.
const FIXED_PLACES: &[&str] = &["home", "other files", "inbox", "downloads", "settings"];
/// Top-level folders that belong to PopcornStash (any case).
const RESERVED_FOLDERS: &[&str] = &["inbox", "downloads", "other"];
/// The four libraries every stash starts with (migration 0002).
pub const STARTING_MOVIES: &str = "0192a000-0000-7000-8000-000000000001";
pub const STARTING_SHOWS: &str = "0192a000-0000-7000-8000-000000000002";
pub const STARTING_HOME_VIDEOS: &str = "0192a000-0000-7000-8000-000000000003";
pub const STARTING_PHOTOS: &str = "0192a000-0000-7000-8000-000000000004";
const FOLDER_UNAVAILABLE: &str =
    "This library's folder is missing or cannot be written, so nothing can move there now.";
/// Kinds a library can claim, in the order claims are listed.
pub const CLAIMABLE: [Category; 4] = [
    Category::Movie,
    Category::Episode,
    Category::HomeVideo,
    Category::Photo,
];

/// One row of `libraries`.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LibraryRow {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub content_type: ContentType,
    pub structure: Option<Structure>,
    pub folder: String,
    pub visibility: Visibility,
    pub position: i64,
    pub created_by: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The three shapes a library can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Episodes,
    OneOffs,
    Photos,
}

impl Shape {
    pub fn of(content_type: ContentType, structure: Option<Structure>) -> Self {
        match (content_type, structure) {
            (ContentType::Photos, _) => Shape::Photos,
            (ContentType::Video, Some(Structure::Episodes)) => Shape::Episodes,
            (ContentType::Video, _) => Shape::OneOffs,
        }
    }

    /// Whether a library of this shape can claim `category` (section 4, claims).
    pub fn fits(self, category: Category) -> bool {
        matches!(
            (self, category),
            (Shape::OneOffs, Category::Movie | Category::HomeVideo)
                | (Shape::Episodes, Category::Episode)
                | (Shape::Photos, Category::Photo)
        )
    }
}

/// A library with the kinds it claims and the members listed for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDef {
    pub row: LibraryRow,
    pub claims: Vec<Category>,
    pub members: Vec<String>,
}

impl LibraryDef {
    pub fn id(&self) -> &str {
        &self.row.id
    }

    pub fn folder(&self) -> &str {
        &self.row.folder
    }

    pub fn shape(&self) -> Shape {
        Shape::of(self.row.content_type, self.row.structure)
    }

    pub fn claims(&self, category: Category) -> bool {
        self.claims.contains(&category)
    }
}

/// Every library in order, with claims and members.
pub async fn load_all(db: &Db) -> Result<Vec<LibraryDef>> {
    let mut conn = db.acquire().await?;
    load_all_conn(&mut conn).await
}

pub async fn load_all_conn(conn: &mut SqliteConnection) -> Result<Vec<LibraryDef>> {
    let rows: Vec<LibraryRow> =
        sqlx::query_as("SELECT * FROM libraries ORDER BY position, created_at, id")
            .fetch_all(&mut *conn)
            .await?;
    let claims: Vec<(Category, String)> =
        sqlx::query_as("SELECT category, library_id FROM library_claims")
            .fetch_all(&mut *conn)
            .await?;
    let members: Vec<(String, String)> = sqlx::query_as(
        "SELECT m.library_id, m.user_id FROM library_members m JOIN users u ON u.id = m.user_id
         ORDER BY u.created_at, u.id",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let claims = CLAIMABLE
                .iter()
                .copied()
                .filter(|c| claims.iter().any(|(k, l)| k == c && *l == row.id))
                .collect();
            let members = members
                .iter()
                .filter(|(l, _)| *l == row.id)
                .map(|(_, u)| u.clone())
                .collect();
            LibraryDef {
                row,
                claims,
                members,
            }
        })
        .collect())
}

pub async fn load_one(db: &Db, id: &str) -> Result<Option<LibraryDef>> {
    Ok(load_all(db).await?.into_iter().find(|l| l.id() == id))
}

/// The library whose folder is the first component of `rel` (exact match), if any.
pub fn library_for_path<'a>(defs: &'a [LibraryDef], rel: &str) -> Option<&'a LibraryDef> {
    let top = paths::top_folder(rel)?;
    defs.iter().find(|l| l.folder() == top)
}

/// Where a path is, worked out from the library rows (the SQL in migration 0002 does
/// the same in the database).
pub fn place_of(defs: &[LibraryDef], rel: &str, status: ItemStatus) -> (Place, Option<String>) {
    if status == ItemStatus::Trashed {
        return (Place::Trash, None);
    }
    if let Some(def) = library_for_path(defs, rel) {
        return (Place::Library, Some(def.id().to_string()));
    }
    let starts = |prefix: &str| rel.starts_with(&format!("{prefix}/"));
    if starts(paths::OTHER) {
        (Place::OtherFiles, None)
    } else if starts(paths::INBOX) || starts(paths::COMPLETED) {
        (Place::Incoming, None)
    } else {
        (Place::Folder, None)
    }
}

/// The library whose folder is an item's first path component (exact case, whatever the
/// column's collation), as SQL on the row being updated. Migration 0002's triggers use
/// the same expressions.
const LIBRARY_EXPR: &str = "(CASE WHEN status = 'trashed' THEN NULL ELSE (
        SELECT l.id FROM libraries l
        WHERE instr(items.rel_path, '/') > 0
          AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) END)";

/// The kind of place that holds an item, as SQL on the row being updated.
const PLACE_EXPR: &str = "(CASE
        WHEN status = 'trashed' THEN 'trash'
        WHEN EXISTS (
            SELECT 1 FROM libraries l
            WHERE instr(items.rel_path, '/') > 0
              AND l.folder = substr(items.rel_path, 1, instr(items.rel_path, '/') - 1) COLLATE BINARY) THEN 'library'
        WHEN substr(rel_path, 1, 6) = 'Other/' THEN 'other_files'
        WHEN substr(rel_path, 1, 6) = 'Inbox/' OR substr(rel_path, 1, 20) = 'Downloads/Completed/' THEN 'incoming'
        ELSE 'folder' END)";

/// `library_id = ..., place = ...` worked out from `rel_path` and `status`.
pub(crate) fn derive_set() -> String {
    format!("library_id = {LIBRARY_EXPR}, place = {PLACE_EXPR}")
}

/// Work out `library_id` and `place` again for every item whose stored values differ
/// (the end of every rescan, under the move lock).
pub(crate) async fn rederive_all(conn: &mut SqliteConnection) -> Result<u64> {
    let done = sqlx::query(&format!(
        "UPDATE items SET {} WHERE library_id IS NOT {LIBRARY_EXPR} OR place IS NOT {PLACE_EXPR}",
        derive_set()
    ))
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected())
}

/// Items under the top-level folder `folder` (exact case), as a SQL condition with one
/// bind: the folder followed by a slash.
const UNDER_FOLDER: &str = "substr(rel_path, 1, length(?1)) = ?1";

// ---------------------------------------------------------------------------
// Folders

/// Check a folder name (section 3.1, Folder names). Returns it trimmed.
pub fn check_folder_name(raw: &str) -> Result<String> {
    let name = raw.trim();
    let invalid = name.is_empty()
        || name.chars().count() > MAX_FOLDER_CHARS
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || name == "."
        || name == "..";
    if invalid {
        return Err(CoreError::bad_field(
            "folder.name",
            "A folder name needs 1 to 100 characters, no slashes, and cannot start with a dot or end with a dot or a space.",
        ));
    }
    if RESERVED_FOLDERS.contains(&name.to_lowercase().as_str()) {
        return Err(CoreError::bad_field(
            "folder.name",
            "Inbox, Downloads and Other are PopcornStash's own folders. Choose another name.",
        ));
    }
    if is_junk_name(name) {
        return Err(CoreError::bad_field(
            "folder.name",
            "PopcornStash skips folders with that name. Choose another name.",
        ));
    }
    Ok(name.to_string())
}

/// Whether a library's folder can be used now (section 3.1). `has_live_items`: the
/// library has items that are not marked missing; then an absent, unreadable or
/// completely empty folder (nothing in it at all, like an unmounted disk's mount point)
/// is missing. An empty library's absent folder is fine: it is created when needed.
pub async fn folder_state(layout: &StashLayout, folder: &str, has_live_items: bool) -> FolderState {
    let path = layout.root().join(folder);
    let meta = match tokio::fs::symlink_metadata(&path).await {
        Ok(meta) => meta,
        Err(_) if !has_live_items => return FolderState::Ok,
        Err(_) => return FolderState::Missing,
    };
    if !meta.is_dir() {
        return FolderState::Missing;
    }
    let path2 = path.clone();
    // (readable, holds anything at all)
    let looked = tokio::task::spawn_blocking(move || match std::fs::read_dir(&path2) {
        Ok(mut read) => (true, read.next().is_some()),
        Err(_) => (false, false),
    })
    .await
    .unwrap_or((false, false));
    match looked {
        (false, _) => return FolderState::Missing,
        // An unmounted disk or share leaves an empty mount point.
        (true, false) if has_live_items => return FolderState::Missing,
        _ => {}
    }
    if nix::unistd::access(&path, nix::unistd::AccessFlags::W_OK).is_err() {
        return FolderState::ReadOnly;
    }
    FolderState::Ok
}

/// Per library: (items in any status, items not marked missing).
pub(crate) async fn item_counts(db: &Db) -> Result<HashMap<String, (i64, i64)>> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT library_id, count(*), sum(status != 'missing') FROM items
         WHERE library_id IS NOT NULL GROUP BY library_id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, all, live)| (id, (all, live)))
        .collect())
}

/// The folder state of every library.
pub async fn folder_states(
    db: &Db,
    layout: &StashLayout,
    defs: &[LibraryDef],
) -> Result<HashMap<String, FolderState>> {
    let counts = item_counts(db).await?;
    let mut out = HashMap::new();
    for def in defs {
        let live = counts.get(def.id()).is_some_and(|(_, live)| *live > 0);
        out.insert(
            def.id().to_string(),
            folder_state(layout, def.folder(), live).await,
        );
    }
    Ok(out)
}

/// Make sure a library's folder can take files now: an empty library's absent folder is
/// created; anything else unusable is a 409 naming `library_id`. The caller holds the
/// move lock.
pub(crate) async fn ready_folder(lib: &Library, def: &LibraryDef) -> Result<()> {
    let (all, live): (i64, i64) = sqlx::query_as(
        "SELECT count(*), coalesce(sum(status != 'missing'), 0) FROM items WHERE library_id = ?",
    )
    .bind(def.id())
    .fetch_one(&lib.db)
    .await?;
    let path = lib.layout.root().join(def.folder());
    if all == 0 && tokio::fs::symlink_metadata(&path).await.is_err() {
        if let Err(e) = tokio::fs::create_dir(&path).await {
            tracing::warn!(error = %e, folder = %def.folder(), "could not create a library's folder");
            return Err(CoreError::conflict_field("library_id", FOLDER_UNAVAILABLE));
        }
        tracing::info!(folder = %def.folder(), "created a library's folder");
    }
    match folder_state(&lib.layout, def.folder(), live > 0).await {
        FolderState::Ok => Ok(()),
        _ => Err(CoreError::conflict_field("library_id", FOLDER_UNAVAILABLE)),
    }
}

/// Library folders that are missing right now: nothing in them may be marked missing.
pub(crate) async fn missing_folders(lib: &Library) -> Result<HashSet<String>> {
    let defs = load_all(&lib.db).await?;
    let states = folder_states(&lib.db, &lib.layout, &defs).await?;
    Ok(defs
        .iter()
        .filter(|d| states.get(d.id()) == Some(&FolderState::Missing))
        .map(|d| d.folder().to_string())
        .collect())
}

/// Create the folder of every empty library whose folder is absent (at startup). A
/// folder that cannot be created is logged and shows as missing; it never stops the server.
pub async fn ensure_folders(db: &Db, layout: &StashLayout) -> Result<()> {
    let defs = load_all(db).await?;
    let counts = item_counts(db).await?;
    for def in defs {
        if counts.get(def.id()).is_some_and(|(all, _)| *all > 0) {
            continue;
        }
        let path = layout.root().join(def.folder());
        let made = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) if meta.is_dir() => continue,
            // Something else is in the way.
            Ok(_) => Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists)),
            Err(_) => tokio::fs::create_dir(&path).await,
        };
        if let Err(e) = made {
            tracing::error!(
                "{} The library {} shows its folder as missing until it is fixed.",
                paths::layout_error("create", &path, &e),
                def.row.name
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// How a library is shown (`Library`, section 8)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryView {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub content_type: ContentType,
    pub structure: Option<Structure>,
    pub claims: Vec<Category>,
    pub layout: Layout,
    pub position: i64,
    pub item_count: i64,
    pub show_count: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub folder: Option<String>,
    pub visibility: Option<Visibility>,
    pub member_ids: Option<Vec<String>>,
    pub folder_state: Option<FolderState>,
    pub is_empty: Option<bool>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    all: i64,
    live: i64,
    ready: i64,
    ready_videos: i64,
    ready_movies: i64,
    shows: i64,
}

async fn view_counts(db: &Db) -> Result<HashMap<String, Counts>> {
    let rows: Vec<(String, i64, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT library_id, count(*), sum(status != 'missing'), sum(status = 'ready'),
                sum(status = 'ready' AND kind = 'video'),
                sum(status = 'ready' AND kind = 'video' AND category = 'movie'),
                count(DISTINCT CASE WHEN status = 'ready' AND category = 'episode'
                                     AND show_name IS NOT NULL AND show_name != '' THEN show_name END)
         FROM items WHERE library_id IS NOT NULL GROUP BY library_id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, all, live, ready, ready_videos, ready_movies, shows)| {
                (
                    id,
                    Counts {
                        all,
                        live,
                        ready,
                        ready_videos,
                        ready_movies,
                        shows,
                    },
                )
            },
        )
        .collect())
}

/// How clients draw a library (section 8, Layout).
fn layout_of(def: &LibraryDef, counts: Counts) -> Layout {
    match def.shape() {
        Shape::Photos => Layout::Timeline,
        Shape::Episodes => Layout::Series,
        Shape::OneOffs => match (def.claims(Category::Movie), def.claims(Category::HomeVideo)) {
            (true, _) => Layout::Posters,
            (false, true) => Layout::Frames,
            (false, false) if counts.ready_movies * 2 >= counts.ready_videos => Layout::Posters,
            (false, false) => Layout::Frames,
        },
    }
}

/// The libraries the caller sees, in order, as `GET /api/libraries` answers.
pub async fn views(db: &Db, layout: &StashLayout, access: &Access) -> Result<Vec<LibraryView>> {
    let defs = load_all(db).await?;
    let counts = view_counts(db).await?;
    let mut out = Vec::new();
    for def in defs.into_iter().filter(|d| access.sees_library(d.id())) {
        let c = counts.get(def.id()).copied().unwrap_or_default();
        let owner = access.full_owner();
        let folder_state = if owner {
            Some(folder_state(layout, def.folder(), c.live > 0).await)
        } else {
            None
        };
        out.push(LibraryView {
            layout: layout_of(&def, c),
            position: out.len() as i64,
            item_count: c.ready,
            show_count: (def.shape() == Shape::Episodes).then_some(c.shows),
            folder: owner.then(|| def.row.folder.clone()),
            visibility: owner.then_some(def.row.visibility),
            member_ids: owner.then(|| def.members.clone()),
            folder_state,
            is_empty: owner.then_some(c.all == 0),
            id: def.row.id,
            name: def.row.name,
            icon: def.row.icon,
            content_type: def.row.content_type,
            structure: def.row.structure,
            claims: def.claims,
            created_at: def.row.created_at,
            updated_at: def.row.updated_at,
        });
    }
    Ok(out)
}

/// One library as the caller sees it; a library they cannot see is not found.
pub async fn view(db: &Db, layout: &StashLayout, access: &Access, id: &str) -> Result<LibraryView> {
    views(db, layout, access)
        .await?
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| CoreError::library_not_found(None))
}

/// What a stream compares to decide whether its caller's library list changed (counts aside).
pub fn fingerprint(views: &[LibraryView]) -> String {
    let stripped: Vec<LibraryView> = views
        .iter()
        .cloned()
        .map(|mut v| {
            v.item_count = 0;
            v.show_count = v.show_count.map(|_| 0);
            v.is_empty = v.is_empty.map(|_| false);
            v
        })
        .collect();
    serde_json::to_string(&stripped).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Folders that are not a library (`UnassignedFolder`, section 8)

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedType {
    pub content_type: ContentType,
    pub structure: Option<Structure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnassignedFolder {
    pub name: String,
    pub item_count: i64,
    pub video_count: i64,
    pub photo_count: i64,
    pub other_count: i64,
    pub suggested: SuggestedType,
}

/// Every top-level folder that is not a library folder or one of PopcornStash's own,
/// by name, with the items indexed in it.
pub async fn unassigned_folders(lib: &Library) -> Result<Vec<UnassignedFolder>> {
    let defs = load_all(&lib.db).await?;
    let taken: HashSet<String> = defs.iter().map(|d| d.row.folder.clone()).collect();
    let root = lib.layout.root().to_path_buf();
    let mut names: Vec<String> = tokio::task::spawn_blocking(move || {
        let Ok(read) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        read.flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| {
                !n.starts_with('.')
                    && !is_junk_name(n)
                    && ![paths::INBOX, paths::DOWNLOADS, paths::OTHER].contains(&n.as_str())
            })
            .collect()
    })
    .await
    .map_err(|e| CoreError::Other(e.into()))?;
    names.retain(|n| !taken.contains(n));
    names.sort_by_key(|n| (n.to_lowercase(), n.clone()));
    let mut out = Vec::new();
    for name in names {
        let (videos, photos, all, episodes): (i64, i64, i64, i64) = sqlx::query_as(&format!(
            "SELECT coalesce(sum(kind = 'video'), 0), coalesce(sum(kind = 'photo'), 0), count(*),
                    coalesce(sum(kind = 'video' AND category = 'episode'), 0)
             FROM items WHERE status IN ('ready', 'needs_review') AND place = 'folder' AND {UNDER_FOLDER}"
        ))
        .bind(format!("{name}/"))
        .fetch_one(&lib.db)
        .await?;
        let suggested = if photos * 2 > all {
            SuggestedType {
                content_type: ContentType::Photos,
                structure: None,
            }
        } else if episodes * 2 > videos {
            SuggestedType {
                content_type: ContentType::Video,
                structure: Some(Structure::Episodes),
            }
        } else {
            SuggestedType {
                content_type: ContentType::Video,
                structure: Some(Structure::OneOffs),
            }
        };
        out.push(UnassignedFolder {
            name,
            item_count: all,
            video_count: videos,
            photo_count: photos,
            other_count: all - videos - photos,
            suggested,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Requests

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FolderRequest {
    pub mode: Option<String>,
    pub name: Option<String>,
}

/// `POST /api/libraries` body. Everything is checked here so each refusal names its field.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CreateLibrary {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub content_type: Option<String>,
    pub structure: Option<String>,
    pub claims: Option<Vec<String>>,
    pub folder: Option<FolderRequest>,
    pub visibility: Option<String>,
    pub member_ids: Option<Vec<String>>,
    pub position: Option<i64>,
}

fn present<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// `PATCH /api/libraries/:id` body: only the fields sent change.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EditLibrary {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub content_type: Option<String>,
    #[serde(deserialize_with = "present")]
    pub structure: Option<Option<String>>,
    pub claims: Option<Vec<String>>,
    pub visibility: Option<String>,
    pub member_ids: Option<Vec<String>>,
    #[serde(deserialize_with = "present")]
    pub folder: Option<Option<serde_json::Value>>,
}

fn check_name(raw: Option<&str>) -> Result<String> {
    let name = raw.unwrap_or_default().trim();
    if name.is_empty() {
        return Err(CoreError::bad_field("name", "Give the library a name."));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(CoreError::bad_field(
            "name",
            "Keep the name to 40 characters or fewer.",
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(CoreError::bad_field(
            "name",
            "The name cannot contain control characters.",
        ));
    }
    Ok(name.to_string())
}

async fn check_name_free(
    conn: &mut SqliteConnection,
    name: &str,
    except: Option<&str>,
) -> Result<()> {
    let lower = name.to_lowercase();
    if FIXED_PLACES.contains(&lower.as_str()) {
        return Err(CoreError::conflict_field(
            "name",
            "That name belongs to a place in PopcornStash. Choose another name.",
        ));
    }
    let names: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM libraries")
        .fetch_all(&mut *conn)
        .await?;
    if names
        .iter()
        .any(|(id, n)| Some(id.as_str()) != except && n.to_lowercase() == lower)
    {
        return Err(CoreError::conflict_field(
            "name",
            "Another library already has that name. Choose another name.",
        ));
    }
    Ok(())
}

fn check_icon(raw: Option<&str>) -> Result<String> {
    match raw {
        Some(icon) if LIBRARY_ICONS.contains(&icon) => Ok(icon.to_string()),
        _ => Err(CoreError::bad_field(
            "icon",
            "Choose an icon from the list.",
        )),
    }
}

fn parse_content_type(raw: Option<&str>) -> Result<ContentType> {
    raw.and_then(|t| t.parse().ok())
        .ok_or_else(|| CoreError::bad_field("content_type", "Choose video or photos."))
}

/// The structure for `content_type`: required for video, absent for photos.
fn check_structure(content_type: ContentType, raw: Option<&str>) -> Result<Option<Structure>> {
    match (content_type, raw) {
        (ContentType::Photos, None) => Ok(None),
        (ContentType::Photos, Some(_)) => Err(CoreError::bad_field(
            "structure",
            "A photos library has no structure. Leave it out.",
        )),
        (ContentType::Video, raw) => raw.and_then(|s| s.parse().ok()).map(Some).ok_or_else(|| {
            CoreError::bad_field(
                "structure",
                "Choose episodes or one-offs for a video library.",
            )
        }),
    }
}

fn check_claims(raw: &[String], shape: Shape) -> Result<Vec<Category>> {
    let mut claims = Vec::new();
    for value in raw {
        let category = value
            .parse::<Category>()
            .ok()
            .filter(|c| CLAIMABLE.contains(c))
            .ok_or_else(|| {
                CoreError::bad_field(
                    "claims",
                    "A library can take movies, TV episodes, camera videos or photos.",
                )
            })?;
        if !shape.fits(category) {
            return Err(CoreError::bad_field("claims", misfit(category)));
        }
        if !claims.contains(&category) {
            claims.push(category);
        }
    }
    claims.sort_by_key(|c| CLAIMABLE.iter().position(|k| k == c));
    Ok(claims)
}

fn misfit(category: Category) -> &'static str {
    match category {
        Category::Movie => "Movies fit only a video library of one-offs.",
        Category::HomeVideo => "Camera videos fit only a video library of one-offs.",
        Category::Episode => "TV episodes fit only a video library of episodes.",
        _ => "Photos fit only a photos library.",
    }
}

fn parse_visibility(raw: &str) -> Result<Visibility> {
    raw.parse()
        .map_err(|_| CoreError::bad_field("visibility", "Choose everyone or selected people."))
}

async fn check_members(conn: &mut SqliteConnection, raw: &[String]) -> Result<Vec<String>> {
    let mut ids: Vec<String> = Vec::new();
    for id in raw {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    for id in &ids {
        let role: Option<Role> = sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
        match role {
            Some(Role::Member) => {}
            Some(Role::Owner) => {
                return Err(CoreError::bad_field(
                    "member_ids",
                    "Owners always see every library. Choose members only.",
                ));
            }
            None => {
                return Err(CoreError::bad_field(
                    "member_ids",
                    "One of the chosen people no longer exists.",
                ));
            }
        }
    }
    Ok(ids)
}

/// Give `claims` to `library_id`, taking them from whichever library held them, and
/// leave `library_id` with exactly those. Returns the libraries that lost a claim.
async fn set_claims(
    conn: &mut SqliteConnection,
    library_id: &str,
    claims: &[Category],
) -> Result<Vec<String>> {
    let mut losers = Vec::new();
    for category in claims {
        let holder: Option<String> =
            sqlx::query_scalar("SELECT library_id FROM library_claims WHERE category = ?")
                .bind(category.as_str())
                .fetch_optional(&mut *conn)
                .await?;
        if let Some(holder) = holder.filter(|h| h != library_id) {
            losers.push(holder);
        }
        sqlx::query(
            "INSERT INTO library_claims (category, library_id) VALUES (?, ?)
             ON CONFLICT(category) DO UPDATE SET library_id = excluded.library_id",
        )
        .bind(category.as_str())
        .bind(library_id)
        .execute(&mut *conn)
        .await?;
    }
    let keep = serde_json::to_string(&claims.iter().map(|c| c.as_str()).collect::<Vec<_>>())?;
    sqlx::query(
        "DELETE FROM library_claims WHERE library_id = ? AND category NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(library_id)
    .bind(keep)
    .execute(&mut *conn)
    .await?;
    let t = now();
    for loser in &losers {
        sqlx::query("UPDATE libraries SET updated_at = ? WHERE id = ?")
            .bind(t)
            .bind(loser)
            .execute(&mut *conn)
            .await?;
    }
    Ok(losers)
}

async fn set_members(conn: &mut SqliteConnection, library_id: &str, ids: &[String]) -> Result<()> {
    sqlx::query("DELETE FROM library_members WHERE library_id = ?")
        .bind(library_id)
        .execute(&mut *conn)
        .await?;
    for id in ids {
        sqlx::query("INSERT INTO library_members (library_id, user_id) VALUES (?, ?)")
            .bind(library_id)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Rewrite positions as 0..n-1 in the given order.
async fn write_order(conn: &mut SqliteConnection, ids: &[String]) -> Result<()> {
    for (n, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE libraries SET position = ? WHERE id = ?")
            .bind(n as i64)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

async fn ordered_ids(conn: &mut SqliteConnection) -> Result<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM libraries ORDER BY position, created_at, id")
            .fetch_all(&mut *conn)
            .await?,
    )
}

/// What a library write did, for the audit log.
#[derive(Debug, Clone)]
pub struct Written {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub changed: Vec<&'static str>,
}

/// An item a library made from an existing folder left out of the index.
struct Dropped {
    id: String,
    facts: ItemFacts,
}

impl Library {
    /// `POST /api/libraries` (section 8, Create).
    pub async fn create_library(
        &self,
        request: CreateLibrary,
        created_by: &str,
    ) -> Result<Written> {
        let name = check_name(request.name.as_deref())?;
        let icon = check_icon(request.icon.as_deref())?;
        let content_type = parse_content_type(request.content_type.as_deref())?;
        let structure = check_structure(content_type, request.structure.as_deref())?;
        let shape = Shape::of(content_type, structure);
        let claims = check_claims(request.claims.as_deref().unwrap_or_default(), shape)?;
        let folder_request = request.folder.clone().unwrap_or_default();
        let existing = match folder_request.mode.as_deref() {
            Some("new") => false,
            Some("existing") => true,
            _ => {
                return Err(CoreError::bad_field(
                    "folder.mode",
                    "Choose a new folder or an existing one.",
                ));
            }
        };
        let folder = check_folder_name(folder_request.name.as_deref().unwrap_or_default())?;
        let visibility = match (request.visibility.as_deref(), existing) {
            (Some(v), _) => parse_visibility(v)?,
            (None, false) => Visibility::Everyone,
            (None, true) => {
                return Err(CoreError::bad_field(
                    "visibility",
                    "Choose who sees this library.",
                ));
            }
        };

        let _moves = self.lock_moves().await;
        let mut tx = self.db.begin().await?;
        check_name_free(&mut tx, &name, None).await?;
        let members =
            check_members(&mut tx, request.member_ids.as_deref().unwrap_or_default()).await?;
        let order = ordered_ids(&mut tx).await?;
        if order.len() >= MAX_LIBRARIES {
            return Err(CoreError::rejected(
                Rejection::Conflict,
                None,
                "A stash can have at most 24 libraries.",
            ));
        }
        let folders: Vec<String> = sqlx::query_scalar("SELECT folder FROM libraries")
            .fetch_all(&mut *tx)
            .await?;
        let path = self.layout.root().join(&folder);
        let mut made_folder = false;
        if existing {
            if folders
                .iter()
                .any(|f| f.to_lowercase() == folder.to_lowercase())
            {
                return Err(CoreError::conflict_field(
                    "folder.name",
                    format!("{folder} is already the folder of another library."),
                ));
            }
            match tokio::fs::symlink_metadata(&path).await {
                Ok(meta) if meta.is_dir() => {}
                _ => {
                    return Err(CoreError::not_found_field(
                        "folder.name",
                        format!("There is no folder called {folder} in the stash folder."),
                    ));
                }
            }
            if let Err(e) = tokio::fs::read_dir(&path).await {
                return Err(CoreError::conflict_field(
                    "folder.name",
                    paths::layout_error("read", &path, &e).to_string(),
                ));
            }
        } else {
            let lower = folder.to_lowercase();
            let root = self.layout.root().to_path_buf();
            let clash = folders.iter().any(|f| f.to_lowercase() == lower)
                || tokio::task::spawn_blocking(move || {
                    std::fs::read_dir(root).is_ok_and(|read| {
                        read.flatten()
                            .any(|e| e.file_name().to_string_lossy().to_lowercase() == lower)
                    })
                })
                .await
                .unwrap_or(true);
            if clash {
                return Err(CoreError::conflict_field(
                    "folder.name",
                    format!(
                        "Something called {folder} is already in the stash folder. Choose another name, or make a library from that folder."
                    ),
                ));
            }
            if let Err(e) = tokio::fs::create_dir(&path).await {
                return Err(CoreError::conflict_field(
                    "folder.name",
                    paths::layout_error("create", &path, &e).to_string(),
                ));
            }
            made_folder = true;
        }

        let result = async {
            let id = new_id();
            let t = now();
            sqlx::query(
                "INSERT INTO libraries (id, name, icon, content_type, structure, folder, visibility, position,
                                        created_by, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&name)
            .bind(&icon)
            .bind(content_type.as_str())
            .bind(structure.map(Structure::as_str))
            .bind(&folder)
            .bind(visibility.as_str())
            .bind(order.len() as i64)
            .bind(created_by)
            .bind(t)
            .bind(t)
            .execute(&mut *tx)
            .await?;
            let mut order = order.clone();
            let at = request
                .position
                .map_or(order.len(), |p| p.clamp(0, order.len() as i64) as usize);
            order.insert(at, id.clone());
            write_order(&mut tx, &order).await?;
            set_claims(&mut tx, &id, &claims).await?;
            set_members(&mut tx, &id, &members).await?;
            let mut joined = Vec::new();
            let mut dropped = Vec::new();
            if existing {
                let def = LibraryDef {
                    row: LibraryRow {
                        id: id.clone(),
                        name: name.clone(),
                        icon: icon.clone(),
                        content_type,
                        structure,
                        folder: folder.clone(),
                        visibility,
                        position: at as i64,
                        created_by: Some(created_by.to_string()),
                        created_at: t,
                        updated_at: t,
                    },
                    claims: claims.clone(),
                    members: members.clone(),
                };
                (joined, dropped) = take_in_folder(&mut tx, &def).await?;
            }
            Ok::<_, CoreError>((id, joined, dropped))
        }
        .await;
        let (id, joined, dropped) = match result {
            Ok(done) => done,
            Err(e) => {
                if made_folder {
                    let _ = tokio::fs::remove_dir(&path).await;
                }
                return Err(e);
            }
        };
        if let Err(e) = tx.commit().await {
            if made_folder {
                let _ = tokio::fs::remove_dir(&path).await;
            }
            return Err(e.into());
        }
        self.bump_generation();
        drop(_moves);
        for item in &dropped {
            super::organizer::remove_cached_media(self, &item.id).await;
            self.events.publish(Event::Library {
                r#type: LibraryChange::Removed,
                item_id: item.id.clone(),
                before: Some(item.facts.clone()),
            });
        }
        if !joined.is_empty() {
            self.events.publish(Event::Regrouped {
                item_ids: joined,
                from: None,
                to: Some(id.clone()),
            });
        }
        self.events.publish(Event::Libraries);
        if existing {
            self.jobs
                .enqueue(JobKind::Scan, None, serde_json::json!({}))
                .await?;
        }
        tracing::info!(library = %id, %name, %folder, existing, "created a library");
        Ok(Written {
            id,
            name,
            folder,
            changed: Vec::new(),
        })
    }

    /// `PATCH /api/libraries/:id` (section 8, Edit).
    pub async fn edit_library(&self, id: &str, request: EditLibrary) -> Result<Written> {
        if request.folder.is_some() {
            return Err(CoreError::bad_field(
                "folder",
                "A library's folder cannot change.",
            ));
        }
        let name = request
            .name
            .as_deref()
            .map(|n| check_name(Some(n)))
            .transpose()?;
        let icon = request
            .icon
            .as_deref()
            .map(|i| check_icon(Some(i)))
            .transpose()?;
        let content_type = request
            .content_type
            .as_deref()
            .map(|t| parse_content_type(Some(t)))
            .transpose()?;
        let visibility = request
            .visibility
            .as_deref()
            .map(parse_visibility)
            .transpose()?;
        let moves_files =
            request.claims.is_some() || content_type.is_some() || request.structure.is_some();
        let _moves = if moves_files {
            Some(self.lock_moves().await)
        } else {
            None
        };
        let mut tx = self.db.begin().await?;
        let defs = load_all_conn(&mut tx).await?;
        let current = defs
            .into_iter()
            .find(|d| d.id() == id)
            .ok_or_else(|| CoreError::library_not_found(None))?;
        let mut changed: Vec<&'static str> = Vec::new();
        let mut row = current.row.clone();
        if let Some(name) = name.filter(|n| *n != row.name) {
            check_name_free(&mut tx, &name, Some(id)).await?;
            row.name = name;
            changed.push("name");
        }
        if let Some(icon) = icon.filter(|i| *i != row.icon) {
            row.icon = icon;
            changed.push("icon");
        }
        let new_type = content_type.unwrap_or(row.content_type);
        let new_structure = match (&request.structure, new_type) {
            (Some(raw), _) => check_structure(new_type, raw.as_deref())?,
            (None, ContentType::Photos) => None,
            (None, ContentType::Video) => match row.structure {
                Some(s) => Some(s),
                None => check_structure(new_type, None)?,
            },
        };
        let type_changed = new_type != row.content_type;
        let structure_changed = new_structure != row.structure;
        if type_changed || structure_changed {
            let items: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE library_id = ?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
            if items > 0 {
                return Err(CoreError::conflict_field(
                    if type_changed {
                        "content_type"
                    } else {
                        "structure"
                    },
                    "Change the type only while the library is empty. Move its files out first, or make a new library.",
                ));
            }
            if type_changed {
                changed.push("content_type");
            }
            if structure_changed {
                changed.push("structure");
            }
            row.content_type = new_type;
            row.structure = new_structure;
        }
        let shape = Shape::of(row.content_type, row.structure);
        let claims = match &request.claims {
            Some(raw) => check_claims(raw, shape)?,
            None => {
                if let Some(&bad) = current.claims.iter().find(|c| !shape.fits(**c)) {
                    return Err(CoreError::bad_field("claims", misfit(bad)));
                }
                current.claims.clone()
            }
        };
        if claims != current.claims {
            changed.push("claims");
        }
        if let Some(v) = visibility.filter(|v| *v != row.visibility) {
            row.visibility = v;
            changed.push("visibility");
        }
        let members = match &request.member_ids {
            Some(raw) => check_members(&mut tx, raw).await?,
            None => current.members.clone(),
        };
        let mut sorted_new = members.clone();
        sorted_new.sort();
        let mut sorted_old = current.members.clone();
        sorted_old.sort();
        if sorted_new != sorted_old {
            changed.push("member_ids");
        }
        if !changed.is_empty() {
            sqlx::query(
                "UPDATE libraries SET name = ?, icon = ?, content_type = ?, structure = ?, visibility = ?, updated_at = ?
                 WHERE id = ?",
            )
            .bind(&row.name)
            .bind(&row.icon)
            .bind(row.content_type.as_str())
            .bind(row.structure.map(Structure::as_str))
            .bind(row.visibility.as_str())
            .bind(now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
            if changed.contains(&"claims") {
                set_claims(&mut tx, id, &claims).await?;
            }
            if changed.contains(&"member_ids") {
                set_members(&mut tx, id, &members).await?;
            }
        }
        tx.commit().await?;
        if moves_files {
            self.bump_generation();
        }
        drop(_moves);
        if !changed.is_empty() {
            self.events.publish(Event::Libraries);
        }
        if type_changed || structure_changed {
            self.jobs
                .enqueue(JobKind::Scan, None, serde_json::json!({}))
                .await?;
        }
        Ok(Written {
            id: id.to_string(),
            name: row.name,
            folder: row.folder,
            changed,
        })
    }

    /// `PUT /api/libraries/order`: `ids` lists every library exactly once.
    pub async fn reorder_libraries(&self, ids: &[String]) -> Result<()> {
        let mut tx = self.db.begin().await?;
        let current = ordered_ids(&mut tx).await?;
        let mut a: Vec<&String> = ids.iter().collect();
        let mut b: Vec<&String> = current.iter().collect();
        a.sort();
        b.sort();
        let unique = {
            let mut d = a.clone();
            d.dedup();
            d.len() == a.len()
        };
        if a != b || !unique {
            return Err(CoreError::bad_field(
                "ids",
                "List every library exactly once.",
            ));
        }
        write_order(&mut tx, ids).await?;
        tx.commit().await?;
        if ids != current.as_slice() {
            self.events.publish(Event::Libraries);
        }
        Ok(())
    }

    /// `DELETE /api/libraries/:id` (section 8, Delete). Files are never touched.
    pub async fn delete_library(&self, id: &str) -> Result<Written> {
        let _moves = self.lock_moves().await;
        let mut tx = self.db.begin().await?;
        let row: LibraryRow = sqlx::query_as("SELECT * FROM libraries WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| CoreError::library_not_found(None))?;
        let item_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM items WHERE library_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
        sqlx::query("UPDATE items SET library_id = NULL, place = 'folder' WHERE library_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for table in ["library_claims", "library_members"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE library_id = ?"))
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM libraries WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let order = ordered_ids(&mut tx).await?;
        write_order(&mut tx, &order).await?;
        tx.commit().await?;
        self.bump_generation();
        drop(_moves);
        if !item_ids.is_empty() {
            self.events.publish(Event::Regrouped {
                item_ids,
                from: Some(id.to_string()),
                to: None,
            });
        }
        self.events.publish(Event::Libraries);
        tracing::info!(library = %id, name = %row.name, folder = %row.folder, "deleted a library; its files stay");
        Ok(Written {
            id: id.to_string(),
            name: row.name,
            folder: row.folder,
            changed: Vec::new(),
        })
    }
}

/// A library made from an existing folder: items it indexes join it, with category and
/// metadata read again from the path (ids, favourites and progress kept); the rest leave
/// the index. Runs in the creating transaction, under the move lock.
async fn take_in_folder(
    conn: &mut SqliteConnection,
    def: &LibraryDef,
) -> Result<(Vec<String>, Vec<Dropped>)> {
    let rows: Vec<ItemRow> = sqlx::query_as(&format!(
        "SELECT * FROM items WHERE status != 'trashed' AND {UNDER_FOLDER}"
    ))
    .bind(format!("{}/", def.folder()))
    .fetch_all(&mut *conn)
    .await?;
    let (mut joined, mut dropped) = (Vec::new(), Vec::new());
    for item in rows {
        let size = u64::try_from(item.size).unwrap_or(0);
        match in_place_category(def, &item.rel_path, item.kind, size) {
            Some(category) => {
                let meta = library_metadata(category, &item.rel_path, item.mtime);
                let taken_at = match category {
                    Category::HomeVideo | Category::Photo => item.taken_at.or(meta.taken_at),
                    _ => None,
                };
                sqlx::query(
                    "UPDATE items SET category = ?, title = ?, sort_title = ?, year = ?, show_name = ?, season = ?,
                        episode = ?, episode_title = ?, taken_at = ?, suggestion_json = NULL, updated_at = ?
                     WHERE id = ?",
                )
                .bind(category.as_str())
                .bind(&meta.title)
                .bind(repo::sort_title(&meta.title))
                .bind(meta.year)
                .bind(&meta.show_name)
                .bind(meta.season)
                .bind(meta.episode)
                .bind(&meta.episode_title)
                .bind(taken_at)
                .bind(now())
                .bind(&item.id)
                .execute(&mut *conn)
                .await?;
                joined.push(item.id);
            }
            None => {
                sqlx::query("DELETE FROM items WHERE id = ?")
                    .bind(&item.id)
                    .execute(&mut *conn)
                    .await?;
                dropped.push(Dropped {
                    facts: item.facts(),
                    id: item.id,
                });
            }
        }
    }
    sqlx::query(&format!(
        "UPDATE items SET {} WHERE {UNDER_FOLDER}",
        derive_set()
    ))
    .bind(format!("{}/", def.folder()))
    .execute(&mut *conn)
    .await?;
    Ok((joined, dropped))
}

/// Whether `kind` could go into a library of `shape` by an accept or Organize, and the
/// sentence when it cannot (section 8, Inbox accept and Organize).
pub fn accepts(shape: Shape, kind: MediaKind) -> Result<()> {
    match (shape, kind) {
        (Shape::Photos, MediaKind::Photo | MediaKind::Video) => Ok(()),
        (Shape::Episodes | Shape::OneOffs, MediaKind::Video) => Ok(()),
        (Shape::Episodes | Shape::OneOffs, MediaKind::Photo) => Err(CoreError::bad_field(
            "library_id",
            "Photos go in a photos library.",
        )),
        _ => Err(CoreError::bad_field(
            "library_id",
            "Only videos and photos go in a library. This file goes in Other files.",
        )),
    }
}

/// True when the path of a stored relative path is under `folder`.
pub fn is_under(rel: &str, folder: &str) -> bool {
    Path::new(rel)
        .components()
        .next()
        .is_some_and(|c| c.as_os_str() == folder)
        && rel.len() > folder.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        for good in ["Kids", "Grandma's photos", "2019 Holiday", "Ünïcødé"] {
            assert_eq!(check_folder_name(good).unwrap(), good);
        }
        assert_eq!(check_folder_name("  Kids  ").unwrap(), "Kids");
        for bad in [
            "",
            " ",
            ".hidden",
            "a/b",
            "a\\b",
            "trailing.",
            "..",
            ".",
            "a\u{7}b",
            "inbox",
            "Downloads",
            "OTHER",
            "lost+found",
        ] {
            let err = check_folder_name(bad).unwrap_err();
            assert!(
                matches!(&err, CoreError::Rejected { field: Some(f), .. } if f == "folder.name"),
                "{bad:?}: {err:?}"
            );
        }
        assert!(check_folder_name(&"x".repeat(101)).is_err());
        assert!(check_folder_name(&"x".repeat(100)).is_ok());
    }

    #[test]
    fn claims_fit_their_library() {
        assert!(check_claims(&["movie".into(), "home_video".into()], Shape::OneOffs).is_ok());
        assert!(check_claims(&["episode".into()], Shape::OneOffs).is_err());
        assert!(check_claims(&["photo".into()], Shape::Photos).is_ok());
        assert!(check_claims(&["other".into()], Shape::Photos).is_err());
        assert_eq!(
            check_claims(
                &["home_video".into(), "movie".into(), "movie".into()],
                Shape::OneOffs
            )
            .unwrap(),
            vec![Category::Movie, Category::HomeVideo]
        );
    }

    #[test]
    fn icons_are_unique_and_never_a_fixed_place() {
        let mut seen = HashSet::new();
        for icon in LIBRARY_ICONS {
            assert!(seen.insert(icon), "{icon}");
        }
        for fixed in [
            "house",
            "file-text",
            "inbox",
            "arrow-down-to-line",
            "settings",
        ] {
            assert!(!LIBRARY_ICONS.contains(&fixed));
        }
        assert_eq!(LIBRARY_ICONS.len(), 32);
    }

    #[test]
    fn places_follow_paths() {
        let def = LibraryDef {
            row: LibraryRow {
                id: "m".into(),
                name: "Movies".into(),
                icon: "film".into(),
                content_type: ContentType::Video,
                structure: Some(Structure::OneOffs),
                folder: "Movies".into(),
                visibility: Visibility::Everyone,
                position: 0,
                created_by: None,
                created_at: 0,
                updated_at: 0,
            },
            claims: vec![Category::Movie],
            members: vec![],
        };
        let defs = [def];
        let p = |rel: &str| place_of(&defs, rel, ItemStatus::Ready);
        assert_eq!(p("Movies/a.mkv"), (Place::Library, Some("m".into())));
        assert_eq!(p("movies/a.mkv"), (Place::Folder, None));
        assert_eq!(p("Movies"), (Place::Folder, None));
        assert_eq!(p("Other/a.pdf"), (Place::OtherFiles, None));
        assert_eq!(p("Inbox/a.pdf"), (Place::Incoming, None));
        assert_eq!(p("Downloads/Completed/a/b.mkv"), (Place::Incoming, None));
        assert_eq!(
            place_of(&defs, "Movies/a.mkv", ItemStatus::Trashed),
            (Place::Trash, None)
        );
        assert!(is_under("Movies/a.mkv", "Movies"));
        assert!(!is_under("Moviesx/a.mkv", "Movies"));
    }
}
