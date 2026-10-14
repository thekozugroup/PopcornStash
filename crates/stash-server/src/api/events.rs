//! /api/events: server-sent events for live UI updates
//! (see docs/architecture.md sections 7 and 8).
//!
//! Every connection reads the live bus ([`crate::live`]) and is sent only what its
//! caller may see, worked out per connection: scopes first (downloads need
//! `downloads:read`; library, job, inbox and libraries events need `library:read`; VPN
//! status goes to everyone), then the library access rule for each item and download.
//! Each connection keeps its caller's role, visible libraries and the claims, and reloads
//! them after any library write, a change of the caller's role and at the 60 s re-check,
//! which also ends the stream once the session or device token is gone.

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Request, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::Stream;
use stash_core::events::LibraryChange;
use stash_core::library::access::Access;
use stash_core::library::{defs, repo};
use stash_core::models::{ItemFacts, Place, Role};
use tokio::sync::broadcast::error::RecvError;

use crate::auth::{AuthUser, Scope};
use crate::downloads::DownloadFilter;
use crate::error::{ApiResult, ErrorCode};
use crate::live::{Live, download_view, links_of, visible_downloads};
use crate::state::AppState;

const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// How long a new connection waits for the initial VPN status.
const INITIAL_VPN_TIMEOUT: Duration = Duration::from_secs(5);
/// How often an open stream re-checks that its caller is still signed in.
const REVALIDATE: Duration = Duration::from_secs(60);
/// Beyond this many items changing view at once, a stream sends no item events for the
/// change; the `libraries` event that comes with it makes clients reload.
const MAX_ITEM_EVENTS: usize = 2000;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/events", get(events))
}

fn frame(name: &str, data: serde_json::Value) -> SseEvent {
    SseEvent::default().event(name).data(data.to_string())
}

fn item_frame(change: LibraryChange, item_id: &str, library_id: Option<&str>) -> SseEvent {
    frame(
        "library",
        serde_json::json!({"type": change.as_str(), "item_id": item_id, "library_id": library_id}),
    )
}

/// What a stream does next.
enum Out {
    Send(SseEvent),
    Close,
}

/// One connection's caller and what it has been told.
struct Conn {
    state: AppState,
    user: AuthUser,
    access: Access,
    /// Downloads this caller may know about (sent here, or listed when the stream
    /// opened): when one leaves their view they get `{id, removed: true}`.
    known_downloads: HashSet<String>,
    /// The caller's `GET /api/libraries` answer, counts aside.
    libraries: Option<String>,
    inbox_count: Option<i64>,
}

impl Conn {
    async fn open(state: AppState, user: AuthUser) -> ApiResult<Self> {
        let access = user.access(&state.db).await?;
        let mut conn = Self {
            state,
            user,
            access,
            known_downloads: HashSet::new(),
            libraries: None,
            inbox_count: None,
        };
        if conn.downloads() {
            conn.known_downloads =
                visible_downloads(&conn.state.db, &conn.access, DownloadFilter::All, 500)
                    .await?
                    .into_iter()
                    .map(|d| d.id)
                    .collect();
        }
        if conn.library() {
            conn.libraries = Some(conn.libraries_now().await?);
        }
        Ok(conn)
    }

    fn library(&self) -> bool {
        self.user.has(Scope::LibraryRead)
    }

    fn downloads(&self) -> bool {
        self.user.has(Scope::DownloadsRead)
    }

    async fn libraries_now(&self) -> ApiResult<String> {
        let views = defs::views(&self.state.db, &self.state.layout, &self.access).await?;
        Ok(defs::fingerprint(&views))
    }

