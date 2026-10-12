//! /api routes: inbox review (see docs/architecture.md sections 4 and 8).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use stash_core::library::repo;
use stash_core::library::{OrganizeRequest, ReviewAction};
use stash_core::models::{Category, ItemDetail};

use super::items::{JsonBody, visible_detail, visible_item};
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
    visible_item(&state, &id).await?;
    state.library.review(&id, body.into_action()).await?;
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
}
