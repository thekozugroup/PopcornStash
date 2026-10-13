//! /api routes: uploads (see docs/architecture.md section 8).

use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{post, put};
use axum::{Json, RequestExt, Router};
use serde::Deserialize;
use stash_core::models::Download;

use super::downloads::{ApiJson, ApiQuery};
use crate::auth::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::uploads::{CreateUpload, MAX_CHUNK, UploadCreated, UploadStatus};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/uploads", post(create))
        .route("/api/uploads/{id}/complete", post(complete))
        .route(
            "/api/uploads/{id}",
            // Route-level limit: overrides the router-wide 2 MiB default for chunk bodies.
            put(put_chunk)
                .layer(DefaultBodyLimit::max(MAX_CHUNK))
                .get(status)
                .delete(cancel),
        )
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<CreateUpload>,
) -> ApiResult<Json<UploadCreated>> {
    user.require(Scope::Full)?;
    Ok(Json(state.uploads.create(&user, request).await?))
}

#[derive(Debug, Deserialize)]
struct OffsetQuery {
    offset: Option<i64>,
}

async fn put_chunk(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    ApiQuery(query): ApiQuery<OffsetQuery>,
    request: Request,
) -> ApiResult<Response> {
    user.require(Scope::Full)?;
    let offset = query
        .offset
        .ok_or_else(|| ApiError::bad_request("Say where this piece starts with ?offset=."))?;
    if offset < 0 {
        return Err(ApiError::bad_request("The offset cannot be negative."));
    }
    let content_length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    // Applies the route's DefaultBodyLimit to the streamed body.
    let body = request.with_limited_body().into_body();
    match state
        .uploads
        .write_chunk(&user, &id, offset, content_length, body)
        .await
    {
        Ok(received) => Ok(Json(serde_json::json!({ "received": received })).into_response()),
        Err(error) => Ok(with_received(error)),
    }
}

/// The error envelope, plus `received` at the top level when the error carries it
/// (section 8: "409 with `{received}`"; the browser reads it there to continue).
fn with_received(error: ApiError) -> Response {
    let Some(received) = error
        .extra
        .as_ref()
        .and_then(|extra| extra.get("received"))
        .cloned()
    else {
        return error.into_response();
    };
    let mut inner = serde_json::json!({ "code": error.code.as_str(), "message": error.message });
    if let (Some(serde_json::Value::Object(extra)), serde_json::Value::Object(map)) =
        (error.extra, &mut inner)
    {
        map.extend(extra);
    }
    (
        error.code.status(),
        Json(serde_json::json!({ "error": inner, "received": received })),
    )
        .into_response()
}

async fn status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<UploadStatus>> {
    user.require(Scope::Full)?;
    Ok(Json(state.uploads.status(&user, &id).await?))
}

async fn complete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    user.require(Scope::Full)?;
    Ok(match state.uploads.complete(&user, &id).await {
        Ok(download) => Json::<Download>(download).into_response(),
        Err(error) => with_received(error),
    })
}

async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require(Scope::Full)?;
    state.uploads.cancel(&user, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
