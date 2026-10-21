use super::{Encoder, Journal, Library, Policy, current};
use crate::{CoreError, Result, media::tools};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::OnceCell,
};
use tokio_util::sync::CancellationToken;

static ENCODERS: OnceCell<Vec<Encoder>> = OnceCell::const_new();

pub(super) async fn encoders() -> &'static Vec<Encoder> {
    ENCODERS.get_or_init(discover).await
}
async fn discover() -> Vec<Encoder> {
    let mut available = Vec::new();
    // A listed encoder is not evidence that its GPU/driver works: encode frames.
    for index in 128..136 {
        let device = format!("/dev/dri/renderD{index}");
        if !Path::new(&device).exists() {
            continue;
        }
        let vendor =
            tokio::fs::read_to_string(format!("/sys/class/drm/renderD{index}/device/vendor"))
                .await
                .unwrap_or_default();
        let model =
            tokio::fs::read_to_string(format!("/sys/class/drm/renderD{index}/device/device"))
                .await
                .unwrap_or_default();
        let label = match vendor.trim() {
            "0x8086" if model.trim().starts_with("0x56") || model.trim().starts_with("0xe2") => {
                "Intel Arc GPU"
            }
            "0x8086" => "Intel GPU",
            "0x1002" => "AMD GPU",
            _ => "GPU",
        };
        for (codec, name) in [("av1", "av1_vaapi"), ("hevc", "hevc_vaapi")] {
            let mut command = tools::command(tools::FFMPEG);
            command.args([
                "-v",
                "error",
                "-nostdin",
                "-vaapi_device",
                &device,
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x180:rate=24",
                "-frames:v",
                "2",
                "-vf",
                "format=nv12,hwupload",
                "-c:v",
                name,
                "-rc_mode",
                "CQP",
                "-qp",
                "28",
                "-f",
                "null",
                "-",
            ]);
            if tools::run(command, tools::FFMPEG, Duration::from_secs(15))
                .await
                .is_ok_and(|o| o.status.success())
            {
                available.push(Encoder {
                    codec: codec.into(),
                    name: name.into(),
                    label: format!("{label} · {}", if codec == "av1" { "AV1" } else { "H.265" }),
                    device: Some(device.clone()),
                    hardware: true,
                });
            }
        }
    }
    for (codec, name, hardware) in [
        ("av1", "av1_nvenc", true),
        ("hevc", "hevc_nvenc", true),
        ("av1", "libsvtav1", false),
        ("av1", "libaom-av1", false),
        ("hevc", "libx265", false),
    ] {
        let mut command = tools::command(tools::FFMPEG);
        command.args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-frames:v",
            "2",
            "-c:v",
            name,
            "-threads",
            "2",
        ]);
        if name == "libsvtav1" {
            command.args(["-svtav1-params", "lp=2", "-preset", "10"]);
        }
        if name == "libaom-av1" {
            command.args(["-cpu-used", "8"]);
        }
        if name == "libx265" {
            command.args([
                "-x265-params",
                "pools=2:frame-threads=1",
                "-preset",
                "ultrafast",
            ]);
        }
        command.args(["-f", "null", "-"]);
        if tools::run(command, tools::FFMPEG, Duration::from_secs(20))
            .await
            .is_ok_and(|o| o.status.success())
        {
            available.push(Encoder {
                codec: codec.into(),
                name: name.into(),
                label: format!(
                    "{} · {}",
                    if hardware { "NVIDIA GPU" } else { "CPU" },
                    if codec == "av1" { "AV1" } else { "H.265" }
                ),
                device: None,
                hardware,
            });
        }
    }
    available
}

pub(super) async fn choose_encoder(p: &Policy) -> Result<Encoder> {
    encoders().await.iter().find(|e| e.codec == p.codec && match p.compute.as_str() { "hardware" => e.hardware, "cpu" => !e.hardware, _ => true }).cloned()
        .ok_or_else(|| CoreError::external("No working encoder matches these settings. Choose Automatic or another format; the original was kept."))
}

