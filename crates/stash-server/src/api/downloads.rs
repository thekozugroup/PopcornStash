//! /api routes: downloads (see docs/architecture.md section 8).

use axum::extract::multipart::MultipartRejection;
use axum::extract::rejection::JsonRejection;
use axum::extract::{
    DefaultBodyLimit, FromRequest, FromRequestParts, Multipart, Path, Query, Request, State,
};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use stash_core::models::Download;

use crate::auth::{AuthUser, Scope};
use crate::downloads::manager::MAX_TORRENT_FILE;
use crate::downloads::{AddDownloadRequest, DownloadFilter};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::state::AppState;

/// JSON body whose rejections use the API error shape. The rejection text is never
/// logged: it can quote request values such as forwarded cookies.
pub(crate) struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(json_rejection(&rejection)),
        }
    }
}

fn json_rejection(rejection: &JsonRejection) -> ApiError {
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
            ApiError::bad_request("Some details in the request are missing or not valid.")
        }
        _ => ApiError::bad_request("The request could not be read."),
    }
}

/// Query string whose rejections use the API error shape.
pub(crate) struct ApiQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(Self(value)),
            Err(_) => Err(ApiError::bad_request(
                "Some options in the address are not valid.",
            )),
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/downloads", get(list).post(add))
        .route("/api/downloads/destinations", get(destinations))
        .route(
            "/api/downloads/torrent",
            // Room for a 10 MB .torrent plus multipart framing.
            post(add_torrent).layer(DefaultBodyLimit::max(MAX_TORRENT_FILE + 1024 * 1024)),
        )
        .route("/api/downloads/{id}", axum::routing::delete(remove))
        .route("/api/downloads/{id}/files", get(files).put(select_files))
        .route("/api/downloads/{id}/pause", post(pause))
        .route("/api/downloads/{id}/resume", post(resume))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    state: Option<DownloadFilter>,
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DownloadList {
    items: Vec<Download>,
}

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> ApiResult<Json<DownloadList>> {
    user.require(Scope::DownloadsRead)?;
    let access = user.access(&state.db).await?;
    let items = crate::live::visible_downloads(
        &state.db,
        &access,
        query.state.unwrap_or_default(),
        query.limit.unwrap_or(100),
    )
    .await?;
    Ok(Json(DownloadList { items }))
}

#[derive(Serialize)]
struct Destination {
    id: String,
    name: String,
    icon: String,
    kinds: Vec<stash_core::models::Category>,
}

