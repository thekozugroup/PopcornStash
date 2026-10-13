//! Tunnel configuration and bring-up.
//!
//! `load` reads `<config>/torrent/vpn` (written by the web server) into a
//! validated [`TunnelSpec`]; [`Tunnel`] runs it and reports health.

pub mod config;
pub mod dns;
pub mod killswitch;
pub mod openvpn;
pub mod routes;
pub mod wireguard;

use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use stash_core::internal_api::{
    OPENVPN_AUTH_FILE, OPENVPN_FILE, VPN_META_FILE, VpnMeta, VpnProtocol, VpnProvider,
    WIREGUARD_FILE,
};

use self::config::{ConfigError, OpenVpnPlan, WireGuardPlan};
use self::openvpn::{OpenVpnState, OpenVpnTunnel};
use self::wireguard::WireGuardTunnel;

/// What to bring up.
#[derive(Debug, Clone)]
pub enum TunnelSpec {
    WireGuard(WireGuardPlan),
    /// The validated text itself, not paths: OpenVPN (root) is handed a
    /// root-only copy, so the web server's user cannot swap the file between
    /// the check and OpenVPN reading it.
    OpenVpn {
        plan: OpenVpnPlan,
        config_text: String,
        auth: Secret,
    },
}

/// A secret string whose `Debug` output is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The VPN directory's contents, validated.
#[derive(Debug, Clone)]
pub struct VpnSource {
    pub meta: Option<VpnMeta>,
    pub spec: TunnelSpec,
}

impl VpnSource {
    pub fn protocol(&self) -> VpnProtocol {
        match self.spec {
            TunnelSpec::WireGuard(_) => VpnProtocol::Wireguard,
            TunnelSpec::OpenVpn { .. } => VpnProtocol::Openvpn,
        }
    }

    pub fn provider(&self) -> VpnProvider {
        self.meta
            .as_ref()
            .map(|m| m.provider)
            .unwrap_or(match self.protocol() {
                VpnProtocol::Wireguard => VpnProvider::Wireguard,
                VpnProtocol::Openvpn => VpnProvider::Openvpn,
            })
    }

    pub fn server(&self) -> Option<String> {
        self.meta
            .as_ref()
            .and_then(|m| m.server.clone())
            .or_else(|| match &self.spec {
                TunnelSpec::WireGuard(plan) => plan.server(),
                TunnelSpec::OpenVpn { plan, .. } => plan.server(),
            })
    }

    /// DNS servers known before bring-up (WireGuard `DNS=`).
    pub fn configured_dns(&self) -> Vec<IpAddr> {
        match &self.spec {
            TunnelSpec::WireGuard(plan) => plan.dns.clone(),
            TunnelSpec::OpenVpn { .. } => Vec::new(),
        }
    }
}

/// Largest file read from the VPN folder.
pub const MAX_VPN_FILE: u64 = 512 * 1024;

