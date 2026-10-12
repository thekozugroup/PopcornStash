//! /api routes: HLS playlists and segments for live transcodes
//! (see docs/architecture.md sections 5 and 8).

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use stash_core::CoreError;
use tokio_util::io::ReaderStream;

use crate::auth::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/stream/{session}", axum::routing::delete(stop))
        .route("/api/stream/{session}/index.m3u8", get(playlist))
        .route("/api/stream/{session}/{segment}", get(segment))
}

fn stream_gone() -> ApiError {
    ApiError::not_found("This stream has ended. Start playback again.")
}

/// Session ids are hyphenated UUIDs; anything else cannot exist.
fn valid_session(id: &str) -> ApiResult<&str> {
    let ok = id.len() == 36
        && id.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
        && uuid::Uuid::try_parse(id).is_ok();
    if ok { Ok(id) } else { Err(stream_gone()) }
}

/// `<n>.ts`, `<n>.m4s` (n up to 6 digits) or `init.mp4`; returns the content type.
fn segment_type(name: &str) -> Option<&'static str> {
    if name == "init.mp4" {
        return Some("video/mp4");
    }
    let (number, ext) = name.split_once('.')?;
    if number.is_empty() || number.len() > 6 || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match ext {
        "ts" => Some("video/mp2t"),
        "m4s" => Some("video/iso.segment"),
        _ => None,
    }
}

fn map_stream_error(e: CoreError) -> ApiError {
    match e {
        CoreError::NotFound => stream_gone(),
        other => other.into(),
    }
}

async fn playlist(
    State(state): State<AppState>,
    user: AuthUser,
    Path(session): Path<String>,
) -> ApiResult<Response> {
    user.require(Scope::LibraryRead)?;
    let session = valid_session(&session)?;
    let body = state
        .transcoder
        .playlist(session)
        .await
        .map_err(map_stream_error)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response())
}

async fn segment(
    State(state): State<AppState>,
    user: AuthUser,
    Path((session, name)): Path<(String, String)>,
) -> ApiResult<Response> {
    user.require(Scope::LibraryRead)?;
    let session = valid_session(&session)?;
    let content_type = segment_type(&name).ok_or_else(stream_gone)?;
    let path = state
        .transcoder
        .segment(session, &name)
        .await
        .map_err(map_stream_error)?;
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(stream_gone()),
        Err(e) => return Err(e.into()),
    };
    let len = file.metadata().await?.len();
    let mut response = Body::from_stream(ReaderStream::new(file)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(response)
}

async fn stop(
    State(state): State<AppState>,
    user: AuthUser,
    Path(session): Path<String>,
) -> ApiResult<StatusCode> {
    user.require(Scope::LibraryRead)?;
    let session = valid_session(&session)?;
    state.transcoder.stop(session).await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::api::library::testing::TestApp;

    #[test]
    fn session_ids_must_be_uuids() {
        assert!(valid_session("0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9d").is_ok());
        assert!(valid_session("../../etc").is_err());
        assert!(valid_session("0192f5a47c1e7b3a9c4d2e5f6a7b8c9d").is_err());
        assert!(valid_session("{0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9}").is_err());
    }

    #[test]
    fn segment_names_are_strict() {
        assert_eq!(segment_type("12.ts"), Some("video/mp2t"));
        assert_eq!(segment_type("0.m4s"), Some("video/iso.segment"));
        assert_eq!(segment_type("init.mp4"), Some("video/mp4"));
        for bad in [
            "",
            ".ts",
            "a.ts",
            "1.ts.ts",
            "../1.ts",
            "1234567.ts",
            "1.mp4",
            "1.TS",
            "-1.ts",
        ] {
            assert_eq!(segment_type(bad), None, "{bad}");
        }
    }

    #[tokio::test]
    async fn unknown_sessions_are_not_found() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let (status, _) = app
            .json("/api/stream/not-a-session/index.m3u8", &token)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = app
            .json(
                "/api/stream/0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9d/..%2F..%2Fsecret",
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = app
            .json(
                "/api/stream/0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9d/index.m3u8",
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
