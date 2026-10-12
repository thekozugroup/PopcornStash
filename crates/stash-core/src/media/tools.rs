//! External tool discovery: ffmpeg/ffprobe/vipsthumbnail versions and hardware encoders.

/// `ffmpeg -version` first line, or None when ffmpeg is missing.
pub async fn ffmpeg_version() -> Option<String> {
    todo!("media agent")
}

/// Hardware encoders that actually work on this host (`vaapi`, `qsv`, `nvenc`),
/// verified by a one-frame test encode, cached after the first call.
pub async fn available_hwaccels() -> Vec<String> {
    todo!("media agent")
}
