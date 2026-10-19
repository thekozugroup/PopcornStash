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
        out.push_str(&format!(
            "FwMark = {:#x}\n",
            stash_core::internal_api::WIREGUARD_TRANSPORT_MARK
        ));
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

/// Largest .ovpn file accepted (the web server's limit).
pub const OPENVPN_MAX_BYTES: usize = 256 * 1024;
/// Longest line OpenVPN reads in one piece (255 bytes including the newline).
const OPENVPN_MAX_LINE: usize = 254;

/// Directives that run programs, load code, read or write files, change
/// directories or pull in other configuration. Refused outright. This is a
/// superset of the web server's list: the supervisor runs the file as root and
/// must not trust that the web server checked it.
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
    "client-crresponse",
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
    "dev-node",
    "replay-persist",
    "tls-export-cert",
    "auth-gen-token-secret",
    "ifconfig-pool-persist",
    "client-config-dir",
    "crl-verify",
    "capath",
    "http-proxy-user-pass",
    "dns-updown",
    "genkey",
];

/// Directives whose first argument names a file. Only `[inline]` is accepted:
/// the upload is a single file, and root must not read arbitrary paths.
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

/// Inline blocks allowed in the file (certificates and keys only).
const OPENVPN_INLINE: &[&str] = &[
    "ca",
    "cert",
    "key",
    "tls-auth",
    "tls-crypt",
    "tls-crypt-v2",
    "extra-certs",
];

fn openvpn_forbidden(name: &str) -> bool {
    OPENVPN_FORBIDDEN.contains(&name)
        || name.starts_with("management")
        || name.starts_with("log")
        || name.starts_with("pkcs11")
}

/// C `isspace()` in the C locale, plus NUL: OpenVPN's `space()`.
fn c_isspace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0')
}

/// Split one line exactly the way OpenVPN 2.6's `parse_line()` does: ASCII
/// whitespace separates tokens; `#` or `;` at the start of a token begins a
/// comment (even after a backslash); a backslash escapes `\`, `"` or
/// whitespace outside single quotes; double quotes group, single quotes group
/// literally; and a closing quote ends the token (`"plugin"/x.so` is two
/// tokens). Lines OpenVPN refuses or drops are refused, so the check never
/// sees less than OpenVPN does. (Same algorithm as the web server's validator.)
fn openvpn_tokens(line: &str) -> Result<Vec<String>, &'static str> {
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

/// `setenv` names that are safe to hand to scripts. Anything else
/// (`LD_PRELOAD`, `PATH`, `GLIBC_TUNABLES`, ...) would reach the root-run hook.
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

