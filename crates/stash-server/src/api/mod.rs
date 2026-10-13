//! JSON API route groups. Auth/account routes live in `crate::auth`.

pub mod downloads;
pub mod events;
pub mod inbox;
pub mod items;
pub mod library;
pub mod settings;
pub mod stream;
pub mod system;
pub mod trash;
pub mod uploads;
pub mod vpn;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(crate::auth::routes())
        .merge(library::routes())
        .merge(items::routes())
        .merge(stream::routes())
        .merge(inbox::routes())
        .merge(trash::routes())
        .merge(downloads::routes())
        .merge(uploads::routes())
        .merge(vpn::routes())
        .merge(settings::routes())
        .merge(events::routes())
        .merge(system::routes())
        .method_not_allowed_fallback(|| async {
            crate::error::ApiError::new(
                crate::error::ErrorCode::MethodNotAllowed,
                "That request method is not supported here.",
            )
        })
}
