//! Small system helpers shared by the supervisor and the engine: running the
//! `ip`/`nft`/`wg` tools, atomic file writes and the clock.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt;

/// Default timeout for one helper command (`ip`, `nft`, `wg`).
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);

/// Seconds since the Unix epoch.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// A helper command failed. `stderr` is the tool's own message (never contains
/// the stdin we fed it, which may hold key material).
#[derive(Debug, thiserror::Error)]
#[error("{program} {args} failed: {message}")]
pub struct CommandError {
    pub program: String,
    pub args: String,
    pub message: String,
    /// Exit code, when the process ran and exited.
    pub code: Option<i32>,
}

impl CommandError {
    fn new(program: &str, args: &[String], message: impl Into<String>, code: Option<i32>) -> Self {
        Self {
            program: program.to_string(),
            args: args.join(" "),
            message: message.into(),
            code,
        }
    }
}

/// Run `program args...`, optionally feeding `stdin`, and return stdout.
/// Fails on a non-zero exit with the tool's stderr as the message.
pub async fn run(
    program: &str,
    args: &[String],
    stdin: Option<&[u8]>,
) -> Result<String, CommandError> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| CommandError::new(program, args, format!("could not start: {e}"), None))?;
    if let Some(input) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        pipe.write_all(input).await.map_err(|e| {
            CommandError::new(program, args, format!("could not write input: {e}"), None)
        })?;
        drop(pipe);
    }
    let output = tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| CommandError::new(program, args, "timed out", None))?
        .map_err(|e| CommandError::new(program, args, e.to_string(), None))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        let message = if message.is_empty() {
            format!("exit status {}", output.status)
        } else {
            message.chars().take(500).collect()
        };
        Err(CommandError::new(
            program,
            args,
            message,
            output.status.code(),
        ))
    }
}

/// Convenience: run with `&str` arguments.
pub async fn run_str(program: &str, args: &[&str]) -> Result<String, CommandError> {
    let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(program, &args, None).await
}

/// Write `bytes` to `path` atomically (temp file in the same directory, fsync,
/// rename) with exactly `mode`.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent directory"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(
        ".{name}.{}.tmp",
        hex::encode(rand::random::<[u8; 6]>())
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&tmp)?;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Random 32-byte token, hex encoded (the internal API token).
pub fn random_token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

/// Constant-time comparison of two byte strings (length is not secret).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Look up a program in `PATH`.
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    if program.contains('/') {
        let path = Path::new(program);
        return path.is_file().then(|| path.to_path_buf());
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            candidate
                .metadata()
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn write_atomic_sets_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        write_atomic(&path, b"{}", 0o644).unwrap();
        write_atomic(&path, b"{\"a\":1}", 0o600).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":1}");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn token_is_64_hex_chars() {
        let token = random_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn run_reports_stderr() {
        let err = run_str("sh", &["-c", "echo boom >&2; exit 3"])
            .await
            .unwrap_err();
        assert_eq!(err.code, Some(3));
        assert!(err.message.contains("boom"));
        let out = run("cat", &[], Some(b"hello")).await.unwrap();
        assert_eq!(out, "hello");
    }
}
