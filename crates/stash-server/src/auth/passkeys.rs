//! webauthn-rs relying party and ceremony state (registration, discoverable login).
//!
//! Ceremony state never leaves the server: it lives in memory keyed by a random
//! `flow_id`, expires after 5 minutes and is consumed by the first finish call.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use dashmap::DashMap;
use stash_core::config::Config;
use stash_core::models::Role;
use webauthn_rs::prelude::{
    AuthenticationResult, CredentialID, DiscoverableAuthentication, DiscoverableKey, Passkey,
    PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential, Url, Uuid, Webauthn,
    WebauthnBuilder,
};

use super::codes;
use crate::error::{ApiError, ApiResult, ErrorCode};

/// How long a started ceremony stays valid.
pub const FLOW_TTL: Duration = Duration::from_secs(300);
const SWEEP_EVERY: Duration = Duration::from_secs(60);
/// Most ceremonies in flight at once. `login/start` needs no account, so this bounds
/// memory even when the per-address rate limit is spread over many addresses.
pub const MAX_FLOWS: usize = 10_000;
/// Shown by authenticators next to the account name.
pub const RP_NAME: &str = "PopcornStash";

/// What a started ceremony will do when it finishes.
pub(crate) enum Flow {
    /// First owner. `code_hash` is the setup code that started it.
    Setup {
        reg: Box<PasskeyRegistration>,
        code_hash: String,
        display_name: String,
        webauthn_id: Uuid,
    },
    /// New person from an invite.
    Invite {
        reg: Box<PasskeyRegistration>,
        invite_id: String,
        role: Role,
        display_name: String,
        webauthn_id: Uuid,
    },
    /// New passkey for the first owner from an owner reset link.
    Recover {
        reg: Box<PasskeyRegistration>,
        code_id: String,
        user_id: String,
    },
    /// Signed-in user adds another passkey.
    Add {
        reg: Box<PasskeyRegistration>,
        user_id: String,
    },
    /// Discoverable sign-in.
    Login {
        auth: Box<DiscoverableAuthentication>,
    },
}

struct PendingFlow {
    created: Instant,
    flow: Flow,
}

/// Relying party plus short-lived ceremony state keyed by flow id (5 minute TTL).
pub struct PasskeyState {
    webauthn: Option<Webauthn>,
    reason: String,
    flows: DashMap<String, PendingFlow>,
    max_flows: usize,
    last_sweep: Mutex<Instant>,
}

/// Why passkeys are unavailable, as a sentence for the setup screen.
pub fn unavailable_reason(config: &Config) -> String {
    match &config.public_url {
        None => "Passkeys need HTTPS on a domain name (or localhost). Set STASH_PUBLIC_URL to your address, \
                 for example https://stash.example.com, and restart the container."
            .to_string(),
        Some(url) => format!(
            "Passkeys cannot use {}. They need HTTPS on a domain name (or localhost): set STASH_PUBLIC_URL to an \
             address like https://stash.example.com and restart the container.",
            url.as_str().trim_end_matches('/')
        ),
    }
}

fn build_webauthn(config: &Config) -> Option<Webauthn> {
    if !config.passkeys_ready() {
        return None;
    }
    let origin = config.public_url.as_ref()?;
    let rp_id = config.rp_id()?;
    let mut builder = match WebauthnBuilder::new(&rp_id, origin) {
        Ok(b) => b.rp_name(RP_NAME),
        Err(e) => {
            tracing::warn!(error = %e, "passkeys disabled: the public URL is not a valid relying party");
            return None;
        }
    };
    for extra in &config.extra_origins {
        builder = builder.append_allowed_origin(extra);
    }
    match builder.build() {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(error = %e, "passkeys disabled: could not configure the relying party");
            None
        }
    }
}

