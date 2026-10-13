//! Owner-editable settings stored in the `settings` table (section 6).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::Result;
use crate::config::HwAccel;
use crate::db::Db;

const KEY: &str = "app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HttpViaVpn {
    Never,
    #[default]
    WhenAvailable,
    Always,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentSettings {
    pub seed_after_complete: bool,
    pub seed_ratio: f64,
    pub seed_hours: u32,
    pub max_active: u32,
    pub down_limit_kbps: u64,
    pub up_limit_kbps: u64,
}

impl Default for TorrentSettings {
    fn default() -> Self {
        Self {
            seed_after_complete: false,
            seed_ratio: 1.0,
            seed_hours: 24,
            max_active: 5,
            down_limit_kbps: 0,
            up_limit_kbps: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscodeSettings {
    pub hwaccel: HwAccel,
    pub max_height: u32,
    pub crf: u32,
    pub max_sessions: u32,
}

impl Default for TranscodeSettings {
    fn default() -> Self {
        Self {
            hwaccel: HwAccel::None,
            max_height: 1080,
            crf: 23,
            max_sessions: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub auto_sort: bool,
    pub http_via_vpn: HttpViaVpn,
    pub max_concurrent_downloads: u32,
    pub torrent: TorrentSettings,
    pub transcode: TranscodeSettings,
    pub trash_retention_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_sort: true,
            http_via_vpn: HttpViaVpn::WhenAvailable,
            max_concurrent_downloads: 3,
            torrent: TorrentSettings::default(),
            transcode: TranscodeSettings::default(),
            trash_retention_days: 30,
        }
    }
}

impl Settings {
    /// Clamp values into sane ranges.
    pub fn normalized(mut self) -> Self {
        self.max_concurrent_downloads = self.max_concurrent_downloads.clamp(1, 10);
        self.torrent.seed_ratio = self.torrent.seed_ratio.clamp(0.0, 100.0);
        self.torrent.max_active = self.torrent.max_active.clamp(1, 50);
        for limit in [
            &mut self.torrent.down_limit_kbps,
            &mut self.torrent.up_limit_kbps,
        ] {
            if *limit != 0 {
                *limit = (*limit).max(crate::internal_api::MIN_SPEED_LIMIT_KBPS);
            }
        }
        self.transcode.max_height = self.transcode.max_height.clamp(240, 4320);
        self.transcode.crf = self.transcode.crf.clamp(15, 35);
        self.transcode.max_sessions = self.transcode.max_sessions.clamp(1, 8);
        self.trash_retention_days = self.trash_retention_days.clamp(1, 365);
        self
    }
}

/// Cached settings with write-through to the database.
#[derive(Clone)]
pub struct SettingsStore {
    db: Db,
    cache: Arc<RwLock<Settings>>,
}

impl SettingsStore {
    pub async fn load(db: Db, hwaccel_default: HwAccel) -> Result<Self> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(KEY)
            .fetch_optional(&db)
            .await?;
        let settings = match row {
            Some((json,)) => serde_json::from_str::<Settings>(&json).unwrap_or_default(),
            None => {
                let mut s = Settings::default();
                s.transcode.hwaccel = hwaccel_default;
                s
            }
        };
        Ok(Self {
            db,
            cache: Arc::new(RwLock::new(settings.normalized())),
        })
    }

    pub async fn get(&self) -> Settings {
        self.cache.read().await.clone()
    }

    /// Merge a partial JSON object into the current settings and persist.
    pub async fn update(&self, patch: serde_json::Value) -> Result<Settings> {
        let mut guard = self.cache.write().await;
        let mut current = serde_json::to_value(&*guard)?;
        merge(&mut current, patch);
        let next: Settings = serde_json::from_value(current)
            .map_err(|e| crate::CoreError::invalid(format!("Invalid settings: {e}")))?;
        let next = next.normalized();
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(KEY)
            .bind(serde_json::to_string(&next)?)
            .execute(&self.db)
            .await?;
        *guard = next.clone();
        Ok(next)
    }
}

fn merge(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base), serde_json::Value::Object(patch)) => {
            for (k, v) in patch {
                merge(base.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (slot, value) => *slot = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn partial_update_merges() {
        let db = crate::db::open_memory().await.unwrap();
        let store = SettingsStore::load(db.clone(), HwAccel::None)
            .await
            .unwrap();
        let s = store
            .update(serde_json::json!({"torrent": {"seed_ratio": 2.0}}))
            .await
            .unwrap();
        assert_eq!(s.torrent.seed_ratio, 2.0);
        assert_eq!(s.torrent.max_active, 5);
        assert!(s.auto_sort);
        let reloaded = SettingsStore::load(db, HwAccel::None)
            .await
            .unwrap()
            .get()
            .await;
        assert_eq!(reloaded.torrent.seed_ratio, 2.0);
        // Speed limits: 0 is "no limit"; anything else is at least 16 KiB/s, the
        // smallest limit the torrent engine can keep.
        let s = store
            .update(serde_json::json!({"torrent": {"down_limit_kbps": 5, "up_limit_kbps": 0}}))
            .await
            .unwrap();
        assert_eq!(
            (s.torrent.down_limit_kbps, s.torrent.up_limit_kbps),
            (16, 0)
        );
        let s = store
            .update(serde_json::json!({"torrent": {"down_limit_kbps": 800, "up_limit_kbps": 16}}))
            .await
            .unwrap();
        assert_eq!(
            (s.torrent.down_limit_kbps, s.torrent.up_limit_kbps),
            (800, 16)
        );
        assert!(
            store
                .update(serde_json::json!({"http_via_vpn": "sometimes"}))
                .await
                .is_err()
        );
    }
}
