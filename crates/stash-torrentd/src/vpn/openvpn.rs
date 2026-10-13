//! OpenVPN process management (section 5, step 5).
//!
//! OpenVPN must never touch the main routing table: `--route-noexec` stops it
//! from installing routes and the pull filters drop gateway redirects and IPv6
//! pushes. The only script it may run is our hook, written at runtime into a
//! root-only directory, which records the DNS servers the server pushes.

use std::net::IpAddr;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

use super::wireguard::stop_child;

/// How long OpenVPN gets to print "Initialization Sequence Completed".
pub const READY_TIMEOUT: Duration = Duration::from_secs(60);
const READY_LINE: &str = "Initialization Sequence Completed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenVpnState {
    Starting,
    Ready,
    /// OpenVPN is reconnecting internally (ping-restart, SIGUSR1).
    Reconnecting,
    /// A fatal problem we can describe to the user.
    Failed(String),
}

#[derive(Debug)]
pub struct OpenVpnTunnel {
    pub ifname: String,
    child: Child,
    state: watch::Receiver<OpenVpnState>,
    hook_dir: PathBuf,
    dns_file: PathBuf,
}

/// Command line of section 5 step 5.
pub fn command_args(config: &Path, auth: &Path, ifname: &str, hook: &Path) -> Vec<String> {
    let hook = hook.display().to_string();
    let mut args: Vec<String> = vec![
        "--config".into(),
        config.display().to_string(),
        "--auth-user-pass".into(),
        auth.display().to_string(),
        "--auth-nocache".into(),
        "--dev".into(),
        ifname.into(),
        "--dev-type".into(),
        "tun".into(),
        "--route-noexec".into(),
        "--pull-filter".into(),
        "ignore".into(),
        "redirect-gateway".into(),
        "--pull-filter".into(),
        "ignore".into(),
        "route-ipv6".into(),
        "--pull-filter".into(),
        "ignore".into(),
        "ifconfig-ipv6".into(),
        "--script-security".into(),
        "2".into(),
        "--up".into(),
        hook.clone(),
        "--down".into(),
        hook,
    ];
    args.extend(["--verb".into(), "3".into(), "--suppress-timestamps".into()]);
    args
}

/// The up/down hook. Values come from the VPN server, so they are only ever
/// expanded inside quotes and written with printf, never evaluated.
pub fn hook_script(dns_file: &Path) -> String {
    let out = dns_file.display();
    format!(
        r#"#!/bin/sh
# Written by PopcornStash. Records DNS servers pushed by the OpenVPN server.
# It changes no routes and no system files.
out='{out}'
case "$script_type" in
  up)
    : > "$out.tmp"
    i=1
    while [ "$i" -le 64 ]; do
      eval "opt=\${{foreign_option_$i:-}}"
      [ -n "$opt" ] || break
      case "$opt" in
        "dhcp-option DNS "*) printf '%s\n' "${{opt#dhcp-option DNS }}" >> "$out.tmp" ;;
      esac
      i=$((i + 1))
    done
    mv -f "$out.tmp" "$out"
    ;;
  down)
    rm -f "$out" "$out.tmp"
    ;;
esac
exit 0
"#
    )
}

/// Parse the hook's DNS file: one IP per line; anything else is ignored.
pub fn parse_dns_file(text: &str) -> Vec<IpAddr> {
    text.lines()
        .filter_map(|l| l.trim().parse().ok())
        .take(8)
        .collect()
}

/// Classify one OpenVPN log line.
pub fn classify_line(line: &str) -> Option<OpenVpnState> {
    if line.contains(READY_LINE) {
        Some(OpenVpnState::Ready)
    } else if line.contains("AUTH_FAILED") {
        Some(OpenVpnState::Failed(
            "The VPN server rejected the username or password.".into(),
        ))
    } else if line.contains("Options error") {
        Some(OpenVpnState::Failed(
            "OpenVPN could not use this configuration file.".into(),
        ))
    } else if line.contains("Cannot open TUN/TAP dev") {
        Some(OpenVpnState::Failed(
            "OpenVPN could not create the tunnel device.".into(),
        ))
    } else if line.contains("SIGUSR1[soft") || line.contains("Restart pause") {
        Some(OpenVpnState::Reconnecting)
    } else {
        None
    }
}

/// Lines that may carry secrets (pushed auth tokens) are not logged verbatim.
fn redact(line: &str) -> &str {
    let lower = line.to_ascii_lowercase();
    if lower.contains("auth-token") || lower.contains("password") || lower.contains("push_reply") {
        "<redacted line>"
    } else {
        line
    }
}

impl OpenVpnTunnel {
    /// Start OpenVPN and wait until it is ready.
    pub async fn bring_up(
        config: &Path,
        auth: &Path,
        run_dir: &Path,
        ifname: &str,
    ) -> Result<Self, String> {
        let hook_dir = run_dir.join("openvpn");
        let _ = std::fs::remove_dir_all(&hook_dir);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&hook_dir)
            .map_err(|e| format!("could not create the OpenVPN hook directory: {e}"))?;
        std::fs::set_permissions(&hook_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let dns_file = hook_dir.join("dns");
        let hook = hook_dir.join("hook.sh");
        crate::sys::write_atomic(&hook, hook_script(&dns_file).as_bytes(), 0o700)
            .map_err(|e| format!("could not write the OpenVPN hook: {e}"))?;

        let _ = crate::sys::run_str("ip", &["link", "del", "dev", ifname]).await;
        let program = crate::sys::which("openvpn")
            .ok_or_else(|| "OpenVPN is not installed in this image.".to_string())?;
        let mut child = Command::new(program)
            .args(command_args(config, auth, ifname, &hook))
            .current_dir(&hook_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("could not start OpenVPN: {e}"))?;
        let (tx, rx) = watch::channel(OpenVpnState::Starting);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tx_err = tx.clone();
        if let Some(out) = stdout {
            tokio::spawn(watch_output(out, tx));
        }
        if let Some(err) = stderr {
            tokio::spawn(watch_output(err, tx_err));
        }
        let mut tunnel = Self {
            ifname: ifname.to_string(),
            child,
            state: rx,
            hook_dir,
            dns_file,
        };
        match tunnel.wait_ready().await {
            Ok(()) => Ok(tunnel),
            Err(e) => {
                tunnel.teardown_ref().await;
                Err(e)
            }
        }
    }

