//! /api/events: server-sent events for live UI updates
//! (see docs/architecture.md section 8).
//!
//! Every connection subscribes to the in-process [`EventBus`](stash_core::events::EventBus)
//! and receives only the events its scopes allow: downloads need
//! `downloads:read`; library, inbox and job events need `library:read`;
//! VPN status goes to everyone. The caller's session or device token is
//! checked again every minute, so signing out or revoking a device also ends
//! its open streams.

use std::convert::Infallible;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Request, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::Stream;
use stash_core::events::Event;
use tokio::sync::broadcast::error::RecvError;

use crate::auth::{AuthUser, Scope};
use crate::error::{ApiResult, ErrorCode};
use crate::state::AppState;

const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// How long a new connection waits for the initial VPN status.
const INITIAL_VPN_TIMEOUT: Duration = Duration::from_secs(5);
/// How often an open stream re-checks that its caller is still signed in.
const REVALIDATE: Duration = Duration::from_secs(60);

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/events", get(events))
}

/// What one connection may see.
#[derive(Debug, Clone, Copy)]
struct Filter {
    downloads: bool,
    library: bool,
}

impl Filter {
    fn for_user(user: &AuthUser) -> Self {
        Self {
            downloads: user.has(Scope::DownloadsRead),
            library: user.has(Scope::LibraryRead),
        }
    }

    fn allows(self, event: &Event) -> bool {
        match event {
            Event::Download(_) => self.downloads,
            Event::Library { .. } | Event::Inbox { .. } | Event::Job { .. } => self.library,
            Event::Vpn(_) => true,
        }
    }
}

fn to_sse(event: &Event) -> SseEvent {
    SseEvent::default()
        .event(event.name())
        .data(event.data().to_string())
}

/// The headers that identify the caller, kept to re-check the session later.
fn credentials(headers: &HeaderMap) -> HeaderMap {
    let mut kept = HeaderMap::new();
    for name in [header::AUTHORIZATION, header::COOKIE] {
        for value in headers.get_all(&name) {
            kept.append(name.clone(), value.clone());
        }
    }
    kept
}

/// False once the session or device token behind `headers` is gone.
/// Transient failures (for example a busy database) keep the stream open.
async fn still_signed_in(state: &AppState, headers: &HeaderMap) -> bool {
    let Ok(request) = Request::builder().uri("/api/events").body(()) else {
        return true;
    };
    let (mut parts, ()) = request.into_parts();
    parts.headers = headers.clone();
    match crate::auth::sessions::authenticate(&mut parts, state).await {
        Ok(_) => true,
        Err(e) => e.code != ErrorCode::Unauthorized,
    }
}

/// One turn of the stream loop. Lives on the stack for a moment only, so the
/// size difference between variants does not matter.
#[allow(clippy::large_enum_variant)]
enum Step {
    Received(Result<Event, RecvError>),
    Revalidate,
}

async fn events(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    Ok(sse_response(event_stream(
        state, &user, &headers, REVALIDATE,
    )))
}

/// The SSE stream for one caller: initial VPN status, then every allowed event until
/// the bus closes or a re-check every `revalidate` finds the caller signed out.
fn event_stream(
    state: AppState,
    user: &AuthUser,
    headers: &HeaderMap,
    revalidate: Duration,
) -> impl Stream<Item = Result<SseEvent, Infallible>> + Send + use<> {
    let filter = Filter::for_user(user);
    let credentials = credentials(headers);
    // Subscribe before anything else so no event published from here on is missed.
    let mut rx = state.events.subscribe();
    // The VPN status comes from another service; fetch it on its own task so a
    // failure there can never take this stream down.
    let initial_vpn = state.torrentd.enabled().then(|| {
        let torrentd = state.torrentd.clone();
        tokio::spawn(async move { torrentd.vpn_status().await })
    });

    async_stream::stream! {
        if let Some(task) = initial_vpn {
            match tokio::time::timeout(INITIAL_VPN_TIMEOUT, task).await {
                Ok(Ok(status)) => yield Ok::<_, Infallible>(to_sse(&Event::Vpn(status))),
                Ok(Err(e)) => tracing::warn!(error = %e, "could not read the VPN status for a new event stream"),
                Err(_) => tracing::debug!("VPN status was slow; the event stream starts without it"),
            }
        }
        let mut check = tokio::time::interval_at(tokio::time::Instant::now() + revalidate, revalidate);
        check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let step = tokio::select! {
                received = rx.recv() => Step::Received(received),
                _ = check.tick() => Step::Revalidate,
            };
            match step {
                Step::Received(Ok(event)) => {
                    if filter.allows(&event) {
                        yield Ok(to_sse(&event));
                    }
                }
                Step::Received(Err(RecvError::Lagged(skipped))) => {
                    tracing::debug!(skipped, "event stream fell behind; continuing");
                }
                Step::Received(Err(RecvError::Closed)) => break,
                Step::Revalidate => {
                    if !still_signed_in(&state, &credentials).await {
                        tracing::debug!("closing an event stream whose session has ended");
                        break;
                    }
                }
            }
        }
    }
}

