//! /api routes: items (see docs/architecture.md section 8).
//!
//! Listing, detail, metadata edits, organize, delete, thumbnails, original
//! files (with Range support), photo views, playback decisions and progress.
//! Every file path comes from the database and is resolved under the stash
//! root with [`StashLayout::abs`](stash_core::paths::StashLayout::abs); a
//! request never names a path itself.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path as UrlPath, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use stash_core::CoreError;
use stash_core::events::{Event, LibraryChange};
use stash_core::library::OrganizeRequest;
use stash_core::library::repo::{self, ItemPatch, ItemQuery, Page};
use stash_core::media::ThumbSize;
use stash_core::media::playback::{self, ClientCaps, PlaybackMode};
use stash_core::media::transcode::StreamOptions;
use stash_core::models::{Item, ItemDetail, ItemRow, ItemStatus, MediaKind};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::security::ClientIp;
use crate::state::AppState;

/// Cache policy for versioned thumbnail URLs (`Item.thumb` carries `v=<updated_at>`).
const THUMB_CACHE: &str = "private, max-age=31536000, immutable";
/// Cache policy for thumbnail URLs without a version: revalidated with the ETag,
/// so a regenerated thumbnail shows up.
const THUMB_REVALIDATE: &str = "private, no-cache";
/// Cache policy for originals: revalidated with ETag / Last-Modified.
const FILE_CACHE: &str = "private, no-cache";
/// Cache policy for photo views (original or rendition).
const VIEW_CACHE: &str = "private, max-age=3600";
/// Library files are untrusted (anything a torrent brings in). When one is opened
/// directly in a tab it runs in a sandbox: no scripts, no forms, an opaque origin.
/// The policy does not affect `<video>` or `<img>` elements that load the file.
const FILE_CSP: &str = "default-src 'none'; img-src 'self' data: blob:; media-src 'self' blob:; style-src 'unsafe-inline'; sandbox";
/// Chrome's PDF viewer refuses to run in a sandboxed document; PDFs get the rest of the policy.
const PDF_CSP: &str = "default-src 'none'; img-src 'self' data: blob:; media-src 'self' blob:; style-src 'unsafe-inline'";
/// Longest accepted search text.
const MAX_SEARCH_CHARS: usize = 200;
/// Largest page the list endpoint returns.
const MAX_PAGE: u32 = 200;
const DEFAULT_PAGE: u32 = 50;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/items", get(list))
        .route("/api/items/{id}", get(detail).patch(update).delete(remove))
        .route("/api/items/{id}/organize", post(organize))
        .route("/api/items/{id}/thumb", get(thumb))
        .route("/api/items/{id}/file", get(file))
        .route("/api/items/{id}/view", get(view))
        .route("/api/items/{id}/playback", post(start_playback))
        .route("/api/items/{id}/progress", put(save_progress))
}

// ---------------------------------------------------------------------------
// Shared helpers (also used by the inbox, trash and library handlers)
// ---------------------------------------------------------------------------

/// JSON request body whose rejections use the API error shape.
pub(crate) struct JsonBody<T>(pub T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(json_rejection(rejection)),
        }
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    tracing::debug!(detail = %rejection.body_text(), "rejected JSON body");
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::new(
            ErrorCode::PayloadTooLarge,
            "The request is too large. Send less at once.",
        );
    }
    match rejection {
        JsonRejection::MissingJsonContentType(_) => {
            ApiError::bad_request("Send the request as JSON.")
        }
        JsonRejection::JsonSyntaxError(_) => {
            ApiError::bad_request("The request is not valid JSON.")
        }
        JsonRejection::JsonDataError(_) => {
            ApiError::bad_request("Some details in the request are not valid.")
        }
        _ => ApiError::bad_request("The request could not be read."),
    }
}

/// Parse a query string, dropping empty values (`?category=` means "any").
pub(crate) fn parse_query<T: DeserializeOwned>(uri: &Uri) -> ApiResult<T> {
    let raw = uri.query().unwrap_or_default();
    let mut filtered = url::form_urlencoded::Serializer::new(String::new());
    filtered.extend_pairs(
        url::form_urlencoded::parse(raw.as_bytes()).filter(|(_, v)| !v.trim().is_empty()),
    );
    let rebuilt: Uri = format!("/?{}", filtered.finish())
        .parse()
        .map_err(|_| ApiError::bad_request("The address has an invalid query."))?;
    match Query::<T>::try_from_uri(&rebuilt) {
        Ok(Query(value)) => Ok(value),
        Err(rejection) => {
            tracing::debug!(detail = %rejection.body_text(), "rejected query string");
            Err(ApiError::bad_request(
                "Some filters in the address are not valid.",
            ))
        }
    }
}

/// The standard "gone" answer for items.
pub(crate) fn item_not_found() -> ApiError {
    ApiError::not_found("That item no longer exists.")
}

/// Load an item that is visible in the library (anything but trashed).
pub(crate) async fn visible_item(state: &AppState, id: &str) -> ApiResult<ItemRow> {
    match repo::get(&state.db, id).await? {
        Some(row) if row.status != ItemStatus::Trashed => Ok(row),
        _ => Err(item_not_found()),
    }
}

/// Item detail for the caller, hiding trashed items.
pub(crate) async fn visible_detail(
    state: &AppState,
    id: &str,
    user_id: &str,
) -> ApiResult<ItemDetail> {
    let detail = repo::detail(&state.db, id, user_id).await?;
    if detail.item.status == ItemStatus::Trashed {
        return Err(item_not_found());
    }
    Ok(detail)
}

