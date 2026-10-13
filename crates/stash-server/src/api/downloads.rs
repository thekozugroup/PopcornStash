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
        .route(
            "/api/downloads/torrent",
            // Room for a 10 MB .torrent plus multipart framing.
            post(add_torrent).layer(DefaultBodyLimit::max(MAX_TORRENT_FILE + 1024 * 1024)),
        )
        .route("/api/downloads/{id}", axum::routing::delete(remove))
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
    let items = state
        .downloads
        .list(query.state.unwrap_or_default(), query.limit.unwrap_or(100))
        .await?;
    Ok(Json(DownloadList { items }))
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
    while let Some(mut field) = multipart.next_field().await.map_err(|_| unreadable())? {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().map(str::to_string);
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|e| {
            if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                too_large()
            } else {
                unreadable()
            }
        })? {
            if bytes.len() + chunk.len() > MAX_TORRENT_FILE {
                return Err(too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(Json(
            state
                .downloads
                .add_torrent_file(&user.user_id, bytes, filename)
                .await?,
        ));
    }
    Err(ApiError::bad_request("Choose a .torrent file to upload."))
}

async fn pause(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Download>> {
    user.require(Scope::Full)?;
    Ok(Json(state.downloads.pause(&id).await?))
}

async fn resume(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Download>> {
    user.require(Scope::Full)?;
    Ok(Json(state.downloads.resume(&id).await?))
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
    state
        .downloads
        .remove(&id, query.delete_files.unwrap_or(false))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