impl PasskeyState {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        Ok(Self {
            webauthn: build_webauthn(config),
            reason: unavailable_reason(config),
            flows: DashMap::new(),
            max_flows: MAX_FLOWS,
            last_sweep: Mutex::new(Instant::now()),
        })
    }

    /// True when passkey ceremonies can run.
    pub fn ready(&self) -> bool {
        self.webauthn.is_some()
    }

    /// The relying party, or `passkeys_unavailable`.
    pub fn webauthn(&self) -> ApiResult<&Webauthn> {
        self.webauthn
            .as_ref()
            .ok_or_else(|| ApiError::new(ErrorCode::PasskeysUnavailable, self.reason.clone()))
    }

    /// Store ceremony state; returns its flow id. Refuses with `rate_limited` when
    /// [`MAX_FLOWS`] ceremonies are already in flight.
    pub(crate) fn begin(&self, flow: Flow) -> ApiResult<String> {
        self.sweep_if_due();
        if self.flows.len() >= self.max_flows {
            self.flows.retain(|_, p| p.created.elapsed() <= FLOW_TTL);
            if self.flows.len() >= self.max_flows {
                tracing::warn!(
                    in_flight = self.flows.len(),
                    "too many passkey ceremonies in progress"
                );
                return Err(ApiError::new(
                    ErrorCode::RateLimited,
                    "Too many sign-ins are in progress. Wait a minute and try again.",
                ));
            }
        }
        let flow_id = codes::random_b64::<16>();
        self.flows.insert(
            flow_id.clone(),
            PendingFlow {
                created: Instant::now(),
                flow,
            },
        );
        Ok(flow_id)
    }

    /// Remove and return ceremony state. Expired or unknown ids fail.
    pub(crate) fn take(&self, flow_id: &str) -> ApiResult<Flow> {
        let expired = || ApiError::bad_request("This passkey request expired. Start again.");
        let (_, pending) = self.flows.remove(flow_id).ok_or_else(expired)?;
        if pending.created.elapsed() > FLOW_TTL {
            return Err(expired());
        }
        Ok(pending.flow)
    }

    /// Number of ceremonies in flight (tests and diagnostics).
    pub fn pending_flows(&self) -> usize {
        self.flows.len()
    }

    fn sweep_if_due(&self) {
        let Ok(mut last) = self.last_sweep.try_lock() else {
            return;
        };
        if last.elapsed() < SWEEP_EVERY {
            return;
        }
        *last = Instant::now();
        drop(last);
        self.flows.retain(|_, p| p.created.elapsed() <= FLOW_TTL);
    }

    /// Start registering a discoverable, user-verified passkey.
    ///
    /// webauthn-rs asks for `residentKey: "discouraged"`; PopcornStash signs in without
    /// a username, so the options sent to the browser require a discoverable credential.
    pub(crate) fn start_registration(
        &self,
        user_handle: Uuid,
        user_name: &str,
        display_name: &str,
        exclude: Vec<CredentialID>,
    ) -> ApiResult<(serde_json::Value, PasskeyRegistration)> {
        let webauthn = self.webauthn()?;
        let exclude = (!exclude.is_empty()).then_some(exclude);
        let (ccr, reg) = webauthn
            .start_passkey_registration(user_handle, user_name, display_name, exclude)
            .map_err(|e| {
                tracing::error!(error = %e, "could not start passkey registration");
                ApiError::internal()
            })?;
        let mut options = serde_json::to_value(&ccr).map_err(|e| {
            tracing::error!(error = %e, "could not encode passkey options");
            ApiError::internal()
        })?;
        require_resident_key(&mut options);
        Ok((options, reg))
    }

    /// Verify a registration response against its ceremony state.
    pub(crate) fn finish_registration(
        &self,
        credential: &RegisterPublicKeyCredential,
        reg: &PasskeyRegistration,
    ) -> ApiResult<Passkey> {
        self.webauthn()?
            .finish_passkey_registration(credential, reg)
            .map_err(|e| {
                tracing::info!(error = %e, "passkey registration rejected");
                ApiError::bad_request("Your passkey could not be verified. Try again.")
            })
    }

    /// Start a username-less sign-in. `conditional` keeps `mediation: "conditional"` for autofill UI;
    /// otherwise the options suit a "Sign in with a passkey" button.
    pub(crate) fn start_login(
        &self,
        conditional: bool,
    ) -> ApiResult<(serde_json::Value, DiscoverableAuthentication)> {
        let (rcr, auth) = self
            .webauthn()?
            .start_discoverable_authentication()
            .map_err(|e| {
                tracing::error!(error = %e, "could not start passkey sign-in");
                ApiError::internal()
            })?;
        let mut options = serde_json::to_value(&rcr).map_err(|e| {
            tracing::error!(error = %e, "could not encode passkey options");
            ApiError::internal()
        })?;
        if !conditional && let Some(map) = options.as_object_mut() {
            map.remove("mediation");
        }
        Ok((options, auth))
    }

    /// The user handle and credential id from a discoverable sign-in response.
    pub(crate) fn identify(&self, credential: &PublicKeyCredential) -> ApiResult<(Uuid, Vec<u8>)> {
        let (uuid, cred_id) = self
            .webauthn()?
            .identify_discoverable_authentication(credential)
            .map_err(|e| {
                tracing::info!(error = %e, "passkey sign-in without a usable user handle");
                not_recognized()
            })?;
        Ok((uuid, cred_id.to_vec()))
    }

    /// Verify a discoverable sign-in against the user's passkeys.
    pub(crate) fn finish_login(
        &self,
        credential: &PublicKeyCredential,
        auth: DiscoverableAuthentication,
        keys: &[DiscoverableKey],
    ) -> ApiResult<AuthenticationResult> {
        self.webauthn()?
            .finish_discoverable_authentication(credential, auth, keys)
            .map_err(|e| {
                tracing::info!(error = %e, "passkey sign-in rejected");
                not_recognized()
            })
    }
}

