//! External tool discovery: ffmpeg/ffprobe/vipsthumbnail versions and hardware encoders,
//! plus the small process helpers every media module uses.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::OnceCell;

use crate::config::HwAccel;
use crate::{CoreError, Result};

pub const FFMPEG: &str = "ffmpeg";
pub const FFPROBE: &str = "ffprobe";
pub const VIPSTHUMBNAIL: &str = "vipsthumbnail";
pub const VIPSHEADER: &str = "vipsheader";
/// Render node used for VAAPI encoding.
pub const VAAPI_DEVICE: &str = "/dev/dri/renderD128";

/// How long one hardware test encode may take before it counts as broken.
const HW_TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a version query may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// A command for an external tool: stdin closed, stdout/stderr captured, killed when dropped.
pub fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

/// An ffmpeg/ffprobe input argument for a local file. The `file:` prefix makes sure a
/// name containing `:` or starting with `-` is never read as a protocol or an option.
pub fn ffmpeg_input(path: &Path) -> OsString {
    let mut arg = OsString::from("file:");
    arg.push(path.as_os_str());
    arg
}

/// Turn a spawn failure into a plain sentence.
pub fn spawn_error(program: &str, err: std::io::Error) -> CoreError {
    if err.kind() == std::io::ErrorKind::NotFound {
        CoreError::external(format!("{program} is not installed on this server."))
    } else {
        CoreError::external(format!("{program} could not be started: {err}"))
    }
}

/// Run a command to completion, killing it when `timeout` passes.
pub async fn run(mut cmd: Command, program: &str, timeout: Duration) -> Result<Output> {
    let child = cmd.spawn().map_err(|e| spawn_error(program, e))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(err)) => Err(err.into()),
        // Dropping the future drops the child, and kill_on_drop stops it.
        Err(_) => Err(CoreError::external(format!(
            "{program} took longer than {} seconds and was stopped.",
            timeout.as_secs()
        ))),
    }
}

/// Run a command and fail with its last stderr line when it exits unsuccessfully.
pub async fn run_ok(cmd: Command, program: &str, timeout: Duration) -> Result<Output> {
    let output = run(cmd, program, timeout).await?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(CoreError::external(format!(
            "{program} failed: {}",
            stderr_summary(&output.stderr)
        )))
    }
}

/// The last meaningful stderr line, shortened for logs and error messages.
pub fn stderr_summary(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("no error output");
    let mut out: String = line.chars().take(300).collect();
    if line.chars().count() > 300 {
        out.push('…');
    }
    out
}

/// `ffmpeg -version` first line without the copyright notice (for example
/// `ffmpeg version 6.1.1-3ubuntu5`), or None when ffmpeg is missing. Cached.
pub async fn ffmpeg_version() -> Option<String> {
    static VERSION: OnceCell<Option<String>> = OnceCell::const_new();
    VERSION
        .get_or_init(|| tool_version(FFMPEG, &["-hide_banner", "-version"]))
        .await
        .clone()
}

/// `vipsthumbnail --vips-version` (for example `libvips 8.15.1`), or None when vips is missing. Cached.
pub async fn vips_version() -> Option<String> {
    static VERSION: OnceCell<Option<String>> = OnceCell::const_new();
    VERSION
        .get_or_init(|| tool_version(VIPSTHUMBNAIL, &["--vips-version"]))
        .await
        .clone()
}

/// True when `vipsthumbnail` can be run. Cached.
pub async fn has_vips() -> bool {
    vips_version().await.is_some()
}

async fn tool_version(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = command(program);
    cmd.args(args);
    match run(cmd, program, VERSION_TIMEOUT).await {
        Ok(output) if output.status.success() => {
            parse_version_line(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(output) => {
            tracing::warn!(program, error = %stderr_summary(&output.stderr), "version query failed");
            None
        }
        Err(err) => {
            tracing::warn!(program, error = %err, "tool unavailable");
            None
        }
    }
}

fn parse_version_line(stdout: &str) -> Option<String> {
    let line = stdout.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = match line.find(" Copyright") {
        Some(i) => &line[..i],
        None => line,
    };
    Some(line.trim().to_string())
}

/// Encoder short name for a hardware choice (`vaapi`, `qsv`, `nvenc`), None for software.
pub fn hwaccel_name(hw: HwAccel) -> Option<&'static str> {
    match hw {
        HwAccel::None => None,
        HwAccel::Vaapi => Some("vaapi"),
        HwAccel::Qsv => Some("qsv"),
        HwAccel::Nvenc => Some("nvenc"),
    }
}

/// Hardware encoders that actually work on this host (`vaapi`, `qsv`, `nvenc`),
/// verified by a one-frame test encode, cached after the first call.
pub async fn available_hwaccels() -> Vec<String> {
    static AVAILABLE: OnceCell<Vec<String>> = OnceCell::const_new();
    AVAILABLE.get_or_init(detect_hwaccels).await.clone()
}

async fn detect_hwaccels() -> Vec<String> {
    if ffmpeg_version().await.is_none() {
        return Vec::new();
    }
    let vaapi = async {
        if Path::new(VAAPI_DEVICE).exists() {
            test_encode(HwAccel::Vaapi).await
        } else {
            false
        }
    };
    let (vaapi, qsv, nvenc) = tokio::join!(
        vaapi,
        test_encode(HwAccel::Qsv),
        test_encode(HwAccel::Nvenc)
    );
    let found: Vec<String> = [(vaapi, "vaapi"), (qsv, "qsv"), (nvenc, "nvenc")]
        .into_iter()
        .filter(|(ok, _)| *ok)
        .map(|(_, name)| name.to_string())
        .collect();
    tracing::info!(available = ?found, "hardware encoders detected");
    found
}

/// Arguments for a one-frame test encode with a hardware encoder.
pub fn test_encode_args(hw: HwAccel) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-loglevel".into(),
        "error".into(),
    ];
    if hw == HwAccel::Vaapi {
        args.extend(["-vaapi_device".into(), VAAPI_DEVICE.into()]);
    }
    args.extend(
        [
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=256x256:r=25:d=1",
            "-frames:v",
            "1",
        ]
        .map(String::from),
    );
    match hw {
        HwAccel::Vaapi => {
            args.extend(["-vf", "format=nv12,hwupload", "-c:v", "h264_vaapi"].map(String::from))
        }
        HwAccel::Qsv => args.extend(["-pix_fmt", "nv12", "-c:v", "h264_qsv"].map(String::from)),
        HwAccel::Nvenc => {
            args.extend(["-pix_fmt", "yuv420p", "-c:v", "h264_nvenc"].map(String::from))
        }
        HwAccel::None => args.extend(["-c:v", "libx264"].map(String::from)),
    }
    args.extend(["-f", "null", "-"].map(String::from));
    args
}

