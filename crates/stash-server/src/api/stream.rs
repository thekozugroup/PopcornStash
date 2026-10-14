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

/// Session ids are 128-bit values written as 32 lowercase hex digits (what the
/// transcoder issues) or as a hyphenated UUID; anything else cannot exist.
fn valid_session(id: &str) -> ApiResult<&str> {
    let lower_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let simple = id.len() == 32 && id.bytes().all(lower_hex);
    let hyphenated = id.len() == 36
        && id.bytes().all(|b| b == b'-' || lower_hex(b))
        && uuid::Uuid::try_parse(id).is_ok();
    if simple || hyphenated {
        Ok(id)
    } else {
        Err(stream_gone())
    }
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

/// The session when it belongs to the caller and the caller still sees its item. A
/// session of someone else answers 404; one whose item the caller can no longer see
/// answers 404 and is stopped (section 7, HLS).
async fn own_session<'a>(
    state: &AppState,
    user: &AuthUser,
    session: &'a str,
) -> ApiResult<&'a str> {
    let Some((owner, item_id)) = state.transcoder.session_owner(session) else {
        return Err(stream_gone());
    };
    if owner != user.user_id {
        return Err(stream_gone());
    }
    let visible = stash_core::library::access::visible_row(&state.db, &user.viewer(), &item_id)
        .await?
        .is_some_and(|row| row.status != stash_core::models::ItemStatus::Trashed);
    if !visible {
        state.transcoder.stop(session).await;
        return Err(stream_gone());
    }
    Ok(session)
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
    let session = own_session(&state, &user, session).await?;
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
    let session = own_session(&state, &user, session).await?;
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
    let session = own_session(&state, &user, session).await?;
    state.transcoder.stop(session).await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::api::library::testing::TestApp;

    #[test]
    fn session_ids_must_be_128_bit_ids() {
        assert!(valid_session("0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9d").is_ok());
        assert!(valid_session("0192f5a47c1e7b3a9c4d2e5f6a7b8c9d").is_ok());
        assert!(valid_session(&hex::encode(rand::random::<[u8; 16]>())).is_ok());
        assert!(valid_session("../../etc").is_err());
        assert!(valid_session("0192F5A47C1E7B3A9C4D2E5F6A7B8C9D").is_err());
        assert!(valid_session("0192f5a47c1e7b3a9c4d2e5f6a7b8c9").is_err());
        assert!(valid_session("{0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9}").is_err());
        assert!(valid_session("0192f5a4-7c1e-7b3a-9c4d-2e5f6a7b8c9z").is_err());
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

    /// A short H.264 + AAC clip, or None when ffmpeg is not installed.
    fn make_clip(path: &std::path::Path, seconds: u32) -> Option<()> {
        std::fs::create_dir_all(path.parent()?).ok()?;
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg(format!("testsrc=s=320x180:r=25:d={seconds}"))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:d={seconds}"))
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .args(["-c:a", "aac", "-shortest"])
            .arg(path)
            .status()
            .ok()?;
        status.success().then_some(())
    }

    #[tokio::test]
    async fn playback_decides_direct_or_hls_end_to_end() {
        use stash_core::models::{Category, ItemStatus, MediaKind};

        use crate::api::library::testing::request;

        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let root = app.state.config.stash_root.clone();
        let mp4 = "Home Videos/2024/06/clip.mp4";
        let mkv = "Movies/Clip (2020)/Clip (2020).mkv";
        if make_clip(&root.join(mp4), 3).is_none() || make_clip(&root.join(mkv), 8).is_none() {
            eprintln!("skipping: ffmpeg with libx264 is not available");
            return;
        }
        let caps =
            serde_json::json!({"video_codecs": ["h264"], "audio_codecs": ["aac"], "hdr": false});

        // MP4 with H.264 + AAC plays directly.
        let direct = app
            .item(
                mp4,
                MediaKind::Video,
                Category::HomeVideo,
                ItemStatus::Ready,
                None,
            )
            .await;
        let (status, _, body) = app
            .send(request(
                "POST",
                &format!("/api/items/{direct}/playback"),
                Some(&token),
                Some(caps.clone()),
            ))
            .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"mode": "direct", "url": format!("/api/items/{direct}/file")})
        );

        // Matroska is not a browser container, so it is transcoded to HLS.
        let hls = app
            .item(
                mkv,
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                None,
            )
            .await;
        let (status, _, body) = app
            .send(request(
                "POST",
                &format!("/api/items/{hls}/playback"),
                Some(&token),
                Some(caps),
            ))
            .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["mode"], "hls", "{json}");
        let session = json["session_id"].as_str().unwrap().to_string();
        let url = json["url"].as_str().unwrap().to_string();
        assert_eq!(url, format!("/api/stream/{session}/index.m3u8"));

        let (status, headers, body) = app.get(&url, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[header::CONTENT_TYPE],
            "application/vnd.apple.mpegurl"
        );
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
        let playlist = String::from_utf8(body.to_vec()).unwrap();
        assert!(playlist.contains("\n0.ts\n"), "{playlist}");

        let (status, headers, body) = app
            .get(&format!("/api/stream/{session}/0.ts"), Some(&token))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "video/mp2t");
        assert_eq!(body.first(), Some(&0x47), "MPEG-TS sync byte");
        assert_eq!(headers[header::CONTENT_LENGTH], body.len().to_string());

        let (status, _, _) = app
            .send(request(
                "DELETE",
                &format!("/api/stream/{session}"),
                Some(&token),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _, _) = app.get(&url, Some(&token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
