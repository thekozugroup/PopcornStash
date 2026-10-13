//! OpenVPN process management (section 5, step 5).
//!
//! OpenVPN must never touch the main routing table: `--route-noexec` stops it
//! from installing routes and `--ifconfig-noexec` from configuring the device
//! (an address with the pushed netmask would add a subnet route to the main
//! table and pull the web server's traffic for that subnet into the tunnel).
//! The only script it may run is our hook, written at runtime into a root-only
//! directory: it gives `psw0` the pushed address as a /32, brings it up and
//! records the DNS servers the server pushes. OpenVPN reads a root-only copy
//! of the validated configuration, never the web server's file.

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

/// Interpreter for the hook script (on the read-only root filesystem, which
/// allows execution, unlike the noexec `/run` tmpfs the script lives on).
pub const HOOK_SHELL: &str = "/bin/sh";

/// Command line of section 5 step 5. `config` and `auth` are the root-only
/// copies in the hook directory.
pub fn command_args(config: &Path, auth: &Path, ifname: &str, hook: &Path) -> Vec<String> {
    // `/run` is a tmpfs mounted noexec in the container (compose.yaml and
    // Docker's defaults), so the hook is run by the shell rather than executed
    // directly; OpenVPN checks and runs the first word, `/bin/sh`.
    let hook = format!("{HOOK_SHELL} {}", hook.display());
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
        "--ifconfig-noexec".into(),
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
/// expanded inside quotes, checked with `case` patterns and passed as single
/// arguments, never evaluated. `ip` is the absolute path of the `ip` tool.
pub fn hook_script(dns_file: &Path, ip: &str, ifname: &str) -> String {
    let out = dns_file.display();
    format!(
        r#"#!/bin/sh
# Written by PopcornStash. Gives the tunnel the address the OpenVPN server
# pushed, as a /32 so the kernel adds no route to any table, and records the
# DNS servers it pushed. It changes no routes and no system files.
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
out='{out}'
ip='{ip}'
dev='{ifname}'
case "$script_type" in
  up)
    case "${{ifconfig_local:-}}" in
      ''|*[!0-9.]*)
        echo "PopcornStash: the VPN server did not send a usable IPv4 address" >&2
        exit 1
        ;;
    esac
    "$ip" -4 address replace "$ifconfig_local/32" dev "$dev" || exit 1
    case "${{tun_mtu:-}}" in
      ''|*[!0-9]*) ;;
      *) "$ip" link set dev "$dev" mtu "$tun_mtu" || exit 1 ;;
    esac
    "$ip" link set dev "$dev" up || exit 1
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

