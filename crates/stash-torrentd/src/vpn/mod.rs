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
use std::path::{Path, PathBuf};

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
    OpenVpn {
        plan: OpenVpnPlan,
        config: PathBuf,
        auth: PathBuf,
    },
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

fn read_optional(path: &Path) -> Result<Option<String>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError(format!(
            "Could not read {}: {e}",
            path.display()
        ))),
    }
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
        None if wg_path.exists() => Some(VpnProtocol::Wireguard),
        None if ovpn_path.exists() => Some(VpnProtocol::Openvpn),
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
                config: ovpn_path,
                auth: auth_path,
            }
        }
    };
    Ok(Some(VpnSource { meta, spec }))
}

/// Tunnel health at one point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Up,
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
            TunnelSpec::OpenVpn { config, auth, .. } => {
                OpenVpnTunnel::bring_up(config, auth, run_dir, ifname)
                    .await
                    .map(Tunnel::OpenVpn)
            }
        }
    }

    pub async fn health(&mut self, now: i64) -> Health {
        match self {
            Tunnel::WireGuard(wg) => {
                if !wg.process_alive() {
                    return Health::Down("boringtun stopped.".into());
                }
                match wg.handshake_age(now).await {
                    Ok(age) if wireguard::handshake_fresh(age) => Health::Up,
                    Ok(None) => Health::Pending,
                    Ok(Some(_)) => Health::Down("No recent handshake with the VPN server.".into()),
                    Err(e) => Health::Down(format!(
                        "Could not read the WireGuard status: {}",
                        e.message
                    )),
                }
            }
            Tunnel::OpenVpn(ovpn) => match ovpn.state() {
                OpenVpnState::Ready => Health::Up,
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