/// Keep this descriptor alive through every child process. On Linux the child
/// opens the parent's descriptor, so a concurrent pathname replacement is harmless.
pub(super) fn descriptor_path(file: &std::fs::File, fallback: &Path) -> PathBuf {
    if cfg!(target_os = "linux") {
        PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            file.as_raw_fd()
        ))
    } else {
        fallback.to_owned()
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct Facts {
    pub streams: Vec<Value>,
    #[serde(default)]
    pub chapters: Vec<Value>,
    pub format: Value,
}
impl Facts {
    pub fn duration(&self) -> f64 {
        self.format["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    }
    fn video(&self) -> Option<&Value> {
        self.streams.iter().find(|s| s["codec_type"] == "video")
    }
}
pub(super) async fn probe(path: &Path) -> Result<Facts> {
    let mut cmd = tools::command(tools::FFPROBE);
    cmd.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "mov,matroska,avi,mpegts,mpeg,asf,ogg,flv,mpegvideo,hevc,h264,yuv4mpegpipe",
        "-show_streams",
        "-show_data_hash",
        "SHA256",
        "-show_format",
        "-show_chapters",
        "-of",
        "json",
    ])
    .arg(tools::ffmpeg_input(path));
    let output = tools::run_ok(cmd, tools::FFPROBE, Duration::from_secs(120)).await?;
    Ok(serde_json::from_slice(&output.stdout)?)
}
pub(super) fn skip_reason(f: &Facts, codec: &str) -> Option<String> {
    if f.video().is_some_and(|v| v["codec_name"] == codec) {
        return Some("Already in the selected format.".into());
    }
    if f.streams
        .iter()
        .filter(|s| s["codec_type"] == "video")
        .count()
        != 1
        || f.duration() <= 0.0
        || !f.duration().is_finite()
    {
        return Some("Unsupported video structure or duration. The original was kept.".into());
    }
    let v = f.video()?;
    let transfer = v["color_transfer"].as_str().unwrap_or("");
    let rotation = v["tags"]["rotate"]
        .as_str()
        .and_then(|r| r.parse::<i64>().ok())
        .unwrap_or(0);
    if ["smpte2084", "arib-std-b67"].contains(&transfer)
        || rotation != 0
        || v["side_data_list"].as_array().is_some_and(|list| {
            list.iter().any(|s| {
                s["rotation"].as_i64().unwrap_or(0) != 0
                    || s.to_string().contains("DOVI")
                    || s["side_data_type"] == "Display Matrix"
            })
        })
    {
        return Some(
            "HDR, Dolby Vision or rotation needs a preservation profile. The original was kept."
                .into(),
        );
    }
    if !["yuv420p", "yuvj420p", "yuv420p10le", "nv12", "p010le"]
        .contains(&v["pix_fmt"].as_str().unwrap_or(""))
    {
        return Some(
            "This colour format needs a preservation profile. The original was kept.".into(),
        );
    }
    None
}

pub(super) async fn hash(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn encode(
    lib: &Library,
    job: &Journal,
    p: &Policy,
    encoder: &Encoder,
    input: &Path,
    output: &Path,
    facts: &Facts,
    cancel: &CancellationToken,
) -> Result<()> {
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-n",
        "-threads",
        "2",
        "-noautorotate",
    ]);
    if let Some(device) = &encoder.device {
        cmd.args(["-vaapi_device", device]);
    }
    cmd.arg("-i").arg(tools::ffmpeg_input(input));
    // Copy every other stream, attachment, metadata and chapter. If Matroska cannot
    // represent one of them FFmpeg fails and the original is retained.
    cmd.args([
        "-map",
        "0",
        "-map_metadata",
        "0",
        "-map_chapters",
        "0",
        "-c",
        "copy",
        "-c:v:0",
        &encoder.name,
    ]);
    let quality = match p.quality.as_str() {
        "high" => "23",
        "small" => "33",
        _ => "28",
    };
    if encoder.device.is_some() {
        let ten = facts
            .video()
            .is_some_and(|s| s["pix_fmt"] == "yuv420p10le" || s["pix_fmt"] == "p010le");
        cmd.args([
            "-vf",
            if ten {
                "format=p010le,hwupload"
            } else {
                "format=nv12,hwupload"
            },
            "-rc_mode",
            "CQP",
            "-qp",
            quality,
        ]);
    } else if encoder.hardware {
        cmd.args(["-rc", "vbr", "-cq", quality, "-b:v", "0"]);
    } else {
        cmd.args(["-crf", quality, "-threads", "2"]);
        match encoder.name.as_str() {
            "libsvtav1" => {
                cmd.args(["-preset", "8", "-svtav1-params", "lp=2"]);
            }
            "libaom-av1" => {
                cmd.args(["-cpu-used", "6", "-b:v", "0"]);
            }
            _ => {
                cmd.args([
                    "-preset",
                    "medium",
                    "-x265-params",
                    "pools=2:frame-threads=1",
                ]);
            }
        }
    }
    cmd.args([
        "-fps_mode",
        "passthrough",
        "-progress",
        "pipe:1",
        "-nostats",
        "-f",
        "matroska",
    ])
    .arg(output);
    run(lib, job, cmd, facts.duration(), true, cancel).await?;
    Ok(())
}

