//! Small, on-demand seek frames. No media is written to disk or decoded in the background.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use tokio::sync::{Mutex, Semaphore};

use super::{MediaService, filters, thumbs, tools};
use crate::models::{ItemRow, ItemStatus, MediaKind};
use crate::{CoreError, Result};

const CACHE_FRAMES: usize = 64;
type Key = (String, String, i64, i64, Option<String>, u32, SystemTime);

pub(crate) struct SeekPreviews {
    // One low-resolution decode at a time, independently of conversion workers.
    pub(super) worker: Semaphore,
    frames: Mutex<VecDeque<(Key, Vec<u8>)>>,
}

impl Default for SeekPreviews {
    fn default() -> Self {
        Self {
            worker: Semaphore::new(1),
            frames: Mutex::new(VecDeque::new()),
        }
    }
}

/// At most 200 distinct frames per source; short videos retain two-second precision.
pub fn preview_second(time_s: f64, duration_s: f64) -> Result<u32> {
    if !time_s.is_finite() || time_s < 0.0 {
        return Err(CoreError::bad_field(
            "time_s",
            "Choose a valid video position.",
        ));
    }
    if !duration_s.is_finite() || duration_s <= 0.0 || duration_s > u32::MAX as f64 {
        return Err(CoreError::NotFound);
    }
    let interval = (duration_s / 200.0).ceil().max(2.0);
    Ok(((time_s.min((duration_s - 0.1).max(0.0)) / interval).floor() * interval) as u32)
}

impl MediaService {
    pub async fn seek_preview(&self, item: &ItemRow, time_s: f64) -> Result<Vec<u8>> {
        if item.kind != MediaKind::Video
            || matches!(item.status, ItemStatus::Missing | ItemStatus::Trashed)
        {
            return Err(CoreError::NotFound);
        }
        let second = preview_second(time_s, item.duration_s.unwrap_or(0.0))?;
        let abs = self.layout.abs(&item.rel_path)?;
        let source = tokio::fs::metadata(&abs).await?;
        if !source.is_file() || source.len() != item.size as u64 {
            return Err(CoreError::NotFound);
        }
        let modified = source.modified()?;
        let key = (
            item.id.clone(),
            item.rel_path.clone(),
            item.size,
            item.mtime,
            item.quick_hash.clone(),
            second,
            modified,
        );
        {
            let _publication = self.publication.lock().await;
            if !thumbs::current_version(self, item).await? {
                return Err(CoreError::NotFound);
            }
            if let Some((_, bytes)) = self
                .previews
                .frames
                .lock()
                .await
                .iter()
                .find(|(k, _)| k == &key)
            {
                return Ok(bytes.clone());
            }
        }
        // Refuse excess work instead of building a queue from fast pointer movement.
        let _worker = self.previews.worker.try_acquire().map_err(|_| {
            CoreError::conflict("A video preview is being prepared. Try again shortly.")
        })?;
        let mut cmd = tools::command(tools::FFMPEG);
        let fit = format!(
            "{},scale=w='min(320,iw)':h='min(180,ih)':force_original_aspect_ratio=decrease",
            filters::fit_box(320)
        );
        let summary = item.media();
        let video = summary.as_ref().and_then(|s| s.video.as_ref());
        let filter = match video.and_then(|v| v.hdr.as_deref()) {
            Some(hdr) => format!("{},{fit}", filters::tonemap_chain(hdr)),
            None => fit,
        };
        let map = video
            .map(|v| format!("0:{}", v.index))
            .unwrap_or_else(|| "0:v:0".into());
        cmd.args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-ss",
        ])
        .arg(second.to_string())
        .arg("-i")
        .arg(tools::ffmpeg_input(&abs))
        .arg("-map")
        .arg(map)
        .args([
            "-an",
            "-sn",
            "-frames:v",
            "1",
            "-vf",
            &filter,
            "-threads",
            "1",
            "-c:v",
            "mjpeg",
            "-q:v",
            "5",
            "-f",
            "image2pipe",
            "pipe:1",
        ]);
        let output = tools::run(cmd, tools::FFMPEG, Duration::from_secs(8)).await?;
        let bytes = output.stdout;
        if !output.status.success() || !bytes.starts_with(&[0xff, 0xd8]) || bytes.len() > 128 * 1024
        {
            // Decoder diagnostics can contain personal paths; never return them to the client.
            return Err(CoreError::external("This video preview is unavailable."));
        }
        let _publication = self.publication.lock().await;
        let current = tokio::fs::metadata(&abs).await?;
        if !thumbs::current_version(self, item).await?
            || current.len() != source.len()
            || current.modified()? != modified
        {
            return Err(CoreError::NotFound);
        }
        let mut frames = self.previews.frames.lock().await;
        if frames.len() == CACHE_FRAMES {
            frames.pop_front();
        }
        frames.push_back((key, bytes.clone()));
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_positions_are_finite_bounded_and_quantized() {
        assert_eq!(preview_second(7.9, 60.0).unwrap(), 6);
        assert_eq!(preview_second(999.0, 60.0).unwrap(), 58);
        assert_eq!(preview_second(0.0, 0.5).unwrap(), 0);
        assert_eq!(preview_second(3600.0, 7200.0).unwrap(), 3600);
        for time in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(preview_second(time, 60.0).is_err());
        }
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(preview_second(1.0, duration).is_err());
        }
    }
}