async fn destinations(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<Destination>>> {
    user.require(Scope::DownloadsCreate)?;
    let access = user.access(&state.db).await?;
    use stash_core::models::Category;
    let defs = stash_core::library::defs::load_all(&state.db).await?;
    Ok(Json(
        defs.into_iter()
            .filter(|def| access.sees_library(def.id()))
            .map(|def| Destination {
                kinds: [
                    Category::Movie,
                    Category::Episode,
                    Category::HomeVideo,
                    Category::Photo,
                ]
                .into_iter()
                .filter(|kind| def.shape().fits(*kind))
                .collect(),
                id: def.row.id,
                name: def.row.name,
                icon: def.row.icon,
            })
            .collect(),
    ))
}

async fn add(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<AddDownloadRequest>,
) -> ApiResult<Json<Download>> {
    user.require(Scope::DownloadsCreate)?;
    Ok(Json(state.downloads.add_url(&user.user_id, request).await?))
}

async fn add_torrent(
    State(state): State<AppState>,
    user: AuthUser,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult<Json<Download>> {
    user.require(Scope::DownloadsCreate)?;
    let mut multipart =
        multipart.map_err(|_| ApiError::bad_request("Send the .torrent file as a form upload."))?;
    let too_large = || ApiError::bad_request("The .torrent file is larger than 10 MB.");
    let unreadable = || ApiError::bad_request("The upload could not be read. Try again.");
    let mut file = None;
    let mut library_id = None;
    let mut file_types = None;
    while let Some(mut field) = multipart.next_field().await.map_err(|_| unreadable())? {
        let is_file = field.name() == Some("file");
        let is_library = field.name() == Some("library_id");
        if field.name() == Some("file_types") {
            if file_types.is_some() {
                return Err(ApiError::bad_request("Send file types once."));
            }
            let bytes = field.bytes().await.map_err(|_| unreadable())?;
            if bytes.len() > 256 {
                return Err(ApiError::bad_request("Choose valid file types."));
            }
            file_types = Some(
                serde_json::from_slice::<Vec<stash_core::internal_api::TorrentFileKind>>(&bytes)
                    .map_err(|_| ApiError::bad_request("Choose valid file types."))?,
            );
            continue;
        }
        if !is_file && !is_library {
            continue;
        }
        if (is_file && file.is_some()) || (is_library && library_id.is_some()) {
            return Err(ApiError::bad_request(
                "Send one file and one destination at most.",
            ));
        }
        let filename = field.file_name().map(str::to_string);
        let mut bytes = Vec::new();
        let limit = if is_file { MAX_TORRENT_FILE } else { 256 };
        while let Some(chunk) = field.chunk().await.map_err(|_| unreadable())? {
            if bytes.len() + chunk.len() > limit {
                return Err(if is_file {
                    too_large()
                } else {
                    ApiError::bad_request("Choose a valid destination library.")
                        .with_field("library_id")
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        if is_file {
            file = Some((bytes, filename));
        } else {
            library_id = Some(String::from_utf8(bytes).map_err(|_| unreadable())?);
        }
    }
    let (bytes, filename) =
        file.ok_or_else(|| ApiError::bad_request("Choose a .torrent file to upload."))?;
    Ok(Json(
        state
            .downloads
            .add_torrent_file_selected(
                &user.user_id,
                bytes,
                filename,
                library_id.as_deref(),
                file_types,
            )
            .await?,
    ))
}

/// A download the caller cannot see answers 404, as one that never existed (section 7).
async fn require_visible(state: &AppState, user: &AuthUser, id: &str) -> ApiResult<()> {
    let access = user.access(&state.db).await?;
    match crate::live::visible_download(&state.db, &access, id).await? {
        Some(_) => Ok(()),
        None => Err(ApiError::not_found("That download no longer exists.")),
    }
}

/// The download as the caller sees it after a change.
async fn as_seen(state: &AppState, user: &AuthUser, id: &str) -> ApiResult<Download> {
    let access = user.access(&state.db).await?;
    crate::live::visible_download(&state.db, &access, id)
        .await?
        .ok_or_else(|| ApiError::not_found("That download no longer exists."))
}

async fn files(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<stash_core::internal_api::TorrentFiles>> {
    user.require(Scope::Full)?;
    require_visible(&state, &user, &id).await?;
    Ok(Json(state.downloads.torrent_files(&id).await?))
}

async fn select_files(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    ApiJson(selection): ApiJson<stash_core::internal_api::SelectTorrentFiles>,
) -> ApiResult<StatusCode> {
    user.require(Scope::Full)?;
    require_visible(&state, &user, &id).await?;
    if selection.indices.is_empty() || selection.indices.len() > 100_000 {
        return Err(ApiError::bad_request("Select at least one file."));
    }
    state
        .downloads
        .select_torrent_files(&id, selection.indices)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pause(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Download>> {
    user.require(Scope::Full)?;
    require_visible(&state, &user, &id).await?;
    state.downloads.pause(&id).await?;
    Ok(Json(as_seen(&state, &user, &id).await?))
}

async fn resume(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Download>> {
    user.require(Scope::Full)?;
    require_visible(&state, &user, &id).await?;
    state.downloads.resume(&id).await?;
    Ok(Json(as_seen(&state, &user, &id).await?))
}

#[derive(Debug, Deserialize)]
struct RemoveQuery {
    delete_files: Option<bool>,
}

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<RemoveQuery>,
) -> ApiResult<StatusCode> {
    user.require(Scope::Full)?;
    require_visible(&state, &user, &id).await?;
    state
        .downloads
        .remove(&id, query.delete_files.unwrap_or(false))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