    async fn handle(&mut self, live: &Live) -> ApiResult<Vec<Out>> {
        let mut out = Vec::new();
        match live {
            Live::Vpn(status) => out.push(Out::Send(frame(
                "vpn",
                serde_json::to_value(status).unwrap_or_default(),
            ))),
            Live::Item {
                change,
                item_id,
                before,
                after,
            } => {
                if self.library() {
                    self.item(&mut out, *change, item_id, before.as_ref(), after.as_ref());
                }
                // A sorted download may have lost its last visible item.
                let before_seen = before
                    .as_ref()
                    .or(after.as_ref())
                    .is_some_and(|f| self.access.sees(f));
                let after_seen = after.as_ref().is_some_and(|f| self.access.sees(f));
                if before_seen && (!after_seen || *change == LibraryChange::Removed) {
                    self.recheck_item_downloads(&mut out, item_id).await?;
                }
            }
            Live::Regrouped { item_ids, from, to } => {
                let facts = |library: &Option<String>| ItemFacts {
                    library_id: library.clone(),
                    place: if library.is_some() {
                        Place::Library
                    } else {
                        Place::Folder
                    },
                    added_by: None,
                    status: stash_core::models::ItemStatus::Ready,
                    category: stash_core::models::Category::Other,
                };
                let (before, after) = (facts(from), facts(to));
                if self.library() && item_ids.len() <= MAX_ITEM_EVENTS {
                    for id in item_ids {
                        self.item(
                            &mut out,
                            LibraryChange::Updated,
                            id,
                            Some(&before),
                            Some(&after),
                        );
                    }
                }
                if self.access.sees(&before) && !self.access.sees(&after) {
                    self.recheck_downloads(&mut out, None).await?;
                }
            }
            Live::Job {
                kind,
                state,
                item_id,
                facts,
            } => {
                let allowed = match item_id {
                    None => true,
                    Some(_) => facts.as_ref().is_some_and(|f| self.access.sees(f)),
                };
                if self.library() && allowed {
                    out.push(Out::Send(frame(
                        "job",
                        serde_json::json!({"kind": kind, "state": state, "item_id": item_id}),
                    )));
                }
            }
            Live::Inbox => self.inbox(&mut out, true).await?,
            Live::Download { download, links } => {
                if self.downloads() {
                    match download_view(&self.access, download, links) {
                        Some(view) => {
                            self.known_downloads.insert(view.id.clone());
                            out.push(Out::Send(frame(
                                "download",
                                serde_json::to_value(&view).unwrap_or_default(),
                            )));
                        }
                        None => {
                            if self.known_downloads.remove(&download.id) {
                                out.push(Out::Send(removed_download(&download.id)));
                            }
                        }
                    }
                }
            }
            Live::Libraries => self.reload(&mut out).await?,
            Live::AccessChanged { user_id } if *user_id == self.user.user_id => {
                self.reload(&mut out).await?;
            }
            Live::UserRemoved { user_id } if *user_id == self.user.user_id => {
                out.push(Out::Close);
            }
            Live::AccessChanged { .. } | Live::UserRemoved { .. } => {}
        }
        Ok(out)
    }

    /// An item event as this caller may see it: a change that takes the item out of
    /// view is `removed` with no library, one that brings it in is `added`.
    fn item(
        &self,
        out: &mut Vec<Out>,
        change: LibraryChange,
        item_id: &str,
        before: Option<&ItemFacts>,
        after: Option<&ItemFacts>,
    ) {
        let sees_after = after.is_some_and(|f| self.access.sees(f));
        let sees_before = match before {
            Some(f) => self.access.sees(f),
            None if change == LibraryChange::Added => false,
            None => sees_after,
        };
        if change == LibraryChange::Removed {
            // Deleted, or moved to the trash: gone from every library view.
            if sees_before || sees_after {
                out.push(Out::Send(item_frame(LibraryChange::Removed, item_id, None)));
            }
            return;
        }
        let sent = match (sees_before, sees_after) {
            (true, true) => Some((change, after.and_then(|f| f.library_id.as_deref()))),
            (false, true) => Some((
                LibraryChange::Added,
                after.and_then(|f| f.library_id.as_deref()),
            )),
            (true, false) => Some((LibraryChange::Removed, None)),
            (false, false) => None,
        };
        if let Some((change, library_id)) = sent {
            let library_id = if change == LibraryChange::Removed {
                None
            } else {
                library_id
            };
            out.push(Out::Send(item_frame(change, item_id, library_id)));
        }
    }

    async fn inbox(&mut self, out: &mut Vec<Out>, always: bool) -> ApiResult<()> {
        if !self.library() {
            return Ok(());
        }
        let count = repo::inbox_count(&self.state.db, &self.access.viewer).await?;
        if always || self.inbox_count != Some(count) {
            self.inbox_count = Some(count);
            out.push(Out::Send(frame(
                "inbox",
                serde_json::json!({"count": count}),
            )));
        }
        Ok(())
    }