pub(super) async fn verify(
    lib: &Library,
    job: &Journal,
    input: &Path,
    output: &Path,
    before: &Facts,
    codec: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    let after = probe(output).await?;
    validate(before, &after, codec)?;
    // Decode BOTH complete files with errors fatal, including every audio stream.
    // Frame totals must match; a playable prefix is not a successful conversion.
    let mut counts = Vec::new();
    for path in [input, output] {
        let mut cmd = tools::command(tools::FFMPEG);
        cmd.args([
            "-v",
            "error",
            "-nostdin",
            "-xerror",
            "-err_detect",
            "explode",
            "-threads",
            "2",
            "-noautorotate",
            "-i",
        ])
        .arg(tools::ffmpeg_input(path))
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-fps_mode",
            "passthrough",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            "null",
            "-",
        ]);
        counts.push(run(lib, job, cmd, before.duration(), false, cancel).await?);
    }
    if counts[0] == 0 || counts[0] != counts[1] {
        return Err(CoreError::external(
            "The frame count changed. The original was kept.",
        ));
    }
    if before
        .streams
        .iter()
        .any(|s| s["codec_type"] == "audio" || s["codec_type"] == "subtitle")
    {
        let original = copied_stream_hash(lib, job, input, cancel).await?;
        let converted = copied_stream_hash(lib, job, output, cancel).await?;
        if original != converted {
            return Err(CoreError::external(
                "Audio or subtitle data changed. The original was kept.",
            ));
        }
    }

    Ok(())
}
async fn copied_stream_hash(
    lib: &Library,
    job: &Journal,
    path: &Path,
    cancel: &CancellationToken,
) -> Result<String> {
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args(["-v", "error", "-nostdin", "-i"])
        .arg(tools::ffmpeg_input(path))
        .args([
            "-map",
            "0:a?",
            "-map",
            "0:s?",
            "-c",
            "copy",
            "-f",
            "streamhash",
            "-hash",
            "sha256",
            "-",
        ]);
    let output = tools::run_ok(cmd, tools::FFMPEG, Duration::from_secs(36 * 60 * 60));
    tokio::pin!(output);
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(CoreError::conflict("Conversion stopped. The original was kept.")),
            _ = tick.tick() => if !current(lib, job).await? { return Err(CoreError::conflict("Library settings changed. The original was kept.")); },
            result = &mut output => return Ok(String::from_utf8_lossy(&result?.stdout).lines().filter(|line| !line.starts_with('#')).collect::<Vec<_>>().join("\n")),
        }
    }
}

