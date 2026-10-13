//! WireGuard bring-up: kernel module, boringtun fallback (section 5, step 5).
//!
//! The encrypted UDP comes from the kernel or the root-owned boringtun process,
//! so it uses the container's normal routes; only the engine user and the DNS
//! forwarder's marked sockets are routed into the tunnel.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use super::config::WireGuardPlan;
use crate::sys::{self, CommandError};

/// A handshake older than this means the tunnel is down (WireGuard rekeys every 2 min).
pub const HANDSHAKE_MAX_AGE: i64 = 180;
/// How long boringtun gets to create its UAPI socket.
const UAPI_WAIT: Duration = Duration::from_secs(10);
pub const BORINGTUN: &str = "boringtun-cli";

pub fn uapi_socket(ifname: &str) -> PathBuf {
    PathBuf::from(format!("/var/run/wireguard/{ifname}.sock"))
}

/// A running WireGuard interface.
#[derive(Debug)]
pub struct WireGuardTunnel {
    pub ifname: String,
    /// The userspace implementation, when the kernel has no WireGuard.
    boringtun: Option<Child>,
}

#[derive(Debug, thiserror::Error)]
pub enum WireGuardError {
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error("{0}")]
    Other(String),
}

fn s(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_string()).collect()
}

/// `ip` commands that configure the interface after `wg setconf`.
pub fn address_commands(plan: &WireGuardPlan, ifname: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    for address in &plan.addresses {
        let family = if address.addr().is_ipv4() { "-4" } else { "-6" };
        commands.push(s(&[
            family,
            "address",
            "add",
            &address.to_string(),
            "dev",
            ifname,
        ]));
    }
    commands.push(s(&[
        "link",
        "set",
        "dev",
        ifname,
        "mtu",
        &plan.mtu().to_string(),
        "up",
    ]));
    commands
}

impl WireGuardTunnel {
    /// Create `ifname`, configure it from `plan` and bring it up. On failure
    /// everything created so far is removed.
    pub async fn bring_up(
        plan: &WireGuardPlan,
        ifname: &str,
        ipv6: bool,
    ) -> Result<Self, WireGuardError> {
        // A stale interface from a crashed run would make `ip link add` fail.
        let _ = sys::run_str("ip", &["link", "del", "dev", ifname]).await;
        let mut tunnel = match sys::run_str(
            "ip",
            &["link", "add", "dev", ifname, "type", "wireguard"],
        )
        .await
        {
            Ok(_) => {
                tracing::info!(interface = ifname, "using kernel WireGuard");
                Self {
                    ifname: ifname.to_string(),
                    boringtun: None,
                }
            }
            Err(e) => {
                tracing::info!(reason = %e.message, "kernel WireGuard is unavailable; starting boringtun");
                let child = start_boringtun(ifname).await?;
                Self {
                    ifname: ifname.to_string(),
                    boringtun: Some(child),
                }
            }
        };
        match tunnel.configure(plan, ipv6).await {
            Ok(()) => Ok(tunnel),
            Err(e) => {
                tunnel.teardown_ref().await;
                Err(e)
            }
        }
    }

    async fn configure(&self, plan: &WireGuardPlan, ipv6: bool) -> Result<(), WireGuardError> {
        // The private key goes through stdin, never onto the command line or disk.
        let conf = plan.setconf_text();
        sys::run(
            "wg",
            &s(&["setconf", &self.ifname, "/dev/stdin"]),
            Some(conf.as_bytes()),
        )
        .await?;
        for command in address_commands(plan, &self.ifname) {
            if command[0] == "-6" && !ipv6 {
                tracing::info!(
                    "skipping the tunnel's IPv6 address: IPv6 is not available in this container"
                );
                continue;
            }
            sys::run("ip", &command, None).await?;
        }
        Ok(())
    }

    /// Seconds since the newest handshake with any peer, if there was one.
    pub async fn handshake_age(&self, now: i64) -> Result<Option<i64>, CommandError> {
        let out = sys::run_str("wg", &["show", &self.ifname, "latest-handshakes"]).await?;
        Ok(handshake_age(&parse_latest_handshakes(&out), now))
    }

    /// False when the userspace implementation exited.
    pub fn process_alive(&mut self) -> bool {
        match &mut self.boringtun {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => true,
        }
    }

    pub fn userspace(&self) -> bool {
        self.boringtun.is_some()
    }

    async fn teardown_ref(&mut self) {
        let _ = sys::run_str("ip", &["link", "del", "dev", &self.ifname]).await;
        if let Some(mut child) = self.boringtun.take() {
            stop_child(&mut child, Duration::from_secs(5)).await;
            let _ = std::fs::remove_file(uapi_socket(&self.ifname));
        }
    }

    /// Remove the interface and stop boringtun.
    pub async fn teardown(mut self) {
        self.teardown_ref().await;
    }
}