    /// Downloads this caller knows that link `item_id`, checked again.
    async fn recheck_item_downloads(&mut self, out: &mut Vec<Out>, item_id: &str) -> ApiResult<()> {
        if !self.downloads() || self.known_downloads.is_empty() {
            return Ok(());
        }
        let linked: Vec<String> =
            sqlx::query_scalar("SELECT download_id FROM download_items WHERE item_id = ?")
                .bind(item_id)
                .fetch_all(&self.state.db)
                .await?;
        let linked: Vec<String> = linked
            .into_iter()
            .filter(|d| self.known_downloads.contains(d))
            .collect();
        if !linked.is_empty() {
            self.recheck_downloads(out, Some(linked)).await?;
        }
        Ok(())
    }

    /// Check known downloads (all, or `only`) again; tell the caller about any that left
    /// their view.
    async fn recheck_downloads(
        &mut self,
        out: &mut Vec<Out>,
        only: Option<Vec<String>>,
    ) -> ApiResult<()> {
        if !self.downloads() {
            return Ok(());
        }
        let ids: Vec<String> =
            only.unwrap_or_else(|| self.known_downloads.iter().cloned().collect());
        for id in ids {
            let row: Option<stash_core::models::DownloadRow> =
                sqlx::query_as("SELECT * FROM downloads WHERE id = ?")
                    .bind(&id)
                    .fetch_optional(&self.state.db)
                    .await?;
            let visible = match row {
                Some(row) => {
                    let links = links_of(&self.state.db, &id).await?;
                    download_view(&self.access, &row.to_download(Vec::new()), &links).is_some()
                }
                None => false,
            };
            if !visible && self.known_downloads.remove(&id) {
                out.push(Out::Send(removed_download(&id)));
            }
        }
        Ok(())
    }

    /// The caller's role or the libraries may have changed: load what they see again and
    /// tell them what changed for them.
    async fn reload(&mut self, out: &mut Vec<Out>) -> ApiResult<()> {
        let role: Option<Role> = sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
            .bind(&self.user.user_id)
            .fetch_optional(&self.state.db)
            .await?;
        let Some(role) = role else {
            out.push(Out::Close);
            return Ok(());
        };
        self.user.role = role;
        let new = self.user.access(&self.state.db).await?;
        if new != self.access {
            let old = std::mem::replace(&mut self.access, new);
            if self.library() {
                self.item_diff(out, &old).await?;
            }
            self.recheck_downloads(out, None).await?;
            self.inbox(out, false).await?;
        }
        if self.library() {
            let now = self.libraries_now().await?;
            if self.libraries.as_deref() != Some(now.as_str()) {
                self.libraries = Some(now);
                out.push(Out::Send(frame("libraries", serde_json::json!({}))));
            }
        }
        Ok(())
    }

    /// Items that came into or went out of view because what the caller sees changed.
    async fn item_diff(&self, out: &mut Vec<Out>, old: &Access) -> ApiResult<()> {
        let new = &self.access;
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT i.id, i.library_id, i.place, i.added_by, i.status, i.category FROM items i WHERE ",
        );
        if old.full_owner() != new.full_owner() {
            qb.push("1 = 1");
        } else {
            let libraries: Vec<&String> =
                old.libraries.symmetric_difference(&new.libraries).collect();
            let kinds: Vec<&str> = defs::CLAIMABLE
                .iter()
                .filter(|c| {
                    old.visible_claimer(**c).is_some() != new.visible_claimer(**c).is_some()
                })
                .map(|c| c.as_str())
                .collect();
            if libraries.is_empty() && kinds.is_empty() {
                return Ok(());
            }
            qb.push("(i.place = 'library' AND i.library_id IN (SELECT value FROM json_each(")
                .push_bind(serde_json::to_string(&libraries).unwrap_or_default())
                .push("))) OR (i.place = 'incoming' AND i.status = 'needs_review' AND i.category IN (SELECT value FROM json_each(")
                .push_bind(serde_json::to_string(&kinds).unwrap_or_default())
                .push(")))");
        }
        qb.push(" LIMIT ").push_bind((MAX_ITEM_EVENTS + 1) as i64);
        type Row = (
            String,
            Option<String>,
            Place,
            Option<String>,
            stash_core::models::ItemStatus,
            stash_core::models::Category,
        );
        let rows: Vec<Row> = qb.build_query_as().fetch_all(&self.state.db).await?;
        if rows.len() > MAX_ITEM_EVENTS {
            return Ok(());
        }
        for (id, library_id, place, added_by, status, category) in rows {
            let facts = ItemFacts {
                library_id,
                place,
                added_by,
                status,
                category,
            };
            match (old.sees(&facts), new.sees(&facts)) {
                (true, false) => out.push(Out::Send(item_frame(LibraryChange::Removed, &id, None))),
                (false, true) => out.push(Out::Send(item_frame(
                    LibraryChange::Added,
                    &id,
                    facts.library_id.as_deref(),
                ))),
                _ => {}
            }
        }
        Ok(())
    }
}