/// Read a file from the VPN folder as root. The folder belongs to the web
/// server's user, so the file is opened without following a symlink and
/// without blocking (a FIFO), and only a small regular file is read.
fn read_optional(path: &Path) -> Result<Option<String>, ConfigError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(nix::libc::ELOOP) => {
            return Err(ConfigError(format!(
                "{name} in the VPN folder is a symbolic link. Upload the VPN settings again."
            )));
        }
        Err(e) => {
            return Err(ConfigError(format!(
                "Could not read {}: {e}",
                path.display()
            )));
        }
    };
    let meta = file
        .metadata()
        .map_err(|e| ConfigError(format!("Could not read {}: {e}", path.display())))?;
    if !meta.is_file() {
        return Err(ConfigError(format!(
            "{name} in the VPN folder is not a regular file. Upload the VPN settings again."
        )));
    }
    if meta.len() > MAX_VPN_FILE {
        return Err(ConfigError(format!(
            "{name} in the VPN folder is too large."
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_VPN_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ConfigError(format!("Could not read {}: {e}", path.display())))?;
    if bytes.len() as u64 > MAX_VPN_FILE {
        return Err(ConfigError(format!(
            "{name} in the VPN folder is too large."
        )));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| ConfigError(format!("{name} in the VPN folder is not a text file.")))
}

fn present(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Read and validate the VPN directory. `Ok(None)` when nothing is configured.
/// `vpn.json` chooses the protocol when present; otherwise whichever file exists.
pub fn load(vpn_dir: &Path) -> Result<Option<VpnSource>, ConfigError> {
    let meta = match read_optional(&vpn_dir.join(VPN_META_FILE))? {
        Some(text) => Some(
            serde_json::from_str::<VpnMeta>(&text)
                .map_err(|_| ConfigError("vpn.json is not valid.".into()))?,
        ),
        None => None,
    };
    let wg_path = vpn_dir.join(WIREGUARD_FILE);
    let ovpn_path = vpn_dir.join(OPENVPN_FILE);
    let auth_path = vpn_dir.join(OPENVPN_AUTH_FILE);
    let protocol = match meta.as_ref().map(|m| m.protocol) {
        Some(p) => Some(p),
        None if present(&wg_path) => Some(VpnProtocol::Wireguard),
        None if present(&ovpn_path) => Some(VpnProtocol::Openvpn),
        None => None,
    };
    let spec = match protocol {
        None => return Ok(None),
        Some(VpnProtocol::Wireguard) => {
            let Some(text) = read_optional(&wg_path)? else {
                return Err(ConfigError("The WireGuard file is missing.".into()));
            };
            TunnelSpec::WireGuard(WireGuardPlan::parse(&text)?)
        }
        Some(VpnProtocol::Openvpn) => {
            let Some(text) = read_optional(&ovpn_path)? else {
                return Err(ConfigError("The OpenVPN file is missing.".into()));
            };
            let plan = OpenVpnPlan::parse(&text)?;
            let auth = read_optional(&auth_path)?.ok_or_else(|| {
                ConfigError("The OpenVPN username and password are missing.".into())
            })?;
            let mut lines = auth.lines();
            let user_ok = lines.next().is_some_and(|l| !l.trim().is_empty());
            let pass_ok = lines.next().is_some_and(|l| !l.is_empty());
            if !user_ok || !pass_ok {
                return Err(ConfigError(
                    "The OpenVPN username and password file is incomplete.".into(),
                ));
            }
            TunnelSpec::OpenVpn {
                plan,
                config_text: text,
                auth: Secret(auth),
            }
        }
    };
    Ok(Some(VpnSource { meta, spec }))
}

/// Tunnel health at one point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Usable. For WireGuard, `session_left` is how long the current keys stay valid
    /// without a new handshake (180 s after the last one).
    Up {
        session_left: Option<Duration>,
    },
    /// Still waiting for the first handshake / initialisation.
    Pending,
    Down(String),
}

#[derive(Debug)]
pub enum Tunnel {
    WireGuard(WireGuardTunnel),
    OpenVpn(OpenVpnTunnel),
}

impl Tunnel {
    pub async fn bring_up(
        source: &VpnSource,
        ifname: &str,
        run_dir: &Path,
        ipv6: bool,
    ) -> Result<Self, String> {
        match &source.spec {
            TunnelSpec::WireGuard(plan) => WireGuardTunnel::bring_up(plan, ifname, ipv6)
                .await
                .map(Tunnel::WireGuard)
                .map_err(|e| e.to_string()),
            TunnelSpec::OpenVpn {
                config_text, auth, ..
            } => OpenVpnTunnel::bring_up(config_text, &auth.0, run_dir, ifname)
                .await
                .map(Tunnel::OpenVpn),
        }
    }

    pub async fn health(&mut self, now: i64) -> Health {
        match self {
            Tunnel::WireGuard(wg) => {
                if !wg.process_alive() {
                    return Health::Down("boringtun stopped.".into());
                }
                match wg.handshake_age(now).await {
                    Ok(age) if wireguard::handshake_fresh(age) => Health::Up {
                        session_left: age.map(wireguard::session_left),
                    },
                    Ok(None) => Health::Pending,
                    Ok(Some(_)) => Health::Down("No recent handshake with the VPN server.".into()),
                    Err(e) => Health::Down(format!(
                        "Could not read the WireGuard status: {}",
                        e.message
                    )),
                }
            }
            Tunnel::OpenVpn(ovpn) => match ovpn.state() {
                OpenVpnState::Ready => Health::Up { session_left: None },
                OpenVpnState::Starting | OpenVpnState::Reconnecting => Health::Pending,
                OpenVpnState::Failed(reason) => Health::Down(reason),
            },
        }
    }

    /// DNS servers the tunnel provides right now.
    pub fn dns_servers(&self, source: &VpnSource) -> Vec<IpAddr> {
        match self {
            Tunnel::WireGuard(_) => source.configured_dns(),
            Tunnel::OpenVpn(ovpn) => ovpn.dns_servers(),
        }
    }

    pub async fn teardown(self) {
        match self {
            Tunnel::WireGuard(wg) => wg.teardown().await,
            Tunnel::OpenVpn(ovpn) => ovpn.teardown().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WG: &str = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.5.0.2/32\nDNS = 103.86.96.100\n[Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nEndpoint = wg.example:51820\nAllowedIPs = 0.0.0.0/0\n";

    #[test]
    fn load_handles_absent_and_present_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_none());

        std::fs::write(dir.path().join(WIREGUARD_FILE), WG).unwrap();
        let source = load(dir.path()).unwrap().unwrap();
        assert_eq!(source.protocol(), VpnProtocol::Wireguard);
        assert_eq!(source.provider(), VpnProvider::Wireguard);
        assert_eq!(source.server().as_deref(), Some("wg.example"));
        assert_eq!(
            source.configured_dns(),
            vec!["103.86.96.100".parse::<IpAddr>().unwrap()]
        );

        let meta = r#"{"provider":"nordvpn","protocol":"wireguard","label":"NordVPN Netherlands #812","server":"nl812.nordvpn.com","country":"Netherlands","created_at":1}"#;
        std::fs::write(dir.path().join(VPN_META_FILE), meta).unwrap();
        let source = load(dir.path()).unwrap().unwrap();
        assert_eq!(source.provider(), VpnProvider::Nordvpn);
        assert_eq!(source.server().as_deref(), Some("nl812.nordvpn.com"));

        // vpn.json says OpenVPN but the files are missing.
        std::fs::write(
            dir.path().join(VPN_META_FILE),
            meta.replace("\"protocol\":\"wireguard\"", "\"protocol\":\"openvpn\""),
        )
        .unwrap();
        assert!(load(dir.path()).is_err());
        std::fs::write(
            dir.path().join(OPENVPN_FILE),
            "client\nremote a.example 1194\nauth-user-pass\n",
        )
        .unwrap();
        assert!(load(dir.path()).unwrap_err().0.contains("username"));
        std::fs::write(dir.path().join(OPENVPN_AUTH_FILE), "user\npass\n").unwrap();
        let source = load(dir.path()).unwrap().unwrap();
        assert_eq!(source.protocol(), VpnProtocol::Openvpn);
        assert!(source.configured_dns().is_empty());
    }

    /// The VPN folder belongs to the web server's user; root must not follow
    /// links out of it, hang on a FIFO or read an endless file.
    #[test]
    fn load_reads_only_small_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere.conf");
        std::fs::write(&elsewhere, WG).unwrap();
        let vpn = dir.path().join("vpn");
        std::fs::create_dir(&vpn).unwrap();
        std::os::unix::fs::symlink(&elsewhere, vpn.join(WIREGUARD_FILE)).unwrap();
        let e = load(&vpn).unwrap_err();
        assert!(e.0.contains("symbolic link"), "{e}");

        std::fs::remove_file(vpn.join(WIREGUARD_FILE)).unwrap();
        nix::unistd::mkfifo(&vpn.join(WIREGUARD_FILE), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let started = std::time::Instant::now();
        let e = load(&vpn).unwrap_err();
        assert!(e.0.contains("not a regular file"), "{e}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));

        std::fs::remove_file(vpn.join(WIREGUARD_FILE)).unwrap();
        let big = format!("{WG}{}", "#".repeat(MAX_VPN_FILE as usize));
        std::fs::write(vpn.join(WIREGUARD_FILE), big).unwrap();
        assert!(load(&vpn).unwrap_err().0.contains("too large"));
    }

    /// OpenVPN gets the text that was checked, not a path the web server's
    /// user can swap after the check.
    #[test]
    fn openvpn_spec_keeps_the_validated_text() {
        let dir = tempfile::tempdir().unwrap();
        let good = "client\nremote a.example 1194\nauth-user-pass\n";
        std::fs::write(dir.path().join(OPENVPN_FILE), good).unwrap();
        std::fs::write(dir.path().join(OPENVPN_AUTH_FILE), "user\npass\n").unwrap();
        let source = load(dir.path()).unwrap().unwrap();
        std::fs::write(
            dir.path().join(OPENVPN_FILE),
            "client\nremote a.example 1194\nroute-up /bin/sh\n",
        )
        .unwrap();
        let TunnelSpec::OpenVpn {
            config_text, auth, ..
        } = &source.spec
        else {
            panic!("expected OpenVPN");
        };
        assert_eq!(config_text, good);
        assert_eq!(auth.0, "user\npass\n");
        assert!(!format!("{:?}", source.spec).contains("pass\n"));
    }

    #[test]
    fn load_refuses_hooks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(WIREGUARD_FILE),
            WG.replace("[Peer]", "PostUp = sh -c evil\n[Peer]"),
        )
        .unwrap();
        assert!(load(dir.path()).is_err());
    }
}
