//! Destination checks for web downloads (server-side request forgery guard).
//!
//! A web download must never reach the server itself, the torrent engine, or
//! anything else on a private network. Every hop is checked:
//! - before the first request (async DNS resolution of the host),
//! - by a DNS resolver installed in the download client, so the addresses that are
//!   actually connected to are the ones that were checked (no DNS rebinding),
//! - by the redirect policy for redirects to IP literals and other schemes.

use std::collections::HashSet;
use std::error::Error as StdError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, RwLock};

use url::{Host, Url};

/// Plain sentence shown when a link points at a local or private address.
pub const MSG_BLOCKED: &str = "Links to local or private network addresses cannot be downloaded.";
/// Plain sentence shown when the host name does not resolve.
pub const MSG_UNRESOLVED: &str = "The server for that link could not be found.";

/// The destination is not allowed. Carried through reqwest's error chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockedDestination;

impl std::fmt::Display for BlockedDestination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(MSG_BLOCKED)
    }
}

impl StdError for BlockedDestination {}

/// Why a destination check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    Blocked,
    Unresolved,
    Unsupported,
}

impl GuardError {
    pub fn message(&self) -> &'static str {
        match self {
            GuardError::Blocked => MSG_BLOCKED,
            GuardError::Unresolved => MSG_UNRESOLVED,
            GuardError::Unsupported => "Only http and https links can be downloaded.",
        }
    }
}

/// True for addresses a web download must never connect to: loopback, private,
/// link-local, unique-local, shared (CGNAT), unspecified, broadcast and multicast,
/// including IPv4 addresses embedded in IPv6.
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_v4(v4),
        IpAddr::V6(v6) => is_forbidden_v6(v6),
    }
}

fn is_forbidden_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        // 100.64.0.0/10 shared address space (carrier-grade NAT, tailnets).
        || (a == 100 && (b & 0xc0) == 64)
}

fn is_forbidden_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_forbidden_v4(v4);
    }
    let s = ip.segments();
    // 64:ff9b::/96 NAT64 carries an IPv4 address in the low 32 bits.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        let v4 = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
        return is_forbidden_v4(v4);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // fc00::/7 unique local
        || (s[0] & 0xfe00) == 0xfc00
        // fe80::/10 link local, fec0::/10 site local (deprecated)
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        // ::/96 IPv4-compatible (deprecated)
        || (s[0..6] == [0, 0, 0, 0, 0, 0] && s[6] != 0)
}

/// Destination policy shared by every web download.
#[derive(Default)]
pub struct AddressGuard {
    /// Addresses explicitly trusted (tests, or a deliberately trusted host).
    allowed: RwLock<HashSet<IpAddr>>,
    /// Host names that are always refused (the torrent engine and its proxy).
    blocked_names: RwLock<HashSet<String>>,
}

impl AddressGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Trust one address even though it is private.
    pub fn allow(&self, ip: IpAddr) {
        if let Ok(mut allowed) = self.allowed.write() {
            allowed.insert(ip);
        }
    }

    /// Refuse a host name outright (case-insensitive).
    pub fn block_name(&self, name: &str) {
        if let Ok(mut names) = self.blocked_names.write() {
            names.insert(name.trim_end_matches('.').to_ascii_lowercase());
        }
    }

    fn is_allowed(&self, ip: IpAddr) -> bool {
        self.allowed
            .read()
            .map(|a| a.contains(&ip))
            .unwrap_or(false)
    }

    /// Synchronous check of one address.
    pub fn ip_ok(&self, ip: IpAddr) -> bool {
        self.is_allowed(ip) || !is_forbidden_ip(ip)
    }

    /// Synchronous check of a host name against the name rules (not DNS).
    pub fn name_ok(&self, name: &str) -> bool {
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        if name == "localhost" || name.ends_with(".localhost") {
            return false;
        }
        self.blocked_names
            .read()
            .map(|n| !n.contains(&name))
            .unwrap_or(true)
    }

    /// Check every resolved address; any forbidden one refuses the whole host.
    pub fn addrs_ok(&self, addrs: &[SocketAddr]) -> bool {
        !addrs.is_empty() && addrs.iter().all(|a| self.ip_ok(a.ip()))
    }

    /// Check a URL before connecting: scheme, host name rules, and every address
    /// the host resolves to.
    pub async fn check_url(&self, url: &Url) -> Result<(), GuardError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(GuardError::Unsupported);
        }
        let port = url.port_or_known_default().unwrap_or(80);
        match url.host() {
            Some(Host::Ipv4(ip)) => self
                .ip_ok(IpAddr::V4(ip))
                .then_some(())
                .ok_or(GuardError::Blocked),
            Some(Host::Ipv6(ip)) => self
                .ip_ok(IpAddr::V6(ip))
                .then_some(())
                .ok_or(GuardError::Blocked),
            Some(Host::Domain(name)) => {
                if !self.name_ok(name) {
                    return Err(GuardError::Blocked);
                }
                let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name, port))
                    .await
                    .map_err(|_| GuardError::Unresolved)?
                    .collect();
                if addrs.is_empty() {
                    Err(GuardError::Unresolved)
                } else if self.addrs_ok(&addrs) {
                    Ok(())
                } else {
                    Err(GuardError::Blocked)
                }
            }
            None => Err(GuardError::Unsupported),
        }
    }

    /// Synchronous check used by the redirect policy: scheme, IP literals and name rules.
    /// Domain names are resolved later by [`GuardedResolver`].
    pub fn redirect_ok(&self, url: &Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        match url.host() {
            Some(Host::Ipv4(ip)) => self.ip_ok(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => self.ip_ok(IpAddr::V6(ip)),
            Some(Host::Domain(name)) => self.name_ok(name),
            None => false,
        }
    }
}

