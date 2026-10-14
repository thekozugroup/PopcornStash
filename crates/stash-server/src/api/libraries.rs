//! /api routes: Home and the libraries the owner defines (see docs/architecture.md
//! sections 7 and 8, Libraries).
//!
//! Every `/api/libraries/:id` route answers 404 for a library the caller cannot see
//! before it looks at anything else, so a hidden library's existence and type cannot be
//! probed.

use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use stash_core::CoreError;
use stash_core::library::access::Access;
use stash_core::library::defs::{
    self, CreateLibrary, EditLibrary, LIBRARY_ICONS, LibraryView, Shape, UnassignedFolder,
};
use stash_core::library::repo::{self, Home, PhotoTimeline, ShowDetail, ShowSummary};

use super::items::{JsonBody, parse_query};
use crate::auth::{AuthUser, Scope};
use crate::error::{ApiError, ApiResult};
use crate::security::ClientIp;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/home", get(home))
        .route("/api/libraries", get(list).post(create))
        .route("/api/libraries/icons", get(icons))
        .route("/api/libraries/folders", get(folders))
        .route("/api/libraries/order", put(order))
        .route("/api/libraries/{id}", get(one).patch(edit).delete(remove))
        .route("/api/libraries/{id}/shows", get(shows))
        .route("/api/libraries/{id}/shows/{name}", get(show))
        .route("/api/libraries/{id}/timeline", get(timeline))
}

/// The library `id` when the caller sees it, else the same 404 as for one that never
/// existed.
async fn visible_library(
    state: &AppState,
    access: &Access,
    id: &str,
) -> ApiResult<defs::LibraryDef> {
    if !access.sees_library(id) {
        return Err(ApiError::library_not_found());
    }
    defs::load_one(&state.db, id)
        .await?
        .ok_or_else(ApiError::library_not_found)
}

async fn home(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Home>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    Ok(Json(repo::home(&state.db, &state.layout, &access).await?))
}

#[derive(Debug, Serialize)]
struct LibraryList {
    libraries: Vec<LibraryView>,
}

async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<LibraryList>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    Ok(Json(LibraryList {
        libraries: defs::views(&state.db, &state.layout, &access).await?,
    }))
}

async fn one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<LibraryView>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    visible_library(&state, &access, &id).await?;
    Ok(Json(
        defs::view(&state.db, &state.layout, &access, &id).await?,
    ))
}

#[derive(Debug, Serialize)]
struct Icons {
    icons: &'static [&'static str],
}

async fn icons(user: AuthUser) -> ApiResult<Json<Icons>> {
    user.require(Scope::LibraryRead)?;
    Ok(Json(Icons {
        icons: LIBRARY_ICONS,
    }))
}

#[derive(Debug, Serialize)]
struct Folders {
    folders: Vec<UnassignedFolder>,
}

async fn folders(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Folders>> {
    user.require_owner()?;
    Ok(Json(Folders {
        folders: defs::unassigned_folders(&state.library).await?,
    }))
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    JsonBody(body): JsonBody<CreateLibrary>,
) -> ApiResult<(StatusCode, Json<LibraryView>)> {
    user.require_owner()?;
    let written = state.library.create_library(body, &user.user_id).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "library_created",
            serde_json::json!({"library_id": written.id, "name": written.name, "folder": written.folder}),
            ip.as_string().as_deref(),
        )
        .await;
    let access = user.access(&state.db).await?;
    Ok((
        StatusCode::CREATED,
        Json(defs::view(&state.db, &state.layout, &access, &written.id).await?),
    ))
}

