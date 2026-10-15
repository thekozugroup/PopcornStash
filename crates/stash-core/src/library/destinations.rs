//! A download's explicit library choice uses the same visibility rule as the library UI.
use super::access::{Access, Viewer};
use super::defs::{self, LibraryDef};
use crate::Result;
use crate::db::Db;
use crate::models::{Category, ItemRow, Role};

pub async fn visible(db: &Db, user_id: &str, library_id: &str) -> Result<Option<LibraryDef>> {
    let role: Option<Role> = sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(db)
        .await?;
    let Some(role) = role else {
        return Ok(None);
    };
    let access = Access::load(
        db,
        &Viewer {
            user_id: user_id.into(),
            role,
            full: false,
        },
    )
    .await?;
    if !access.sees_library(library_id) {
        return Ok(None);
    }
    defs::load_one(db, library_id).await
}

/// Internal suggestion metadata survives clearing the download's history. Only a visible,
/// compatible library can become the public suggestion's target.
pub fn suggested<'a>(
    row: &ItemRow,
    access: &Access,
    defs: &'a [LibraryDef],
    category: Category,
) -> Option<&'a LibraryDef> {
    if !access.full_owner() && row.added_by.as_deref() != Some(access.viewer.user_id.as_str()) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(row.suggestion_json.as_deref()?).ok()?;
    let id = value.get("preferred_library_id")?.as_str()?;
    defs.iter()
        .find(|def| def.id() == id && access.sees_library(id) && def.shape().fits(category))
}
