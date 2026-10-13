//! Spawning `serve` and `torrent-engine` with dropped privileges
//! (section 5, step 6) and the environment of section 2.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use nix::sys::stat::Mode;
use nix::unistd::{Gid, Uid};
use stash_core::internal_api::{ENGINE_API_ADDR, ENGINE_PROXY_ADDR, TUNNEL_IF};
use tokio::process::{Child, Command};

/// Variables only the supervisor sets; never passed through from the container environment.
pub const INTERNAL_VARS: &[&str] = &[
    "STASH_TORRENTD_URL",
    "STASH_TORRENTD_PROXY",
    "STASH_INTERNAL_TOKEN",
    "STASH_SUPERVISOR_SOCKET",
    "STASH_TUNNEL_IF",
    "STASH_ENGINE_STATE",
    "STASH_ENGINE_DOWNLOADS",
    "STASH_ENGINE_STATUS",
];

const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Credentials a child runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    /// umask for the child (the engine uses 0002 so the web server's group can move its files).
    pub umask: Option<u32>,
}

/// Everything needed to start one child.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    pub name: &'static str,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// None: keep the supervisor's identity (only when the supervisor is not root).
    pub credentials: Option<Credentials>,
    /// Pipe stdout to the caller instead of the container log (tests).
    pub capture_stdout: bool,
}

/// Inputs for building child environments.
#[derive(Debug, Clone)]
pub struct EnvInputs {
    /// The supervisor's own environment.
    pub vars: Vec<(String, String)>,
    pub token: String,
    pub run_dir: PathBuf,
    pub engine_state: PathBuf,
    pub engine_downloads: PathBuf,
    pub status_file: PathBuf,
    pub allow_no_vpn: bool,
}

fn lookup<'a>(vars: &'a [(String, String)], key: &str) -> Option<&'a str> {
    vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// PATH, locale and time zone: the only non-STASH variables children get.
fn base_env(vars: &[(String, String)], home: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "PATH".to_string(),
            lookup(vars, "PATH").unwrap_or(DEFAULT_PATH).to_string(),
        ),
        ("HOME".to_string(), home.display().to_string()),
    ];
    for key in ["TZ", "LANG", "LC_ALL", "RUST_BACKTRACE"] {
        if let Some(value) = lookup(vars, key) {
            env.push((key.to_string(), value.to_string()));
        }
    }
    env
}

/// Environment for `serve`. With `torrents` false (degraded mode) the engine
/// variables are left out, which turns torrents off in the web server.
pub fn serve_env(inputs: &EnvInputs, torrents: bool) -> Vec<(String, String)> {
    let mut env = base_env(&inputs.vars, Path::new("/tmp"));
    for (key, value) in &inputs.vars {
        let user_setting = key.starts_with("STASH_") || key == "PUID" || key == "PGID";
        if user_setting && !INTERNAL_VARS.contains(&key.as_str()) {
            env.push((key.clone(), value.clone()));
        }
    }
    if torrents {
        env.push((
            "STASH_TORRENTD_URL".into(),
            format!("http://{ENGINE_API_ADDR}"),
        ));
        env.push((
            "STASH_TORRENTD_PROXY".into(),
            format!("http://{ENGINE_PROXY_ADDR}"),
        ));
        env.push(("STASH_INTERNAL_TOKEN".into(), inputs.token.clone()));
        env.push((
            "STASH_SUPERVISOR_SOCKET".into(),
            inputs
                .run_dir
                .join(stash_core::internal_api::SUPERVISOR_SOCKET)
                .display()
                .to_string(),
        ));
    }
    env
}

/// Environment for `torrent-engine`: no user settings beyond logging.
pub fn engine_env(inputs: &EnvInputs) -> Vec<(String, String)> {
    let mut env = base_env(&inputs.vars, &inputs.engine_state);
    if let Some(log) = lookup(&inputs.vars, "STASH_LOG") {
        env.push(("STASH_LOG".into(), log.to_string()));
    }
    env.push((
        "STASH_ENGINE_STATE".into(),
        inputs.engine_state.display().to_string(),
    ));
    env.push((
        "STASH_ENGINE_DOWNLOADS".into(),
        inputs.engine_downloads.display().to_string(),
    ));
    env.push((
        "STASH_ENGINE_STATUS".into(),
        inputs.status_file.display().to_string(),
    ));
    if !inputs.allow_no_vpn {
        env.push(("STASH_TUNNEL_IF".into(), TUNNEL_IF.into()));
    }
    env.push(("STASH_ALLOW_NO_VPN".into(), inputs.allow_no_vpn.to_string()));
    env.push(("STASH_INTERNAL_TOKEN".into(), inputs.token.clone()));
    env
}

/// Runs in the forked child before `exec`. Only async-signal-safe system
/// calls: no allocation, no locks, no logging.
fn drop_privileges(credentials: Credentials) -> std::io::Result<()> {
    let errno = |e: nix::errno::Errno| std::io::Error::from_raw_os_error(e as i32);
    let gid = Gid::from_raw(credentials.gid);
    let uid = Uid::from_raw(credentials.uid);
    nix::unistd::setgroups(&[]).map_err(errno)?;
    nix::unistd::setresgid(gid, gid, gid).map_err(errno)?;
    nix::unistd::setresuid(uid, uid, uid).map_err(errno)?;
    nix::sys::prctl::set_no_new_privs().map_err(errno)?;
    if let Some(mask) = credentials.umask {
        nix::sys::stat::umask(Mode::from_bits_truncate(mask));
    }
    Ok(())
}