/// reqwest DNS resolver that refuses hosts resolving to forbidden addresses.
pub struct GuardedResolver {
    pub guard: Arc<AddressGuard>,
}

impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let guard = self.guard.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            if !guard.name_ok(&host) {
                return Err(Box::new(BlockedDestination) as Box<dyn StdError + Send + Sync>);
            }
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| Box::new(e) as Box<dyn StdError + Send + Sync>)?
                .collect();
            if !guard.addrs_ok(&addrs) {
                return Err(Box::new(BlockedDestination) as Box<dyn StdError + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// True when `error` (or anything in its source chain) is a refused destination.
pub fn is_blocked_error(error: &(dyn StdError + 'static)) -> bool {
    let mut current: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(e) = current {
        if e.downcast_ref::<BlockedDestination>().is_some() {
            return true;
        }
        current = e.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn forbidden_ranges() {
        for bad in [
            "127.0.0.1",
            "127.255.0.9",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.20",
            "169.254.169.254",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "224.0.0.1",
            "100.64.0.1",
            "100.127.255.254",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "::127.0.0.1",
        ] {
            assert!(is_forbidden_ip(ip(bad)), "{bad} should be forbidden");
        }
        for good in [
            "1.1.1.1",
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "2606:4700::1111",
            "::ffff:1.1.1.1",
        ] {
            assert!(!is_forbidden_ip(ip(good)), "{good} should be allowed");
        }
    }

    #[tokio::test]
    async fn check_url_rules() {
        let guard = AddressGuard::new();
        let check = |s: &str| {
            let url = Url::parse(s).unwrap();
            let guard = &guard;
            async move { guard.check_url(&url).await }
        };
        assert_eq!(
            check("http://127.0.0.1:8788/x").await,
            Err(GuardError::Blocked)
        );
        assert_eq!(check("http://[::1]/x").await, Err(GuardError::Blocked));
        assert_eq!(
            check("http://169.254.169.254/latest/meta-data").await,
            Err(GuardError::Blocked)
        );
        assert_eq!(
            check("http://localhost:9091/torrents").await,
            Err(GuardError::Blocked)
        );
        assert_eq!(
            check("http://foo.localhost/").await,
            Err(GuardError::Blocked)
        );
        assert_eq!(
            check("ftp://example.com/x").await,
            Err(GuardError::Unsupported)
        );
        assert_eq!(check("http://1.1.1.1/x").await, Ok(()));

        guard.allow(ip("127.0.0.1"));
        assert_eq!(check("http://127.0.0.1:8788/x").await, Ok(()));
        assert_eq!(
            check("http://127.0.0.2:8788/x").await,
            Err(GuardError::Blocked)
        );

        guard.block_name("popcornstash-vpn");
        assert_eq!(
            check("http://PopcornStash-VPN:9092/").await,
            Err(GuardError::Blocked)
        );
    }

    #[test]
    fn redirect_rules() {
        let guard = AddressGuard::new();
        let ok = |s: &str| guard.redirect_ok(&Url::parse(s).unwrap());
        assert!(ok("https://example.com/file"));
        assert!(!ok("http://10.0.0.1/file"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("http://localhost/"));
    }
}
