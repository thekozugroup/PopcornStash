//! Parsing wg-quick and .ovpn files into validated tunnel plans.
//!
//! The supervisor runs these files as root, so this module re-validates them
//! (the web server validates on upload too): no wg-quick script hooks, no
//! OpenVPN directives that run programs, load code, write files or read
//! other files. Error messages are plain sentences for the UI and never
//! contain key material.

use std::fmt;
use std::net::IpAddr;

use base64::Engine as _;
use ipnet::IpNet;

/// MTU used when the WireGuard file does not set one (section 5, step 5).
pub const DEFAULT_MTU: u32 = 1420;
/// Keepalive forced on every peer when the file has none, so the handshake age
/// stays meaningful for health checks and NAT mappings stay open.
pub const DEFAULT_KEEPALIVE: u16 = 25;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

fn err(message: impl Into<String>) -> ConfigError {
    ConfigError(message.into())
}

/// A validated WireGuard client configuration.
#[derive(Clone, PartialEq, Eq)]
pub struct WireGuardPlan {
    /// Base64 private key. Never logged.
    pub private_key: String,
    pub addresses: Vec<IpNet>,
    pub dns: Vec<IpAddr>,
    pub mtu: Option<u32>,
    pub listen_port: Option<u16>,
    pub peers: Vec<WireGuardPeer>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct WireGuardPeer {
    pub public_key: String,
    /// Never logged.
    pub preshared_key: Option<String>,
    /// `host:port` as written (wg resolves host names itself).
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<IpNet>,
    pub persistent_keepalive: Option<u16>,
}

impl fmt::Debug for WireGuardPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireGuardPlan")
            .field("private_key", &"<redacted>")
            .field("addresses", &self.addresses)
            .field("dns", &self.dns)
            .field("mtu", &self.mtu)
            .field("listen_port", &self.listen_port)
            .field("peers", &self.peers)
            .finish()
    }
}

impl fmt::Debug for WireGuardPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireGuardPeer")
            .field("public_key", &self.public_key)
            .field(
                "preshared_key",
                &self.preshared_key.as_ref().map(|_| "<redacted>"),
            )
            .field("endpoint", &self.endpoint)
            .field("allowed_ips", &self.allowed_ips)
            .field("persistent_keepalive", &self.persistent_keepalive)
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Interface,
    Peer,
}

fn valid_key(value: &str) -> bool {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map(|bytes| bytes.len() == 32)
        .unwrap_or(false)
}

fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty())
}

fn parse_net(value: &str, what: &str) -> Result<IpNet, ConfigError> {
    if let Ok(net) = value.parse::<IpNet>() {
        return Ok(net);
    }
    value.parse::<IpAddr>().map(IpNet::from).map_err(|_| {
        err(format!(
            "The WireGuard file has an invalid {what}: {value:?}."
        ))
    })
}

fn valid_endpoint(value: &str) -> bool {
    // host:port or [v6]:port
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        match rest.split_once("]:") {
            Some((host, port)) => (host, port),
            None => return false,
        }
    } else {
        match value.rsplit_once(':') {
            Some(pair) => pair,
            None => return false,
        }
    };
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'))
        && port.parse::<u16>().is_ok_and(|p| p != 0)
}

