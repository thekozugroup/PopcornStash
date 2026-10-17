//! Explicit owner cleanup, including sidecars omitted from the media index.
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use stash_core::library::cleanup::{CleanupResult, FileChoice, FileTrash, Inventory};

use super::items::{JsonBody, parse_query};
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::security::ClientIp;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/files", get(list))
        .route("/api/files/trash", get(trash).post(remove))
        .route("/api/files/trash/{id}/restore", post(restore))
}

#[derive(Default, Deserialize)]
struct Filter {
    folder: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    uri: Uri,
) -> ApiResult<Json<Inventory>> {
    user.require_owner()?;
    let filter: Filter = parse_query(&uri)?;
    Ok(Json(
        state
            .library
            .cleanup_inventory(filter.folder.as_deref())
            .await?,
    ))
}

#[derive(Deserialize)]
struct Selection {
    files: Vec<FileChoice>,
}

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    JsonBody(body): JsonBody<Selection>,
) -> ApiResult<Json<CleanupResult>> {
    user.require_owner()?;
    let result = state
        .library
        .cleanup_files(&body.files, &user.user_id)
        .await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "files_trashed",
            serde_json::json!({"count": result.removed.len(), "failed": result.failed.len()}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(Json(result))
}

async fn trash(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<FileTrash>>> {
    user.require_owner()?;
    Ok(Json(state.library.file_trash().await?))
}

async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    user.require_owner()?;
    state.library.restore_file_trash(&id).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "files_restored",
            serde_json::json!({"trash_id": id}),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::library::testing::TestApp;
    use serde_json::json;

    #[tokio::test]
    async fn cleanup_is_owner_only_and_restores_unindexed_files() {
        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let (member_id, member) = app.user("member").await;
        let limited = app.device_token(&member_id, &["library:read"]).await;
        let path = app.state.layout.root().join("Inbox/notes.txt");
        std::fs::write(&path, b"keep until explicitly selected").unwrap();
        for token in [&member, &limited] {
            assert_eq!(app.json("/api/files", token).await.0, StatusCode::FORBIDDEN);
            assert_eq!(
                app.json("/api/files/trash", token).await.0,
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                app.send_json(
                    "POST",
                    "/api/files/trash",
                    token,
                    json!({"files": [{"path": "Inbox/notes.txt", "fingerprint": "old"}]})
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
        }
        let (status, inventory) = app.json("/api/files?folder=Inbox", &owner).await;
        assert_eq!(status, StatusCode::OK);
        let file = &inventory["files"][0];
        assert_eq!(file["kind"], "other");
        let (_, stale) = app
            .send_json(
                "POST",
                "/api/files/trash",
                &owner,
                json!({"files": [{"path": file["path"], "fingerprint": "stale"}]}),
            )
            .await;
        assert_eq!(stale["failed"].as_array().unwrap().len(), 1);
        assert!(path.exists());
        let (status, result) = app
            .send_json("POST", "/api/files/trash", &owner, json!({"files": [file]}))
            .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["removed"], json!(["Inbox/notes.txt"]));
        assert!(!path.exists());
        let (_, trash) = app.json("/api/files/trash", &owner).await;
        let restore = format!(
            "/api/files/trash/{}/restore",
            trash[0]["id"].as_str().unwrap()
        );
        assert_eq!(
            app.send_json("POST", &restore, &member, json!({})).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.send_json("POST", &restore, &owner, json!({})).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"keep until explicitly selected"
        );
    }
}
