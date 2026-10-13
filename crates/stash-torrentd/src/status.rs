//! Reading and writing the shared status.json (`/run/popcornstash/status.json`).
//!
//! The supervisor writes [`TunnelStatus`] atomically every few seconds; the
//! engine reads it to decide whether it may run torrents.

use std::path::Path;

use stash_core::internal_api::{TunnelState, TunnelStatus};

use crate::sys;

/// A status older than this is treated as "tunnel down" (the supervisor writes every 5 s).
pub const STALE_AFTER_SECS: i64 = 30;

/// Write `status` atomically, world-readable (the engine runs as another user).
pub fn write(path: &Path, status: &TunnelStatus) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(status).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    sys::write_atomic(path, &bytes, 0o644)
}

/// Read the status file. None when missing or unreadable.
pub fn read(path: &Path) -> Option<TunnelStatus> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// True when the tunnel is connected according to a fresh status.
pub fn tunnel_connected(status: Option<&TunnelStatus>, now: i64) -> bool {
    status.is_some_and(|s| {
        s.state == TunnelState::Connected && now.saturating_sub(s.updated_at) <= STALE_AFTER_SECS
    })
}

/// The status the engine reports when the file is missing or stale.
pub fn unknown(now: i64) -> TunnelStatus {
    TunnelStatus {
        state: TunnelState::Disconnected,
        provider: None,
        protocol: None,
        label: None,
        server: None,
        country: None,
        interface: None,
        since: None,
        error: Some("The VPN supervisor has not reported a tunnel status.".into()),
        kill_switch: true,
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn round_trip_and_staleness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        assert!(read(&path).is_none());
        let mut status = TunnelStatus::not_configured(1000);
        status.state = TunnelState::Connected;
        write(&path, &status).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let back = read(&path).unwrap();
        assert_eq!(back, status);
        assert!(tunnel_connected(Some(&back), 1010));
        assert!(!tunnel_connected(Some(&back), 1000 + STALE_AFTER_SECS + 1));
        assert!(!tunnel_connected(None, 1000));
        status.state = TunnelState::Connecting;
        assert!(!tunnel_connected(Some(&status), 1000));
        std::fs::write(&path, b"{not json").unwrap();
        assert!(read(&path).is_none());
    }
}