fn removed_download(id: &str) -> SseEvent {
    frame("download", serde_json::json!({"id": id, "removed": true}))
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
    Received(Result<Arc<Live>, RecvError>),
    Revalidate,
}

async fn events(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let conn = Conn::open(state, user).await?;
    Ok(sse_response(event_stream(conn, &headers, REVALIDATE)))
}

/// The SSE stream for one caller: initial VPN status, then every event they may see
/// until the bus closes, the caller is deleted, or a re-check every `revalidate` finds
/// them signed out.
fn event_stream(
    mut conn: Conn,
    headers: &HeaderMap,
    revalidate: Duration,
) -> impl Stream<Item = Result<SseEvent, Infallible>> + Send + use<> {
    let credentials = credentials(headers);
    // Subscribe before anything else so no event published from here on is missed.
    let mut rx = conn.state.live.subscribe();
    // The VPN status comes from another service; fetch it on its own task so a
    // failure there can never take this stream down.
    let initial_vpn = conn.state.torrentd.enabled().then(|| {
        let torrentd = conn.state.torrentd.clone();
        tokio::spawn(async move { torrentd.vpn_status().await })
    });

    async_stream::stream! {
        if let Some(task) = initial_vpn {
            match tokio::time::timeout(INITIAL_VPN_TIMEOUT, task).await {
                Ok(Ok(status)) => yield Ok::<_, Infallible>(frame("vpn", serde_json::to_value(&status).unwrap_or_default())),
                Ok(Err(e)) => tracing::warn!(error = %e, "could not read the VPN status for a new event stream"),
                Err(_) => tracing::debug!("VPN status was slow; the event stream starts without it"),
            }
        }
        let mut check = tokio::time::interval_at(tokio::time::Instant::now() + revalidate, revalidate);
        check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        'stream: loop {
            let step = tokio::select! {
                received = rx.recv() => Step::Received(received),
                _ = check.tick() => Step::Revalidate,
            };
            let outs = match step {
                Step::Received(Ok(live)) => match conn.handle(&live).await {
                    Ok(outs) => outs,
                    Err(e) => {
                        tracing::warn!(error = %e, "could not filter a live event for a stream");
                        Vec::new()
                    }
                },
                Step::Received(Err(RecvError::Lagged(skipped))) => {
                    tracing::debug!(skipped, "event stream fell behind; continuing");
                    Vec::new()
                }
                Step::Received(Err(RecvError::Closed)) => break,
                Step::Revalidate => {
                    if !still_signed_in(&conn.state, &credentials).await {
                        tracing::debug!("closing an event stream whose session has ended");
                        break;
                    }
                    let mut outs = Vec::new();
                    if let Err(e) = conn.reload(&mut outs).await {
                        tracing::warn!(error = %e, "could not re-check an event stream's access");
                    }
                    outs
                }
            };
            for out in outs {
                match out {
                    Out::Send(event) => yield Ok(event),
                    Out::Close => break 'stream,
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
    use stash_core::library::defs::{STARTING_MOVIES, STARTING_SHOWS};
    use stash_core::models::{Category, ItemStatus, MediaKind, VpnStatus};
    use tower::ServiceExt;

    use super::*;
    use crate::api::library::testing::{TestApp, request};

    async fn read_until(body: &mut axum::body::Body, needle: &str) -> String {
        let mut seen = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !seen.contains(needle) {
            let frame = tokio::time::timeout_at(deadline, body.frame())
                .await
                .unwrap_or_else(|_| panic!("{needle:?} did not arrive; got {seen}"));
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

    async fn open(app: &TestApp, token: &str) -> axum::body::Body {
        let response = app
            .router
            .clone()
            .oneshot(request("GET", "/api/events", Some(token), None))
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
        response.into_body()
    }

    /// Publish a VPN status naming `marker` and read the stream up to it: whatever was
    /// published before and is not in the text was not sent.
    async fn up_to_marker(app: &TestApp, body: &mut axum::body::Body, marker: &str) -> String {
        app.state
            .events
            .publish(Event::Vpn(VpnStatus::unreachable(false, marker)));
        read_until(body, marker).await
    }

    async fn hidden_library(app: &TestApp, owner: &str) -> String {
        let (status, json) = app
            .send_json(
                "POST",
                "/api/libraries",
                owner,
                serde_json::json!({"name": "Private", "icon": "heart", "content_type": "video",
                    "structure": "one_offs", "folder": {"mode": "new", "name": "Private"},
                    "visibility": "selected"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn stream_emits_events_the_caller_may_see() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (_, token) = app.user("member").await;
        let hidden = hidden_library(&app, &owner).await;
        let seen = app
            .item(
                "Movies/Heat/Heat.mkv",
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                None,
            )
            .await;
        let unseen = app
            .item(
                "Private/secret.mkv",
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                None,
            )
            .await;
        let mut body = open(&app, &token).await;
        for id in [&unseen, &seen] {
            app.state.events.publish(Event::Library {
                r#type: LibraryChange::Added,
                item_id: id.clone(),
                before: None,
            });
        }
        let text = up_to_marker(&app, &mut body, "marker-1").await;
        assert!(text.contains("event: library"), "{text}");
        assert!(
            text.contains(&format!(
                r#"data: {{"item_id":"{seen}","library_id":"{STARTING_MOVIES}","type":"added"}}"#
            )),
            "{text}"
        );
        assert!(
            !text.contains(&unseen),
            "an item in a hidden library: {text}"
        );
        assert!(!text.contains(&hidden), "{text}");
    }

    #[tokio::test]
    async fn downloads_only_tokens_do_not_see_library_events() {
        let app = TestApp::new().await;
        let (user_id, _) = app.user("member").await;
        let token = app
            .device_token(&user_id, &["downloads:create", "downloads:read"])
            .await;
        let item = app
            .item(
                "Movies/Heat/Heat.mkv",
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                None,
            )
            .await;
        let mut body = open(&app, &token).await;
        app.state.events.publish(Event::Library {
            r#type: LibraryChange::Added,
            item_id: item.clone(),
            before: None,
        });
        app.state.events.publish(Event::Job {
            kind: "scan".into(),
            state: "done".into(),
            item_id: None,
        });
        app.state.events.publish(Event::Inbox { count: 3 });
        app.state.events.publish(Event::Libraries);
        let text = up_to_marker(&app, &mut body, "visible").await;
        assert!(text.contains("event: vpn"), "{text}");
        for name in [
            "event: library",
            "event: job",
            "event: inbox",
            "event: libraries",
        ] {
            assert!(!text.contains(name), "{name}: {text}");
        }
        assert!(!text.contains(&item), "{text}");
    }

    /// The `libraries` event goes only to streams whose own library list changed.
    #[tokio::test]
    async fn libraries_events_only_for_libraries_the_caller_sees() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (_, member) = app.user("member").await;
        let hidden = hidden_library(&app, &owner).await;
        let mut body = open(&app, &member).await;
        let (status, json) = app
            .send_json(
                "PATCH",
                &format!("/api/libraries/{hidden}"),
                &owner,
                serde_json::json!({"name": "Still private"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let text = up_to_marker(&app, &mut body, "marker-a").await;
        assert!(!text.contains("event: libraries"), "{text}");
        let (status, json) = app
            .send_json(
                "PATCH",
                &format!("/api/libraries/{STARTING_SHOWS}"),
                &owner,
                serde_json::json!({"name": "Series"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let text = read_until(&mut body, "event: libraries").await;
        assert!(text.contains("data: {}"), "{text}");
    }

    /// A download that leaves the caller's view is announced as removed, with nothing
    /// more about it.
    #[tokio::test]
    async fn a_download_that_leaves_view_is_removed() {
        let app = TestApp::new().await;
        let (owner_id, owner) = app.user("owner").await;
        let (_, member) = app.user("member").await;
        let item = app
            .item(
                "Movies/Heat/Heat.mkv",
                MediaKind::Video,
                Category::Movie,
                ItemStatus::Ready,
                None,
            )
            .await;
        sqlx::query(
            "INSERT INTO downloads (id, kind, name, source, state, created_by, created_at, updated_at)
             VALUES ('dl', 'torrent', 'Heat', 'magnet:?xt=urn:btih:a', 'sorted', ?, 1, 1)",
        )
        .bind(&owner_id)
        .execute(&app.state.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO download_items (download_id, item_id) VALUES ('dl', ?)")
            .bind(&item)
            .execute(&app.state.db)
            .await
            .unwrap();
        let (_, listed) = app.json("/api/downloads", &member).await;
        assert_eq!(listed["items"][0]["id"], "dl", "{listed}");
        let mut body = open(&app, &member).await;
        let (status, json) = app
            .send_json(
                "PATCH",
                &format!("/api/libraries/{STARTING_MOVIES}"),
                &owner,
                serde_json::json!({"visibility": "selected"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let text = read_until(&mut body, r#""removed":true"#).await;
        assert!(
            text.contains(r#"data: {"id":"dl","removed":true}"#),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                r#"data: {{"item_id":"{item}","library_id":null,"type":"removed"}}"#
            )),
            "{text}"
        );
        let (_, listed) = app.json("/api/downloads", &member).await;
        assert_eq!(listed["items"], serde_json::json!([]));
    }

    /// Organize moves an item into a library the member cannot see: they are told it was
    /// removed (naming no library), and its download, which showed nothing else, goes too.
    #[tokio::test]
    async fn an_item_moved_out_of_view_takes_its_download_along() {
        let app = TestApp::new().await;
        let (owner_id, owner) = app.user("owner").await;
        let (_, member) = app.user("member").await;
        let hidden = hidden_library(&app, &owner).await;
        let item = app
            .item(
                "Home Videos/2024/06/beach.mp4",
                MediaKind::Video,
                Category::HomeVideo,
                ItemStatus::Ready,
                Some(b"beach"),
            )
            .await;
        sqlx::query(
            "INSERT INTO downloads (id, kind, name, source, state, created_by, created_at, updated_at)
             VALUES ('dl', 'upload', 'beach.mp4', 'beach.mp4', 'sorted', ?, 1, 1)",
        )
        .bind(&owner_id)
        .execute(&app.state.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO download_items (download_id, item_id) VALUES ('dl', ?)")
            .bind(&item)
            .execute(&app.state.db)
            .await
            .unwrap();
        let mut body = open(&app, &member).await;
        let (status, json) = app
            .send_json(
                "POST",
                &format!("/api/items/{item}/organize"),
                &owner,
                serde_json::json!({"place": "library", "library_id": hidden}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["library_id"], hidden.as_str());
        let text = read_until(&mut body, r#""removed":true"#).await;
        assert!(
            text.contains(&format!(
                r#"data: {{"item_id":"{item}","library_id":null,"type":"removed"}}"#
            )),
            "{text}"
        );
        assert!(!text.contains(&hidden), "{text}");
        assert!(
            text.contains(r#"data: {"id":"dl","removed":true}"#),
            "{text}"
        );
    }

    /// Making a member an owner shows on their open stream at once, not at the 60 s re-check.
    #[tokio::test]
    async fn a_role_change_reaches_open_streams_at_once() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (member_id, member) = app.user("member").await;
        hidden_library(&app, &owner).await;
        let mut body = open(&app, &member).await;
        let (status, json) = app
            .send_json(
                "PATCH",
                &format!("/api/users/{member_id}"),
                &owner,
                serde_json::json!({"role": "owner"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        read_until(&mut body, "event: libraries").await;
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
            let conn = Conn::open(app.state.clone(), user).await.unwrap();
            let stream = event_stream(conn, &parts.headers, Duration::from_millis(50));
            let mut stream = Box::pin(stream);
            // Let the stream subscribe before publishing.
            let first = stream.next();
            app.state.events.publish(Event::Inbox { count: 1 });
            let first = tokio::time::timeout(Duration::from_secs(5), first)
                .await
                .expect("an event arrives");
            assert!(first.is_some(), "{revoke}");
            sqlx::query(revoke).execute(&app.state.db).await.unwrap();
            let end = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(_event) = stream.next().await {}
            })
            .await;
            assert!(end.is_ok(), "{revoke}: stream stayed open");
        }
    }

    /// Deleting a person closes their streams.
    #[tokio::test]
    async fn deleting_a_person_closes_their_streams() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (member_id, member) = app.user("member").await;
        let mut body = open(&app, &member).await;
        up_to_marker(&app, &mut body, "warm").await;
        let (status, _, _) = app
            .send(request(
                "DELETE",
                &format!("/api/users/{member_id}"),
                Some(&owner),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let end = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(Ok(_)) = body.frame().await {}
        })
        .await;
        assert!(end.is_ok(), "the stream stayed open");
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