impl WireGuardPlan {
    /// Parse a wg-quick file. Refuses script hooks (PreUp/PostUp/PreDown/PostDown)
    /// and unknown keys; ignores SaveConfig, Table and FwMark (the supervisor owns routing).
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut section = Section::None;
        let mut interfaces = 0;
        let mut private_key = None;
        let mut addresses = Vec::new();
        let mut dns = Vec::new();
        let mut mtu = None;
        let mut listen_port = None;
        let mut peers: Vec<WireGuardPeer> = Vec::new();

        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                match line.to_ascii_lowercase().as_str() {
                    "[interface]" => {
                        interfaces += 1;
                        section = Section::Interface;
                    }
                    "[peer]" => {
                        peers.push(WireGuardPeer {
                            public_key: String::new(),
                            preshared_key: None,
                            endpoint: None,
                            allowed_ips: Vec::new(),
                            persistent_keepalive: None,
                        });
                        section = Section::Peer;
                    }
                    _ => {
                        return Err(err(format!(
                            "The WireGuard file has an unknown section on line {line_no}."
                        )));
                    }
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(err(format!(
                    "The WireGuard file has an invalid line {line_no}."
                )));
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match (section, key.as_str()) {
                (_, "preup" | "postup" | "predown" | "postdown") => {
                    return Err(err(
                        "The WireGuard file contains script hooks (PreUp/PostUp/PreDown/PostDown). Remove them and upload it again.",
                    ));
                }
                (Section::None, _) => {
                    return Err(err(format!(
                        "The WireGuard file has a setting outside a section on line {line_no}."
                    )));
                }
                (Section::Interface, "privatekey") => {
                    if !valid_key(value) {
                        return Err(err("The WireGuard file has an invalid PrivateKey."));
                    }
                    private_key = Some(value.to_string());
                }
                (Section::Interface, "address") => {
                    for item in split_list(value) {
                        addresses.push(parse_net(item, "Address")?);
                    }
                }
                (Section::Interface, "dns") => {
                    // Entries that are not IP addresses are search domains; they do not apply here.
                    dns.extend(split_list(value).filter_map(|item| item.parse::<IpAddr>().ok()));
                }
                (Section::Interface, "mtu") => {
                    let parsed = value
                        .parse::<u32>()
                        .ok()
                        .filter(|m| (1280..=9000).contains(m))
                        .ok_or_else(|| err("The WireGuard file has an invalid MTU."))?;
                    mtu = Some(parsed);
                }
                (Section::Interface, "listenport") => {
                    listen_port = Some(
                        value
                            .parse::<u16>()
                            .map_err(|_| err("The WireGuard file has an invalid ListenPort."))?,
                    );
                }
                (Section::Interface, "saveconfig" | "table" | "fwmark") => {}
                (Section::Peer, field) => {
                    let peer = peers
                        .last_mut()
                        .ok_or_else(|| err("The WireGuard file has an empty peer."))?;
                    match field {
                        "publickey" => {
                            if !valid_key(value) {
                                return Err(err(
                                    "The WireGuard file has an invalid peer PublicKey.",
                                ));
                            }
                            peer.public_key = value.to_string();
                        }
                        "presharedkey" => {
                            if !valid_key(value) {
                                return Err(err("The WireGuard file has an invalid PresharedKey."));
                            }
                            peer.preshared_key = Some(value.to_string());
                        }
                        "endpoint" => {
                            if !valid_endpoint(value) {
                                return Err(err(format!(
                                    "The WireGuard file has an invalid Endpoint: {value:?}."
                                )));
                            }
                            peer.endpoint = Some(value.to_string());
                        }
                        "allowedips" => {
                            for item in split_list(value) {
                                peer.allowed_ips.push(parse_net(item, "AllowedIPs entry")?);
                            }
                        }
                        "persistentkeepalive" => {
                            let parsed = if value.eq_ignore_ascii_case("off") {
                                0
                            } else {
                                value.parse::<u16>().map_err(|_| {
                                    err("The WireGuard file has an invalid PersistentKeepalive.")
                                })?
                            };
                            peer.persistent_keepalive = Some(parsed);
                        }
                        other => {
                            return Err(err(format!(
                                "The WireGuard file has an unsupported peer setting {other:?}."
                            )));
                        }
                    }
                }
                (Section::Interface, other) => {
                    return Err(err(format!(
                        "The WireGuard file has an unsupported setting {other:?}."
                    )));
                }
            }
        }

