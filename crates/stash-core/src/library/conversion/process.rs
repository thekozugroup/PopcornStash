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
            ]);
            vaapi_quality(&mut command, codec, "balanced");
            command.args(["-f", "null", "-"]);
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

fn vaapi_quality(cmd: &mut tokio::process::Command, codec: &str, quality: &str) {
    // AV1 VAAPI uses its native 0–255 qindex through global_quality. Unlike
    // HEVC, it does not consume -qp: FFmpeg silently falls back to qindex 25.
    let (option, value) = if codec == "av1" {
        (
            "-global_quality",
            match quality {
                "high" => "92",
                "small" => "132",
                _ => "112",
            },
        )
    } else {
        (
            "-qp",
            match quality {
                "high" => "23",
                "small" => "33",
                _ => "28",
            },
        )
    };
    cmd.args(["-rc_mode", "CQP", option, value]);
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
        "mov,matroska,avi,mpegts,mpeg,asf,ogg,flv,mpegvideo,hevc,h264,yuv4mpegpipe,rm",
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
    if encoder.device.is_some() && hardware_decodable(facts) {
        match encode_with_decode(lib, job, p, encoder, input, output, facts, cancel, true).await {
            Ok(()) => return Ok(()),
            Err(_) if !cancel.is_cancelled() && current(lib, job).await? => {
                // This is the job's private output, never the source. Some valid
                // profiles cannot be decoded by a particular GPU/driver.
                match tokio::fs::remove_file(output).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            Err(error) => return Err(error),
        }
    }
    encode_with_decode(lib, job, p, encoder, input, output, facts, cancel, false).await
}

fn hardware_decodable(facts: &Facts) -> bool {
    facts.video().is_some_and(|video| {
        ["h264", "hevc", "av1", "vp9", "mpeg2video"]
            .contains(&video["codec_name"].as_str().unwrap_or(""))
    })
}

/// Share CPU headroom with the other conversion and the rest of the NAS.
fn decode_threads() -> String {
    (std::thread::available_parallelism().map_or(2, usize::from) / 2)
        .clamp(2, 4)
        .to_string()
}

fn gpu_decode(cmd: &mut tokio::process::Command, device: &str) {
    cmd.args([
        "-hwaccel",
        "vaapi",
        "-hwaccel_device",
        device,
        "-hwaccel_output_format",
        "vaapi",
    ]);
}

fn convert_audio(stream: &Value) -> bool {
    stream["codec_type"] == "audio"
        && ["cook", "sipr", "ra_288"].contains(&stream["codec_name"].as_str().unwrap_or(""))
}

#[allow(clippy::too_many_arguments)]
async fn encode_with_decode(
    lib: &Library,
    job: &Journal,
    p: &Policy,
    encoder: &Encoder,
    input: &Path,
    output: &Path,
    facts: &Facts,
    cancel: &CancellationToken,
    hardware_decode: bool,
) -> Result<()> {
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-n",
        "-threads",
        &decode_threads(),
        "-noautorotate",
    ]);
    if let Some(device) = &encoder.device {
        cmd.args(["-vaapi_device", device]);
        if hardware_decode {
            gpu_decode(&mut cmd, device);
        }
    }
    cmd.arg("-i").arg(tools::ffmpeg_input(input));
    // Copy other streams except RealAudio formats that FFmpeg cannot mux into
    // Matroska. Their AAC replacement is checked separately from copied bytes.
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
    // Explicit zero flags prevent Matroska from inventing default tracks.
    for (index, stream) in facts.streams.iter().enumerate() {
        if convert_audio(stream) {
            cmd.args([format!("-c:{index}"), "aac".into()]);
            cmd.args([format!("-b:{index}"), "128k".into()]);
        }
        let flags = stream["disposition"]
            .as_object()
            .map(|flags| {
                flags
                    .iter()
                    .filter(|(_, value)| value.as_i64() == Some(1))
                    .map(|(key, _)| key.as_str())
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .unwrap_or_default();
        cmd.args([
            format!("-disposition:{index}"),
            if flags.is_empty() { "0".into() } else { flags },
        ]);
    }
    let quality = match p.quality.as_str() {
        "high" => "23",
        "small" => "33",
        _ => "28",
    };
    if encoder.device.is_some() {
        let ten = facts
            .video()
            .is_some_and(|s| s["pix_fmt"] == "yuv420p10le" || s["pix_fmt"] == "p010le");
        if !hardware_decode {
            cmd.args([
                "-vf",
                if ten {
                    "format=p010le,hwupload"
                } else {
                    "format=nv12,hwupload"
                },
            ]);
        }
        vaapi_quality(&mut cmd, &encoder.codec, &p.quality);
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
    run(lib, job, cmd, facts.duration(), Progress::Encoding, cancel).await?;
    Ok(())
}

pub(super) async fn verify(
    lib: &Library,
    job: &Journal,
    input: &Path,
    output: &Path,
    before: &Facts,
    encoder: &Encoder,
    cancel: &CancellationToken,
) -> Result<()> {
    let after = probe(output).await?;
    validate(before, &after, &encoder.codec)?;
    // Decode BOTH complete files with errors fatal, including every audio stream.
    // Frame totals must match; a playable prefix is not a successful conversion.
    let mut counts = Vec::new();
    for (index, (path, facts)) in [(input, before), (output, &after)].into_iter().enumerate() {
        let device = encoder
            .device
            .as_deref()
            .filter(|_| hardware_decodable(facts));
        let count = if let Some(device) = device {
            match decode(
                lib,
                job,
                path,
                before.duration(),
                Some(device),
                index,
                cancel,
            )
            .await
            {
                Ok(count) => count,
                Err(_) if !cancel.is_cancelled() && current(lib, job).await? => {
                    // Recheck the complete file in software. A hardware failure
                    // never becomes a successful check or a partial-file check.
                    decode(lib, job, path, before.duration(), None, index, cancel).await?
                }
                Err(error) => return Err(error),
            }
        } else {
            decode(lib, job, path, before.duration(), None, index, cancel).await?
        };
        counts.push(count);
    }
    if counts[0] == 0 || counts[0] != counts[1] {
        return Err(CoreError::external(
            "The frame count changed. The original was kept.",
        ));
    }
    let copied: Vec<_> = before
        .streams
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            (s["codec_type"] == "audio" && !convert_audio(s)) || s["codec_type"] == "subtitle"
        })
        .map(|(index, _)| index)
        .collect();
    if !copied.is_empty() {
        let original = copied_stream_hash(lib, job, input, &copied, cancel).await?;
        let converted = copied_stream_hash(lib, job, output, &copied, cancel).await?;
        if original != converted {
            return Err(CoreError::external(
                "Audio or subtitle data changed. The original was kept.",
            ));
        }
    }
    for (index, stream) in before.streams.iter().enumerate() {
        if convert_audio(stream) {
            let original = audio_extent(lib, job, input, index, cancel).await?;
            let converted = audio_extent(lib, job, output, index, cancel).await?;
            validate_audio_extent(&original, &converted)?;
        }
    }

    Ok(())
}