/// The sign-in failure every client sees, whatever the cause.
pub fn not_recognized() -> ApiError {
    ApiError::new(
        ErrorCode::Unauthorized,
        "That passkey was not recognized. Try another one.",
    )
}

/// Force `authenticatorSelection.residentKey = "required"` and `requireResidentKey = true`.
pub fn require_resident_key(options: &mut serde_json::Value) {
    let Some(public_key) = options.get_mut("publicKey").and_then(|v| v.as_object_mut()) else {
        return;
    };
    let selection = public_key
        .entry("authenticatorSelection")
        .or_insert_with(|| serde_json::json!({"userVerification": "required"}));
    if let Some(selection) = selection.as_object_mut() {
        selection.insert("residentKey".into(), "required".into());
        selection.insert("requireResidentKey".into(), true.into());
        selection.insert("userVerification".into(), "required".into());
    }
}

/// `passkeys.credential_id`: unpadded base64url of the raw credential id.
pub fn credential_id_text(id: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(id)
}

/// A new random WebAuthn user handle (UUIDv4 layout, nothing derived from the account).
pub fn new_user_handle() -> Uuid {
    uuid::Builder::from_random_bytes(codes::random_bytes::<16>()).into_uuid()
}

/// Parse `users.webauthn_id`.
pub fn parse_user_handle(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text).ok()
}

/// A friendly default passkey name from the browser's user agent.
pub fn default_passkey_name(user_agent: Option<&str>) -> String {
    let ua = user_agent.unwrap_or_default();
    let device = if ua.contains("iPhone") {
        Some("iPhone")
    } else if ua.contains("iPad") {
        Some("iPad")
    } else if ua.contains("Android") {
        Some("Android")
    } else if ua.contains("Macintosh") || ua.contains("Mac OS X") {
        Some("Mac")
    } else if ua.contains("Windows") {
        Some("Windows")
    } else if ua.contains("CrOS") {
        Some("Chromebook")
    } else if ua.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    match device {
        Some(d) => format!("Passkey on {d}"),
        None => "Passkey".to_string(),
    }
}

/// Validate an optional passkey name; falls back to the user-agent default.
pub fn passkey_name(requested: Option<&str>, user_agent: Option<&str>) -> ApiResult<String> {
    match requested.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => super::users::clean_name(name, "Passkey name"),
        None => Ok(default_passkey_name(user_agent)),
    }
}