        if interfaces != 1 {
            return Err(err(
                "The WireGuard file needs exactly one [Interface] section.",
            ));
        }
        let private_key =
            private_key.ok_or_else(|| err("The WireGuard file has no PrivateKey."))?;
        if addresses.is_empty() {
            return Err(err("The WireGuard file has no Address."));
        }
        if peers.is_empty() {
            return Err(err("The WireGuard file has no [Peer] section."));
        }
        for peer in &peers {
            if peer.public_key.is_empty() {
                return Err(err("A peer in the WireGuard file has no PublicKey."));
            }
            if peer.endpoint.is_none() {
                return Err(err("A peer in the WireGuard file has no Endpoint."));
            }
            if peer.allowed_ips.is_empty() {
                return Err(err("A peer in the WireGuard file has no AllowedIPs."));
            }
        }
        Ok(Self {
            private_key,
            addresses,
            dns,
            mtu,
            listen_port,
            peers,
        })
    }

    /// The `wg setconf` text: only the keys `wg` understands (no Address/DNS/MTU,
    /// no hooks). Peers without a keepalive get [`DEFAULT_KEEPALIVE`].
    pub fn setconf_text(&self) -> String {
        let mut out = String::from("[Interface]\n");
        out.push_str(&format!("PrivateKey = {}\n", self.private_key));
        if let Some(port) = self.listen_port {
            out.push_str(&format!("ListenPort = {port}\n"));
        }
        for peer in &self.peers {
            out.push_str("\n[Peer]\n");
            out.push_str(&format!("PublicKey = {}\n", peer.public_key));
            if let Some(psk) = &peer.preshared_key {
                out.push_str(&format!("PresharedKey = {psk}\n"));
            }
            if let Some(endpoint) = &peer.endpoint {
                out.push_str(&format!("Endpoint = {endpoint}\n"));
            }
            let allowed: Vec<String> = peer.allowed_ips.iter().map(ToString::to_string).collect();
            out.push_str(&format!("AllowedIPs = {}\n", allowed.join(", ")));
            let keepalive = match peer.persistent_keepalive {
                Some(0) | None => DEFAULT_KEEPALIVE,
                Some(k) => k,
            };
            out.push_str(&format!("PersistentKeepalive = {keepalive}\n"));
        }
        out
    }

    pub fn mtu(&self) -> u32 {
        self.mtu.unwrap_or(DEFAULT_MTU)
    }

    /// Host part of the first peer's endpoint, for the status display.
    pub fn server(&self) -> Option<String> {
        let endpoint = self.peers.first()?.endpoint.as_deref()?;
        let host = if let Some(rest) = endpoint.strip_prefix('[') {
            rest.split_once(']').map(|(h, _)| h)?
        } else {
            endpoint.rsplit_once(':').map(|(h, _)| h)?
        };
        Some(host.to_string())
    }

    pub fn has_ipv6(&self) -> bool {
        self.addresses.iter().any(|a| matches!(a, IpNet::V6(_)))
    }
}

/// One `remote` line of an .ovpn file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenVpnRemote {
    pub host: String,
    pub port: Option<u16>,
    pub proto: Option<String>,
}

/// What the supervisor needs to know about an .ovpn file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenVpnPlan {
    pub remotes: Vec<OpenVpnRemote>,
    pub proto: Option<String>,
    pub port: Option<u16>,
}

/// Directives that run programs, load code, write files, change directories or
/// pull in other configuration. Refused outright.
const OPENVPN_FORBIDDEN: &[&str] = &[
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
    "tls-crypt-v2-verify",
    "auth-user-pass-verify",
    "iproute",
    "daemon",
    "writepid",
    "cd",
    "chroot",
    "config",
    "status",
    "tmp-dir",
    "askpass",
    "engine",
    "providers",
    "pkcs11-providers",
    "dev-node",
    "replay-persist",
    "tls-export-cert",
    "auth-gen-token-secret",
];

/// Directives whose first argument names a file. Only inline blocks (`[inline]`)
/// are accepted: the upload is a single file, and root must not read arbitrary paths.
const OPENVPN_FILE_ARGS: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "crl-verify",
    "secret",
    "pkcs12",
    "dh",
    "extra-certs",
    "http-proxy-user-pass",
    "capath",
];

fn openvpn_forbidden(name: &str) -> bool {
    OPENVPN_FORBIDDEN.contains(&name) || name.starts_with("management") || name.starts_with("log")
}

