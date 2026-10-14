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
use crate::security::ClientIp;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/trash", get(list).delete(empty))
        .route("/api/trash/{id}/restore", post(restore))
}

/// One entry of `GET /api/trash`, with the sentence a restore would get now.
#[derive(Debug, Clone, Serialize)]
pub struct TrashView {
    #[serde(flatten)]
    pub entry: TrashEntry,
    /// Why a restore would be refused now (section 11, Restore from Trash), else None.
    pub restore_refusal: Option<String>,
}

/// `GET /api/trash`: newest first, as returned by the library.
async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<TrashView>>> {
    user.require_owner()?;
    let mut out = Vec::new();
    for entry in repo::trash_list(&state.db).await? {
        let restore_refusal = state.library.restore_refusal(&entry.item.id).await?;
        out.push(TrashView {
            entry,
            restore_refusal,
        });
    }
    Ok(Json(out))
}

async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
    Path(id): Path<String>,
) -> ApiResult<Json<ItemDetail>> {
    user.require_owner()?;
    match repo::get(&state.db, &id).await? {
        Some(row) if row.status == ItemStatus::Trashed => {}
        _ => return Err(item_not_found()),
    }
    let row = state.library.restore_item(&id).await?;
    let access = user.access(&state.db).await?;
    state
        .auth
        .audit(
            Some(&user.user_id),
            "item_restored",
            serde_json::json!({ "item_id": row.id, "title": row.title, "rel_path": row.rel_path }),
            ip.as_string().as_deref(),
        )
        .await;
    Ok(Json(
        repo::detail(&state.db, &state.layout, &access, &id).await?,
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Emptied {
    pub deleted: u64,
}

async fn empty(
    State(state): State<AppState>,
    user: AuthUser,
    ip: ClientIp,
) -> ApiResult<Json<Emptied>> {
    user.require_owner()?;
    let deleted = state.library.empty_trash(None).await?;
    tracing::info!(deleted, user = %user.user_id, "trash emptied");
    state
        .auth
        .audit(
            Some(&user.user_id),
            "trash_emptied",
            serde_json::json!({ "deleted": deleted }),
            ip.as_string().as_deref(),
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

    /// Owner-only actions need the owner's full access: a limited device token
    /// belonging to the owner cannot use them.
    #[tokio::test]
    async fn owner_device_tokens_without_full_scope_are_refused() {
        use stash_core::models::{Category, ItemStatus, MediaKind};

        let app = TestApp::new().await;
        let (owner_id, _) = app.user("owner").await;
        let reader = app
            .device_token(&owner_id, &["library:read", "downloads:create"])
            .await;
        let id = app
            .item(
                "Other/keep.bin",
                MediaKind::Other,
                Category::Other,
                ItemStatus::Ready,
                Some(b"keep"),
            )
            .await;
        let (status, _) = app.json("/api/trash", &reader).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        for (method, uri) in [
            ("DELETE", "/api/trash".to_string()),
            ("DELETE", format!("/api/items/{id}")),
            ("POST", format!("/api/trash/{id}/restore")),
        ] {
            let (status, _, _) = app.send(request(method, &uri, Some(&reader), None)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        }
        assert!(app.state.config.stash_root.join("Other/keep.bin").is_file());
        let (status, _) = app.json("/api/system", &reader).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn delete_restore_and_empty() {
        use stash_core::models::{Category, ItemStatus, MediaKind};

        let app = TestApp::new().await;
        let (_, owner) = app.user("owner").await;
        let rel = "Home Videos/2024/06/party.mp4";
        let id = app
            .item(
                rel,
                MediaKind::Video,
                Category::HomeVideo,
                ItemStatus::Ready,
                Some(b"party"),
            )
            .await;
        let root = app.state.config.stash_root.clone();

        let (status, _, _) = app
            .send(request(
                "DELETE",
                &format!("/api/items/{id}"),
                Some(&owner),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!root.join(rel).exists());
        // Trashed items are gone from the library API.
        let (status, _) = app.json(&format!("/api/items/{id}"), &owner).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = app
            .get(&format!("/api/items/{id}/file"), Some(&owner))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, json) = app.json("/api/trash", &owner).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json.as_array().unwrap().len(), 1, "{json}");
        assert_eq!(json[0]["original_rel_path"], rel);

        let (status, _, body) = app
            .send(request(
                "POST",
                &format!("/api/trash/{id}/restore"),
                Some(&owner),
                Some(serde_json::json!({})),
            ))
            .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["rel_path"], rel);
        assert!(root.join(rel).is_file());
        // Restoring twice is a 404: it is no longer in the trash.
        let (status, _, _) = app
            .send(request(
                "POST",
                &format!("/api/trash/{id}/restore"),
                Some(&owner),
                Some(serde_json::json!({})),
            ))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _, _) = app
            .send(request(
                "DELETE",
                &format!("/api/items/{id}"),
                Some(&owner),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _, body) = app
            .send(request("DELETE", "/api/trash", Some(&owner), None))
            .await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["deleted"], 1);
        let (_, json) = app.json("/api/trash", &owner).await;
        assert!(json.as_array().unwrap().is_empty());

        let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log WHERE event IN ('item_trashed', 'item_restored', 'trash_emptied')")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        assert_eq!(count, 4);
    }
}