/// SIGTERM, then SIGKILL after `grace`.
pub async fn stop_child(child: &mut Child, grace: Duration) {
    if let Some(pid) = child.id() {
        let pid = nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap_or(i32::MAX));
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            return;
        }
    }
    let _ = child.kill().await;
}

async fn start_boringtun(ifname: &str) -> Result<Child, WireGuardError> {
    let program = sys::which(BORINGTUN).ok_or_else(|| {
        WireGuardError::Other(
            "The kernel has no WireGuard and boringtun-cli is not installed.".into(),
        )
    })?;
    let socket = uapi_socket(ifname);
    let _ = std::fs::remove_file(&socket);
    let mut child = Command::new(program)
        .args(["--foreground", "--disable-drop-privileges", ifname])
        .env("WG_LOG_LEVEL", "error")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| WireGuardError::Other(format!("could not start boringtun: {e}")))?;
    forward_output(child.stdout.take(), "boringtun");
    forward_output(child.stderr.take(), "boringtun");
    let deadline = tokio::time::Instant::now() + UAPI_WAIT;
    while tokio::time::Instant::now() < deadline {
        if socket_ready(&socket) {
            return Ok(child);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(WireGuardError::Other(format!(
                "boringtun exited during start-up ({status})"
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stop_child(&mut child, Duration::from_secs(2)).await;
    Err(WireGuardError::Other(
        "boringtun did not create its control socket in time".into(),
    ))
}

fn socket_ready(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_socket())
        .unwrap_or(false)
}

/// Log a child's output line by line at debug level.
pub fn forward_output<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    reader: Option<R>,
    name: &'static str,
) {
    if let Some(reader) = reader {
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(process = name, "{line}");
            }
        });
    }
}

/// Parse `wg show <if> latest-handshakes`: `<public key>\t<unix time>` per line.
pub fn parse_latest_handshakes(text: &str) -> Vec<(String, i64)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let key = parts.next()?;
            let time = parts.next()?.parse().ok()?;
            Some((key.to_string(), time))
        })
        .collect()
}

/// Values below this are ages, not Unix times: boringtun's UAPI reports the
/// time *since* the last handshake where kernel WireGuard reports when it happened.
const AGE_NOT_TIMESTAMP: i64 = 1_000_000_000;

/// Age in seconds of the newest handshake across peers. `0` means "never"
/// (for boringtun it can also mean "less than a second ago"; the next check
/// a few seconds later resolves that).
pub fn handshake_age(handshakes: &[(String, i64)], now: i64) -> Option<i64> {
    handshakes
        .iter()
        .map(|(_, t)| *t)
        .filter(|t| *t > 0)
        .map(|t| {
            if t < AGE_NOT_TIMESTAMP {
                t
            } else {
                now.saturating_sub(t).max(0)
            }
        })
        .min()
}

/// True when the newest handshake is recent enough.
pub fn handshake_fresh(age: Option<i64>) -> bool {
    age.is_some_and(|a| a < HANDSHAKE_MAX_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_handshakes() {
        let out = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\t1700000000\nHIgo9xNzJMWLKASShiTqIybxZ0U3wGLiUeJ1PKf8ykw=\t0\n";
        let parsed = parse_latest_handshakes(out);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].1, 1_700_000_000);
        assert!(parse_latest_handshakes("").is_empty());
        // Kernel: Unix times.
        assert_eq!(handshake_age(&parsed, 1_700_000_100), Some(100));
        assert!(handshake_fresh(handshake_age(&parsed, 1_700_000_100)));
        assert!(!handshake_fresh(handshake_age(&parsed, 1_700_000_180)));
        // boringtun: ages.
        let boringtun = parse_latest_handshakes("k\t12\n");
        assert_eq!(handshake_age(&boringtun, 1_791_850_796), Some(12));
        assert!(handshake_fresh(handshake_age(&boringtun, 1_791_850_796)));
        assert!(!handshake_fresh(handshake_age(
            &parse_latest_handshakes("k\t200\n"),
            1_791_850_796
        )));
        // Never.
        assert_eq!(handshake_age(&parse_latest_handshakes("k\t0\n"), 5), None);
        assert!(!handshake_fresh(None));
    }

    #[test]
    fn address_commands_render() {
        let text = "[Interface]\nPrivateKey = yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=\nAddress = 10.5.0.2/32, fd00::2/128\n[Peer]\nPublicKey = xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=\nEndpoint = 1.2.3.4:51820\nAllowedIPs = 0.0.0.0/0\n";
        let plan = WireGuardPlan::parse(text).unwrap();
        let rendered: Vec<String> = address_commands(&plan, "psw0")
            .iter()
            .map(|c| c.join(" "))
            .collect();
        assert_eq!(
            rendered,
            vec![
                "-4 address add 10.5.0.2/32 dev psw0",
                "-6 address add fd00::2/128 dev psw0",
                "link set dev psw0 mtu 1420 up",
            ]
        );
        assert_eq!(
            uapi_socket("psw0"),
            PathBuf::from("/var/run/wireguard/psw0.sock")
        );
    }
}