/// Origin check helper for tests: the relying party's allowed origins.
pub fn allowed_origins(state: &PasskeyState) -> Vec<Url> {
    state
        .webauthn
        .as_ref()
        .map(|w| w.get_allowed_origins().to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn config(url: Option<&str>) -> Config {
        let mut c = Config::for_tests(Path::new("/tmp/s"), Path::new("/tmp/c"));
        c.public_url = url.map(|u| url::Url::parse(u).unwrap());
        c
    }

    #[test]
    fn unavailable_without_secure_url() {
        for url in [
            None,
            Some("http://192.168.1.20:8788"),
            Some("http://stash.example.com"),
        ] {
            let state = PasskeyState::new(&config(url)).unwrap();
            assert!(!state.ready());
            let err = state.start_login(false).unwrap_err();
            assert_eq!(err.code, ErrorCode::PasskeysUnavailable);
            assert!(err.message.contains("STASH_PUBLIC_URL"), "{}", err.message);
            assert!(err.message.contains("HTTPS"));
        }
    }

    #[test]
    fn registration_requires_discoverable_credential() {
        let mut c = config(Some("https://stash.example.com"));
        c.extra_origins = vec![url::Url::parse("https://stash.example.com:8443").unwrap()];
        let state = PasskeyState::new(&c).unwrap();
        assert!(state.ready());
        assert_eq!(allowed_origins(&state).len(), 2);
        let (options, _reg) = state
            .start_registration(new_user_handle(), "Ana", "Ana", vec![])
            .unwrap();
        let pk = &options["publicKey"];
        assert_eq!(pk["rp"]["id"], "stash.example.com");
        assert_eq!(pk["rp"]["name"], RP_NAME);
        let sel = &pk["authenticatorSelection"];
        assert_eq!(sel["residentKey"], "required");
        assert_eq!(sel["requireResidentKey"], true);
        assert_eq!(sel["userVerification"], "required");
    }

    #[test]
    fn login_options_are_discoverable() {
        let state = PasskeyState::new(&config(Some("http://localhost:8788"))).unwrap();
        let (options, _) = state.start_login(false).unwrap();
        assert!(options.get("mediation").is_none());
        assert_eq!(options["publicKey"]["rpId"], "localhost");
        assert_eq!(options["publicKey"]["userVerification"], "required");
        let allow = &options["publicKey"]["allowCredentials"];
        assert!(allow.is_null() || allow.as_array().is_some_and(|a| a.is_empty()));
        let (options, _) = state.start_login(true).unwrap();
        assert_eq!(options["mediation"], "conditional");
    }

    #[test]
    fn flows_are_single_use() {
        let state = PasskeyState::new(&config(Some("http://localhost:8788"))).unwrap();
        let (_, auth) = state.start_login(false).unwrap();
        let id = state
            .begin(Flow::Login {
                auth: Box::new(auth),
            })
            .unwrap();
        assert_eq!(id.len(), 22);
        assert_eq!(state.pending_flows(), 1);
        assert!(state.take(&id).is_ok());
        assert!(state.take(&id).is_err());
        assert!(state.take("nope").is_err());
    }

    #[test]
    fn in_flight_ceremonies_are_capped() {
        let mut state = PasskeyState::new(&config(Some("http://localhost:8788"))).unwrap();
        state.max_flows = 3;
        let login = |state: &PasskeyState| {
            let (_, auth) = state.start_login(false).unwrap();
            state.begin(Flow::Login {
                auth: Box::new(auth),
            })
        };
        let first = login(&state).unwrap();
        login(&state).unwrap();
        login(&state).unwrap();
        let err = login(&state).unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
        assert_eq!(state.pending_flows(), 3);
        // A finished ceremony frees its slot.
        assert!(state.take(&first).is_ok());
        let fourth = login(&state).unwrap();
        // Expired ceremonies are dropped to make room, without waiting for the periodic sweep.
        let stale = Instant::now()
            .checked_sub(FLOW_TTL + Duration::from_secs(1))
            .unwrap();
        state.flows.get_mut(&fourth).unwrap().created = stale;
        assert!(login(&state).is_ok());
        assert_eq!(state.pending_flows(), 3);
        assert!(state.take(&fourth).is_err());
    }

    #[test]
    fn passkey_names() {
        let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit";
        assert_eq!(default_passkey_name(Some(iphone)), "Passkey on iPhone");
        assert_eq!(default_passkey_name(None), "Passkey");
        assert_eq!(
            passkey_name(Some("  Kitchen iPad "), None).unwrap(),
            "Kitchen iPad"
        );
        assert_eq!(
            passkey_name(Some("  "), Some("Windows NT")).unwrap(),
            "Passkey on Windows"
        );
    }

    #[test]
    fn user_handles_are_random_uuids() {
        let a = new_user_handle();
        assert_ne!(a, new_user_handle());
        assert_eq!(a.get_version_num(), 4);
        assert_eq!(parse_user_handle(&a.to_string()), Some(a));
    }
}
