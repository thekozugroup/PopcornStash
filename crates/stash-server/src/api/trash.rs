//! /api routes: trash, owner only (see docs/architecture.md sections 3 and 8).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use stash_core::library::repo::{self, TrashEntry};
use stash_core::models::{ItemDetail, ItemStatus};

use super::items::item_not_found;
use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/trash", get(list).delete(empty))
        .route("/api/trash/{id}/restore", post(restore))
}

/// `GET /api/trash`: newest first, as returned by the library.
async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<TrashEntry>>> {
    user.require_owner()?;
    Ok(Json(repo::trash_list(&state.db).await?))
}

async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<ItemDetail>> {
    user.require_owner()?;
    match repo::get(&state.db, &id).await? {
        Some(row) if row.status == ItemStatus::Trashed => {}
        _ => return Err(item_not_found()),
    }
    let row = state.library.restore_item(&id).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "item.restored",
            serde_json::json!({ "item_id": row.id, "title": row.title, "rel_path": row.rel_path }),
            None,
        )
        .await;
    Ok(Json(repo::detail(&state.db, &id, &user.user_id).await?))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Emptied {
    pub deleted: u64,
}

async fn empty(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Emptied>> {
    user.require_owner()?;
    let deleted = state.library.empty_trash(None).await?;
    tracing::info!(deleted, user = %user.user_id, "trash emptied");
    state
        .auth
        .audit(
            Some(&user.user_id),
            "trash.emptied",
            serde_json::json!({ "deleted": deleted }),
            None,
        )
        .await;
    Ok(Json(Emptied { deleted }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::api::library::testing::{TestApp, request};

    #[tokio::test]
    async fn trash_is_owner_only() {
        let app = TestApp::new().await;
        let (_, member) = app.user("member").await;
        let (status, json) = app.json("/api/trash", &member).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert_eq!(json["error"]["code"], "forbidden");
        let (status, _, _) = app
            .send(request("DELETE", "/api/trash", Some(&member), None))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, owner) = app.user("owner").await;
        let (status, json) = app.json("/api/trash", &owner).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert!(json.is_array());
        let (status, _, body) = app
            .send(request("DELETE", "/api/trash", Some(&owner), None))
            .await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["deleted"], 0);
    }
}
