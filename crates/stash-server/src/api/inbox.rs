//! /api routes: inbox review (see docs/architecture.md sections 4 and 8).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use stash_core::library::repo;
use stash_core::library::{OrganizeRequest, ReviewAction};
use stash_core::models::{Category, ItemDetail};

use super::items::{JsonBody, check_organize, visible_detail, visible_item};
use crate::auth::{AuthUser, Scope};
use crate::error::ApiResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/inbox", get(list))
        .route("/api/inbox/{id}", post(decide))
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxList {
    pub items: Vec<ItemDetail>,
}

async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<InboxList>> {
    user.require(Scope::LibraryRead)?;
    Ok(Json(InboxList {
        items: repo::inbox(&state.db, &user.user_id).await?,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Accept,
    Ignore,
}

/// `POST /api/inbox/:id` body. Fields other than `action` only apply to `accept`
/// and override the suggestion.
#[derive(Debug, Clone, Deserialize)]
struct DecisionBody {
    action: Decision,
    #[serde(default)]
    category: Option<Category>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    show_name: Option<String>,
    #[serde(default)]
    season: Option<i64>,
    #[serde(default)]
    episode: Option<i64>,
}

impl DecisionBody {
    fn into_action(self) -> ReviewAction {
        let text = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        match self.action {
            Decision::Ignore => ReviewAction::Ignore,
            Decision::Accept => ReviewAction::Accept(OrganizeRequest {
                category: self.category,
                title: text(self.title),
                year: self.year,
                show_name: text(self.show_name),
                season: self.season,
                episode: self.episode,
            }),
        }
    }
}

async fn decide(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<DecisionBody>,
) -> ApiResult<Json<ItemDetail>> {
    user.require(Scope::Full)?;
    let action = body.into_action();
    if let ReviewAction::Accept(request) = &action {
        check_organize(request)?;
    }
    visible_item(&state, &id).await?;
    state.library.review(&id, action).await?;
    Ok(Json(visible_detail(&state, &id, &user.user_id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_bodies() {
        let body: DecisionBody =
            serde_json::from_value(serde_json::json!({"action": "ignore"})).unwrap();
        assert_eq!(body.into_action(), ReviewAction::Ignore);
        let body: DecisionBody = serde_json::from_value(serde_json::json!({
            "action": "accept", "category": "movie", "title": " Heat ", "year": 1995, "show_name": null, "season": null
        }))
        .unwrap();
        match body.into_action() {
            ReviewAction::Accept(r) => {
                assert_eq!(r.category, Some(Category::Movie));
                assert_eq!(r.title.as_deref(), Some("Heat"));
                assert_eq!(r.year, Some(1995));
                assert_eq!(r.show_name, None);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            serde_json::from_value::<DecisionBody>(serde_json::json!({"action": "maybe"})).is_err()
        );
        assert!(serde_json::from_value::<DecisionBody>(serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn accept_limits_text_and_numbers() {
        use axum::http::StatusCode;
        use stash_core::models::{ItemStatus, MediaKind};

        use crate::api::library::testing::{TestApp, request};

        let app = TestApp::new().await;
        let (_, token) = app.user("member").await;
        let id = app
            .item(
                "Downloads/Completed/show.mkv",
                MediaKind::Video,
                Category::Other,
                ItemStatus::NeedsReview,
                Some(b"show"),
            )
            .await;
        let long = "x".repeat(501);
        for body in [
            serde_json::json!({"action": "accept", "category": "movie", "title": long}),
            serde_json::json!({"action": "accept", "category": "episode", "show_name": long, "season": 1, "episode": 2}),
            serde_json::json!({"action": "accept", "category": "episode", "show_name": "Bluey", "season": -1, "episode": 2}),
            serde_json::json!({"action": "accept", "category": "episode", "show_name": "Bluey", "season": 1, "episode": 99_999}),
        ] {
            let (status, _, bytes) = app
                .send(request(
                    "POST",
                    &format!("/api/inbox/{id}"),
                    Some(&token),
                    Some(body.clone()),
                ))
                .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{body}: {}",
                String::from_utf8_lossy(&bytes)
            );
        }
        let (_, json) = app.json("/api/inbox", &token).await;
        assert_eq!(json["items"][0]["rel_path"], "Downloads/Completed/show.mkv");
    }

    #[tokio::test]
    async fn review_accepts_and_ignores_through_the_api() {
        use axum::http::StatusCode;
        use stash_core::models::{ItemStatus, MediaKind};

        use crate::api::library::testing::{TestApp, request};

        let app = TestApp::new().await;
        let (user_id, token) = app.user("member").await;
        let accept = app
            .item(
                "Downloads/Completed/heat.1995.mkv",
                MediaKind::Video,
                Category::Other,
                ItemStatus::NeedsReview,
                Some(b"movie"),
            )
            .await;
        let ignore = app
            .item(
                "Downloads/Completed/notes.mkv",
                MediaKind::Video,
                Category::Other,
                ItemStatus::NeedsReview,
                Some(b"other"),
            )
            .await;

        let (status, json) = app.json("/api/inbox", &token).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["items"].as_array().unwrap().len(), 2, "{json}");

        // Device tokens without the full scope cannot decide.
        let reader = app.device_token(&user_id, &["library:read"]).await;
        let body = serde_json::json!({"action": "ignore"});
        let (status, _, _) = app
            .send(request(
                "POST",
                &format!("/api/inbox/{ignore}"),
                Some(&reader),
                Some(body.clone()),
            ))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _, bytes) = app
            .send(request(
                "POST",
                &format!("/api/inbox/{ignore}"),
                Some(&token),
                Some(body),
            ))
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ready");
        assert_eq!(json["rel_path"], "Downloads/Completed/notes.mkv");

        let body = serde_json::json!({"action": "accept", "category": "movie", "title": "Heat", "year": 1995});
        let (status, _, bytes) = app
            .send(request(
                "POST",
                &format!("/api/inbox/{accept}"),
                Some(&token),
                Some(body),
            ))
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json["rel_path"], "Movies/Heat (1995)/Heat (1995).mkv",
            "{json}"
        );
        let root = &app.state.config.stash_root;
        assert!(root.join("Movies/Heat (1995)/Heat (1995).mkv").is_file());
        assert!(!root.join("Downloads/Completed/heat.1995.mkv").exists());

        // Already reviewed.
        let (status, _, _) = app
            .send(request(
                "POST",
                &format!("/api/inbox/{accept}"),
                Some(&token),
                Some(serde_json::json!({"action": "ignore"})),
            ))
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, json) = app.json("/api/inbox", &token).await;
        assert_eq!(status, StatusCode::OK);
        assert!(json["items"].as_array().unwrap().is_empty(), "{json}");
    }
}