async fn test_encode(hw: HwAccel) -> bool {
    let mut cmd = command(FFMPEG);
    cmd.args(test_encode_args(hw));
    match run(cmd, FFMPEG, HW_TEST_TIMEOUT).await {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            tracing::debug!(encoder = ?hw, error = %stderr_summary(&output.stderr), "hardware encoder unavailable");
            false
        }
        Err(err) => {
            tracing::debug!(encoder = ?hw, error = %err, "hardware encoder test failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_drops_copyright() {
        let out = "ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023 the FFmpeg developers\nbuilt with gcc\n";
        assert_eq!(
            parse_version_line(out).as_deref(),
            Some("ffmpeg version 6.1.1-3ubuntu5")
        );
        assert_eq!(
            parse_version_line("libvips 8.15.1\n").as_deref(),
            Some("libvips 8.15.1")
        );
        assert_eq!(parse_version_line("\n\n"), None);
    }

    #[test]
    fn stderr_summary_takes_last_line() {
        assert_eq!(
            stderr_summary(b"first\nsecond problem\n\n"),
            "second problem"
        );
        assert_eq!(stderr_summary(b""), "no error output");
        let long = "x".repeat(400);
        assert_eq!(stderr_summary(long.as_bytes()).chars().count(), 301);
    }

    #[test]
    fn input_is_file_prefixed() {
        assert_eq!(
            ffmpeg_input(Path::new("/stash/a:b.mkv")),
            OsString::from("file:/stash/a:b.mkv")
        );
        assert_eq!(
            ffmpeg_input(Path::new("-rf.mkv")),
            OsString::from("file:-rf.mkv")
        );
    }

    #[test]
    fn hw_args_pick_encoders() {
        assert!(test_encode_args(HwAccel::Vaapi).contains(&"h264_vaapi".to_string()));
        assert!(test_encode_args(HwAccel::Vaapi).contains(&VAAPI_DEVICE.to_string()));
        assert!(test_encode_args(HwAccel::Qsv).contains(&"h264_qsv".to_string()));
        assert!(test_encode_args(HwAccel::Nvenc).contains(&"h264_nvenc".to_string()));
        assert_eq!(hwaccel_name(HwAccel::None), None);
        assert_eq!(hwaccel_name(HwAccel::Qsv), Some("qsv"));
    }

    #[tokio::test]
    async fn ffmpeg_is_found_and_cached() {
        let first = ffmpeg_version().await;
        let version = first
            .clone()
            .expect("ffmpeg is installed in the test environment");
        assert!(version.starts_with("ffmpeg version"), "{version}");
        assert_eq!(ffmpeg_version().await, first);
    }

    #[tokio::test]
    async fn no_hardware_here() {
        // Test machines usually have no GPU: detection must then come back empty, not fail,
        // and on any machine it may only name encoders PopcornStash knows.
        let hw = available_hwaccels().await;
        assert!(
            hw.iter()
                .all(|h| ["vaapi", "qsv", "nvenc"].contains(&h.as_str()))
        );
        assert_eq!(available_hwaccels().await, hw);
    }

    #[tokio::test]
    async fn missing_tool_is_a_plain_error() {
        let cmd = command("popcornstash-no-such-tool");
        let err = run(cmd, "popcornstash-no-such-tool", Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "popcornstash-no-such-tool is not installed on this server."
        );
    }

    #[tokio::test]
    async fn timeout_kills() {
        let mut cmd = command("sleep");
        cmd.arg("5");
        let started = std::time::Instant::now();
        let err = run(cmd, "sleep", Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("took longer"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