    async fn wait_ready(&mut self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        loop {
            match self.state.borrow().clone() {
                OpenVpnState::Ready => return Ok(()),
                OpenVpnState::Failed(reason) => return Err(reason),
                _ => {}
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!("OpenVPN exited ({status})"));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("OpenVPN did not connect within 60 seconds.".into());
            }
            let _ = tokio::time::timeout(Duration::from_millis(500), self.state.changed()).await;
        }
    }

    /// Current state; `Failed` when the process exited.
    pub fn state(&mut self) -> OpenVpnState {
        if let Ok(Some(status)) = self.child.try_wait() {
            return OpenVpnState::Failed(format!("OpenVPN exited ({status})"));
        }
        self.state.borrow().clone()
    }

    /// DNS servers pushed by the server (empty when none).
    pub fn dns_servers(&self) -> Vec<IpAddr> {
        std::fs::read_to_string(&self.dns_file)
            .map(|t| parse_dns_file(&t))
            .unwrap_or_default()
    }

    async fn teardown_ref(&mut self) {
        stop_child(&mut self.child, Duration::from_secs(5)).await;
        let _ = crate::sys::run_str("ip", &["link", "del", "dev", &self.ifname]).await;
        let _ = std::fs::remove_dir_all(&self.hook_dir);
    }

    pub async fn teardown(mut self) {
        self.teardown_ref().await;
    }
}

async fn watch_output<R: tokio::io::AsyncRead + Unpin>(reader: R, tx: watch::Sender<OpenVpnState>) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(process = "openvpn", "{}", redact(&line));
        if let Some(state) = classify_line(&line) {
            if state == OpenVpnState::Ready {
                tracing::info!("OpenVPN connected");
            }
            let _ = tx.send(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_never_lets_openvpn_route() {
        let args = command_args(
            Path::new("/config/torrent/vpn/openvpn.ovpn"),
            Path::new("/config/torrent/vpn/openvpn.auth"),
            "psw0",
            Path::new("/run/popcornstash/openvpn/hook.sh"),
        );
        let joined = args.join(" ");
        assert!(joined.starts_with("--config /config/torrent/vpn/openvpn.ovpn --auth-user-pass /config/torrent/vpn/openvpn.auth --auth-nocache --dev psw0 --dev-type tun --route-noexec"));
        for filter in ["redirect-gateway", "route-ipv6", "ifconfig-ipv6"] {
            assert!(
                joined.contains(&format!("--pull-filter ignore {filter}")),
                "{filter}"
            );
        }
        assert!(joined.contains("--script-security 2 --up /run/popcornstash/openvpn/hook.sh --down /run/popcornstash/openvpn/hook.sh"));
        // The config file comes first so every later flag overrides it.
        assert_eq!(args[0], "--config");
    }

    #[test]
    fn hook_records_pushed_dns() {
        let dir = tempfile::tempdir().unwrap();
        let dns = dir.path().join("dns");
        let hook = dir.path().join("hook.sh");
        crate::sys::write_atomic(&hook, hook_script(&dns).as_bytes(), 0o700).unwrap();
        let status = std::process::Command::new(&hook)
            .env("script_type", "up")
            .env("foreign_option_1", "dhcp-option DNS 103.86.96.100")
            .env("foreign_option_2", "dhcp-option DOMAIN example.com")
            .env(
                "foreign_option_3",
                "dhcp-option DNS 10.8.0.1; touch /tmp/pwned-by-dns",
            )
            .env("foreign_option_4", "dhcp-option DNS $(id)")
            .status()
            .unwrap();
        assert!(status.success());
        let text = std::fs::read_to_string(&dns).unwrap();
        assert_eq!(
            text,
            "103.86.96.100\n10.8.0.1; touch /tmp/pwned-by-dns\n$(id)\n"
        );
        assert!(!Path::new("/tmp/pwned-by-dns").exists());
        let ips = parse_dns_file(&text);
        assert_eq!(ips, vec!["103.86.96.100".parse::<IpAddr>().unwrap()]);
        let status = std::process::Command::new(&hook)
            .env("script_type", "down")
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!dns.exists());
    }

    #[test]
    fn classifies_log_lines() {
        assert_eq!(
            classify_line("2024 Initialization Sequence Completed"),
            Some(OpenVpnState::Ready)
        );
        assert!(matches!(
            classify_line("AUTH: Received control message: AUTH_FAILED"),
            Some(OpenVpnState::Failed(_))
        ));
        assert_eq!(
            classify_line("SIGUSR1[soft,ping-restart] received, process restarting"),
            Some(OpenVpnState::Reconnecting)
        );
        assert_eq!(
            classify_line("TCP/UDP: Preserving recently used remote address"),
            None
        );
        assert_eq!(
            redact("PUSH: Received control message: 'PUSH_REPLY,auth-token SECRET'"),
            "<redacted line>"
        );
    }
}