async fn decode(
    lib: &Library,
    job: &Journal,
    path: &Path,
    duration: f64,
    device: Option<&str>,
    index: usize,
    cancel: &CancellationToken,
) -> Result<u64> {
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-xerror",
        "-err_detect",
        "explode",
        "-threads",
        &decode_threads(),
        "-noautorotate",
    ]);
    if let Some(device) = device {
        gpu_decode(&mut cmd, device);
    }
    cmd.arg("-i").arg(tools::ffmpeg_input(path)).args([
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
    run(lib, job, cmd, duration, Progress::Verifying(index), cancel).await
}
async fn copied_stream_hash(
    lib: &Library,
    job: &Journal,
    path: &Path,
    streams: &[usize],
    cancel: &CancellationToken,
) -> Result<String> {
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args(["-v", "error", "-nostdin", "-i"])
        .arg(tools::ffmpeg_input(path));
    for index in streams {
        cmd.args(["-map", &format!("0:{index}")]);
    }
    cmd.args(["-c", "copy", "-f", "streamhash", "-hash", "sha256", "-"]);
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

#[derive(Default, Debug)]
struct AudioExtent {
    rate: u64,
    samples: u64,
    start: Option<i64>,
    end: Option<i64>,
}

impl AudioExtent {
    fn read(&mut self, line: &str) -> Result<()> {
        if let Some(timebase) = line.strip_prefix("#tb 0:") {
            let Some((numerator, denominator)) = timebase.trim().split_once('/') else {
                return Err(audio_integrity_error());
            };
            if numerator.trim() != "1" {
                return Err(audio_integrity_error());
            }
            self.rate = denominator
                .trim()
                .parse()
                .map_err(|_| audio_integrity_error())?;
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            if fields.len() != 6 || fields[0] != "0" {
                return Err(audio_integrity_error());
            }
            let pts: i64 = fields[2].parse().map_err(|_| audio_integrity_error())?;
            let samples: u64 = fields[3].parse().map_err(|_| audio_integrity_error())?;
            if self.rate == 0 || samples == 0 || self.start.is_some_and(|start| pts < start) {
                return Err(audio_integrity_error());
            }
            let end = pts
                .checked_add(i64::try_from(samples).map_err(|_| audio_integrity_error())?)
                .ok_or_else(audio_integrity_error)?;
            self.start.get_or_insert(pts);
            self.end = Some(end);
            self.samples = self
                .samples
                .checked_add(samples)
                .ok_or_else(audio_integrity_error)?;
        }
        Ok(())
    }
}

fn audio_integrity_error() -> CoreError {
    CoreError::external(
        "Converted audio is incomplete or its timing changed. The original was kept.",
    )
}

fn validate_audio_extent(before: &AudioExtent, after: &AudioExtent) -> Result<()> {
    let (Some(start), Some(end), Some(result_start), Some(result_end)) =
        (before.start, before.end, after.start, after.end)
    else {
        return Err(audio_integrity_error());
    };
    if before.rate == 0
        || before.rate != after.rate
        || before.samples == 0
        || after.samples < before.samples
        || after.samples - before.samples > 2048
    {
        return Err(audio_integrity_error());
    }
    // AAC has one 1024-sample priming frame and at most one final padding frame.
    // Account for that known delay, rather than accepting arbitrary A/V shifts.
    // Matroska timestamps are rounded to milliseconds.
    let rounding = before.rate.div_ceil(500);
    let difference = result_start
        .checked_sub(start)
        .ok_or_else(audio_integrity_error)?;
    let primed = difference
        .checked_add(1024)
        .ok_or_else(audio_integrity_error)?;
    let end_difference = result_end
        .checked_sub(end)
        .ok_or_else(audio_integrity_error)?;
    if (difference.unsigned_abs() > rounding && primed.unsigned_abs() > rounding)
        || end_difference.unsigned_abs() > 1024_u64.saturating_add(rounding)
    {
        return Err(audio_integrity_error());
    }
    Ok(())
}

async fn audio_extent(
    lib: &Library,
    job: &Journal,
    path: &Path,
    index: usize,
    cancel: &CancellationToken,
) -> Result<AudioExtent> {
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-copyts",
        "-xerror",
        "-err_detect",
        "explode",
        "-threads",
        &decode_threads(),
        "-i",
    ])
    .arg(tools::ffmpeg_input(path))
    .args([
        "-map",
        &format!("0:{index}"),
        "-c:a",
        "pcm_f32le",
        "-f",
        "framehash",
        "-hash",
        "sha256",
        "-",
    ]);
    let mut child = cmd
        .spawn()
        .map_err(|e| tools::spawn_error(tools::FFMPEG, e))?;
    let stderr = tokio::spawn(diagnostic_tail(
        child.stderr.take().ok_or_else(audio_integrity_error)?,
    ));
    let mut lines = BufReader::new(child.stdout.take().ok_or_else(audio_integrity_error)?).lines();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let deadline = tokio::time::sleep(Duration::from_secs(36 * 60 * 60));
    tokio::pin!(deadline);
    let mut extent = AudioExtent::default();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => { child.kill().await?; return Err(CoreError::conflict("Conversion stopped. The original was kept.")); }
            _ = &mut deadline => { child.kill().await?; return Err(audio_integrity_error()); }
            _ = tick.tick() => if !current(lib, job).await? { child.kill().await?; return Err(CoreError::conflict("Library settings or the file changed. The original was kept.")); },
            line = lines.next_line() => {
                let Some(line) = line? else { break; };
                extent.read(&line)?;
            }
        }
    }
    let status = child.wait().await?;
    let _ = stderr.await;
    if !status.success() {
        return Err(audio_integrity_error());
    }
    Ok(extent)
}

