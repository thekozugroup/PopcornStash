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
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Cache policy for thumbnails: their URL carries `v=<updated_at>`.
const THUMB_CACHE: &str = "private, max-age=31536000, immutable";
/// Cache policy for originals: revalidated with ETag / Last-Modified.
const FILE_CACHE: &str = "private, no-cache";
/// Cache policy for photo views (original or rendition).
const VIEW_CACHE: &str = "private, max-age=3600";
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

/// Build an [`ItemPatch`] from raw JSON so `null` (clear) and a missing key
/// (leave alone) stay distinct.
fn patch_from_json(body: serde_json::Map<String, serde_json::Value>) -> ApiResult<ItemPatch> {
    use serde_json::Value;

    fn int(
        body: &serde_json::Map<String, Value>,
        key: &str,
        range: std::ops::RangeInclusive<i64>,
        msg: &str,
    ) -> ApiResult<Option<Option<i64>>> {
        match body.get(key) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(v) => match v.as_i64() {
                Some(n) if range.contains(&n) => Ok(Some(Some(n))),
                _ => Err(ApiError::bad_request(msg)),
            },
        }
    }
    fn text(
        body: &serde_json::Map<String, Value>,
        key: &str,
        msg: &str,
    ) -> ApiResult<Option<Option<String>>> {
        match body.get(key) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(s)) if s.chars().count() <= 300 => {
                let s = s.trim();
                Ok(Some((!s.is_empty()).then(|| s.to_string())))
            }
            Some(_) => Err(ApiError::bad_request(msg)),
        }
    }

    let title = match text(&body, "title", "The title is too long.")? {
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
        year: int(&body, "year", 1800..=2200, "The year is not valid.")?,
        favorite,
        // The patch has no way to express "clear"; an empty name clears it.
        show_name: text(&body, "show_name", "The show name is too long.")?
            .map(Option::unwrap_or_default),
        season: int(&body, "season", 0..=9999, "The season number is not valid.")?,
        episode: int(
            &body,
            "episode",
            0..=9999,
            "The episode number is not valid.",
        )?,
        episode_title: text(&body, "episode_title", "The episode title is too long.")?,
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

async fn organize(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    JsonBody(request): JsonBody<OrganizeRequest>,
) -> ApiResult<Json<ItemDetail>> {
    user.require(Scope::Full)?;
    visible_item(&state, &id).await?;
    state.library.organize(&id, request).await?;
    Ok(Json(visible_detail(&state, &id, &user.user_id).await?))
}

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
) -> ApiResult<StatusCode> {
    user.require_owner()?;
    let item = visible_item(&state, &id).await?;
    state.library.trash_item(&id, Some(&user.user_id)).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "item.trashed",
            serde_json::json!({ "item_id": id, "title": item.title, "rel_path": item.rel_path }),
            None,
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
}

async fn thumb(
    State(state): State<AppState>,
    user: AuthUser,
    UrlPath(id): UrlPath<String>,
    uri: Uri,
) -> ApiResult<Response> {
    user.require(Scope::LibraryRead)?;
    let query: ThumbQuery = parse_query(&uri)?;
    let size = query.size.unwrap_or(ThumbSize::Sm);
    let item = visible_item(&state, &id).await?;
    let path = state.media.thumb_path(&item.id, size);
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::not_found("This picture is not ready yet."));
        }
        Err(e) => return Err(e.into()),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, THUMB_CACHE),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
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
    let disposition = content_disposition(if download { "attachment" } else { "inline" }, &name);
    let mut response = serve_path(req, &abs, FILE_CACHE).await?;
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
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
    let path = state.media.photo_view(&item).await?;
    if !tokio::fs::metadata(&path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return Err(ApiError::not_found(
            "This photo could not be prepared for viewing.",
        ));
    }
    let mut response = serve_path(req, &path, VIEW_CACHE).await?;
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

/// Serve a file with Range, If-Range, ETag / If-None-Match and Last-Modified support.
pub(crate) async fn serve_path(
    mut req: Request,
    path: &Path,
    cache_control: &'static str,
) -> ApiResult<Response> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(meta) if meta.is_file() => meta,
        _ => return Err(missing_file()),
    };
    let etag = etag_for(&meta);
    let safe_method = matches!(*req.method(), Method::GET | Method::HEAD);
    let headers = req.headers_mut();
    if let Some(etag) = &etag {
        if let Some(inm) = headers
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
            http_date(&meta).as_deref() == Some(if_range.as_str())
        };
        if !fresh {
            headers.remove(header::RANGE);
        }
        headers.remove(header::IF_RANGE);
    }
    let response = match ServeFile::new(path).oneshot(req).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let mut response = response.map(Body::new);
    if response.status() == StatusCode::NOT_FOUND {
        return Err(missing_file());
    }
    let headers = response.headers_mut();
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
    if let Some(start) = caps.start_s {
        if !start.is_finite() || start < 0.0 {
            return Err(ApiError::bad_request("The start position is not valid."));
        }
    }
    if let Some(h) = caps.max_height {
        if !(144..=8640).contains(&h) {
            return Err(ApiError::bad_request("The maximum height is not valid."));
        }
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
}