async fn edit(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<EditLibrary>,
) -> ApiResult<Json<LibraryView>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    visible_library(&state, &access, &id).await?;
    user.require_owner()?;
    let written = state.library.edit_library(&id, body).await?;
    if !written.changed.is_empty() {
        state
            .auth
            .audit(
                Some(&user.user_id),
                "library_changed",
                serde_json::json!({"library_id": id, "name": written.name, "fields": written.changed}),
                ip.as_string().as_deref(),
            )
            .await;
    }
    let access = user.access(&state.db).await?;
    Ok(Json(
        defs::view(&state.db, &state.layout, &access, &id).await?,
    ))
}

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    visible_library(&state, &access, &id).await?;
    user.require_owner()?;
    let written = state.library.delete_library(&id).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "library_deleted",
            serde_json::json!({"library_id": id, "name": written.name, "folder": written.folder}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct OrderBody {
    ids: Vec<String>,
}

async fn order(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    JsonBody(body): JsonBody<OrderBody>,
) -> ApiResult<Json<LibraryList>> {
    user.require_owner()?;
    state.library.reorder_libraries(&body.ids).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "libraries_reordered",
            serde_json::json!({"ids": body.ids}),
            ip.as_string().as_deref(),
        )
        .await;
    let access = user.access(&state.db).await?;
    Ok(Json(LibraryList {
        libraries: defs::views(&state.db, &state.layout, &access).await?,
    }))
}

async fn shows(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ShowSummary>>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    let library = visible_library(&state, &access, &id).await?;
    if library.shape() != Shape::Episodes {
        return Err(ApiError::bad_request(
            "Only a library of episodes has shows.",
        ));
    }
    Ok(Json(repo::shows(&state.db, &id).await?))
}