// Unspecified legacy SAR displays as square pixels. Named non-square ratios
// must still match, including equivalent rational representations.
fn aspect_ratio(stream: &Value) -> (u64, u64) {
    let ratio = stream["sample_aspect_ratio"].as_str().unwrap_or("");
    let Some((a, b)) = ratio.split_once(':') else {
        return (1, 1);
    };
    let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        return (1, 1);
    };
    if a == 0 || b == 0 {
        return (1, 1);
    }
    let (mut n, mut d) = (a, b);
    while d != 0 {
        (n, d) = (d, n % d);
    }
    (a / n, b / n)
}

pub(super) fn validate(before: &Facts, after: &Facts, codec: &str) -> Result<()> {
    let fail = |reason: &str| {
        CoreError::external(format!(
            "Integrity check failed: {reason}. The original was kept."
        ))
    };
    if before.streams.len() != after.streams.len()
        || before.chapters.len() != after.chapters.len()
        || (before.duration() - after.duration()).abs() > 0.25
    {
        return Err(fail("stream count, chapters or duration changed"));
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
            return Err(fail("container metadata changed"));
        }
    }
    for (a, b) in before.streams.iter().zip(&after.streams) {
        if a["codec_type"] != b["codec_type"] {
            return Err(fail("stream order or type changed"));
        }
        if a["codec_type"] == "video" {
            if b["codec_name"] != codec
                || a["width"] != b["width"]
                || a["height"] != b["height"]
                || aspect_ratio(a) != aspect_ratio(b)
            {
                return Err(fail("video format, dimensions or aspect ratio changed"));
            }
            let ten_before =
                a["pix_fmt"].as_str().unwrap_or("").contains("10") || a["pix_fmt"] == "p010le";
            let ten_after =
                b["pix_fmt"].as_str().unwrap_or("").contains("10") || b["pix_fmt"] == "p010le";
            if ten_before != ten_after {
                return Err(fail("video bit depth changed"));
            }
            for key in [
                "color_range",
                "color_space",
                "color_transfer",
                "color_primaries",
            ] {
                if a.get(key).is_some_and(|v| !v.is_null() && v != "unknown") && a[key] != b[key] {
                    return Err(fail("video colour metadata changed"));
                }
            }
        } else if if convert_audio(a) {
            b["codec_name"] != "aac"
        } else {
            a["codec_name"] != b["codec_name"]
        } {
            return Err(fail("a copied stream codec changed"));
        }
        if a["codec_type"] == "audio"
            && ((number(&a["start_time"]) - before_start)
                - (number(&b["start_time"]) - after_start))
                .abs()
                > 0.05
                    + if convert_audio(a) {
                        1024.0 / number(&a["sample_rate"]).max(1.0)
                    } else {
                        0.0
                    }
        {
            return Err(fail("audio and video timing changed"));
        }
        if a["codec_type"] == "audio"
            && (a["channels"] != b["channels"] || a["sample_rate"] != b["sample_rate"])
        {
            return Err(fail("audio channels or sample rate changed"));
        }
        // FFmpeg's Matroska demuxer omits the literal language "und".
        // Missing and "und" both mean undetermined; a named language must
        // still match exactly, in both directions, on every stream.
        let language = |stream: &Value| {
            stream["tags"]["language"]
                .as_str()
                .filter(|value| *value != "und")
                .map(str::to_owned)
        };
        if language(a) != language(b) {
            return Err(fail("a stream language changed"));
        }
        if a["tags"].get("title").is_some() && a["tags"]["title"] != b["tags"]["title"] {
            return Err(fail("a stream title changed"));
        }
        if a["codec_type"] != "video"
            && !convert_audio(a)
            && a.get("extradata_hash").is_some()
            && a["extradata_hash"] != b["extradata_hash"]
        {
            return Err(fail("copied stream headers changed"));
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
                return Err(fail("a stream disposition changed"));
            }
        }
        if a["disposition"].get("default").is_some()
            && a["disposition"]["default"] != b["disposition"]["default"]
        {
            return Err(fail("a default stream changed"));
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
                return Err(fail("chapter timing changed"));
            }
        }
        if a["tags"] != b["tags"] {
            return Err(fail("chapter metadata changed"));
        }
    }
    Ok(())
}