/// Absolute path of an item's file. Refuses stored paths that escape the stash
/// and files that are no longer on disk.
pub(crate) async fn item_file(state: &AppState, item: &ItemRow) -> ApiResult<PathBuf> {
    let abs = state.layout.abs(&item.rel_path).map_err(|_| {
        tracing::warn!(item = %item.id, "refusing stored path outside the stash");
        missing_file()
    })?;
    match tokio::fs::metadata(&abs).await {
        Ok(meta) if meta.is_file() => Ok(abs),
        _ => Err(missing_file()),
    }
}

fn missing_file() -> ApiError {
    ApiError::not_found("The file for this item is missing from the stash.")
}

// ---------------------------------------------------------------------------
// Listing and detail
// ---------------------------------------------------------------------------

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    uri: Uri,
) -> ApiResult<Json<Page<Item>>> {
    user.require(Scope::LibraryRead)?;
    let mut query: ItemQuery = parse_query(&uri)?;
    if query.status == Some(ItemStatus::Trashed) {
        return Err(ApiError::bad_request(
            "Deleted items are listed in the trash.",
        ));
    }
    query.q = query
        .q
        .map(|q| q.trim().chars().take(MAX_SEARCH_CHARS).collect::<String>())
        .filter(|q| !q.is_empty());
    query.limit = Some(query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE));
    Ok(Json(repo::list(&state.db, &query, &user.user_id).await?))
}

async fn detail(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<Json<ItemDetail>> {
    user.require(Scope::LibraryRead)?;
    Ok(Json(visible_detail(&state, &id, &user.user_id).await?))
}

/// Longest accepted text field in a metadata edit.
const MAX_TEXT_CHARS: usize = 500;

/// Build an [`ItemPatch`] from raw JSON so `null` (clear) and a missing key
/// (leave alone) stay distinct. Types are checked here; value ranges (year,
/// season and episode numbers) are checked by the library.
fn patch_from_json(body: serde_json::Map<String, serde_json::Value>) -> ApiResult<ItemPatch> {
    use serde_json::Value;

    fn int(
        body: &serde_json::Map<String, Value>,
        key: &str,
        label: &str,
    ) -> ApiResult<Option<Option<i64>>> {
        match body.get(key) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(v) => v.as_i64().map(|n| Some(Some(n))).ok_or_else(|| {
                ApiError::bad_request(format!("The {label} must be a whole number."))
            }),
        }
    }
    fn text(
        body: &serde_json::Map<String, Value>,
        key: &str,
        label: &str,
    ) -> ApiResult<Option<Option<String>>> {
        match body.get(key) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(s)) if s.chars().count() <= MAX_TEXT_CHARS => {
                let s = s.trim();
                Ok(Some((!s.is_empty()).then(|| s.to_string())))
            }
            Some(_) => Err(ApiError::bad_request(format!(
                "The {label} must be text of {MAX_TEXT_CHARS} characters or fewer."
            ))),
        }
    }

    let title = match text(&body, "title", "title")? {
        None => None,
        Some(Some(title)) => Some(title),
        Some(None) => return Err(ApiError::bad_request("The title cannot be empty.")),
    };
    let favorite = match body.get("favorite") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(ApiError::bad_request("Favorite must be true or false.")),
    };
    Ok(ItemPatch {
        title,
        year: int(&body, "year", "year")?,
        favorite,
        // The patch has no way to express "clear"; an empty name clears it.
        show_name: text(&body, "show_name", "show name")?.map(Option::unwrap_or_default),
        season: int(&body, "season", "season number")?,
        episode: int(&body, "episode", "episode number")?,
        episode_title: text(&body, "episode_title", "episode title")?,
    })
}

async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    JsonBody(body): JsonBody<serde_json::Map<String, serde_json::Value>>,
) -> ApiResult<Json<ItemDetail>> {
    user.require(Scope::Full)?;
    let patch = patch_from_json(body)?;
    visible_item(&state, &id).await?;
    repo::update_metadata(&state.db, &id, &patch).await?;
    state.events.publish(Event::Library {
        r#type: LibraryChange::Updated,
        item_id: id.clone(),
    });
    Ok(Json(visible_detail(&state, &id, &user.user_id).await?))
}

/// Highest season or episode number accepted, the same bound metadata edits use.
const MAX_NUMBER: i64 = 10_000;

/// Check an organize or accept request with the limits metadata edits use: the
/// title and show name end up in the database and in folder names. The year range
/// is checked by the library.
pub(crate) fn check_organize(request: &OrganizeRequest) -> ApiResult<()> {
    for (value, label) in [(&request.title, "title"), (&request.show_name, "show name")] {
        if value
            .as_deref()
            .is_some_and(|v| v.chars().count() > MAX_TEXT_CHARS)
        {
            return Err(ApiError::bad_request(format!(
                "The {label} must be {MAX_TEXT_CHARS} characters or fewer."
            )));
        }
    }
    for (value, label) in [(request.season, "season"), (request.episode, "episode")] {
        if value.is_some_and(|n| !(0..=MAX_NUMBER).contains(&n)) {
            return Err(ApiError::bad_request(format!(
                "The {label} number must be between 0 and {MAX_NUMBER}."
            )));
        }
    }
    Ok(())
}

