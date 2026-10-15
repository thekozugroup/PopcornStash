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
    pub add_public_trackers: bool,
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
            add_public_trackers: false,
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
        self.torrent.seed_hours = self.torrent.seed_hours.clamp(1, 720);
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

/// STASH_HWACCEL as the settings store sees it. Set (not empty), it overrides the
/// owner's saved choice; unset or empty, the saved choice is in charge
/// (docs/architecture.md section 2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HwAccelEnv(pub Option<HwAccel>);

impl From<Option<HwAccel>> for HwAccelEnv {
    fn from(value: Option<HwAccel>) -> Self {
        Self(value)
    }
}

/// A plain value, as tests pass it: `none` leaves the saved choice in charge.
impl From<HwAccel> for HwAccelEnv {
    fn from(value: HwAccel) -> Self {
        Self((value != HwAccel::None).then_some(value))
    }
}

/// Cached settings with write-through to the database.
#[derive(Clone)]
pub struct SettingsStore {
    db: Db,
    /// What the owner saved. `get` puts STASH_HWACCEL on top.
    cache: Arc<RwLock<Settings>>,
    hwaccel_env: Option<HwAccel>,
}

impl SettingsStore {
    pub async fn load(db: Db, hwaccel_env: impl Into<HwAccelEnv>) -> Result<Self> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(KEY)
            .fetch_optional(&db)
            .await?;
        let settings = match row {
            Some((json,)) => serde_json::from_str::<Settings>(&json).unwrap_or_default(),
            None => Settings::default(),
        };
        Ok(Self {
            db,
            cache: Arc::new(RwLock::new(settings.normalized())),
            hwaccel_env: hwaccel_env.into().0,
        })
    }

    /// The settings in effect: the saved ones, with STASH_HWACCEL when it is set.
    pub async fn get(&self) -> Settings {
        let saved = self.cache.read().await.clone();
        self.in_effect(saved)
    }

    /// The hardware acceleration STASH_HWACCEL sets, when it is set. The owner's
    /// saved choice is then kept but not used.
    pub fn hwaccel_from_env(&self) -> Option<HwAccel> {
        self.hwaccel_env
    }

    fn in_effect(&self, mut settings: Settings) -> Settings {
        if let Some(hwaccel) = self.hwaccel_env {
            settings.transcode.hwaccel = hwaccel;
        }
        settings
    }

    /// Merge a partial JSON object into the current settings and persist. While
    /// STASH_HWACCEL is set, `transcode.hwaccel` may only repeat its value (a client
    /// sending back what it read), and the owner's saved choice stays as it was.
    pub async fn update(&self, mut patch: serde_json::Value) -> Result<Settings> {
        if let Some(env) = self.hwaccel_env {
            drop_env_hwaccel(&mut patch, env)?;
        }
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
        Ok(self.in_effect(next))
    }
}

/// Takes `transcode.hwaccel` out of a patch when it repeats STASH_HWACCEL, and
/// refuses any other value: the environment decides while the variable is set.
fn drop_env_hwaccel(patch: &mut serde_json::Value, env: HwAccel) -> Result<()> {
    let Some(transcode) = patch
        .get_mut("transcode")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };
    let Some(sent) = transcode.get("hwaccel") else {
        return Ok(());
    };
    if serde_json::from_value::<HwAccel>(sent.clone()).ok() != Some(env) {
        return Err(crate::CoreError::invalid(
            "Hardware acceleration is set by STASH_HWACCEL on the container. To change it, edit or remove that variable, then recreate the container (docker compose up -d, or Apply on Unraid).",
        ));
    }
    transcode.remove("hwaccel");
    Ok(())
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
    async fn public_trackers_default_off_for_existing_settings_and_persist_when_enabled() {
        let old: Settings = serde_json::from_str(r#"{"torrent":{"max_active":3}}"#).unwrap();
        assert!(!old.torrent.add_public_trackers);
        let db = crate::db::open_memory().await.unwrap();
        let store = SettingsStore::load(db.clone(), None).await.unwrap();
        store
            .update(serde_json::json!({"torrent":{"add_public_trackers":true}}))
            .await
            .unwrap();
        assert!(
            SettingsStore::load(db, None)
                .await
                .unwrap()
                .get()
                .await
                .torrent
                .add_public_trackers
        );
    }

    #[tokio::test]
    async fn seeding_hours_are_bounded_and_persisted() {
        let db = crate::db::open_memory().await.unwrap();
        let store = SettingsStore::load(db.clone(), HwAccel::None)
            .await
            .unwrap();
        for (requested, expected) in [(0, 1), (1, 1), (24, 24), (720, 720), (u32::MAX, 720)] {
            let value = store
                .update(serde_json::json!({"torrent": {"seed_hours": requested}}))
                .await
                .unwrap();
            assert_eq!(value.torrent.seed_hours, expected);
            assert_eq!(
                SettingsStore::load(db.clone(), HwAccel::None)
                    .await
                    .unwrap()
                    .get()
                    .await
                    .torrent
                    .seed_hours,
                expected
            );
        }
    }

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

    /// An owner who saved any setting and later sets STASH_HWACCEL=vaapi gets VAAPI,
    /// not the hardware acceleration saved before.
    #[tokio::test]
    async fn stash_hwaccel_overrides_the_saved_setting() {
        let db = crate::db::open_memory().await.unwrap();
        let store = SettingsStore::load(db.clone(), HwAccel::None)
            .await
            .unwrap();
        store
            .update(
                serde_json::json!({"transcode": {"hwaccel": "qsv"}, "trash_retention_days": 10}),
            )
            .await
            .unwrap();

        let store = SettingsStore::load(db.clone(), HwAccel::Vaapi)
            .await
            .unwrap();
        assert_eq!(store.get().await.transcode.hwaccel, HwAccel::Vaapi);
        assert_eq!(store.get().await.trash_retention_days, 10);
        assert_eq!(store.hwaccel_from_env(), Some(HwAccel::Vaapi));

        // A client that sends back what it read saves its other changes; the owner's
        // own choice is kept for when the variable goes away.
        let s = store
            .update(serde_json::json!({"transcode": {"hwaccel": "vaapi", "crf": 20}}))
            .await
            .unwrap();
        assert_eq!((s.transcode.hwaccel, s.transcode.crf), (HwAccel::Vaapi, 20));
        // Choosing something else is refused while the variable is set.
        let err = store
            .update(serde_json::json!({"transcode": {"hwaccel": "nvenc"}}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("STASH_HWACCEL"), "{err}");
        assert_eq!(store.get().await.transcode.hwaccel, HwAccel::Vaapi);

        // STASH_HWACCEL=none set on purpose also wins.
        let store = SettingsStore::load(db.clone(), Some(HwAccel::None))
            .await
            .unwrap();
        assert_eq!(store.get().await.transcode.hwaccel, HwAccel::None);
        assert_eq!(store.hwaccel_from_env(), Some(HwAccel::None));

        // Unset or empty: the saved choice is in charge again.
        let store = SettingsStore::load(db, None).await.unwrap();
        let s = store.get().await;
        assert_eq!((s.transcode.hwaccel, s.transcode.crf), (HwAccel::Qsv, 20));
        assert_eq!(store.hwaccel_from_env(), None);
    }
}