fn sse_response<S>(stream: S) -> Response
where
    S: Stream<Item = Result<SseEvent, Infallible>> + Send + 'static,
{
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
        .into_response();
    // Ask reverse proxies such as nginx not to buffer the stream.
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    response
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use stash_core::events::{Event, LibraryChange};
    use stash_core::models::Role;
    use tower::ServiceExt;

    use super::*;
    use crate::api::library::testing::{TestApp, request};

    fn user(scopes: Vec<Scope>) -> AuthUser {
        AuthUser {
            user_id: "u".into(),
            display_name: "U".into(),
            role: Role::Member,
            scopes,
            session_id: None,
            token_id: Some("t".into()),
        }
    }

    #[test]
    fn scopes_filter_events() {
        let library = Event::Library {
            r#type: LibraryChange::Added,
            item_id: "x".into(),
        };
        let job = Event::Job {
            kind: "scan".into(),
            state: "done".into(),
            item_id: None,
        };
        let inbox = Event::Inbox { count: 2 };
        let vpn = Event::Vpn(stash_core::models::VpnStatus::unreachable(false, "off"));

        let downloads_only =
            Filter::for_user(&user(vec![Scope::DownloadsRead, Scope::DownloadsCreate]));
        assert!(!downloads_only.allows(&library));
        assert!(!downloads_only.allows(&job));
        assert!(!downloads_only.allows(&inbox));
        assert!(downloads_only.allows(&vpn));

        let reader = Filter::for_user(&user(vec![Scope::LibraryRead]));
        assert!(
            reader.allows(&library)
                && reader.allows(&job)
                && reader.allows(&inbox)
                && reader.allows(&vpn)
        );

        let full = Filter::for_user(&user(vec![Scope::Full]));
        assert!(full.downloads && full.library);
    }

    #[test]
    fn sse_frame_uses_event_name_and_json_data() {
        let e = Event::Inbox { count: 3 };
        // Rendering goes through axum; check the pieces we feed it.
        assert_eq!(e.name(), "inbox");
        assert_eq!(e.data().to_string(), r#"{"count":3}"#);
        let _ = to_sse(&e);
    }

    async fn read_until(body: &mut axum::body::Body, needle: &str) -> String {
        let mut seen = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !seen.contains(needle) {
            let frame = tokio::time::timeout_at(deadline, body.frame())
                .await
                .expect("event arrives in time");
            match frame {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        seen.push_str(&String::from_utf8_lossy(data));
                    }
                }
                _ => break,
            }
        }
        seen
    }

    #[tokio::test]
    async fn stream_emits_published_events() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let response = app
            .router
            .clone()
            .oneshot(request("GET", "/api/events", Some(&token), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            content_type.starts_with("text/event-stream"),
            "{content_type}"
        );
        let mut body = response.into_body();
        app.state.events.publish(Event::Library {
            r#type: LibraryChange::Added,
            item_id: "abc".into(),
        });
        let seen = read_until(&mut body, "abc").await;
        assert!(seen.contains("event: library"), "{seen}");
        assert!(
            seen.contains(r#"data: {"item_id":"abc","type":"added"}"#)
                || seen.contains(r#"data: {"type":"added","item_id":"abc"}"#),
            "{seen}"
        );
    }

    #[tokio::test]
    async fn downloads_only_tokens_do_not_see_library_events() {
        let app = TestApp::new().await;
        let (user_id, _) = app.user("member").await;
        let token = app
            .device_token(&user_id, &["downloads:create", "downloads:read"])
            .await;
        let response = app
            .router
            .clone()
            .oneshot(request("GET", "/api/events", Some(&token), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        app.state.events.publish(Event::Library {
            r#type: LibraryChange::Added,
            item_id: "hidden".into(),
        });
        app.state
            .events
            .publish(Event::Vpn(stash_core::models::VpnStatus::unreachable(
                false, "visible",
            )));
        let seen = read_until(&mut body, "visible").await;
        assert!(seen.contains("event: vpn"), "{seen}");
        assert!(!seen.contains("hidden"), "{seen}");
    }

    /// Signing out (or revoking a device) ends an open stream at the next re-check.
    #[tokio::test]
    async fn open_streams_close_after_sign_out() {
        use futures::StreamExt;

        let app = TestApp::new().await;
        let (user_id, token) = app.user("member").await;
        let device = app.device_token(&user_id, &["library:read"]).await;
        for (secret, revoke) in [
            (token, "DELETE FROM sessions"),
            (device, "DELETE FROM api_tokens"),
        ] {
            let mut parts = request("GET", "/api/events", Some(&secret), None)
                .into_parts()
                .0;
            let user = crate::auth::sessions::authenticate(&mut parts, &app.state)
                .await
                .unwrap();
            let stream = event_stream(
                app.state.clone(),
                &user,
                &parts.headers,
                Duration::from_millis(50),
            );
            let mut stream = Box::pin(stream);
            app.state.events.publish(Event::Inbox { count: 1 });
            let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("an event arrives");
            assert!(first.is_some(), "{revoke}");
            sqlx::query(revoke).execute(&app.state.db).await.unwrap();
            let end = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("the stream ends after the re-check");
            assert!(end.is_none(), "{revoke}: stream stayed open");
        }
    }

    #[tokio::test]
    async fn revoked_sessions_fail_revalidation() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        headers.insert(header::USER_AGENT, "test".parse().unwrap());
        let kept = credentials(&headers);
        assert_eq!(kept.len(), 1);
        assert!(still_signed_in(&app.state, &kept).await);
        sqlx::query("DELETE FROM sessions")
            .execute(&app.state.db)
            .await
            .unwrap();
        assert!(!still_signed_in(&app.state, &kept).await);
    }
}