enum Progress {
    Encoding,
    Verifying(usize),
}

// Drain the pipe even after the bounded diagnostic buffer is full. Raw tool
// output can contain private paths and metadata; only fixed messages leave here.
async fn diagnostic_tail(mut stderr: impl tokio::io::AsyncRead + Unpin) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut buffer = [0_u8; 2048];
    while let Ok(count) = stderr.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&buffer[..count]);
        if tail.len() > 8192 {
            tail.drain(..tail.len() - 8192);
        }
    }
    tail
}

fn failure_message(progress: Progress, stderr: &[u8]) -> &'static str {
    let message = String::from_utf8_lossy(stderr);
    if message.contains("No space left on device") {
        return "Conversion ran out of free space. The original was kept.";
    }
    match progress {
        Progress::Encoding if message.contains("does not yet support muxing") => {
            "A legacy stream cannot be stored in MKV. The original was kept."
        }
        Progress::Encoding => "Video encoding failed. The original was kept.",
        Progress::Verifying(0) => "The original could not be fully decoded. It was kept.",
        Progress::Verifying(_) => {
            "The converted video could not be fully decoded. The original was kept."
        }
    }
}
impl Progress {
    fn state(&self) -> &'static str {
        match self {
            Self::Encoding => "encoding",
            Self::Verifying(_) => "verifying",
        }
    }
    fn fraction(&self, value: f64) -> f64 {
        match self {
            Self::Encoding => value,
            Self::Verifying(index) => (*index as f64 + value) / 2.0,
        }
    }
}