/// Split an OpenVPN config line into words, honouring double/single quotes and backslash escapes.
fn openvpn_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, '#' | ';') if !in_word => break,
            (None, c) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

impl OpenVpnPlan {
    /// Parse and validate an .ovpn file.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut remotes = Vec::new();
        let mut proto = None;
        let mut port = None;
        // Inline blocks other than <connection> hold certificates and keys; skip them.
        let mut skipping: Option<String> = None;

        for raw in text.lines() {
            let line = raw.trim();
            if let Some(tag) = &skipping {
                if line.eq_ignore_ascii_case(&format!("</{tag}>")) {
                    skipping = None;
                }
                continue;
            }
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(tag) = line.strip_prefix("</") {
                let tag = tag.trim_end_matches('>').to_ascii_lowercase();
                if tag == "connection" {
                    continue;
                }
                return Err(err("The OpenVPN file has an unexpected closing tag."));
            }
            if let Some(tag) = line.strip_prefix('<') {
                let tag = tag.trim_end_matches('>').trim().to_ascii_lowercase();
                if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Err(err("The OpenVPN file has an invalid inline block."));
                }
                if tag != "connection" {
                    skipping = Some(tag);
                }
                continue;
            }
            let words = openvpn_words(line);
            let Some(first) = words.first() else { continue };
            let name = first.trim_start_matches("--").to_ascii_lowercase();
            let args = &words[1..];
            if openvpn_forbidden(&name) {
                return Err(err(format!(
                    "The OpenVPN file uses \"{name}\", which PopcornStash does not allow because it runs programs or writes files. Remove that line and upload it again."
                )));
            }
            if name == "auth-user-pass" && !args.is_empty() {
                return Err(err(
                    "The OpenVPN file names a password file in auth-user-pass. Remove the file name; PopcornStash supplies the username and password itself.",
                ));
            }
            if OPENVPN_FILE_ARGS.contains(&name.as_str())
                && args.first().is_some_and(|a| a != "[inline]")
            {
                return Err(err(format!(
                    "The OpenVPN file refers to a separate file for \"{name}\". Use a single .ovpn file with certificates and keys inline."
                )));
            }
            match name.as_str() {
                "remote" => {
                    let host = args.first().ok_or_else(|| {
                        err("The OpenVPN file has a remote line without a server.")
                    })?;
                    if !host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'))
                    {
                        return Err(err(format!(
                            "The OpenVPN file has an invalid remote server: {host:?}."
                        )));
                    }
                    let remote_port = match args.get(1) {
                        Some(p) => Some(
                            p.parse::<u16>()
                                .map_err(|_| err("The OpenVPN file has an invalid remote port."))?,
                        ),
                        None => None,
                    };
                    remotes.push(OpenVpnRemote {
                        host: host.clone(),
                        port: remote_port,
                        proto: args.get(2).map(|p| p.to_ascii_lowercase()),
                    });
                }
                "proto" => proto = args.first().map(|p| p.to_ascii_lowercase()),
                "port" | "rport" => {
                    port = Some(
                        args.first()
                            .and_then(|p| p.parse::<u16>().ok())
                            .ok_or_else(|| err("The OpenVPN file has an invalid port."))?,
                    );
                }
                _ => {}
            }
        }
        if let Some(tag) = skipping {
            return Err(err(format!(
                "The OpenVPN file has an unterminated <{tag}> block."
            )));
        }
        if remotes.is_empty() {
            return Err(err("The OpenVPN file has no remote server."));
        }
        Ok(Self {
            remotes,
            proto,
            port,
        })
    }

    pub fn server(&self) -> Option<String> {
        self.remotes.first().map(|r| r.host.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    const KEY_B: &str = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";
    const KEY_C: &str = "HIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw=";

    fn sample() -> String {
        format!(
            "# NordVPN\n[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.5.0.2/32\nDNS = 103.86.96.100, 103.86.99.100, corp.example\n\n[Peer]\nPublicKey = {KEY_B}\nAllowedIPs = 0.0.0.0/0, ::/0\nEndpoint = nl812.nordvpn.com:51820\n"
        )
    }

    #[test]
    fn parses_nordvpn_style_file() {
        let plan = WireGuardPlan::parse(&sample()).unwrap();
        assert_eq!(
            plan.addresses,
            vec!["10.5.0.2/32".parse::<IpNet>().unwrap()]
        );
        assert_eq!(plan.dns.len(), 2);
        assert_eq!(plan.mtu(), DEFAULT_MTU);
        assert_eq!(plan.peers[0].allowed_ips.len(), 2);
        assert_eq!(plan.server().as_deref(), Some("nl812.nordvpn.com"));
        assert!(!plan.has_ipv6());
    }

    #[test]
    fn setconf_strips_wg_quick_keys_and_adds_keepalive() {
        let text = sample().replace(
            "[Peer]",
            "Table = off\nMTU = 1380\nSaveConfig = true\nFwMark = 0x1234\n[Peer]",
        );
        let plan = WireGuardPlan::parse(&text).unwrap();
        let conf = plan.setconf_text();
        assert_eq!(
            conf,
            format!(
                "[Interface]\nPrivateKey = {KEY_A}\n\n[Peer]\nPublicKey = {KEY_B}\nEndpoint = nl812.nordvpn.com:51820\nAllowedIPs = 0.0.0.0/0, ::/0\nPersistentKeepalive = 25\n"
            )
        );
        assert!(!conf.contains("Address"));
        assert!(!conf.contains("DNS"));
        assert_eq!(plan.mtu(), 1380);
    }

    #[test]
    fn keeps_psk_and_keepalive_and_ipv6() {
        let text = format!(
            "[Interface]\nPrivateKey={KEY_A}\nAddress=10.0.0.2/32,fd00::2/128\nListenPort=51000\n[Peer]\nPublicKey={KEY_B}\nPresharedKey={KEY_C}\nEndpoint=[2001:db8::1]:51820\nAllowedIPs=0.0.0.0/0\nPersistentKeepalive=15\n"
        );
        let plan = WireGuardPlan::parse(&text).unwrap();
        assert!(plan.has_ipv6());
        assert_eq!(plan.server().as_deref(), Some("2001:db8::1"));
        let conf = plan.setconf_text();
        assert!(conf.contains(&format!("PresharedKey = {KEY_C}")));
        assert!(conf.contains("ListenPort = 51000"));
        assert!(conf.contains("PersistentKeepalive = 15"));
    }

    #[test]
    fn refuses_hooks_and_unknown_keys() {
        for hook in [
            "PostUp = iptables -F",
            "PreUp=curl x|sh",
            "predown = rm -rf /",
            "PostDown = true",
        ] {
            let text = sample().replace("[Peer]", &format!("{hook}\n[Peer]"));
            let e = WireGuardPlan::parse(&text).unwrap_err();
            assert!(e.0.contains("script hooks"), "{hook}: {e}");
        }
        let text = sample().replace("[Peer]", "Frobnicate = 1\n[Peer]");
        assert!(WireGuardPlan::parse(&text).is_err());
    }

    #[test]
    fn refuses_incomplete_files_without_leaking_keys() {
        assert!(WireGuardPlan::parse("").is_err());
        let no_peer = format!("[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.0.0.2/32\n");
        assert!(
            WireGuardPlan::parse(&no_peer)
                .unwrap_err()
                .0
                .contains("[Peer]")
        );
        let bad_key = sample().replace(KEY_A, "notakey");
        let e = WireGuardPlan::parse(&bad_key).unwrap_err();
        assert!(!e.0.contains("notakey"));
        let no_endpoint = sample().replace("Endpoint = nl812.nordvpn.com:51820\n", "");
        assert!(WireGuardPlan::parse(&no_endpoint).is_err());
        let bad_endpoint = sample().replace("nl812.nordvpn.com:51820", "host;rm:1");
        assert!(WireGuardPlan::parse(&bad_endpoint).is_err());
        let debug = format!("{:?}", WireGuardPlan::parse(&sample()).unwrap());
        assert!(!debug.contains(KEY_A));
    }

    fn ovpn(extra: &str) -> String {
        format!(
            "client\ndev tun\nproto udp\nremote nl812.nordvpn.com 1194\nremote 1.2.3.4 443 tcp\nresolv-retry infinite\nnobind\nauth-user-pass\n{extra}\n<ca>\n-----BEGIN CERTIFICATE-----\nup /bin/sh\n-----END CERTIFICATE-----\n</ca>\nkey-direction 1\n<tls-auth>\nlog /tmp/x\n</tls-auth>\n"
        )
    }

    #[test]
    fn parses_ovpn_remotes() {
        let plan = OpenVpnPlan::parse(&ovpn("")).unwrap();
        assert_eq!(plan.remotes.len(), 2);
        assert_eq!(
            plan.remotes[0],
            OpenVpnRemote {
                host: "nl812.nordvpn.com".into(),
                port: Some(1194),
                proto: None
            }
        );
        assert_eq!(plan.remotes[1].proto.as_deref(), Some("tcp"));
        assert_eq!(plan.proto.as_deref(), Some("udp"));
        assert_eq!(plan.server().as_deref(), Some("nl812.nordvpn.com"));
    }

    #[test]
    fn refuses_dangerous_ovpn_directives() {
        for line in [
            "up /tmp/x.sh",
            "down /tmp/x.sh",
            "route-up /x",
            "route-pre-down /x",
            "ipchange /x",
            "script-security 3",
            "plugin /lib/evil.so",
            "client-connect /x",
            "client-disconnect /x",
            "learn-address /x",
            "tls-verify /x",
            "auth-user-pass-verify /x via-env",
            "iproute /bin/sh",
            "management 0.0.0.0 7505",
            "management-hold",
            "daemon",
            "log /tmp/l",
            "log-append /tmp/l",
            "writepid /tmp/p",
            "cd /",
            "chroot /",
            "config /etc/other.conf",
            "status /tmp/s",
            "tmp-dir /tmp",
            "askpass /etc/shadow",
            "auth-user-pass /etc/shadow",
            "ca /etc/shadow",
            "--up /x",
            "UP /x",
            "  up   \"/tmp/a b\"",
            "engine dynamic",
            "dev-node /dev/sda",
        ] {
            assert!(
                OpenVpnPlan::parse(&ovpn(line)).is_err(),
                "{line} should be refused"
            );
        }
        // Inline blocks may contain anything; they are certificate data.
        assert!(OpenVpnPlan::parse(&ovpn("tls-auth [inline] 1")).is_ok());
        // Comments are not directives.
        assert!(OpenVpnPlan::parse(&ovpn("# up /x\n; down /y")).is_ok());
    }

    #[test]
    fn refuses_broken_ovpn() {
        assert!(OpenVpnPlan::parse("client\n").is_err());
        assert!(OpenVpnPlan::parse("remote a.b 1194\n<ca>\nxx\n").is_err());
        assert!(OpenVpnPlan::parse("remote a.b notaport\n").is_err());
        assert!(OpenVpnPlan::parse("remote $(evil) 1194\n").is_err());
    }

    #[test]
    fn connection_blocks_are_checked() {
        let text = "client\n<connection>\nremote a.example 1194 udp\n</connection>\n<connection>\nremote b.example 443 tcp\nup /x\n</connection>\n";
        assert!(OpenVpnPlan::parse(text).is_err());
        let ok = "client\n<connection>\nremote a.example 1194 udp\n</connection>\n";
        assert_eq!(OpenVpnPlan::parse(ok).unwrap().remotes.len(), 1);
    }

    #[test]
    fn word_splitting() {
        assert_eq!(
            openvpn_words("remote \"a b\" 1 # c"),
            vec!["remote", "a b", "1"]
        );
        assert_eq!(openvpn_words("x 'y z';q"), vec!["x", "y z;q"]);
        assert_eq!(openvpn_words("a\\ b c"), vec!["a b", "c"]);
    }
}
