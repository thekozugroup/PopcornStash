//! The access matrix (docs/architecture.md section 7, Library access): a person who
//! cannot see a library learns nothing about its items through any route. Every item
//! route, list, count, stream and event is checked for a member without access, and
//! for the owner credentials that are not full owners.

use std::time::Duration;

use axum::http::StatusCode;
use http_body_util::BodyExt;
use stash_core::events::{Event, LibraryChange};
use stash_core::library::defs::{
    STARTING_HOME_VIDEOS, STARTING_MOVIES, STARTING_PHOTOS, STARTING_SHOWS,
};
use stash_core::media::ThumbSize;
use stash_core::models::{Category, ItemStatus, MediaKind, VpnStatus};
use tower::ServiceExt;

use super::library::testing::{TestApp, request};

/// A short H.264 + AAC clip in a container browsers do not play (so playback is HLS),
/// or None when ffmpeg is not installed.
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

const NOT_FOUND_ITEM: &str = "That item no longer exists.";
const NOT_FOUND_LIBRARY: &str = "That library no longer exists.";

struct World {
    app: TestApp,
    owner_id: String,
    owner: String,
    member_id: String,
    member: String,
    /// In libraries the member cannot see, in Other files and folders not in a
    /// library, waiting for a hidden library, and in the trash.
    hidden: Vec<(&'static str, String)>,
    /// In Home Videos, which everyone sees.
    visible_clip: String,
    hidden_movie: String,
    hidden_photo: String,
    hidden_waiting: String,
    visible_waiting: String,
}

async fn world() -> World {
    let app = TestApp::new().await;
    let (owner_id, owner) = app.user("owner").await;
    let (member_id, member) = app.user("member").await;
    // Movies, Shows and Photos are shown to selected people only, and nobody is selected.
    for id in [STARTING_MOVIES, STARTING_SHOWS, STARTING_PHOTOS] {
        let (status, json) = app
            .send_json(
                "PATCH",
                &format!("/api/libraries/{id}"),
                &owner,
                serde_json::json!({"visibility": "selected", "member_ids": []}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
    }
    let root = app.state.config.stash_root.clone();
    let movie_rel = "Movies/Secret Movie (2001)/Secret Movie (2001).mkv";
    let have_clip = make_clip(&root.join(movie_rel), 4).is_some();
    let hidden_movie = app
        .item(
            movie_rel,
            MediaKind::Video,
            Category::Movie,
            ItemStatus::Ready,
            (!have_clip).then_some(&b"secret movie"[..]),
        )
        .await;
    let hidden_photo = app
        .item(
            "Photos/2024/06/secret.jpg",
            MediaKind::Photo,
            Category::Photo,
            ItemStatus::Ready,
            Some(b"\xff\xd8\xff\xe0secret photo"),
        )
        .await;
    let hidden_episode = app
        .item(
            "Shows/Secret Show/Season 01/Secret Show - S01E01.mkv",
            MediaKind::Video,
            Category::Episode,
            ItemStatus::Ready,
            Some(b"secret episode"),
        )
        .await;
    sqlx::query("UPDATE items SET show_name = 'Secret Show', season = 1, episode = 1 WHERE id = ?")
        .bind(&hidden_episode)
        .execute(&app.state.db)
        .await
        .unwrap();
    let owners_file = app
        .item(
            "Other/secret-recipe.pdf",
            MediaKind::Other,
            Category::Other,
            ItemStatus::Ready,
            Some(b"%PDF secret"),
        )
        .await;
    app.added_by(&owners_file, &owner_id).await;
    let folder_item = app
        .item(
            "Family Secrets/secret-clip.mkv",
            MediaKind::Video,
            Category::HomeVideo,
            ItemStatus::Ready,
            Some(b"secret folder"),
        )
        .await;
    let hidden_waiting = app
        .item(
            "Inbox/Secret.Waiting.2009.mkv",
            MediaKind::Video,
            Category::Movie,
            ItemStatus::NeedsReview,
            Some(b"secret waiting"),
        )
        .await;
    let owners_upload = app
        .item(
            "Downloads/Completed/secret-upload.mp4",
            MediaKind::Video,
            Category::Movie,
            ItemStatus::Ready,
            Some(b"ignored upload"),
        )
        .await;
    app.added_by(&owners_upload, &owner_id).await;
    let trashed = app
        .item(
            ".popcornstash/Trash/x/secret-trashed.mkv",
            MediaKind::Video,
            Category::Movie,
            ItemStatus::Trashed,
            Some(b"secret trashed"),
        )
        .await;
    sqlx::query(
        "INSERT INTO trash (item_id, original_rel_path, trashed_rel_path, trashed_at) VALUES (?, 'Movies/x.mkv', '.popcornstash/Trash/x/secret-trashed.mkv', 1)",
    )
    .bind(&trashed)
    .execute(&app.state.db)
    .await
    .unwrap();
    let visible_clip = app
        .item(
            "Home Videos/2024/06/beach.mp4",
            MediaKind::Video,
            Category::HomeVideo,
            ItemStatus::Ready,
            Some(b"beach"),
        )
        .await;
    let visible_waiting = app
        .item(
            "Inbox/VID_20240613_101500.mp4",
            MediaKind::Video,
            Category::HomeVideo,
            ItemStatus::NeedsReview,
            Some(b"waiting clip"),
        )
        .await;
    // Every item is a favourite with a thumbnail, and the member has watched each.
    for id in [
        &hidden_movie,
        &hidden_photo,
        &hidden_episode,
        &owners_file,
        &folder_item,
        &hidden_waiting,
        &owners_upload,
        &trashed,
        &visible_clip,
    ] {
        sqlx::query(
            "UPDATE items SET favorite = 1, thumb_state = 'ready', size = 1000 WHERE id = ?",
        )
        .bind(id)
        .execute(&app.state.db)
        .await
        .unwrap();
        let thumb = app.state.media.thumb_path(id, ThumbSize::Sm);
        std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
        std::fs::write(&thumb, b"RIFF....WEBP").unwrap();
        sqlx::query(
            "INSERT INTO item_progress (user_id, item_id, position_s, duration_s, finished, updated_at)
             VALUES (?, ?, 120, 3600, 0, 1)",
        )
        .bind(&member_id)
        .bind(id)
        .execute(&app.state.db)
        .await
        .unwrap();
    }
    World {
        app,
        owner_id,
        owner,
        member_id,
        member,
        hidden: vec![
            ("movie in a hidden library", hidden_movie.clone()),
            ("photo in a hidden library", hidden_photo.clone()),
            ("episode in a hidden library", hidden_episode),
            ("someone else's Other file", owners_file),
            ("folder not in a library", folder_item),
            ("waiting for a hidden library", hidden_waiting.clone()),
            ("someone else's ignored upload", owners_upload),
            ("trashed", trashed),
        ],
        visible_clip,
        hidden_movie,
        hidden_photo,
        hidden_waiting,
        visible_waiting,
    }
}

fn ids(json: &serde_json::Value, key: &str) -> Vec<String> {
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} in {json}"))
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

/// The same 404 body as an item that never existed.
fn assert_item_not_found(what: &str, route: &str, status: StatusCode, body: &[u8]) {
    assert_eq!(status, StatusCode::NOT_FOUND, "{what}: {route}");
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    assert_eq!(
        json["error"]["code"], "not_found",
        "{what}: {route}: {json}"
    );
}

#[tokio::test]
async fn a_member_learns_nothing_about_items_they_cannot_see() {
    let w = world().await;
    let app = &w.app;
    let caps = serde_json::json!({"video_codecs": ["h264"], "audio_codecs": ["aac"], "hdr": false});

    // Every single-item route answers exactly as for an id that never existed.
    let (status, _, never) = app.get("/api/items/no-such-item", Some(&w.member)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let never: serde_json::Value = serde_json::from_slice(&never).unwrap();
    assert_eq!(never["error"]["message"], NOT_FOUND_ITEM);
    for (what, id) in &w.hidden {
        let routes: Vec<(&str, String, Option<serde_json::Value>)> = vec![
            ("GET", format!("/api/items/{id}"), None),
            ("GET", format!("/api/items/{id}/thumb?size=sm"), None),
            ("GET", format!("/api/items/{id}/thumb?size=sm&v=1"), None),
            ("GET", format!("/api/items/{id}/file"), None),
            ("GET", format!("/api/items/{id}/file?download=1"), None),
            ("GET", format!("/api/items/{id}/view"), None),
            (
                "POST",
                format!("/api/items/{id}/playback"),
                Some(caps.clone()),
            ),
            (
                "PUT",
                format!("/api/items/{id}/progress"),
                Some(serde_json::json!({"position_s": 999, "duration_s": 3600})),
            ),
            (
                "PATCH",
                format!("/api/items/{id}"),
                Some(serde_json::json!({"favorite": false})),
            ),
            (
                "POST",
                format!("/api/inbox/{id}"),
                Some(serde_json::json!({"action": "ignore"})),
            ),
            (
                "POST",
                format!("/api/inbox/{id}"),
                Some(serde_json::json!({"action": "accept", "place": "other_files"})),
            ),
        ];
        for (method, route, body) in routes {
            let (status, _, bytes) = app
                .send(request(method, &route, Some(&w.member), body))
                .await;
            assert_item_not_found(what, &route, status, &bytes);
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["error"]["message"], NOT_FOUND_ITEM, "{what}: {route}");
        }
        // Organize is for full owners anyway; nothing moves.
        let (status, _, _) = app
            .send(request(
                "POST",
                &format!("/api/items/{id}/organize"),
                Some(&w.member),
                Some(serde_json::json!({"place": "other_files"})),
            ))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{what}");
    }
    // Nothing above changed a favourite or saved a position.
    let (favourites, saved): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM items WHERE favorite = 1),
                (SELECT count(*) FROM item_progress WHERE position_s = 999)",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!((favourites, saved), (9, 0));

    // Lists, search, favourites and filters cover visible items only.
    for route in [
        "/api/items",
        "/api/items?q=secret",
        "/api/items?favorite=true",
        "/api/items?sort=title&order=asc",
        "/api/items?category=movie",
        "/api/items?category=photo",
        "/api/items?category=episode",
        "/api/items?category=other",
        "/api/items?place=other_files",
        "/api/items?place=folder",
        "/api/items?place=incoming",
        "/api/items?status=needs_review",
        "/api/items?status=missing",
        "/api/items?year=2001",
    ] {
        let (status, json) = app.json(route, &w.member).await;
        assert_eq!(status, StatusCode::OK, "{route}: {json}");
        let listed = ids(&json, "items");
        for (what, id) in &w.hidden {
            assert!(!listed.contains(id), "{what} listed by {route}");
        }
        assert!(json["next_cursor"].is_null(), "{route}");
    }
    let (_, json) = app.json("/api/items", &w.member).await;
    assert_eq!(ids(&json, "items"), vec![w.visible_clip.clone()]);
    let (_, json) = app.json("/api/items?status=needs_review", &w.member).await;
    assert_eq!(ids(&json, "items"), vec![w.visible_waiting.clone()]);
    // A list filtered by a hidden library is the same 404 as a library that never existed.
    for library in [STARTING_MOVIES, "no-such-library"] {
        let (status, json) = app
            .json(&format!("/api/items?library_id={library}"), &w.member)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
        assert_eq!(json["error"]["message"], NOT_FOUND_LIBRARY);
    }

    // Library routes answer 404 before looking at the type, the path or the query.
    for library in [
        STARTING_MOVIES,
        STARTING_SHOWS,
        STARTING_PHOTOS,
        "no-such-library",
    ] {
        for route in [
            format!("/api/libraries/{library}"),
            format!("/api/libraries/{library}/shows"),
            format!("/api/libraries/{library}/shows/Secret%20Show"),
            format!("/api/libraries/{library}/timeline"),
            format!("/api/libraries/{library}/timeline?cursor=garbage&limit=-1"),
        ] {
            let (status, json) = app.json(&route, &w.member).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{route}: {json}");
            assert_eq!(json["error"]["message"], NOT_FOUND_LIBRARY, "{route}");
        }
    }
    let (_, json) = app.json("/api/libraries", &w.member).await;
    let libraries: Vec<&str> = json["libraries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(libraries, vec![STARTING_HOME_VIDEOS]);
    assert_eq!(
        json["libraries"][0]["position"], 0,
        "no gap for hidden libraries"
    );
    assert!(
        json["libraries"][0]["folder"].is_null(),
        "folders are for full owners"
    );

    // Home: libraries, continue watching, counts and totals.
    let (status, home) = app.json("/api/home", &w.member).await;
    assert_eq!(status, StatusCode::OK, "{home}");
    let watched = ids(&home, "continue_watching");
    assert_eq!(watched, vec![w.visible_clip.clone()], "{home}");
    assert_eq!(home["libraries"].as_array().unwrap().len(), 1);
    assert_eq!(home["other_files_count"], 0);
    assert_eq!(home["inbox_count"], 1);
    assert_eq!(home["total_bytes"], 1000, "only the visible clip counts");
    assert!(!home.to_string().contains("ecret"), "{home}");

    // The Inbox.
    let (_, inbox) = app.json("/api/inbox", &w.member).await;
    assert_eq!(ids(&inbox, "items"), vec![w.visible_waiting.clone()]);
    assert_eq!(ids(&inbox, "ignored"), Vec::<String>::new());
    assert!(!inbox.to_string().contains("ecret"), "{inbox}");
    assert!(!inbox.to_string().contains(w.hidden_waiting.as_str()));

    // The trash is for full owners.
    let (status, _) = app.json("/api/trash", &w.member).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Downloads that only brought in hidden items, in every state.
    for (id, state, item) in [
        ("d-sorted", "sorted", Some(w.hidden_movie.clone())),
        ("d-active", "downloading", None),
        ("d-sorted-visible", "sorted", Some(w.visible_clip.clone())),
    ] {
        sqlx::query(
            "INSERT INTO downloads (id, kind, name, source, state, created_by, created_at, updated_at)
             VALUES (?, 'torrent', 'Secret Download', 'magnet:?xt=urn:btih:a', ?, ?, 1, 1)",
        )
        .bind(id)
        .bind(state)
        .bind(&w.owner_id)
        .execute(&app.state.db)
        .await
        .unwrap();
        if let Some(item) = item {
            sqlx::query("INSERT INTO download_items (download_id, item_id) VALUES (?, ?)")
                .bind(id)
                .bind(item)
                .execute(&app.state.db)
                .await
                .unwrap();
        }
    }
    let (_, json) = app.json("/api/downloads?state=all", &w.member).await;
    assert_eq!(ids(&json, "items"), vec!["d-sorted-visible".to_string()]);
    assert_eq!(
        json["items"][0]["item_ids"],
        serde_json::json!([w.visible_clip])
    );
    assert_eq!(
        json["items"][0]["destinations"],
        serde_json::json!([{"place": "library", "library_id": STARTING_HOME_VIDEOS, "count": 1}])
    );
    for id in ["d-sorted", "d-active"] {
        for (method, route) in [
            ("POST", format!("/api/downloads/{id}/pause")),
            ("POST", format!("/api/downloads/{id}/resume")),
            ("DELETE", format!("/api/downloads/{id}")),
        ] {
            let (status, _, _) = app
                .send(request(method, &route, Some(&w.member), None))
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{route}");
        }
    }
}

/// HLS sessions belong to the person who started them, and stop once that person can no
/// longer see the item.
#[tokio::test]
async fn hls_sessions_follow_access() {
    let w = world().await;
    let app = &w.app;
    let caps = serde_json::json!({"video_codecs": ["h264"], "audio_codecs": ["aac"], "hdr": false});
    if make_clip(&app.state.config.stash_root.join("Home Videos/clip.mkv"), 4).is_none() {
        eprintln!("skipping: ffmpeg with libx264 is not available");
        return;
    }
    let clip = app
        .item(
            "Home Videos/clip.mkv",
            MediaKind::Video,
            Category::HomeVideo,
            ItemStatus::Ready,
            None,
        )
        .await;
    // The owner's session for a movie the member cannot see.
    let (status, json) = app
        .send_json(
            "POST",
            &format!("/api/items/{}/playback", w.hidden_movie),
            &w.owner,
            caps.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["mode"], "hls", "{json}");
    let owners = json["session_id"].as_str().unwrap().to_string();
    for (method, route) in [
        ("GET", format!("/api/stream/{owners}/index.m3u8")),
        ("GET", format!("/api/stream/{owners}/0.ts")),
        ("DELETE", format!("/api/stream/{owners}")),
    ] {
        let (status, _, _) = app
            .send(request(method, &route, Some(&w.member), None))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{route}");
    }
    assert!(
        app.state.transcoder.session_owner(&owners).is_some(),
        "someone else's request does not stop the owner's session"
    );

    // The member's own session stops when Home Videos is hidden from them.
    let (status, json) = app
        .send_json(
            "POST",
            &format!("/api/items/{clip}/playback"),
            &w.member,
            caps,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let session = json["session_id"].as_str().unwrap().to_string();
    let (status, _, _) = app
        .get(
            &format!("/api/stream/{session}/index.m3u8"),
            Some(&w.member),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, json) = app
        .send_json(
            "PATCH",
            &format!("/api/libraries/{STARTING_HOME_VIDEOS}"),
            &w.owner,
            serde_json::json!({"visibility": "selected"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, _, _) = app
        .get(&format!("/api/stream/{session}/0.ts"), Some(&w.member))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        app.state.transcoder.session_owner(&session).is_none(),
        "the session was stopped"
    );
    let (status, _, _) = app
        .send(request(
            "PUT",
            &format!("/api/items/{clip}/progress"),
            Some(&w.member),
            Some(serde_json::json!({"position_s": 5, "duration_s": 10})),
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    app.state.transcoder.stop_all().await;
}

/// Live events: nothing about a hidden item reaches the member's stream.
#[tokio::test]
async fn events_about_hidden_items_never_reach_the_member() {
    let w = world().await;
    let app = &w.app;
    let response = app
        .router
        .clone()
        .oneshot(request("GET", "/api/events", Some(&w.member), None))
        .await
        .unwrap();
    let mut body = response.into_body();
    for (_, id) in &w.hidden {
        for change in [
            LibraryChange::Added,
            LibraryChange::Updated,
            LibraryChange::Removed,
        ] {
            app.state.events.publish(Event::Library {
                r#type: change,
                item_id: id.clone(),
                before: None,
            });
        }
        app.state.events.publish(Event::Job {
            kind: "thumb".into(),
            state: "done".into(),
            item_id: Some(id.clone()),
        });
    }
    app.state.events.publish(Event::Inbox { count: 9 });
    app.state.events.publish(Event::Job {
        kind: "thumb".into(),
        state: "done".into(),
        item_id: Some(w.visible_clip.clone()),
    });
    app.state
        .events
        .publish(Event::Vpn(VpnStatus::unreachable(false, "the-end")));
    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !seen.contains("the-end") {
        match tokio::time::timeout_at(deadline, body.frame()).await {
            Ok(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    seen.push_str(&String::from_utf8_lossy(data));
                }
            }
            other => panic!("stream ended early: {other:?} {seen}"),
        }
    }
    for (what, id) in &w.hidden {
        assert!(!seen.contains(id.as_str()), "{what}: {seen}");
    }
    assert!(
        seen.contains(&w.visible_clip),
        "the visible clip's job: {seen}"
    );
    assert!(
        seen.contains(r#"data: {"count":1}"#),
        "the member's own count: {seen}"
    );
}

/// An owner's limited token (a living-room TV with `library:read`) sees every library,
/// and otherwise only what a member would.
#[tokio::test]
async fn an_owner_tv_token_sees_libraries_but_not_private_files() {
    let w = world().await;
    let app = &w.app;
    let tv = app.device_token(&w.owner_id, &["library:read"]).await;
    let (_, json) = app.json("/api/libraries", &tv).await;
    let positions: Vec<i64> = json["libraries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["position"].as_i64().unwrap())
        .collect();
    assert_eq!(positions, vec![0, 1, 2, 3], "every library, in order");
    assert!(json["libraries"][0]["folder"].is_null(), "not a full owner");
    let (_, items) = app.json("/api/items", &tv).await;
    let listed = ids(&items, "items");
    assert!(listed.contains(&w.hidden_movie) && listed.contains(&w.hidden_photo));
    // Folders not in a library and the trash stay closed to it.
    for (what, id) in &w.hidden {
        let open = !matches!(*what, "folder not in a library" | "trashed");
        let (status, _, body) = app.get(&format!("/api/items/{id}"), Some(&tv)).await;
        if open {
            // In a library, waiting for one, or the owner's own Other file and upload.
            if *what != "trashed" && status == StatusCode::NOT_FOUND {
                panic!(
                    "{what} should be open to the owner's TV: {}",
                    String::from_utf8_lossy(&body)
                );
            }
        } else {
            assert_item_not_found(what, "detail", status, &body);
            let (status, _, body) = app.get(&format!("/api/items/{id}/thumb"), Some(&tv)).await;
            assert_item_not_found(what, "thumb", status, &body);
        }
    }
    let (_, json) = app.json("/api/items?place=folder", &tv).await;
    assert!(json["items"].as_array().unwrap().is_empty(), "{json}");
    let (status, _) = app.json("/api/trash", &tv).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A member's own Other file and upload stay theirs, even from the owner's TV.
    let mine = app
        .item(
            "Other/member-notes.pdf",
            MediaKind::Other,
            Category::Other,
            ItemStatus::Ready,
            Some(b"notes"),
        )
        .await;
    app.added_by(&mine, &w.member_id).await;
    let (status, _, _) = app.get(&format!("/api/items/{mine}"), Some(&tv)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = app
        .get(&format!("/api/items/{mine}"), Some(&w.member))
        .await;
    assert_eq!(status, StatusCode::OK, "the person who added it sees it");
    let (status, _, _) = app.get(&format!("/api/items/{mine}"), Some(&w.owner)).await;
    assert_eq!(status, StatusCode::OK, "full owners see everything");
}

/// An owner's Chrome token (`downloads:create downloads:read`) does not see another
/// person's upload or download in progress.
#[tokio::test]
async fn an_owner_chrome_token_sees_only_its_own_downloads() {
    let w = world().await;
    let app = &w.app;
    let chrome = app
        .device_token(&w.owner_id, &["downloads:create", "downloads:read"])
        .await;
    for (id, kind, state, by) in [
        ("member-upload", "upload", "completed", &w.member_id),
        ("member-torrent", "torrent", "downloading", &w.member_id),
        ("owner-torrent", "torrent", "downloading", &w.owner_id),
    ] {
        sqlx::query(
            "INSERT INTO downloads (id, kind, name, source, state, created_by, created_at, updated_at)
             VALUES (?, ?, 'x', 'x', ?, ?, 1, 1)",
        )
        .bind(id)
        .bind(kind)
        .bind(state)
        .bind(by)
        .execute(&app.state.db)
        .await
        .unwrap();
    }
    let (_, json) = app.json("/api/downloads?state=all", &chrome).await;
    assert_eq!(ids(&json, "items"), vec!["owner-torrent".to_string()]);
    let (_, json) = app.json("/api/downloads?state=all", &w.member).await;
    let mut mine = ids(&json, "items");
    mine.sort();
    assert_eq!(
        mine,
        vec!["member-torrent".to_string(), "member-upload".to_string()]
    );
    let (_, json) = app.json("/api/downloads?state=all", &w.owner).await;
    assert_eq!(
        json["items"].as_array().unwrap().len(),
        3,
        "full owners see all"
    );
}

/// A member with a hidden library in the middle of the order gets positions 0 to k-1.
#[tokio::test]
async fn positions_leave_no_gap_for_hidden_libraries() {
    let app = TestApp::new().await;
    let (_, owner) = app.user("owner").await;
    let (_, member) = app.user("member").await;
    let (status, _) = app
        .send_json(
            "PATCH",
            &format!("/api/libraries/{STARTING_SHOWS}"),
            &owner,
            serde_json::json!({"visibility": "selected"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, json) = app.json("/api/libraries", &member).await;
    let got: Vec<(String, i64)> = json["libraries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["id"].as_str().unwrap().to_string(),
                l["position"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (STARTING_MOVIES.to_string(), 0),
            (STARTING_HOME_VIDEOS.to_string(), 1),
            (STARTING_PHOTOS.to_string(), 2),
        ]
    );
}