async fn run(
    lib: &Library,
    job: &Journal,
    mut command: tokio::process::Command,
    duration: f64,
    progress: Progress,
    cancel: &CancellationToken,
) -> Result<u64> {
    command.stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| tools::spawn_error(tools::FFMPEG, e))?;
    let stderr =
        tokio::spawn(diagnostic_tail(child.stderr.take().ok_or_else(|| {
            CoreError::external("Missing encoder diagnostics.")
        })?));
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
                sqlx::query("UPDATE conversion_jobs SET progress=?,updated_at=? WHERE id=?").bind(progress.fraction(fraction)).bind(crate::now()).bind(&job.id).execute(&lib.db).await?;
                super::changed(lib, progress.state());
            }
            line = lines.next_line() => {
                let Some(line) = line? else { break; };
                if let Some(value) = line.strip_prefix("frame=") { frames = value.trim().parse().unwrap_or(0); }
                if let Some(value) = line.strip_prefix("out_time_us=") { fraction = (value.trim().parse::<f64>().unwrap_or(0.0) / 1_000_000.0 / duration).clamp(0.0,1.0); }
            }
        }
    }
    let status = child.wait().await?;
    let diagnostics = stderr.await.unwrap_or_default();
    if !status.success() {
        return Err(CoreError::external(failure_message(progress, &diagnostics)));
    }
    sqlx::query("UPDATE conversion_jobs SET progress=?,updated_at=? WHERE id=?")
        .bind(progress.fraction(1.0))
        .bind(crate::now())
        .bind(&job.id)
        .execute(&lib.db)
        .await?;
    super::changed(lib, progress.state());
    Ok(frames)
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn diagnostics_larger_than_a_pipe_are_drained_with_bounded_memory() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        let task = tokio::spawn(diagnostic_tail(reader));
        writer.write_all(&vec![b'x'; 128 * 1024]).await.unwrap();
        writer.write_all(b"No space left on device").await.unwrap();
        drop(writer);
        let tail = task.await.unwrap();
        assert_eq!(tail.len(), 8192);
        assert!(tail.ends_with(b"No space left on device"));
        assert!(failure_message(Progress::Encoding, &tail).contains("free space"));
    }

    #[test]
    fn diagnostics_explain_known_causes_without_exposing_private_tool_output() {
        let stderr =
            b"/private/personal-title.rm: The Matroska muxer does not yet support muxing cook";
        assert_eq!(
            failure_message(Progress::Encoding, stderr),
            "A legacy stream cannot be stored in MKV. The original was kept."
        );
        assert_eq!(
            failure_message(Progress::Verifying(0), b"private source"),
            "The original could not be fully decoded. It was kept."
        );
        assert_eq!(
            failure_message(Progress::Verifying(1), b"private converted file"),
            "The converted video could not be fully decoded. The original was kept."
        );
    }
    #[test]
    fn decoded_audio_counts_allow_only_aac_priming_and_final_padding() {
        for rate in [16000, 44100, 48000] {
            let before = AudioExtent {
                rate,
                samples: 100000,
                start: Some(2048),
                end: Some(102048),
            };
            for start in [2048, 1024] {
                let mut after = AudioExtent {
                    rate,
                    samples: 100000 + if start == 2048 { 1024 } else { 1376 },
                    start: Some(start),
                    end: Some(start + 100000 + if start == 2048 { 1024 } else { 1376 }),
                };
                assert!(validate_audio_extent(&before, &after).is_ok());
                after.samples = 99999;
                assert!(validate_audio_extent(&before, &after).is_err());
                after.samples = 102049;
                assert!(validate_audio_extent(&before, &after).is_err());
                after.samples = 101376;
                after.start = Some(1548);
                assert!(validate_audio_extent(&before, &after).is_err());
                after.start = Some(start);
                after.end = Some(99999);
                assert!(validate_audio_extent(&before, &after).is_err());
                after.end = Some(i64::MIN);
                assert!(validate_audio_extent(&before, &after).is_err());
                after.rate += 1;
                assert!(validate_audio_extent(&before, &after).is_err());
            }
        }
        assert!(validate_audio_extent(&AudioExtent::default(), &AudioExtent::default()).is_err());
    }

    #[test]
    fn decoded_audio_framehash_parser_is_checked_and_bounded() {
        let mut extent = AudioExtent::default();
        for line in [
            "#format: frame checksums",
            "#tb 0: 1/44100",
            "0, -1024, -1024, 1024, 8192, abc",
            "0, 0, 0, 1024, 8192, def",
        ] {
            extent.read(line).unwrap();
        }
        assert_eq!(extent.rate, 44100);
        assert_eq!(extent.samples, 2048);
        assert_eq!(extent.start, Some(-1024));
        assert_eq!(extent.end, Some(1024));
        for line in [
            "#tb 0: 2/44100",
            "0, 0, 0, 0, 0, x",
            "0, 0, 9223372036854775807, 1024, 0, x",
            "0, 0, -2048, 1024, 0, x",
            "1, 0, 0, 1024, 0, x",
            "invalid",
        ] {
            assert!(extent.read(line).is_err(), "{line}");
        }
    }

    #[test]
    fn legacy_audio_exception_is_narrow_and_preserves_other_audio_contracts() {
        use serde_json::json;
        for name in ["cook", "sipr", "ra_288", "aac", "ac3", "wmav2", "pcm_f32le"] {
            let legacy = ["cook", "sipr", "ra_288"].contains(&name);
            let before = Facts {
                streams: vec![
                    json!({"codec_type":"audio", "codec_name":name, "sample_rate":"44100", "channels":2, "start_time":"0", "extradata_hash":"original", "tags":{"language":"eng", "title":"Main"}}),
                ],
                chapters: vec![],
                format: json!({"duration":"3"}),
            };
            assert_eq!(convert_audio(&before.streams[0]), legacy);
            let mut after = Facts {
                streams: before.streams.clone(),
                chapters: vec![],
                format: before.format.clone(),
            };
            after.streams[0]["codec_name"] = json!("aac");
            after.streams[0]["extradata_hash"] = json!("new AAC headers");
            assert_eq!(validate(&before, &after, "av1").is_ok(), legacy);
            if legacy {
                for (key, value) in [
                    ("channels", json!(1)),
                    ("sample_rate", json!("48000")),
                    ("codec_name", json!(name)),
                    ("tags", json!({"language":"fra", "title":"Main"})),
                ] {
                    let saved = after.streams[0][key].clone();
                    after.streams[0][key] = value;
                    assert!(validate(&before, &after, "av1").is_err(), "{name}: {key}");
                    after.streams[0][key] = saved;
                }
            }
        }
    }
}