async fn organize(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    JsonBody(request): JsonBody<OrganizeRequest>,
) -> ApiResult<Json<ItemDetail>> {
    user.require(Scope::Full)?;
    check_organize(&request)?;
    visible_item(&state, &id).await?;
    state.library.organize(&id, request).await?;
    Ok(Json(visible_detail(&state, &id, &user.user_id).await?))
}

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<StatusCode> {
    user.require_owner()?;
    let item = visible_item(&state, &id).await?;
    state.library.trash_item(&id, Some(&user.user_id)).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "item_trashed",
            serde_json::json!({ "item_id": id, "title": item.title, "rel_path": item.rel_path }),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Thumbnails, originals and photo views
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThumbQuery {
    size: Option<ThumbSize>,
    /// Cache buster from `Item.thumb` (`updated_at`). Only versioned URLs are immutable.
    v: Option<String>,
}

async fn thumb(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    req: Request,
) -> ApiResult<Response> {
    user.require(Scope::LibraryRead)?;
    let query: ThumbQuery = parse_query(req.uri())?;
    let size = query.size.unwrap_or(ThumbSize::Sm);
    // Owners see trashed items in Settings, Library, Trash, so their pictures stay
    // available to owners until the trash is emptied. Everyone else gets a 404.
    let item = match repo::get(&state.db, &id).await? {
        Some(row) if row.status != ItemStatus::Trashed || user.is_owner() => row,
        _ => return Err(item_not_found()),
    };
    let path = state.media.thumb_path(&item.id, size);
    let not_ready = || ApiError::not_found("This picture is not ready yet.");
    if !tokio::fs::metadata(&path)
        .await
        .is_ok_and(|meta| meta.is_file())
    {
        return Err(not_ready());
    }
    let cache = if query.v.is_some() {
        THUMB_CACHE
    } else {
        THUMB_REVALIDATE
    };
    let thumbs = state.config.thumbs_dir();
    let mut response = serve_path(req, &path, &thumbs, cache).await.map_err(|e| {
        if e.code == ErrorCode::NotFound {
            not_ready()
        } else {
            e
        }
    })?;
    // Thumbnails are always WebP, whatever the cache file is called.
    if response.status().is_success() {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/webp"));
    }
    Ok(response)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileQuery {
    download: Option<String>,
}

async fn file(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    req: Request,
) -> ApiResult<Response> {
    user.require(Scope::LibraryRead)?;
    let query: FileQuery = parse_query(req.uri())?;
    let download = matches!(
        query
            .download
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes")
    );
    let item = visible_item(&state, &id).await?;
    let abs = item_file(&state, &item).await?;
    let name = item
        .rel_path
        .rsplit('/')
        .next()
        .unwrap_or(&item.rel_path)
        .to_string();
    let mut response = serve_path(req, &abs, state.layout.root(), FILE_CACHE).await?;
    // Markup and scripts are always downloaded, never rendered in our origin.
    let active = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(is_active_content);
    if active {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
    }
    let kind = if download || active {
        "attachment"
    } else {
        "inline"
    };
    if let Ok(value) = HeaderValue::from_str(&content_disposition(kind, &name)) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// Types a browser would execute or render as a document with scripts.
fn is_active_content(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    essence.ends_with("+xml")
        || essence.contains("javascript")
        || essence.contains("ecmascript")
        || matches!(
            essence.as_str(),
            "text/html" | "text/xml" | "application/xml" | "text/xsl" | "application/xslt+xml"
        )
}

async fn view(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    req: Request,
) -> ApiResult<Response> {
    user.require(Scope::LibraryRead)?;
    let item = visible_item(&state, &id).await?;
    if item.kind != MediaKind::Photo {
        return Err(ApiError::bad_request("Only photos can be viewed this way."));
    }
    // Make sure the original is still there before asking for a rendition.
    item_file(&state, &item).await?;
    let path = state.media.photo_view(&item).await.map_err(|e| match e {
        // vips and ffmpeg both failed (damaged file, or a format they cannot read).
        CoreError::External(detail) => {
            tracing::warn!(item = %item.id, error = %detail, "could not prepare a photo for viewing");
            ApiError::conflict(
                "This photo could not be shown in the browser. Download the original instead.",
            )
        }
        other => other.into(),
    })?;
    if !tokio::fs::metadata(&path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return Err(ApiError::not_found(
            "This photo could not be prepared for viewing.",
        ));
    }
    // The original lives in the stash; a rendition in the config cache.
    let renditions = state.config.renditions_dir();
    let root = if path.starts_with(&renditions) {
        renditions.as_path()
    } else {
        state.layout.root()
    };
    let mut response = serve_path(req, &path, root, VIEW_CACHE).await?;
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    Ok(response)
}

/// RFC 5987 `attr-char`: everything else is percent-encoded in `filename*`.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

/// `inline|attachment; filename="ascii fallback"; filename*=UTF-8''percent-encoded` (RFC 6266).
pub(crate) fn content_disposition(kind: &str, filename: &str) -> String {
    let fallback: String = filename
        .chars()
        .map(|c| match c {
            '"' | '\\' | '%' => '_',
            c if c == ' ' || c.is_ascii_graphic() => c,
            _ => '_',
        })
        .collect();
    let encoded = utf8_percent_encode(filename, ATTR_CHAR);
    format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

/// Strong validator from size and modification time.
fn etag_for(meta: &std::fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!(
        "\"{:x}-{:x}{:08x}\"",
        meta.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    ))
}

/// HTTP-date of the modification time, formatted like tower-http's `Last-Modified`.
fn http_date(meta: &std::fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    let at = time::OffsetDateTime::from(modified).to_offset(time::UtcOffset::UTC);
    at.format(time::macros::format_description!(
        "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
    ))
    .ok()
}

fn etag_matches(list: &str, etag: &str) -> bool {
    let bare = etag.trim_start_matches("W/");
    list.split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.trim_start_matches("W/") == bare)
}

/// A file opened for serving whose real location was checked.
struct PinnedFile {
    /// Held open until ServeFile has opened `serve`, so the descriptor number stays ours.
    _file: std::fs::File,
    meta: std::fs::Metadata,
    /// What ServeFile opens: `/proc/self/fd/<n>` (the very file checked) on Linux,
    /// else the resolved path.
    serve: PathBuf,
}

/// Open `path` and make sure the file actually opened, after every symlink, lies
/// under `root`. Checking the opened descriptor rather than the name means a file
/// swapped for a symlink after an earlier check (for example over an SMB share)
/// cannot escape. Directories, FIFOs and devices are refused without blocking.
fn pin_file(path: &Path, root: &Path) -> std::io::Result<PinnedFile> {
    use std::io::{Error, ErrorKind};

    let outside = || Error::new(ErrorKind::PermissionDenied, "outside its root");
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Opening a FIFO for reading would otherwise wait for a writer.
        options.custom_flags(nix::libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(Error::new(ErrorKind::NotFound, "not a regular file"));
    }
    let real_root = root.canonicalize()?;
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let fd_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        if let Ok(real) = std::fs::read_link(&fd_path) {
            if !real.starts_with(&real_root) {
                return Err(outside());
            }
            return Ok(PinnedFile {
                _file: file,
                meta,
                serve: fd_path,
            });
        }
    }
    // Without /proc, check the resolved name instead.
    let real = path.canonicalize()?;
    if !real.starts_with(&real_root) {
        return Err(outside());
    }
    Ok(PinnedFile {
        _file: file,
        meta,
        serve: real,
    })
}

/// Serve the file at `path`, which must lie under `root` once opened, with Range,
/// If-Range, ETag / If-None-Match and Last-Modified support. The content type comes
/// from the name in `path`.
pub(crate) async fn serve_path(
    mut req: Request,
    path: &Path,
    root: &Path,
    cache_control: &'static str,
) -> ApiResult<Response> {
    let (owned_path, owned_root) = (path.to_path_buf(), root.to_path_buf());
    let pinned = match tokio::task::spawn_blocking(move || pin_file(&owned_path, &owned_root)).await
    {
        Ok(Ok(pinned)) => pinned,
        Ok(Err(e)) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                tracing::warn!(error = %e, "refusing to serve a file that resolves outside its folder");
            }
            return Err(missing_file());
        }
        Err(e) => {
            tracing::error!(error = %e, "file open task failed");
            return Err(ApiError::internal());
        }
    };
    let meta = &pinned.meta;
    let etag = etag_for(meta);
    let safe_method = matches!(*req.method(), Method::GET | Method::HEAD);
    let headers = req.headers_mut();
    if let Some(etag) = &etag
        && let Some(inm) = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
    {
        if etag_matches(inm, etag) && safe_method {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            if let Ok(value) = HeaderValue::from_str(etag) {
                response.headers_mut().insert(header::ETAG, value);
            }
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            );
            return Ok(response);
        }
        // If-None-Match takes precedence over If-Modified-Since (RFC 9110 13.2.2).
        headers.remove(header::IF_MODIFIED_SINCE);
    }
    let headers = req.headers_mut();
    if let Some(if_range) = headers
        .get(header::IF_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .map(str::to_string)
    {
        let fresh = if if_range.starts_with('"') {
            // Strong comparison only.
            etag.as_deref() == Some(if_range.as_str())
        } else {
            http_date(meta).as_deref() == Some(if_range.as_str())
        };
        if !fresh {
            headers.remove(header::RANGE);
        }
        headers.remove(header::IF_RANGE);
    }
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let response = match ServeFile::new_with_mime(&pinned.serve, &mime)
        .oneshot(req)
        .await
    {
        Ok(response) => response,
        Err(never) => match never {},
    };
    // ServeFile has its own handle (or failed) by now.
    drop(pinned);
    let mut response = response.map(Body::new);
    if response.status() == StatusCode::NOT_FOUND {
        return Err(missing_file());
    }
    let pdf = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/pdf"));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(if pdf { PDF_CSP } else { FILE_CSP }),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Some(value) = etag.as_deref().and_then(|e| HeaderValue::from_str(e).ok()) {
        headers.insert(header::ETAG, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// Playback and progress
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PlaybackResponse {
    Direct { url: String },
    Hls { url: String, session_id: String },
}

fn validate_caps(caps: &ClientCaps) -> ApiResult<()> {
    if caps
        .start_s
        .is_some_and(|start| !start.is_finite() || start < 0.0)
    {
        return Err(ApiError::bad_request("The start position is not valid."));
    }
    if caps.max_height.is_some_and(|h| !(144..=8640).contains(&h)) {
        return Err(ApiError::bad_request("The maximum height is not valid."));
    }
    if caps.video_codecs.len() > 32 || caps.audio_codecs.len() > 32 {
        return Err(ApiError::bad_request("Too many codecs were listed."));
    }
    Ok(())
}

async fn start_playback(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    JsonBody(mut caps): JsonBody<ClientCaps>,
) -> ApiResult<Json<PlaybackResponse>> {
    user.require(Scope::LibraryRead)?;
    validate_caps(&caps)?;
    for codec in caps
        .video_codecs
        .iter_mut()
        .chain(caps.audio_codecs.iter_mut())
    {
        *codec = codec.trim().to_ascii_lowercase();
    }
    let item = visible_item(&state, &id).await?;
    if item.kind != MediaKind::Video {
        return Err(ApiError::bad_request("Only videos can be played this way."));
    }
    let abs = item_file(&state, &item).await?;
    let media = match item.media() {
        Some(media) => media,
        None => state.media.probe_file(&abs).await.map_err(|e| {
            tracing::warn!(item = %item.id, error = %e, "probe before playback failed");
            ApiError::conflict(
                "This video could not be read. It may be damaged or still being copied.",
            )
        })?,
    };
    let ext = abs
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let response = match playback::decide(&media, &ext, &caps) {
        PlaybackMode::Direct => PlaybackResponse::Direct {
            url: format!("/api/items/{}/file", item.id),
        },
        PlaybackMode::Hls { reason } => {
            tracing::debug!(item = %item.id, %reason, "transcoding for playback");
            let options = StreamOptions {
                max_height: caps.max_height,
                start_s: caps.start_s,
                audio_index: caps.audio_index,
                hdr: caps.hdr,
            };
            let session = state
                .transcoder
                .start(&item.id, abs, &media, options)
                .await?;
            PlaybackResponse::Hls {
                url: session.url,
                session_id: session.session_id,
            }
        }
    };
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct ProgressBody {
    position_s: f64,
    duration_s: f64,
}

async fn save_progress(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    JsonBody(body): JsonBody<ProgressBody>,
) -> ApiResult<StatusCode> {
    user.require(Scope::LibraryRead)?;
    let valid = |v: f64| v.is_finite() && v >= 0.0;
    if !valid(body.position_s) || !valid(body.duration_s) {
        return Err(ApiError::bad_request("The playback position is not valid."));
    }
    visible_item(&state, &id).await?;
    let position = if body.duration_s > 0.0 {
        body.position_s.min(body.duration_s)
    } else {
        body.position_s
    };
    repo::set_progress(&state.db, &user.user_id, &id, position, body.duration_s).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_encodes_unicode_and_quotes() {
        let v = content_disposition("attachment", "Beach \"day\" été.mp4");
        assert_eq!(
            v,
            "attachment; filename=\"Beach _day_ _t_.mp4\"; filename*=UTF-8''Beach%20%22day%22%20%C3%A9t%C3%A9.mp4"
        );
        assert!(HeaderValue::from_str(&v).is_ok());
        let v = content_disposition("inline", "a\nb.mkv");
        assert!(HeaderValue::from_str(&v).is_ok());
    }

    #[test]
    fn active_content_is_detected() {
        for t in [
            "text/html",
            "text/html; charset=utf-8",
            "image/svg+xml",
            "application/xhtml+xml",
            "text/javascript",
            "application/javascript",
            "application/xml",
        ] {
            assert!(is_active_content(t), "{t}");
        }
        for t in [
            "video/mp4",
            "image/jpeg",
            "application/pdf",
            "text/plain",
            "application/octet-stream",
        ] {
            assert!(!is_active_content(t), "{t}");
        }
    }

    #[test]
    fn etag_list_matching() {
        assert!(etag_matches("\"a\", \"b\"", "\"b\""));
        assert!(etag_matches("W/\"b\"", "\"b\""));
        assert!(etag_matches("*", "\"b\""));
        assert!(!etag_matches("\"c\"", "\"b\""));
    }

    #[test]
    fn patch_keeps_null_and_missing_apart() {
        let body = serde_json::json!({"year": null, "favorite": true, "title": "  Heat "});
        let patch = patch_from_json(body.as_object().unwrap().clone()).unwrap();
        assert_eq!(patch.year, Some(None));
        assert_eq!(patch.season, None);
        assert_eq!(patch.favorite, Some(true));
        assert_eq!(patch.title.as_deref(), Some("Heat"));
        let bad = serde_json::json!({"title": ""});
        assert!(patch_from_json(bad.as_object().unwrap().clone()).is_err());
        let bad = serde_json::json!({"year": "soon"});
        assert!(patch_from_json(bad.as_object().unwrap().clone()).is_err());
    }

    #[test]
    fn query_drops_empty_values_and_rejects_bad_ones() {
        let q: ItemQuery =
            parse_query(&"/api/items?category=&sort=title&limit=5".parse().unwrap()).unwrap();
        assert_eq!(q.category, None);
        assert_eq!(q.sort, repo::ItemSort::Title);
        assert_eq!(q.limit, Some(5));
        assert!(parse_query::<ItemQuery>(&"/api/items?sort=sideways".parse().unwrap()).is_err());
        assert!(parse_query::<ItemQuery>(&"/api/items?limit=-1".parse().unwrap()).is_err());
        let q: ItemQuery = parse_query(
            &"/api/items?q=hello%20world&category=home_video"
                .parse()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(q.q.as_deref(), Some("hello world"));
    }

    #[test]
    fn playback_response_shape() {
        let direct = serde_json::to_value(PlaybackResponse::Direct {
            url: "/api/items/x/file".into(),
        })
        .unwrap();
        assert_eq!(
            direct,
            serde_json::json!({"mode": "direct", "url": "/api/items/x/file"})
        );
        let hls = serde_json::to_value(PlaybackResponse::Hls {
            url: "/u".into(),
            session_id: "s".into(),
        })
        .unwrap();
        assert_eq!(
            hls,
            serde_json::json!({"mode": "hls", "url": "/u", "session_id": "s"})
        );
    }

    // ---- integration tests through the full router ----

    use axum::http::StatusCode;
    use stash_core::models::{Category, ItemStatus, MediaKind};

    use crate::api::library::testing::{TestApp, request};

    fn with_header(mut req: Request, name: header::HeaderName, value: &str) -> Request {
        req.headers_mut()
            .insert(name, HeaderValue::from_str(value).unwrap());
        req
    }

    #[tokio::test]
    async fn downloads_only_tokens_cannot_browse() {
        let app = TestApp::new().await;
        let (user_id, _) = app.user("member").await;
        let token = app
            .device_token(&user_id, &["downloads:create", "downloads:read"])
            .await;
        let (status, json) = app.json("/api/items", &token).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(json["error"]["code"], "forbidden");
        let (status, _) = app.json("/api/library/summary", &token).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = app.get("/api/items", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn lists_inserted_items_and_rejects_bad_filters() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let a = app
            .item(
                "Movies/Heat (1995)/Heat (1995).mkv",
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                None,
            )
            .await;
        let b = app
            .item(
                "Photos/2024/06/IMG_0001.jpg",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Ready,
                None,
            )
            .await;
        let gone = app
            .item(
                "Other/old.bin",
                MediaKind::Other,
                Category::Other,
                ItemStatus::Trashed,
                None,
            )
            .await;
        let (status, json) = app.json("/api/items?limit=10", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let ids: Vec<&str> = json["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap())
            .collect();
        assert!(
            ids.contains(&a.as_str()) && ids.contains(&b.as_str()),
            "{json}"
        );
        assert!(!ids.contains(&gone.as_str()));
        assert!(json.get("next_cursor").is_some());

        let (status, json) = app
            .json("/api/items?category=photo&category=", &token)
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let (status, json) = app.json("/api/items?category=photo", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["items"].as_array().unwrap().len(), 1);
        assert_eq!(json["items"][0]["id"], b.as_str());

        for bad in [
            "/api/items?sort=sideways",
            "/api/items?category=films",
            "/api/items?favorite=maybe",
            "/api/items?status=trashed",
        ] {
            let (status, json) = app.json(bad, &token).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {json}");
            assert_eq!(json["error"]["code"], "bad_request");
        }

        let (status, json) = app.json(&format!("/api/items/{a}"), &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["id"], a.as_str());
        let (status, _) = app.json(&format!("/api/items/{gone}"), &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = app.json("/api/items/does-not-exist", &token).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn thumbnails_404_until_written() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Photos/2024/06/a.jpg",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Ready,
                None,
            )
            .await;
        let (status, headers, body) = app
            .get(&format!("/api/items/{id}/thumb?size=sm"), Some(&token))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "not_found");

        let path = app.state.media.thumb_path(&id, ThumbSize::Lg);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"RIFFxxxxWEBP").unwrap();
        let (status, headers, body) = app
            .get(&format!("/api/items/{id}/thumb?size=lg&v=1"), Some(&token))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/webp");
        assert_eq!(headers[header::CACHE_CONTROL], THUMB_CACHE);
        assert_eq!(&body[..], b"RIFFxxxxWEBP");
        let (status, _, _) = app
            .get(&format!("/api/items/{id}/thumb?size=huge"), Some(&token))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// The trash list (owner only) shows each item's picture; members still get 404.
    #[tokio::test]
    async fn trashed_thumbnails_are_for_owners_only() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (_, member) = app.user("member").await;
        let id = app
            .item(
                "Photos/2024/06/t.jpg",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Trashed,
                None,
            )
            .await;
        let path = app.state.media.thumb_path(&id, ThumbSize::Sm);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"RIFFxxxxWEBP").unwrap();
        let url = format!("/api/items/{id}/thumb?size=sm&v=1");
        let (status, _, body) = app.get(&url, Some(&owner)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&body[..], b"RIFFxxxxWEBP");
        let (status, _, _) = app.get(&url, Some(&member)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    /// Only a versioned thumbnail URL (`v=<updated_at>`) may be cached forever; the
    /// web asks for `?size=lg` without a version, which must be revalidated.
    #[tokio::test]
    async fn unversioned_thumbnails_are_revalidated() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Photos/2024/06/b.jpg",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Ready,
                None,
            )
            .await;
        let path = app.state.media.thumb_path(&id, ThumbSize::Lg);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"RIFFoldxWEBP").unwrap();

        let uri = format!("/api/items/{id}/thumb?size=lg");
        let (status, headers, body) = app.get(&uri, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&body[..], b"RIFFoldxWEBP");
        let cache = headers[header::CACHE_CONTROL].to_str().unwrap();
        assert!(!cache.contains("immutable"), "{cache}");
        assert!(cache.contains("no-cache"), "{cache}");
        let etag = headers[header::ETAG].to_str().unwrap().to_string();
        let req = with_header(
            request("GET", &uri, Some(&token), None),
            header::IF_NONE_MATCH,
            &etag,
        );
        let (status, _, _) = app.send(req).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);

        // A regenerated thumbnail is picked up by the unversioned URL.
        std::fs::write(&path, b"RIFFnew!WEBP!").unwrap();
        let (status, _, body) = app.get(&uri, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&body[..], b"RIFFnew!WEBP!");

        let (status, headers, _) = app.get(&format!("{uri}&v=17"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CACHE_CONTROL], THUMB_CACHE);
        assert_eq!(headers[header::CONTENT_TYPE], "image/webp");
    }

    #[tokio::test]
    async fn organize_limits_text_and_numbers_like_edits() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Other/clip.mkv",
                MediaKind::Video,
                Category::Other,
                ItemStatus::Ready,
                Some(b"clip"),
            )
            .await;
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        for body in [
            serde_json::json!({"category": "movie", "title": long}),
            serde_json::json!({"category": "episode", "show_name": long, "season": 1, "episode": 1}),
            serde_json::json!({"category": "episode", "show_name": "Bluey", "season": 20_000, "episode": 1}),
            serde_json::json!({"category": "episode", "show_name": "Bluey", "season": 1, "episode": 1_000_000}),
        ] {
            let (status, _, bytes) = app
                .send(request(
                    "POST",
                    &format!("/api/items/{id}/organize"),
                    Some(&token),
                    Some(body.clone()),
                ))
                .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{body}: {}",
                String::from_utf8_lossy(&bytes)
            );
        }
        // Nothing was moved or renamed.
        let (_, json) = app.json(&format!("/api/items/{id}"), &token).await;
        assert_eq!(json["rel_path"], "Other/clip.mkv", "{json}");
        assert_eq!(json["title"], "clip.mkv", "{json}");
    }

    #[tokio::test]
    async fn oversized_json_is_payload_too_large() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Other/a.bin",
                MediaKind::Other,
                Category::Other,
                ItemStatus::Ready,
                Some(b"x"),
            )
            .await;
        let body = serde_json::json!({"title": "y".repeat(3 * 1024 * 1024)});
        let (status, _, bytes) = app
            .send(request(
                "PATCH",
                &format!("/api/items/{id}"),
                Some(&token),
                Some(body),
            ))
            .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "payload_too_large");
    }

    #[tokio::test]
    async fn unconvertible_photos_explain_themselves() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Photos/2024/06/broken.heic",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Ready,
                Some(b"not really a photo"),
            )
            .await;
        let (status, _, bytes) = app
            .get(&format!("/api/items/{id}/view"), Some(&token))
            .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["code"], "conflict");
        assert!(
            !json["error"]["message"]
                .as_str()
                .unwrap()
                .contains("container logs"),
            "{json}"
        );

        // A browser-friendly original is served as is.
        let jpg = app
            .item(
                "Photos/2024/06/fine.jpg",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Ready,
                Some(b"\xff\xd8\xff\xe0jpeg"),
            )
            .await;
        let (status, headers, body) = app
            .get(&format!("/api/items/{jpg}/view"), Some(&token))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(headers[header::CONTENT_DISPOSITION], "inline");
        assert_eq!(&body[..], b"\xff\xd8\xff\xe0jpeg");
        let video = app
            .item(
                "Movies/a.mp4",
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                Some(b"mp4"),
            )
            .await;
        let (status, _, _) = app
            .get(&format!("/api/items/{video}/view"), Some(&token))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn files_support_ranges_and_downloads() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Home Videos/2024/06/Beach été.mp4",
                MediaKind::Video,
                Category::HomeVideo,
                ItemStatus::Ready,
                Some(b"0123456789"),
            )
            .await;
        let uri = format!("/api/items/{id}/file");

        let (status, headers, body) = app.get(&uri, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&body[..], b"0123456789");
        assert_eq!(headers[header::ACCEPT_RANGES], "bytes");
        assert_eq!(headers[header::CACHE_CONTROL], FILE_CACHE);
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert!(
            headers[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .starts_with("inline; ")
        );
        let etag = headers[header::ETAG].to_str().unwrap().to_string();
        let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(csp.ends_with("sandbox"), "{csp}");

        let req = with_header(
            request("GET", &uri, Some(&token), None),
            header::RANGE,
            "bytes=2-5",
        );
        let (status, headers, body) = app.send(req).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(&body[..], b"2345");
        assert_eq!(headers[header::CONTENT_RANGE], "bytes 2-5/10");

        // A stale If-Range validator gets the whole file instead of a mismatched range.
        let req = with_header(
            request("GET", &uri, Some(&token), None),
            header::RANGE,
            "bytes=2-5",
        );
        let req = with_header(req, header::IF_RANGE, "\"stale\"");
        let (status, _, body) = app.send(req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.len(), 10);
        let req = with_header(
            request("GET", &uri, Some(&token), None),
            header::RANGE,
            "bytes=8-",
        );
        let req = with_header(req, header::IF_RANGE, &etag);
        let (status, _, body) = app.send(req).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(&body[..], b"89");

        let req = with_header(
            request("GET", &uri, Some(&token), None),
            header::IF_NONE_MATCH,
            &etag,
        );
        let (status, _, body) = app.send(req).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert!(body.is_empty());

        let (status, headers, _) = app.get(&format!("{uri}?download=1"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let disposition = headers[header::CONTENT_DISPOSITION].to_str().unwrap();
        assert!(disposition.starts_with("attachment; "), "{disposition}");
        assert!(
            disposition.contains("filename*=UTF-8''Beach%20%C3%A9t%C3%A9.mp4"),
            "{disposition}"
        );

        let (status, headers, body) = app.send(request("HEAD", &uri, Some(&token), None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_LENGTH], "10");
        assert!(body.is_empty());

        std::fs::remove_file(
            app.state
                .config
                .stash_root
                .join("Home Videos/2024/06/Beach été.mp4"),
        )
        .unwrap();
        let (status, _, body) = app.get(&uri, Some(&token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn markup_from_the_library_is_never_rendered() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        for (rel, body) in [
            ("Other/evil.html", "<script>alert(1)</script>"),
            ("Other/evil.svg", "<svg onload=alert(1)/>"),
        ] {
            let id = app
                .item(
                    rel,
                    MediaKind::Other,
                    Category::Other,
                    ItemStatus::Ready,
                    Some(body.as_bytes()),
                )
                .await;
            let (status, headers, _) = app
                .get(&format!("/api/items/{id}/file"), Some(&token))
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                headers[header::CONTENT_TYPE],
                "application/octet-stream",
                "{rel}"
            );
            assert!(
                headers[header::CONTENT_DISPOSITION]
                    .to_str()
                    .unwrap()
                    .starts_with("attachment; ")
            );
            assert!(
                headers[header::CONTENT_SECURITY_POLICY]
                    .to_str()
                    .unwrap()
                    .contains("sandbox")
            );
        }
    }

    #[tokio::test]
    async fn stored_paths_cannot_escape_the_stash() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        // A file right next to the stash folder that must never be served.
        let outside = app.state.config.stash_root.parent().unwrap().join("x");
        std::fs::write(&outside, b"secret").unwrap();
        for rel in ["../x", "Movies/../../x", "/etc/passwd"] {
            let id = app
                .item(
                    rel,
                    MediaKind::Video,
                    Category::Movie,
                    ItemStatus::Ready,
                    None,
                )
                .await;
            for suffix in ["file", "view"] {
                let (status, _, body) = app
                    .get(&format!("/api/items/{id}/{suffix}"), Some(&token))
                    .await;
                assert!(
                    status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST,
                    "{rel}: {status}"
                );
                assert!(!body.windows(6).any(|w| w == b"secret"), "{rel}");
            }
        }
        #[cfg(unix)]
        {
            let link = app.state.config.stash_root.join("Movies/link.mkv");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let id = app
                .item(
                    "Movies/link.mkv",
                    MediaKind::Video,
                    Category::Movie,
                    ItemStatus::Ready,
                    None,
                )
                .await;
            let (status, _, body) = app
                .get(&format!("/api/items/{id}/file"), Some(&token))
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert!(!body.windows(6).any(|w| w == b"secret"));
        }
    }

    /// `serve_path` re-checks the file it actually opens, so a file swapped for a
    /// symlink after `item_file` checked the path (a race over an SMB share) is refused.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn serving_pins_the_file_inside_its_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("stash");
        std::fs::create_dir_all(root.join("Movies")).unwrap();
        let secret = dir.path().join("secret.db");
        std::fs::write(&secret, b"secret").unwrap();
        std::fs::write(root.join("Movies/real.mkv"), b"inside").unwrap();
        let escape = root.join("Movies/escape.mkv");
        std::os::unix::fs::symlink(&secret, &escape).unwrap();
        let alias = root.join("Movies/alias.mkv");
        std::os::unix::fs::symlink(root.join("Movies/real.mkv"), &alias).unwrap();
        let fifo = root.join("Movies/pipe.mkv");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRWXU).unwrap();
        let get = || request("GET", "/", None, None);

        let err = serve_path(get(), &escape, &root, FILE_CACHE)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        // A FIFO is refused at once instead of blocking a worker thread.
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            serve_path(get(), &fifo, &root, FILE_CACHE),
        )
        .await
        .expect("does not block")
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        for (path, expected) in [(root.join("Movies/real.mkv"), "inside"), (alias, "inside")] {
            let response = serve_path(get(), &path, &root, FILE_CACHE).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "video/x-matroska",
                "the type still comes from the real name"
            );
            let body = http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes();
            assert_eq!(&body[..], expected.as_bytes());
        }
        let req = with_header(get(), header::RANGE, "bytes=1-3");
        let response = serve_path(req, &root.join("Movies/real.mkv"), &root, FILE_CACHE)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 1-3/6");
    }

    #[tokio::test]
    async fn playback_refuses_non_video_items() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Photos/2024/06/a.jpg",
                MediaKind::Photo,
                Category::Photo,
                ItemStatus::Ready,
                Some(b"jpg"),
            )
            .await;
        let body =
            serde_json::json!({"video_codecs": ["h264"], "audio_codecs": ["aac"], "hdr": false});
        let (status, _, _) = app
            .send(request(
                "POST",
                &format!("/api/items/{id}/playback"),
                Some(&token),
                Some(body),
            ))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, _) = app
            .send(request(
                "PUT",
                &format!("/api/items/{id}/progress"),
                Some(&token),
                Some(serde_json::json!({"position_s": -1, "duration_s": 10})),
            ))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn members_cannot_delete_items() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Other/a.bin",
                MediaKind::Other,
                Category::Other,
                ItemStatus::Ready,
                Some(b"x"),
            )
            .await;
        let (status, _, body) = app
            .send(request(
                "DELETE",
                &format!("/api/items/{id}"),
                Some(&token),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "forbidden");
    }

    #[tokio::test]
    async fn edits_metadata_organizes_and_saves_progress() {
        let app = TestApp::new().await;
        let (user_id, token) = app.user("member").await;
        let id = app
            .item(
                "Other/heat.mkv",
                MediaKind::Video,
                Category::Other,
                ItemStatus::Ready,
                Some(b"heat"),
            )
            .await;
        let mut events = app.state.events.subscribe();

        let patch = serde_json::json!({"title": "Heat", "year": 1995, "favorite": true});
        let (status, _, body) = app
            .send(request(
                "PATCH",
                &format!("/api/items/{id}"),
                Some(&token),
                Some(patch),
            ))
            .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            (
                json["title"].as_str(),
                json["year"].as_i64(),
                json["favorite"].as_bool()
            ),
            (Some("Heat"), Some(1995), Some(true))
        );
        assert_eq!(
            json["rel_path"], "Other/heat.mkv",
            "metadata edits never move files"
        );
        let event = events.try_recv().unwrap();
        assert_eq!(event.name(), "library");

        let (status, _, body) = app
            .send(request(
                "PATCH",
                &format!("/api/items/{id}"),
                Some(&token),
                Some(serde_json::json!({"year": null})),
            ))
            .await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["year"].is_null());
        let (status, _, _) = app
            .send(request(
                "PATCH",
                &format!("/api/items/{id}"),
                Some(&token),
                Some(serde_json::json!({"year": 1066})),
            ))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let reader = app.device_token(&user_id, &["library:read"]).await;
        let (status, _, _) = app
            .send(request(
                "PATCH",
                &format!("/api/items/{id}"),
                Some(&reader),
                Some(serde_json::json!({"favorite": false})),
            ))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let organize = serde_json::json!({"category": "movie", "title": "Heat", "year": 1995});
        let (status, _, body) = app
            .send(request(
                "POST",
                &format!("/api/items/{id}/organize"),
                Some(&token),
                Some(organize),
            ))
            .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["rel_path"], "Movies/Heat (1995)/Heat (1995).mkv");
        assert_eq!(json["category"], "movie");

        let progress = serde_json::json!({"position_s": 30.0, "duration_s": 100.0});
        let (status, _, _) = app
            .send(request(
                "PUT",
                &format!("/api/items/{id}/progress"),
                Some(&reader),
                Some(progress),
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, json) = app.json(&format!("/api/items/{id}"), &token).await;
        assert_eq!(json["progress"]["position_s"], 30.0, "{json}");
    }
}
