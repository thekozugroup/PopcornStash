//! webauthn-rs relying party and ceremony state (registration, discoverable login).

use stash_core::config::Config;

/// Relying party plus short-lived ceremony state keyed by flow id (5 minute TTL).
pub struct PasskeyState {
    // auth agent: Option<webauthn_rs::Webauthn> (None when passkeys are not ready),
    // DashMap<String, PendingFlow> with expiry.
}

impl PasskeyState {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let _ = config;
        Ok(Self {})
    }
}
