//! nftables kill switch: table `inet popcornstash` (section 5, step 3).
//!
//! The table is replaced atomically with one `nft -f -` transaction that only
//! names our table, so Docker's tables (`ip nat`, `ip filter`, ...) are never
//! touched. Everything is keyed on the engine's uid (`meta skuid`), so the web
//! server's traffic is unaffected.

use std::net::SocketAddrV4;

use stash_core::internal_api::{DNS_FORWARDER_ADDR, FWMARK, NFT_TABLE, TUNNEL_IF};

use crate::sys::{self, CommandError};

/// Peer port the engine listens on (TCP and uTP).
pub const ENGINE_LISTEN_PORT: u16 = 6881;
/// The engine's DHT port (UDP).
pub const ENGINE_DHT_PORT: u16 = 6882;

/// Inputs of the ruleset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillSwitch {
    pub engine_uid: u32,
    pub tunnel_if: String,
    pub dns_forwarder: SocketAddrV4,
    pub tcp_ports: Vec<u16>,
    pub udp_ports: Vec<u16>,
    /// Conntrack mark for flows that use the tunnel (leak guard), and the
    /// socket mark of the DNS forwarder's upstream sockets (same value): marked
    /// packets may only leave through the tunnel, whatever routing decides.
    pub ct_mark: u32,
}

impl KillSwitch {
    pub fn new(engine_uid: u32) -> Self {
        Self {
            engine_uid,
            tunnel_if: TUNNEL_IF.to_string(),
            dns_forwarder: DNS_FORWARDER_ADDR
                .parse()
                .expect("DNS_FORWARDER_ADDR is an IPv4 socket address"),
            tcp_ports: vec![ENGINE_LISTEN_PORT],
            udp_ports: vec![ENGINE_LISTEN_PORT, ENGINE_DHT_PORT],
            ct_mark: FWMARK,
        }
    }