/// Start a child. stdout/stderr go to the container log.
#[allow(unsafe_code)]
pub fn spawn(spec: &ChildSpec) -> std::io::Result<Child> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(
            spec.env
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        )
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(if spec.capture_stdout {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stderr(Stdio::inherit());
    if let Some(credentials) = spec.credentials {
        // SAFETY: the closure runs in the child between fork and exec, where only
        // async-signal-safe functions may be used. It captures one Copy value and
        // calls setgroups, setresgid, setresuid, prctl(PR_SET_NO_NEW_PRIVS) and
        // umask through nix wrappers that issue the system call directly, without
        // allocating or taking locks; errors become `io::Error::from_raw_os_error`,
        // which does not allocate either.
        unsafe {
            command.pre_exec(move || drop_privileges(credentials));
        }
    }
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> EnvInputs {
        EnvInputs {
            vars: vec![
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("HOME".into(), "/root".into()),
                ("TZ".into(), "Europe/Amsterdam".into()),
                (
                    "STASH_PUBLIC_URL".into(),
                    "https://stash.example.com".into(),
                ),
                ("STASH_LOG".into(), "debug".into()),
                ("STASH_INTERNAL_TOKEN".into(), "from-the-user".into()),
                ("STASH_TORRENTD_URL".into(), "http://evil".into()),
                ("PUID".into(), "99".into()),
                ("AWS_SECRET_ACCESS_KEY".into(), "nope".into()),
                ("HTTPS_PROXY".into(), "http://proxy".into()),
            ],
            token: "tok".into(),
            run_dir: PathBuf::from("/run/popcornstash"),
            engine_state: PathBuf::from("/config/torrent/engine"),
            engine_downloads: PathBuf::from("/stash/Downloads/In Progress"),
            status_file: PathBuf::from("/run/popcornstash/status.json"),
            allow_no_vpn: false,
        }
    }

    fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        lookup(env, key)
    }

    #[test]
    fn serve_env_passes_settings_and_sets_internal_vars() {
        let env = serve_env(&inputs(), true);
        assert_eq!(
            get(&env, "STASH_PUBLIC_URL"),
            Some("https://stash.example.com")
        );
        assert_eq!(get(&env, "TZ"), Some("Europe/Amsterdam"));
        assert_eq!(get(&env, "PUID"), Some("99"));
        assert_eq!(
            get(&env, "STASH_TORRENTD_URL"),
            Some("http://127.0.0.1:9091")
        );
        assert_eq!(
            get(&env, "STASH_TORRENTD_PROXY"),
            Some("http://127.0.0.1:9092")
        );
        assert_eq!(get(&env, "STASH_INTERNAL_TOKEN"), Some("tok"));
        assert_eq!(
            get(&env, "STASH_SUPERVISOR_SOCKET"),
            Some("/run/popcornstash/supervisor.sock")
        );
        assert_eq!(get(&env, "HOME"), Some("/tmp"));
        assert!(get(&env, "AWS_SECRET_ACCESS_KEY").is_none());
        assert!(get(&env, "HTTPS_PROXY").is_none());
        assert_eq!(
            env.iter()
                .filter(|(k, _)| k == "STASH_INTERNAL_TOKEN")
                .count(),
            1
        );
        assert_eq!(
            env.iter()
                .filter(|(k, _)| k == "STASH_TORRENTD_URL")
                .count(),
            1
        );
    }

    #[test]
    fn degraded_serve_env_has_no_torrent_vars() {
        let env = serve_env(&inputs(), false);
        for key in INTERNAL_VARS {
            assert!(get(&env, key).is_none(), "{key}");
        }
    }

    #[test]
    fn engine_env_is_minimal() {
        let env = engine_env(&inputs());
        assert_eq!(get(&env, "STASH_TUNNEL_IF"), Some("psw0"));
        assert_eq!(get(&env, "STASH_ALLOW_NO_VPN"), Some("false"));
        assert_eq!(
            get(&env, "STASH_ENGINE_DOWNLOADS"),
            Some("/stash/Downloads/In Progress")
        );
        assert_eq!(get(&env, "STASH_LOG"), Some("debug"));
        assert_eq!(get(&env, "HOME"), Some("/config/torrent/engine"));
        assert!(get(&env, "STASH_PUBLIC_URL").is_none());
        assert!(get(&env, "PUID").is_none());
        let mut no_vpn = inputs();
        no_vpn.allow_no_vpn = true;
        let env = engine_env(&no_vpn);
        assert!(get(&env, "STASH_TUNNEL_IF").is_none());
        assert_eq!(get(&env, "STASH_ALLOW_NO_VPN"), Some("true"));
    }

    #[tokio::test]
    async fn spawned_child_drops_privileges() {
        use tokio::io::AsyncReadExt;
        if !nix::unistd::geteuid().is_root() {
            eprintln!("skipped: needs root");
            return;
        }
        let spec = ChildSpec {
            name: "probe",
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "id -u; id -g; id -G; umask; grep -E '^(NoNewPrivs|CapEff)' /proc/self/status; echo \"secret=$SECRET\"".into()],
            env: vec![("PATH".into(), DEFAULT_PATH.into())],
            cwd: PathBuf::from("/"),
            credentials: Some(Credentials { uid: 6881, gid: 4343, umask: Some(0o002) }),
            capture_stdout: true,
        };
        let mut child = spawn(&spec).unwrap();
        let mut output = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .await
            .unwrap();
        assert!(child.wait().await.unwrap().success());
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "6881");
        assert_eq!(lines[1], "4343");
        assert_eq!(lines[2], "4343", "supplementary groups cleared");
        assert_eq!(lines[3], "0002");
        assert!(output.contains("NoNewPrivs:\t1"), "{output}");
        assert!(output.contains("CapEff:\t0000000000000000"), "{output}");
        assert!(
            output.contains("secret=\n"),
            "environment cleared: {output}"
        );
    }
}