fn validate(before: &Facts, after: &Facts, codec: &str) -> Result<()> {
    let fail = || CoreError::external("Integrity checks failed. The original was kept.");
    if before.streams.len() != after.streams.len()
        || before.chapters.len() != after.chapters.len()
        || (before.duration() - after.duration()).abs() > 0.25
    {
        return Err(fail());
    }
    let number = |value: &Value| {
        value
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let before_start = before
        .video()
        .map(|v| number(&v["start_time"]))
        .unwrap_or(0.0);
    let after_start = after
        .video()
        .map(|v| number(&v["start_time"]))
        .unwrap_or(0.0);
    let tag = |format: &Value, key: &str| {
        format["tags"].as_object().and_then(|tags| {
            tags.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.clone())
        })
    };
    for key in [
        "title",
        "artist",
        "album",
        "comment",
        "copyright",
        "date",
        "location",
    ] {
        if tag(&before.format, key).is_some() && tag(&before.format, key) != tag(&after.format, key)
        {
            return Err(fail());
        }
    }
    for (a, b) in before.streams.iter().zip(&after.streams) {
        if a["codec_type"] != b["codec_type"] {
            return Err(fail());
        }
        if a["codec_type"] == "video" {
            if b["codec_name"] != codec
                || a["width"] != b["width"]
                || a["height"] != b["height"]
                || a["sample_aspect_ratio"] != b["sample_aspect_ratio"]
            {
                return Err(fail());
            }
            let ten_before =
                a["pix_fmt"].as_str().unwrap_or("").contains("10") || a["pix_fmt"] == "p010le";
            let ten_after =
                b["pix_fmt"].as_str().unwrap_or("").contains("10") || b["pix_fmt"] == "p010le";
            if ten_before != ten_after {
                return Err(fail());
            }
            for key in [
                "color_range",
                "color_space",
                "color_transfer",
                "color_primaries",
            ] {
                if a.get(key).is_some_and(|v| !v.is_null() && v != "unknown") && a[key] != b[key] {
                    return Err(fail());
                }
            }
        } else if a["codec_name"] != b["codec_name"] {
            return Err(fail());
        }
        if a["codec_type"] == "audio"
            && ((number(&a["start_time"]) - before_start)
                - (number(&b["start_time"]) - after_start))
                .abs()
                > 0.05
        {
            return Err(fail());
        }
        if a["codec_type"] == "audio"
            && (a["channels"] != b["channels"] || a["sample_rate"] != b["sample_rate"])
        {
            return Err(fail());
        }
        for key in ["language", "title"] {
            if a["tags"].get(key).is_some() && a["tags"][key] != b["tags"][key] {
                return Err(fail());
            }
        }
        if a["codec_type"] != "video"
            && a.get("extradata_hash").is_some()
            && a["extradata_hash"] != b["extradata_hash"]
        {
            return Err(fail());
        }
        for key in [
            "forced",
            "hearing_impaired",
            "visual_impaired",
            "dub",
            "original",
            "comment",
            "lyrics",
            "karaoke",
        ] {
            if a["disposition"].get(key).is_some() && a["disposition"][key] != b["disposition"][key]
            {
                return Err(fail());
            }
        }
        if a["disposition"].get("default").is_some()
            && a["disposition"]["default"] != b["disposition"]["default"]
        {
            return Err(fail());
        }
    }
    for (a, b) in before.chapters.iter().zip(&after.chapters) {
        for key in ["start_time", "end_time"] {
            let parse = |v: &Value| {
                v[key]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(f64::NAN)
            };
            if !(parse(a) - parse(b)).abs().is_finite() || (parse(a) - parse(b)).abs() > 0.02 {
                return Err(fail());
            }
        }
        if a["tags"] != b["tags"] {
            return Err(fail());
        }
    }
    Ok(())
}

async fn run(
    lib: &Library,
    job: &Journal,
    mut command: tokio::process::Command,
    duration: f64,
    progress: bool,
    cancel: &CancellationToken,
) -> Result<u64> {
    command.stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| tools::spawn_error(tools::FFMPEG, e))?;
    let mut lines = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| CoreError::external("Missing encoder progress."))?,
    )
    .lines();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let deadline = tokio::time::sleep(Duration::from_secs(36 * 60 * 60));
    tokio::pin!(deadline);
    let mut frames = 0;
    let mut fraction = 0.0;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => { child.kill().await?; return Err(CoreError::conflict("Conversion stopped. The original was kept.")); }
            _ = &mut deadline => { child.kill().await?; return Err(CoreError::external("Conversion timed out. The original was kept.")); }
            _ = tick.tick() => {
                if !current(lib, job).await? { child.kill().await?; return Err(CoreError::conflict("Library settings or the file changed. The original was kept.")); }
                if progress { sqlx::query("UPDATE conversion_jobs SET progress=?,updated_at=? WHERE id=?").bind(fraction).bind(crate::now()).bind(&job.id).execute(&lib.db).await?; super::changed(lib, "encoding"); }
            }
            line = lines.next_line() => {
                let Some(line) = line? else { break; };
                if let Some(value) = line.strip_prefix("frame=") { frames = value.trim().parse().unwrap_or(0); }
                if let Some(value) = line.strip_prefix("out_time_us=") { fraction = (value.trim().parse::<f64>().unwrap_or(0.0) / 1_000_000.0 / duration).clamp(0.0,1.0); }
            }
        }
    }
    if !child.wait().await?.success() {
        return Err(CoreError::external(
            "The encoder or full-video check failed. The original was kept.",
        ));
    }
    Ok(frames)
}