async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, name)): Path<(String, String)>,
) -> ApiResult<Json<ShowDetail>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    let library = visible_library(&state, &access, &id).await?;
    let not_found = || ApiError::not_found("That show is not in this library.");
    if library.shape() != Shape::Episodes {
        return Err(not_found());
    }
    // `Path` has already percent-decoded the segment, so "Doctor%20Who" arrives as "Doctor Who".
    if name.trim().is_empty() || name.chars().count() > 500 {
        return Err(not_found());
    }
    // The exact name first: links are built from `/shows`, and a stored name can carry
    // spaces (a "Bluey " folder). Then the trimmed form, for typed addresses.
    let mut candidates = vec![name.as_str()];
    if name.trim() != name {
        candidates.push(name.trim());
    }
    for candidate in candidates {
        match repo::show(&state.db, &id, candidate, &user.user_id).await {
            Ok(detail) if detail.seasons.iter().any(|s| !s.episodes.is_empty()) => {
                return Ok(Json(detail));
            }
            Ok(_) | Err(CoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(not_found())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TimelineQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn timeline(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    uri: Uri,
) -> ApiResult<Json<PhotoTimeline>> {
    user.require(Scope::LibraryRead)?;
    let access = user.access(&state.db).await?;
    let library = visible_library(&state, &access, &id).await?;
    if library.shape() != Shape::Photos {
        return Err(ApiError::bad_request(
            "Only a photos library has a timeline.",
        ));
    }
    let query: TimelineQuery = parse_query(&uri)?;
    // Section 11: 200 photos per page unless the client asks, never more than 500.
    let limit = query
        .limit
        .unwrap_or(repo::DEFAULT_TIMELINE_LIMIT)
        .clamp(1, repo::MAX_TIMELINE_LIMIT);
    Ok(Json(
        repo::photo_timeline(
            &state.db,
            &id,
            query.cursor.as_deref(),
            limit,
            &user.user_id,
        )
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use stash_core::library::defs::{
        STARTING_HOME_VIDEOS, STARTING_MOVIES, STARTING_PHOTOS, STARTING_SHOWS,
    };
    use stash_core::models::{Category, ItemStatus, MediaKind};

    use crate::api::library::testing::{TestApp, request};

    fn kids(folder: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"name": "Kids", "icon": "baby", "content_type": "video",
            "structure": "one_offs", "claims": ["home_video"], "folder": folder})
    }

    async fn refused(
        app: &TestApp,
        method: &str,
        uri: &str,
        token: &str,
        body: serde_json::Value,
        status: StatusCode,
        field: Option<&str>,
    ) {
        let (got, json) = app.send_json(method, uri, token, body.clone()).await;
        assert_eq!(got, status, "{body}: {json}");
        assert_eq!(json["error"]["field"].as_str(), field, "{body}: {json}");
    }

    #[tokio::test]
    async fn old_fixed_category_routes_are_gone() {
        let app = TestApp::new().await;
        let (_, token) = app.user("owner").await;
        for route in [
            "/api/library/summary",
            "/api/shows",
            "/api/shows/Bluey",
            "/api/photos/timeline",
        ] {
            let (status, _, _) = app.get(route, Some(&token)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{route}");
        }
    }

    #[tokio::test]
    async fn libraries_are_created_changed_reordered_and_deleted() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (member_id, member) = app.user("member").await;
        let (owner2_id, _) = app.user("owner").await;

        let (status, json) = app.json("/api/libraries", &owner).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = json["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Movies", "Shows", "Home Videos", "Photos"]);
        let movies = &json["libraries"][0];
        assert_eq!(movies["layout"], "posters");
        assert_eq!(movies["claims"], serde_json::json!(["movie"]));
        assert_eq!(movies["folder"], "Movies");
        assert_eq!(movies["folder_state"], "ok");
        assert_eq!(movies["is_empty"], true);
        assert_eq!(json["libraries"][1]["layout"], "series");
        assert_eq!(json["libraries"][2]["layout"], "frames");
        assert_eq!(json["libraries"][3]["layout"], "timeline");
        assert_eq!(json["libraries"][3]["structure"], serde_json::Value::Null);

        let (status, json) = app.json("/api/libraries/icons", &member).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["icons"][0], "film");
        assert_eq!(json["icons"].as_array().unwrap().len(), 32);

        // Only owners write.
        refused(
            &app,
            "POST",
            "/api/libraries",
            &member,
            kids(serde_json::json!({"mode": "new", "name": "Kids"})),
            StatusCode::FORBIDDEN,
            None,
        )
        .await;
        let (status, _) = app.json("/api/libraries/folders", &member).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Each refusal names its field.
        let mut body = kids(serde_json::json!({"mode": "new", "name": "Kids"}));
        body["name"] = "Movies".into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::CONFLICT,
            Some("name"),
        )
        .await;
        body["name"] = "inbox".into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::CONFLICT,
            Some("name"),
        )
        .await;
        body["name"] = "x".repeat(41).into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("name"),
        )
        .await;
        body["name"] = "Kids".into();
        body["icon"] = "house".into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("icon"),
        )
        .await;
        body["icon"] = "baby".into();
        body["structure"] = serde_json::Value::Null;
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("structure"),
        )
        .await;
        body["structure"] = "one_offs".into();
        body["content_type"] = "music".into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("content_type"),
        )
        .await;
        body["content_type"] = "video".into();
        body["claims"] = serde_json::json!(["photo"]);
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("claims"),
        )
        .await;
        body["claims"] = serde_json::json!(["home_video"]);
        body["folder"] = serde_json::json!({"mode": "new", "name": "Downloads"});
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("folder.name"),
        )
        .await;
        body["folder"] = serde_json::json!({"mode": "new", "name": "inbox"});
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("folder.name"),
        )
        .await;
        body["folder"] = serde_json::json!({"mode": "new", "name": "movies"});
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::CONFLICT,
            Some("folder.name"),
        )
        .await;
        body["folder"] = serde_json::json!({"mode": "existing", "name": "Nowhere"});
        body["visibility"] = "everyone".into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::NOT_FOUND,
            Some("folder.name"),
        )
        .await;
        std::fs::create_dir_all(app.state.config.stash_root.join("Cartoons")).unwrap();
        body["folder"] = serde_json::json!({"mode": "existing", "name": "Cartoons"});
        body["visibility"] = serde_json::Value::Null;
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("visibility"),
        )
        .await;
        body["folder"] = serde_json::json!({"mode": "existing", "name": "Movies"});
        body["visibility"] = "everyone".into();
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::CONFLICT,
            Some("folder.name"),
        )
        .await;
        body["folder"] = serde_json::json!({"mode": "new", "name": "Kids"});
        body["visibility"] = "selected".into();
        body["member_ids"] = serde_json::json!([owner2_id]);
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            body.clone(),
            StatusCode::BAD_REQUEST,
            Some("member_ids"),
        )
        .await;

        // Folders that are not a library, with "Make a library".
        app.item(
            "Cartoons/Bluey/S01E01.mkv",
            MediaKind::Video,
            Category::Episode,
            ItemStatus::Ready,
            None,
        )
        .await;
        let (status, json) = app.json("/api/libraries/folders", &owner).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(
            json["folders"],
            serde_json::json!([{"name": "Cartoons", "item_count": 1, "video_count": 1, "photo_count": 0,
                "other_count": 0, "suggested": {"content_type": "video", "structure": "episodes"}}])
        );

        // Created: the claim moves, the folder is made, the member is listed.
        body["member_ids"] = serde_json::json!([member_id]);
        body["position"] = 1.into();
        let (status, created) = app.send_json("POST", "/api/libraries", &owner, body).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["position"], 1);
        assert_eq!(created["member_ids"], serde_json::json!([member_id]));
        assert!(app.state.config.stash_root.join("Kids").is_dir());
        let (_, home_videos) = app
            .json(&format!("/api/libraries/{STARTING_HOME_VIDEOS}"), &owner)
            .await;
        assert_eq!(home_videos["claims"], serde_json::json!([]));
        let (_, seen) = app.json(&format!("/api/libraries/{id}"), &member).await;
        assert_eq!(seen["name"], "Kids", "{seen}");
        assert!(seen["member_ids"].is_null() && seen["folder"].is_null());

        // Edits: a fixed folder; the type changes only while empty.
        refused(
            &app,
            "PATCH",
            &format!("/api/libraries/{id}"),
            &owner,
            serde_json::json!({"folder": {"name": "Elsewhere"}}),
            StatusCode::BAD_REQUEST,
            Some("folder"),
        )
        .await;
        refused(
            &app,
            "PATCH",
            &format!("/api/libraries/{id}"),
            &member,
            serde_json::json!({"name": "Mine"}),
            StatusCode::FORBIDDEN,
            None,
        )
        .await;
        let (status, json) = app.send_json("PATCH", &format!("/api/libraries/{id}"), &owner,
            serde_json::json!({"content_type": "photos", "structure": null, "claims": ["photo"]})).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["layout"], "timeline");
        app.item(
            "Kids/2024/a.jpg",
            MediaKind::Photo,
            Category::Photo,
            ItemStatus::Ready,
            None,
        )
        .await;
        refused(
            &app,
            "PATCH",
            &format!("/api/libraries/{id}"),
            &owner,
            serde_json::json!({"content_type": "video", "structure": "one_offs"}),
            StatusCode::CONFLICT,
            Some("content_type"),
        )
        .await;
        let (_, photos) = app
            .json(&format!("/api/libraries/{STARTING_PHOTOS}"), &owner)
            .await;
        assert_eq!(
            photos["claims"],
            serde_json::json!([]),
            "the photo claim moved"
        );

        // Order.
        refused(
            &app,
            "PUT",
            "/api/libraries/order",
            &owner,
            serde_json::json!({"ids": [STARTING_MOVIES]}),
            StatusCode::BAD_REQUEST,
            Some("ids"),
        )
        .await;
        let order = [
            id.as_str(),
            STARTING_PHOTOS,
            STARTING_HOME_VIDEOS,
            STARTING_SHOWS,
            STARTING_MOVIES,
        ];
        let (status, json) = app
            .send_json(
                "PUT",
                "/api/libraries/order",
                &owner,
                serde_json::json!({"ids": order}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        let got: Vec<&str> = json["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["id"].as_str().unwrap())
            .collect();
        assert_eq!(got, order);

        // Delete: the files stay; the folder is no longer a library.
        let (status, _, _) = app
            .send(request(
                "DELETE",
                &format!("/api/libraries/{id}"),
                Some(&owner),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(app.state.config.stash_root.join("Kids").is_dir());
        let (status, _) = app.json(&format!("/api/libraries/{id}"), &owner).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (_, json) = app.json("/api/libraries/folders", &owner).await;
        assert!(
            json["folders"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["name"] == "Kids"),
            "{json}"
        );
        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM audit_log WHERE event IN ('library_created', 'library_changed', 'libraries_reordered', 'library_deleted')",
        )
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        assert_eq!(count, 4);
    }

    #[tokio::test]
    async fn a_stash_has_at_most_24_libraries() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        for n in 0..20 {
            let (status, json) = app
                .send_json(
                    "POST",
                    "/api/libraries",
                    &owner,
                    serde_json::json!({
                "name": format!("Library {n}"), "icon": "star", "content_type": "photos",
                "folder": {"mode": "new", "name": format!("Library {n}")}}),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED, "{json}");
        }
        refused(
            &app,
            "POST",
            "/api/libraries",
            &owner,
            serde_json::json!({
            "name": "One more", "icon": "star", "content_type": "photos",
            "folder": {"mode": "new", "name": "One more"}}),
            StatusCode::CONFLICT,
            None,
        )
        .await;
        assert!(!app.state.config.stash_root.join("One more").exists());
    }

    #[tokio::test]
    async fn shows_and_timeline_belong_to_their_library() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        app.item(
            "Shows/Bluey/Season 01/Bluey - S01E01.mkv",
            MediaKind::Video,
            Category::Episode,
            ItemStatus::Ready,
            None,
        )
        .await;
        app.item(
            "Shows/Bluey /Season 01/Bluey - S01E02.mkv",
            MediaKind::Video,
            Category::Episode,
            ItemStatus::Ready,
            None,
        )
        .await;
        sqlx::query("UPDATE items SET show_name = 'Bluey', season = 1, episode = 1 WHERE rel_path LIKE 'Shows/Bluey/%'")
            .execute(&app.state.db).await.unwrap();
        sqlx::query("UPDATE items SET show_name = 'Bluey ', season = 1, episode = 2 WHERE rel_path LIKE 'Shows/Bluey /%'")
            .execute(&app.state.db).await.unwrap();
        let (status, json) = app
            .json(&format!("/api/libraries/{STARTING_SHOWS}/shows"), &token)
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json.as_array().unwrap().len(), 2);
        let (status, json) = app
            .json(
                &format!("/api/libraries/{STARTING_SHOWS}/shows/Bluey"),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        // Stored names with stray spaces still belong to the show.
        assert_eq!(json["seasons"][0]["episodes"].as_array().unwrap().len(), 2);
        // The web links to the name exactly as listed, spaces and all.
        let (status, json) = app
            .json(
                &format!("/api/libraries/{STARTING_SHOWS}/shows/Bluey%20"),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["show_name"], "Bluey ");
        let (status, json) = app
            .json(
                &format!("/api/libraries/{STARTING_SHOWS}/shows/Nope"),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            json["error"]["message"],
            "That show is not in this library."
        );
        let (status, _) = app
            .json(
                &format!("/api/libraries/{STARTING_SHOWS}/shows/%20%20"),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // Shows and timelines only where they make sense.
        let (status, _) = app
            .json(&format!("/api/libraries/{STARTING_MOVIES}/shows"), &token)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = app
            .json(
                &format!("/api/libraries/{STARTING_MOVIES}/timeline"),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// A client that does not send `limit` gets the documented 200 photos per page, and
    /// no page holds more than 500 whatever it asks for.
    #[tokio::test]
    async fn timeline_pages_hold_200_photos_by_default_and_500_at_most() {
        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let now = stash_core::now();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 600)
             INSERT INTO items (id, rel_path, kind, category, status, title, sort_title, size, mtime,
                                thumb_state, taken_at, added_at, updated_at)
             SELECT printf('photo-%04d', i), printf('Photos/2024/06/IMG_%04d.jpg', i), 'photo', 'photo',
                    'ready', printf('IMG_%04d', i), printf('img_%04d', i), 10, ?, 'pending',
                    1718236800 + i, ?, ? FROM n",
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&app.state.db)
        .await
        .unwrap();
        let photos = |json: &serde_json::Value| -> usize {
            json["months"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["items"].as_array().unwrap().len())
                .sum()
        };
        let base = format!("/api/libraries/{STARTING_PHOTOS}/timeline");
        let (status, json) = app.json(&base, &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(photos(&json), 200);
        assert_eq!(json["months"][0]["count"], 600);
        assert!(json["next_cursor"].is_string());
        let (status, json) = app.json(&format!("{base}?limit=1000"), &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(photos(&json), 500);
    }
}
