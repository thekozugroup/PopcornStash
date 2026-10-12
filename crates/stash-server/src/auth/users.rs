//! Users, invites, setup/recovery codes and the audit log.

use stash_core::config::Config;
use stash_core::db::Db;

use super::AuthService;
use crate::error::ApiResult;

pub async fn ensure_setup_code(auth: &AuthService) -> ApiResult<Option<String>> {
    let _ = auth;
    todo!("auth agent")
}

pub async fn audit(db: &Db, user_id: Option<&str>, event: &str, detail: serde_json::Value, ip: Option<&str>) {
    let _ = (db, user_id, event, detail, ip);
    todo!("auth agent")
}

pub async fn create_owner_reset_link(db: &Db, config: &Config) -> anyhow::Result<String> {
    let _ = (db, config);
    todo!("auth agent")
}