    /// The full `nft -f` script: create-then-delete-then-define, so the swap
    /// is atomic and works whether or not the table exists.
    pub fn ruleset(&self) -> String {
        let uid = self.engine_uid;
        let tun = &self.tunnel_if;
        let fwd_ip = self.dns_forwarder.ip();
        let fwd_port = self.dns_forwarder.port();
        let mark = format!("{:#x}", self.ct_mark);
        let ports = |ports: &[u16]| {
            if ports.len() == 1 {
                ports[0].to_string()
            } else {
                format!(
                    "{{ {} }}",
                    ports
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        };
        let tcp = ports(&self.tcp_ports);
        let udp = ports(&self.udp_ports);
        format!(
            "table inet {NFT_TABLE}
delete table inet {NFT_TABLE}
table inet {NFT_TABLE} {{
	chain nat_output {{
		type nat hook output priority -110; policy accept;
		meta nfproto ipv4 meta skuid {uid} udp dport 53 dnat ip to {fwd_ip}:{fwd_port}
		meta nfproto ipv4 meta skuid {uid} tcp dport 53 dnat ip to {fwd_ip}:{fwd_port}
	}}

	chain output {{
		type filter hook output priority 0; policy accept;
		meta mark {mark} oifname != \"{tun}\" counter drop
		ct mark {mark} oifname != {{ \"lo\", \"{tun}\" }} counter drop
		oifname \"{tun}\" ct mark set {mark}
		meta skuid {uid} oifname \"lo\" ct state established,related accept
		meta skuid {uid} oifname \"lo\" ip daddr {fwd_ip} udp dport {fwd_port} accept
		meta skuid {uid} oifname \"lo\" ip daddr {fwd_ip} tcp dport {fwd_port} accept
		meta skuid {uid} oifname \"{tun}\" accept
		meta skuid {uid} counter drop
	}}

	chain input {{
		type filter hook input priority 0; policy accept;
		iifname \"{tun}\" ct mark set {mark}
		iifname \"{tun}\" ct state established,related accept
		iifname \"{tun}\" tcp dport {tcp} accept
		iifname \"{tun}\" udp dport {udp} accept
		iifname \"{tun}\" counter drop
	}}
}}
"
        )
    }

    /// Validate the ruleset against the kernel without applying it (needs NET_ADMIN).
    pub async fn check(&self) -> Result<(), CommandError> {
        sys::run(
            "nft",
            &["-c".to_string(), "-f".to_string(), "-".to_string()],
            Some(self.ruleset().as_bytes()),
        )
        .await
        .map(drop)
    }

    /// Replace table `inet popcornstash` atomically.
    pub async fn apply(&self) -> Result<(), CommandError> {
        sys::run(
            "nft",
            &["-f".to_string(), "-".to_string()],
            Some(self.ruleset().as_bytes()),
        )
        .await
        .map(drop)
    }
}

/// Script that removes only our table (and succeeds when it does not exist).
pub fn removal_script() -> String {
    format!("table inet {NFT_TABLE}\ndelete table inet {NFT_TABLE}\n")
}

pub async fn remove() -> Result<(), CommandError> {
    sys::run(
        "nft",
        &["-f".to_string(), "-".to_string()],
        Some(removal_script().as_bytes()),
    )
    .await
    .map(drop)
}

/// `nft list table inet popcornstash` (for diagnostics and tests).
pub async fn list() -> Result<String, CommandError> {
    sys::run_str("nft", &["list", "table", "inet", NFT_TABLE]).await
}

/// True when our table exists with its three chains.
pub async fn in_place() -> bool {
    match list().await {
        Ok(text) => ["chain nat_output", "chain output", "chain input"]
            .iter()
            .all(|c| text.contains(c)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED: &str = "table inet popcornstash
delete table inet popcornstash
table inet popcornstash {
	chain nat_output {
		type nat hook output priority -110; policy accept;
		meta nfproto ipv4 meta skuid 6881 udp dport 53 dnat ip to 127.0.0.54:5354
		meta nfproto ipv4 meta skuid 6881 tcp dport 53 dnat ip to 127.0.0.54:5354
	}

	chain output {
		type filter hook output priority 0; policy accept;
		meta mark 0x5150 oifname != \"psw0\" counter drop
		ct mark 0x5150 oifname != { \"lo\", \"psw0\" } counter drop
		oifname \"psw0\" ct mark set 0x5150
		meta skuid 6881 oifname \"lo\" ct state established,related accept
		meta skuid 6881 oifname \"lo\" ip daddr 127.0.0.54 udp dport 5354 accept
		meta skuid 6881 oifname \"lo\" ip daddr 127.0.0.54 tcp dport 5354 accept
		meta skuid 6881 oifname \"psw0\" accept
		meta skuid 6881 counter drop
	}

	chain input {
		type filter hook input priority 0; policy accept;
		iifname \"psw0\" ct mark set 0x5150
		iifname \"psw0\" ct state established,related accept
		iifname \"psw0\" tcp dport 6881 accept
		iifname \"psw0\" udp dport { 6881, 6882 } accept
		iifname \"psw0\" counter drop
	}
}
";

    #[test]
    fn ruleset_snapshot() {
        assert_eq!(KillSwitch::new(6881).ruleset(), EXPECTED);
    }

    /// The DNS forwarder's upstream sockets carry the mark. Policy routing sends
    /// them to table 51820, but a loopback or local destination (an upstream of
    /// 127.0.0.11 is Docker's resolver, which answers from the host network)
    /// is routed by the `local` table first. Marked packets may only leave
    /// through the tunnel.
    #[test]
    fn marked_sockets_only_leave_through_the_tunnel() {
        let text = KillSwitch::new(6881).ruleset();
        let output = text.split("chain output").nth(1).unwrap();
        let rule = "meta mark 0x5150 oifname != \"psw0\" counter drop";
        assert!(output.contains(rule), "{output}");
        // Before any rule that accepts.
        let first_accept = output
            .lines()
            .position(|l| l.trim_end().ends_with(" accept") && !l.contains("policy"))
            .unwrap();
        let position = output.lines().position(|l| l.trim() == rule).unwrap();
        assert!(position < first_accept);
    }

    #[test]
    fn ruleset_only_names_our_table() {
        let text = KillSwitch::new(6881).ruleset();
        for line in text
            .lines()
            .filter(|l| l.starts_with("table") || l.starts_with("delete"))
        {
            assert!(line.contains("inet popcornstash"), "{line}");
        }
        assert!(!text.contains("flush ruleset"));
        assert_eq!(
            removal_script(),
            "table inet popcornstash\ndelete table inet popcornstash\n"
        );
    }
}