/// Write root-only (0600) copies of the validated configuration and the
/// credentials into `hook_dir` (0700, root). Returns their paths.
pub fn stage_files(
    hook_dir: &Path,
    config_text: &str,
    auth: &str,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let config = hook_dir.join("client.ovpn");
    let auth_path = hook_dir.join("auth");
    crate::sys::write_atomic(&config, config_text.as_bytes(), 0o600)?;
    crate::sys::write_atomic(&auth_path, auth.as_bytes(), 0o600)?;
    Ok((config, auth_path))
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
    /// Start OpenVPN with the validated `config_text` and `auth` (written to
    /// root-only copies) and wait until it is ready.
    pub async fn bring_up(
        config_text: &str,
        auth: &str,
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
        let (config, auth) = stage_files(&hook_dir, config_text, auth)
            .map_err(|e| format!("could not prepare the OpenVPN files: {e}"))?;
        let dns_file = hook_dir.join("dns");
        let hook = hook_dir.join("hook.sh");
        let ip = crate::sys::which("ip")
            .map(|p| p.display().to_string())
            .filter(|p| !p.contains('\''))
            .unwrap_or_else(|| "ip".into());
        crate::sys::write_atomic(&hook, hook_script(&dns_file, &ip, ifname).as_bytes(), 0o700)
            .map_err(|e| format!("could not write the OpenVPN hook: {e}"))?;

        let _ = crate::sys::run_str("ip", &["link", "del", "dev", ifname]).await;
        let program = crate::sys::which("openvpn")
            .ok_or_else(|| "OpenVPN is not installed in this image.".to_string())?;
        let mut child = Command::new(program)
            .args(command_args(&config, &auth, ifname, &hook))
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
    fn command_line_never_lets_openvpn_route_or_configure_the_device() {
        let args = command_args(
            Path::new("/run/popcornstash/openvpn/client.ovpn"),
            Path::new("/run/popcornstash/openvpn/auth"),
            "psw0",
            Path::new("/run/popcornstash/openvpn/hook.sh"),
        );
        let joined = args.join(" ");
        assert!(joined.starts_with("--config /run/popcornstash/openvpn/client.ovpn --auth-user-pass /run/popcornstash/openvpn/auth --auth-nocache --dev psw0 --dev-type tun --route-noexec --ifconfig-noexec"));
        for filter in ["redirect-gateway", "route-ipv6", "ifconfig-ipv6"] {
            assert!(
                joined.contains(&format!("--pull-filter ignore {filter}")),
                "{filter}"
            );
        }
        // The hook runs through /bin/sh: /run is mounted noexec.
        let up = args.iter().position(|a| a == "--up").unwrap();
        let down = args.iter().position(|a| a == "--down").unwrap();
        assert_eq!(args[up + 1], "/bin/sh /run/popcornstash/openvpn/hook.sh");
        assert_eq!(args[down + 1], "/bin/sh /run/popcornstash/openvpn/hook.sh");
        assert_eq!(args[up - 2..up], ["--script-security", "2"]);
        // The config file comes first so every later flag overrides it.
        assert_eq!(args[0], "--config");
    }

    #[test]
    fn staged_copies_are_root_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (config, auth) = stage_files(dir.path(), "client\n", "user\npass\n").unwrap();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "client\n");
        assert_eq!(std::fs::read_to_string(&auth).unwrap(), "user\npass\n");
        for path in [config, auth] {
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    /// Run the hook with a fake `ip` that records its arguments.
    fn run_hook(dir: &Path, env: &[(&str, &str)]) -> (bool, String) {
        let dns = dir.join("dns");
        let log = dir.join("ip.log");
        let fake_ip = dir.join("fake-ip");
        std::fs::write(
            &fake_ip,
            format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(
            &fake_ip,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let hook = dir.join("hook.sh");
        let script = hook_script(&dns, &fake_ip.display().to_string(), "psw0");
        crate::sys::write_atomic(&hook, script.as_bytes(), 0o700).unwrap();
        let _ = std::fs::remove_file(&log);
        // As OpenVPN runs it: through the shell (the hook directory is noexec).
        let mut command = std::process::Command::new(HOOK_SHELL);
        command.arg(&hook).env_clear();
        for (k, v) in env {
            command.env(k, v);
        }
        let ok = command.status().unwrap().success();
        (ok, std::fs::read_to_string(&log).unwrap_or_default())
    }

    #[test]
    fn hook_sets_a_host_address_and_records_pushed_dns() {
        let dir = tempfile::tempdir().unwrap();
        let dns = dir.path().join("dns");
        let (ok, calls) = run_hook(
            dir.path(),
            &[
                ("script_type", "up"),
                ("ifconfig_local", "10.8.0.2"),
                ("ifconfig_netmask", "128.0.0.0"),
                ("tun_mtu", "1500"),
                ("foreign_option_1", "dhcp-option DNS 103.86.96.100"),
                ("foreign_option_2", "dhcp-option DOMAIN example.com"),
                (
                    "foreign_option_3",
                    "dhcp-option DNS 10.8.0.1; touch /tmp/pwned-by-dns",
                ),
                ("foreign_option_4", "dhcp-option DNS $(id)"),
            ],
        );
        assert!(ok);
        // The pushed netmask is ignored: a /32 adds no route to the main table.
        assert_eq!(
            calls,
            "-4 address replace 10.8.0.2/32 dev psw0\nlink set dev psw0 mtu 1500\nlink set dev psw0 up\n"
        );
        let text = std::fs::read_to_string(&dns).unwrap();
        assert_eq!(
            text,
            "103.86.96.100\n10.8.0.1; touch /tmp/pwned-by-dns\n$(id)\n"
        );
        assert!(!Path::new("/tmp/pwned-by-dns").exists());
        let ips = parse_dns_file(&text);
        assert_eq!(ips, vec!["103.86.96.100".parse::<IpAddr>().unwrap()]);

        // Anything but a dotted address fails the hook (OpenVPN then stops) before `ip` runs.
        for bad in ["", "10.8.0.2/1", "$(id)", "10.8.0.2 dev eth0", "-x"] {
            let (ok, calls) = run_hook(
                dir.path(),
                &[("script_type", "up"), ("ifconfig_local", bad)],
            );
            assert!(!ok, "{bad:?}");
            assert_eq!(calls, "", "{bad:?}");
        }
        // A strange MTU is ignored, not passed on.
        let (ok, calls) = run_hook(
            dir.path(),
            &[
                ("script_type", "up"),
                ("ifconfig_local", "10.8.0.2"),
                ("tun_mtu", "1500; reboot"),
            ],
        );
        assert!(ok);
        assert!(!calls.contains("mtu"), "{calls}");

        let (ok, _) = run_hook(dir.path(), &[("script_type", "down")]);
        assert!(ok);
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