/// Refuse one directive (`name` lower-case, without `--`).
fn check_openvpn_directive(name: &str, args: &[String], line_no: usize) -> Result<(), ConfigError> {
    if openvpn_forbidden(name) {
        return Err(err(format!(
            "The OpenVPN file uses \"{name}\" on line {line_no}, which PopcornStash does not allow because it runs programs or touches files. Remove that line and upload it again."
        )));
    }
    match name {
        "auth-user-pass" if !args.is_empty() => Err(err(
            "The OpenVPN file names a password file in auth-user-pass. Remove the file name; PopcornStash supplies the username and password itself.",
        )),
        "server" | "server-bridge" | "tls-server" => Err(err(
            "This is an OpenVPN server configuration. Upload the client .ovpn file from your VPN provider.",
        )),
        "mode"
            if args
                .first()
                .is_some_and(|m| m.eq_ignore_ascii_case("server")) =>
        {
            Err(err(
                "This is an OpenVPN server configuration. Upload the client .ovpn file from your VPN provider.",
            ))
        }
        n if OPENVPN_FILE_ARGS.contains(&n) => match args.first() {
            Some(arg) if !arg.eq_ignore_ascii_case("[inline]") => Err(err(format!(
                "The OpenVPN file refers to a separate file for \"{n}\". Use a single .ovpn file with certificates and keys inline."
            ))),
            _ => Ok(()),
        },
        // `http-proxy host port authfile` / `socks-proxy host port authfile`.
        "http-proxy" | "socks-proxy"
            if args.get(2).is_some_and(|a| {
                !matches!(a.to_ascii_lowercase().as_str(), "auto" | "auto-nct")
            }) =>
        {
            Err(err(format!(
                "The OpenVPN file points {name} at a password file on line {line_no}. PopcornStash does not allow that."
            )))
        }
        // `setenv opt <directive>` makes OpenVPN run the directive; check it too.
        "setenv" | "setenv-safe" if args.first().is_some_and(|a| a.eq_ignore_ascii_case("opt")) => {
            match args.get(1) {
                Some(inner) => check_openvpn_directive(
                    &inner.trim_start_matches("--").to_ascii_lowercase(),
                    &args[2..],
                    line_no,
                ),
                None => Ok(()),
            }
        }
        // Plain `setenv` variables reach the root-run hook unchanged
        // (`setenv-safe` prefixes them with OPENVPN_ and stays allowed).
        "setenv" => match args.first() {
            Some(var) if !safe_setenv_name(var) => Err(err(format!(
                "The OpenVPN file sets an environment variable on line {line_no} that PopcornStash does not allow."
            ))),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

impl OpenVpnPlan {
    /// Parse and validate an .ovpn file the way OpenVPN 2.6 reads it. Refuses
    /// everything the web server's validator refuses, so a file written
    /// directly into the VPN folder cannot make the root supervisor run code,
    /// read files or act as a server.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        if text.len() > OPENVPN_MAX_BYTES {
            return Err(err("The OpenVPN file is larger than 256 KB."));
        }
        if text.contains('\0') {
            return Err(err("The OpenVPN file is not a text file."));
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut remotes = Vec::new();
        let mut proto = None;
        let mut port = None;
        let (mut client, mut tls_client, mut pull) = (false, false, false);
        // Close tag and start line of the inline block being skipped.
        let mut block: Option<(String, usize)> = None;

        // Lines are split on '\n' only, like OpenVPN's fgets().
        for (index, raw) in text.split('\n').enumerate() {
            let line_no = index + 1;
            if raw.len() > OPENVPN_MAX_LINE {
                return Err(err(format!(
                    "Line {line_no} of the OpenVPN file is longer than OpenVPN allows."
                )));
            }
            if let Some((close, _)) = &block {
                // OpenVPN ends the block at the first line that starts with the
                // close tag (case-sensitive, after leading whitespace).
                if let Some(rest) = raw
                    .trim_start_matches(c_isspace)
                    .strip_prefix(close.as_str())
                {
                    if !rest.trim_matches(c_isspace).is_empty() {
                        return Err(err(format!(
                            "Line {line_no} of the OpenVPN file has text after {close}."
                        )));
                    }
                    block = None;
                }
                continue;
            }
            let words = openvpn_tokens(raw)
                .map_err(|e| err(format!("Line {line_no} of the OpenVPN file {e}.")))?;
            let Some(first) = words.first() else { continue };
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
                if words.len() > 1 || tag.is_empty() || tag.starts_with('/') {
                    return Err(err(format!(
                        "Line {line_no} of the OpenVPN file is not a valid block tag."
                    )));
                }
                if !OPENVPN_INLINE.contains(&tag) {
                    return Err(err(format!(
                        "The OpenVPN file has a <{}> block, which PopcornStash does not support. Only certificate and key blocks are allowed.",
                        tag.chars()
                            .filter(|c| !c.is_control())
                            .take(40)
                            .collect::<String>()
                    )));
                }
                block = Some((format!("</{tag}>"), line_no));
                continue;
            }
            let name = first.trim_start_matches("--").to_ascii_lowercase();
            let args = &words[1..];
            check_openvpn_directive(&name, args, line_no)?;
            match name.as_str() {
                "client" => client = true,
                "tls-client" => tls_client = true,
                "pull" => pull = true,
                "remote" => {
                    let host = args.first().ok_or_else(|| {
                        err("The OpenVPN file has a remote line without a server.")
                    })?;
                    if host.is_empty()
                        || !host.chars().all(|c| {
                            c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_')
                        })
                    {
                        return Err(err(format!(
                            "The OpenVPN file has an invalid remote server: {host:?}."
                        )));
                    }
                    let remote_port =
                        match args.get(1) {
                            Some(p) => Some(p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(
                                || err("The OpenVPN file has an invalid remote port."),
                            )?),
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
        if let Some((close, line)) = block {
            return Err(err(format!(
                "The block that starts on line {line} of the OpenVPN file is never closed with {close}."
            )));
        }
        if !(client || tls_client && pull) {
            return Err(err(
                "The OpenVPN file is missing the client line. Upload the client .ovpn file from your VPN provider.",
            ));
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
                "[Interface]\nPrivateKey = {KEY_A}\nFwMark = 0x5151\n\n[Peer]\nPublicKey = {KEY_B}\nEndpoint = nl812.nordvpn.com:51820\nAllowedIPs = 0.0.0.0/0, ::/0\nPersistentKeepalive = 25\n"
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

    /// NordVPN's own .ovpn (as served by nordvpn.com), with CRLF line endings too.
    #[test]
    fn accepts_provider_files() {
        let nord = "client\ndev tun\nproto udp\nremote 185.153.177.12 1194\nresolv-retry infinite\nremote-random\nnobind\n\
tun-mtu 1500\ntun-mtu-extra 32\nmssfix 1450\npersist-key\npersist-tun\nping 15\nping-restart 0\nping-timer-rem\nreneg-sec 0\n\
comp-lzo no\nverify-x509-name CN=nl812.nordvpn.com\n\nremote-cert-tls server\n\nauth-user-pass\nverb 3\npull\nfast-io\n\
cipher AES-256-CBC\nauth SHA512\n<ca>\n-----BEGIN CERTIFICATE-----\nMIIFCjCCAvKgAwIBAgIBATANBgkqhkiG9w0BAQ0FADA5MQswCQYDVQQGEwJQQTEQ\n\
-----END CERTIFICATE-----\n</ca>\nkey-direction 1\n<tls-auth>\n#\n# 2048 bit OpenVPN static key\n#\n-----BEGIN OpenVPN Static key V1-----\n\
e685bdaf659a25a200e2b9e39e51ff03\n-----END OpenVPN Static key V1-----\n</tls-auth>\n";
        let plan = OpenVpnPlan::parse(nord).unwrap();
        assert_eq!(plan.server().as_deref(), Some("185.153.177.12"));
        assert!(OpenVpnPlan::parse(&nord.replace('\n', "\r\n")).is_ok());
    }

    #[test]
    fn connection_blocks_are_refused() {
        // The web server never writes <connection> blocks; the supervisor refuses them too.
        let text = "client\n<connection>\nremote a.example 1194 udp\n</connection>\n";
        assert!(OpenVpnPlan::parse(text).is_err());
        let text = "client\nremote a.example 1194\n<connection>\nremote b.example 443 tcp\nup /x\n</connection>\n";
        assert!(OpenVpnPlan::parse(text).is_err());
    }

    /// Lines OpenVPN 2.6 reads as a directive that a simpler reader misses. The
    /// supervisor runs the file as root, so it must refuse at least everything
    /// the web server refuses (a compromised web server can write the file itself).
    #[test]
    fn refuses_what_openvpn_would_read_differently() {
        let cert = "<ca>\nMIIFCjCCAvKgAwIBAgIBATANBgkqhkiG9w0BAQ0FADA5MQswCQYDVQQGEwJQQTEQ\n";
        let base = "client\nremote a.example 1194\n";
        let cases = [
            // A closing quote ends the token: OpenVPN reads `route-up /tmp/x`.
            format!("{base}\"route-up\"/tmp/x\n"),
            format!("{base}'plugin'/tmp/x.so\n"),
            // Bad escapes make OpenVPN drop or re-read the line.
            format!("{base}u\\p /bin/sh\n"),
            // `setenv opt <directive>` still runs the directive.
            format!("{base}setenv opt route-up /tmp/x\n"),
            format!("{base}setenv-safe opt plugin /tmp/x.so\n"),
            // Variables that reach the root-run hook script.
            format!("{base}setenv LD_PRELOAD /stash/Inbox/x.so\n"),
            format!("{base}setenv PATH /stash/Inbox\n"),
            format!("{base}setenv GLIBC_TUNABLES x\n"),
            // Credential files read by root and sent to a proxy.
            format!("{base}http-proxy proxy.example 8080 /etc/shadow\n"),
            format!("{base}socks-proxy proxy.example 1080 /etc/shadow\n"),
            // Server mode and server-only file directives.
            format!("{base}server 10.8.0.0 255.255.255.0\n"),
            format!("{base}mode server\n"),
            format!("{base}tls-server\n"),
            format!("{base}ifconfig-pool-persist /etc/passwd\n"),
            format!("{base}client-config-dir /etc\n"),
            format!("{base}crl-verify /etc/shadow\n"),
            format!("{base}capath /etc\n"),
            format!("{base}dns-updown /bin/sh\n"),
            format!("{base}genkey secret /config/popcornstash.db\n"),
            format!("{base}pkcs11-id x\n"),
            // OpenVPN ends an inline block at a line that starts with the close tag.
            format!("{base}{cert}</ca>junk\nplugin /tmp/x.so\n{cert}</ca>\n"),
            // OpenVPN reads 255 bytes at a time: the tail of a long line is its own line.
            format!(
                "{base}{cert}{}</ca>\nplugin /tmp/x.so\n{cert}</ca>\n",
                "A".repeat(255)
            ),
            format!("{base}#{}plugin /tmp/x.so\n", "0".repeat(255)),
            // `--<ca>` opens a block in OpenVPN (it strips one leading `--`).
            format!("{base}--<ca>\nAAAA\n</ca>junk\nplugin /x.so\n{cert}</ca>\n"),
            // Unterminated quotes and missing client mode.
            format!("{base}remote \"unterminated\n"),
            "remote a.example 1194\n".to_string(),
        ];
        let accepted: Vec<&String> = cases
            .iter()
            .filter(|text| OpenVpnPlan::parse(text).is_ok())
            .collect();
        assert!(accepted.is_empty(), "should refuse: {accepted:#?}");
        for line in [
            "setenv CLIENT_CERT 0",
            "setenv FORWARD_COMPATIBLE 1",
            "setenv UV_IPV6 yes",
            "setenv opt block-outside-dns",
            "setenv-safe FRIENDLY_NAME home",
            "http-proxy proxy.example 8080 auto",
            "tls-auth [inline] 1",
        ] {
            let text = format!("{base}{line}\n");
            assert!(OpenVpnPlan::parse(&text).is_ok(), "should allow {line:?}");
        }
        let long_form = "tls-client\npull\nremote 1.2.3.4 1194 udp\n";
        assert!(OpenVpnPlan::parse(long_form).is_ok());
    }

    #[test]
    fn word_splitting_matches_openvpn() {
        let words = |line: &str| openvpn_tokens(line).unwrap();
        assert_eq!(words("remote \"a b\" 1 # c"), vec!["remote", "a b", "1"]);
        // A closing quote ends the token, and `;` then starts a comment.
        assert_eq!(words("x 'y z';q"), vec!["x", "y z"]);
        assert_eq!(words("a\\ b c"), vec!["a b", "c"]);
        assert_eq!(words("\"plugin\"/x.so"), vec!["plugin", "/x.so"]);
        assert_eq!(words("verb 3;still-token"), vec!["verb", "3;still-token"]);
        assert_eq!(words("up\"x\""), vec!["up\"x\""]);
        assert!(words("\\# up /x").is_empty());
        assert!(openvpn_tokens("u\\p /x").is_err());
        assert!(openvpn_tokens("x \"open").is_err());
    }
}
