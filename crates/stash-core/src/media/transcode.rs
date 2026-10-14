//! Live HLS transcoding sessions (section 5).
//!
//! Each playback that needs conversion gets a session: a folder in
//! `cache/transcode/<session>/`, a complete VOD playlist with fixed 6 s segments
//! generated up front, and one ffmpeg process writing `<n>.ts` files from the
//! segment the player asked for. A request just ahead of ffmpeg waits for the
//! segment; a request elsewhere (a seek) restarts ffmpeg at that segment.
//! Segment boundaries are forced at multiples of 6 s, so segments from different
//! ffmpeg runs line up. Every run also shifts its output by the same
//! [`TIMESTAMP_BASE_S`], so segment `n` carries the same timestamps (`10 + 6n`)
//! whichever run wrote it.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{filters, playback, tools};
use crate::config::{Config, HwAccel};
use crate::models::MediaSummary;
use crate::settings::SettingsStore;
use crate::{CoreError, Result};

pub const SEGMENT_SECONDS: f64 = 6.0;

/// Sessions nobody has asked anything of for this long are killed by the reaper.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// A session untouched for this long may be evicted to make room for a new one.
pub const ACTIVE_WINDOW: Duration = Duration::from_secs(20);
/// How long a request just ahead of ffmpeg waits for its segment.
const SEGMENT_WAIT: Duration = Duration::from_secs(20);
/// How long a request that restarted ffmpeg waits for the first segment.
const RESTART_WAIT: Duration = Duration::from_secs(30);
/// Requests up to this many segments past the newest one wait instead of restarting.
const LOOKAHEAD_SEGMENTS: u32 = 3;
/// How often the reaper looks for idle sessions.
const REAP_INTERVAL: Duration = Duration::from_secs(10);
/// How long a hardware encoder gets to prove it works before falling back to libx264.
const HW_START_CHECK: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);
/// Trailing bits shorter than this are folded into the previous segment.
const MIN_LAST_SEGMENT: f64 = 0.1;
/// Constant added to every run's output timestamps. Without it the run that starts at
/// segment 0 begins with negative decode times (B-frame delay, AAC priming), which the
/// muxer shifts forward by a few frames, so its segments would not match those of runs
/// restarted later. Ten seconds covers that delay for any real frame rate (Apple's
/// segmenter uses the same base).
pub const TIMESTAMP_BASE_S: f64 = 10.0;
/// Longest video a session accepts. A damaged header can claim a length of years, and the
/// playlist for that would not fit in memory.
pub const MAX_DURATION_S: f64 = 72.0 * 3600.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StreamOptions {
    pub max_height: Option<u32>,
    pub start_s: Option<f64>,
    pub audio_index: Option<i64>,
    /// Client can display HDR; otherwise HDR is tone mapped.
    pub hdr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInfo {
    pub session_id: String,
    /// `/api/stream/<session>/index.m3u8`
    pub url: String,
}

pub struct TranscodeManager {
    pub config: Arc<Config>,
    pub settings: SettingsStore,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Removal of folders left over from a previous run; awaited before the first session starts.
    cleanup: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    /// One bit per hardware encoder that failed to start; later sessions using that
    /// encoder go straight to libx264. Another encoder chosen in settings is still tried.
    hw_broken: Arc<AtomicU8>,
    /// Read input at playback speed (tests use this to keep ffmpeg behind the player).
    realtime_input: bool,
    /// Trust the configured hardware encoder without the detection probe (tests).
    trust_hwaccel: bool,
    /// How long a hardware encoder gets to show it works before a session relies on it.
    hw_start_check: Duration,
}

impl TranscodeManager {
    pub fn new(config: Arc<Config>, settings: SettingsStore) -> Self {
        let dir = config.transcode_dir();
        let cleanup = match tokio::runtime::Handle::try_current() {
            Ok(handle) => Some(handle.spawn(clear_leftovers(dir))),
            Err(_) => {
                clear_leftovers_blocking(&dir);
                None
            }
        };
        Self {
            config,
            settings,
            sessions: Mutex::new(HashMap::new()),
            cleanup: tokio::sync::Mutex::new(cleanup),
            hw_broken: Arc::new(AtomicU8::new(0)),
            realtime_input: false,
            trust_hwaccel: false,
            hw_start_check: HW_START_CHECK,
        }
    }

    /// Start a session for `item_id` whose original is `source`, belonging to nobody.
    pub async fn start(
        &self,
        item_id: &str,
        source: PathBuf,
        media: &MediaSummary,
        options: StreamOptions,
    ) -> Result<SessionInfo> {
        self.start_for("", item_id, source, media, options).await
    }

    /// Start a session for `item_id` that belongs to the person `user_id`: only they may
    /// request its playlist and segments (section 7).
    pub async fn start_for(
        &self,
        user_id: &str,
        item_id: &str,
        source: PathBuf,
        media: &MediaSummary,
        options: StreamOptions,
    ) -> Result<SessionInfo> {
        self.await_cleanup().await;
        let duration = media
            .duration_s
            .filter(|d| d.is_finite() && *d > 0.0)
            .ok_or_else(|| CoreError::invalid("This video's length is not known yet, so it cannot be converted for streaming."))?;
        if duration > MAX_DURATION_S {
            return Err(CoreError::invalid(
                "This video says it is longer than 72 hours, which is too long to convert for streaming.",
            ));
        }
        if media.video.is_none() && media.audio.is_empty() {
            return Err(CoreError::invalid(
                "This file has no video or audio to play.",
            ));
        }
        if let Some(index) = options.audio_index
            && !media.audio.iter().any(|a| a.index == index)
        {
            return Err(CoreError::invalid(
                "That audio track does not exist in this file.",
            ));
        }
        let settings = self.settings.get().await.transcode;
        let encoder = self.pick_encoder(settings.hwaccel).await;
        let segments = segment_count(duration);
        let start_index = options
            .start_s
            .filter(|s| s.is_finite() && *s > 0.0)
            .map_or(0, |s| {
                ((s / SEGMENT_SECONDS).floor() as u32).min(segments - 1)
            });

        let id = hex::encode(rand::random::<[u8; 16]>());
        let dir = self.config.transcode_dir().join(&id);
        let session = Arc::new(Session {
            id: id.clone(),
            user_id: user_id.to_string(),
            item_id: item_id.to_string(),
            source,
            dir,
            media: media.clone(),
            options,
            duration,
            segments,
            crf: settings.crf,
            max_height: settings.max_height,
            encoder,
            realtime_input: self.realtime_input,
            hw_start_check: self.hw_start_check,
            hw_broken: self.hw_broken.clone(),
            last_access: Mutex::new(Instant::now()),
            stopped: AtomicBool::new(false),
            encoder_state: tokio::sync::Mutex::new(None),
        });

        let (victims, admitted) = {
            let mut map = self.lock_sessions();
            let max = settings.max_sessions.max(1) as usize;
            let mut victims = Vec::new();
            while map.len() >= max {
                let lru = map
                    .values()
                    .filter(|s| s.idle_for() >= ACTIVE_WINDOW)
                    .min_by_key(|s| s.last_access())
                    .map(|s| s.id.clone());
                match lru.and_then(|id| map.remove(&id)) {
                    Some(victim) => victims.push(victim),
                    None => break,
                }
            }
            let admitted = map.len() < max;
            if admitted {
                map.insert(id.clone(), session.clone());
            }
            (victims, admitted)
        };
        for victim in victims {
            tracing::info!(session = %victim.id, "evicting an idle transcode to make room");
            victim.shutdown().await;
        }
        if !admitted {
            return Err(CoreError::conflict(
                "Too many videos are transcoding right now. Try again in a minute.",
            ));
        }

        let launched = async {
            tokio::fs::create_dir_all(&session.dir).await?;
            let mut state = session.encoder_state.lock().await;
            *state = Some(session.launch(start_index).await?);
            Ok::<_, CoreError>(())
        }
        .await;
        if let Err(err) = launched {
            self.lock_sessions().remove(&id);
            session.shutdown().await;
            return Err(err);
        }
        tracing::info!(session = %id, item_id, start_index, segments, encoder = ?encoder, "transcode started");
        Ok(SessionInfo {
            url: format!("/api/stream/{id}/index.m3u8"),
            session_id: id,
        })
    }

    /// The full VOD playlist for a session.
    pub async fn playlist(&self, session_id: &str) -> Result<String> {
        let session = self.get(session_id)?;
        session.touch();
        Ok(build_playlist(session.duration, session.segments))
    }

    /// Path of a finished segment (waits for it or restarts ffmpeg as needed). `name` is e.g. `12.ts`.
    pub async fn segment(&self, session_id: &str, name: &str) -> Result<PathBuf> {
        let index = parse_segment_name(name).ok_or(CoreError::NotFound)?;
        let session = self.get(session_id)?;
        session.touch();
        if index >= session.segments {
            return Err(CoreError::NotFound);
        }
        let result = session.segment(index).await;
        session.touch();
        result
    }

    pub async fn stop(&self, session_id: &str) {
        let removed = self.lock_sessions().remove(session_id);
        if let Some(session) = removed {
            session.shutdown().await;
            tracing::info!(session = %session_id, "transcode stopped");
        }
    }

    /// Stop every session (shutdown).
    pub async fn stop_all(&self) {
        let all: Vec<Arc<Session>> = self.lock_sessions().drain().map(|(_, s)| s).collect();
        for session in all {
            session.shutdown().await;
        }
    }

    /// Stop sessions nobody has touched for `max_idle`. Returns how many were stopped.
    pub async fn reap_idle(&self, max_idle: Duration) -> usize {
        let idle: Vec<Arc<Session>> = {
            let mut map = self.lock_sessions();
            let ids: Vec<String> = map
                .values()
                .filter(|s| s.idle_for() >= max_idle)
                .map(|s| s.id.clone())
                .collect();
            ids.iter().filter_map(|id| map.remove(id)).collect()
        };
        for session in &idle {
            tracing::info!(session = %session.id, "stopping idle transcode");
            session.shutdown().await;
        }
        idle.len()
    }

    /// Number of live sessions.
    pub fn session_count(&self) -> usize {
        self.lock_sessions().len()
    }

    /// The item a session plays, None when the session does not exist.
    pub fn session_item_id(&self, session_id: &str) -> Option<String> {
        self.lock_sessions()
            .get(session_id)
            .map(|s| s.item_id.clone())
    }

    /// The person a session belongs to and the item it plays.
    pub fn session_owner(&self, session_id: &str) -> Option<(String, String)> {
        self.lock_sessions()
            .get(session_id)
            .map(|s| (s.user_id.clone(), s.item_id.clone()))
    }

    /// Kill sessions idle for 60s and delete their folders.
    pub fn spawn_reaper(self: &Arc<Self>, cancel: CancellationToken) -> JoinHandle<()> {
        let manager: Weak<Self> = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REAP_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        if let Some(manager) = manager.upgrade() {
                            manager.stop_all().await;
                        }
                        break;
                    }
                    _ = tick.tick() => {
                        let Some(manager) = manager.upgrade() else { break };
                        manager.reap_idle(IDLE_TIMEOUT).await;
                    }
                }
            }
        })
    }

    fn get(&self, session_id: &str) -> Result<Arc<Session>> {
        self.lock_sessions()
            .get(session_id)
            .cloned()
            .ok_or(CoreError::NotFound)
    }

    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Wait for the startup cleanup. Concurrent callers queue on the lock, so no session
    /// folder is created while old ones are still being deleted.
    async fn await_cleanup(&self) {
        let mut guard = self.cleanup.lock().await;
        if let Some(handle) = guard.take() {
            let _ = handle.await;
        }
    }

    async fn pick_encoder(&self, hw: HwAccel) -> VideoEncoder {
        let wanted = VideoEncoder::from(hw);
        if wanted == VideoEncoder::Software || wanted.is_broken(&self.hw_broken) {
            return VideoEncoder::Software;
        }
        let name = tools::hwaccel_name(hw).unwrap_or_default();
        if self.trust_hwaccel || tools::available_hwaccels().await.iter().any(|h| h == name) {
            wanted
        } else {
            tracing::warn!(
                hwaccel = name,
                "hardware encoder is not usable on this host; using libx264"
            );
            wanted.mark_broken(&self.hw_broken);
            VideoEncoder::Software
        }
    }

    #[cfg(test)]
    fn realtime(mut self) -> Self {
        self.realtime_input = true;
        self
    }

    #[cfg(test)]
    fn trust_hwaccel(mut self) -> Self {
        self.trust_hwaccel = true;
        self
    }

    #[cfg(test)]
    fn no_hw_start_check(mut self) -> Self {
        self.hw_start_check = Duration::ZERO;
        self
    }

    #[cfg(test)]
    fn make_idle(&self, session_id: &str, idle: Duration) {
        if let Some(s) = self.lock_sessions().get(session_id) {
            *s.last_access.lock().unwrap() = Instant::now() - idle;
        }
    }

    #[cfg(test)]
    async fn encoder_start(&self, session_id: &str) -> Option<u32> {
        let session = self.get(session_id).ok()?;
        let state = session.encoder_state.lock().await;
        state.as_ref().map(|e| e.start)
    }
}

/// Which H.264 encoder a session uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoder {
    Software,
    Vaapi,
    Qsv,
    Nvenc,
}

impl From<HwAccel> for VideoEncoder {
    fn from(hw: HwAccel) -> Self {
        match hw {
            HwAccel::None => Self::Software,
            HwAccel::Vaapi => Self::Vaapi,
            HwAccel::Qsv => Self::Qsv,
            HwAccel::Nvenc => Self::Nvenc,
        }
    }
}

impl VideoEncoder {
    /// This encoder's bit in a "failed to start" set (none for libx264, which never is).
    fn bit(self) -> u8 {
        match self {
            Self::Software => 0,
            Self::Vaapi => 1,
            Self::Qsv => 2,
            Self::Nvenc => 4,
        }
    }

    fn is_broken(self, broken: &AtomicU8) -> bool {
        broken.load(Ordering::Relaxed) & self.bit() != 0
    }

    fn mark_broken(self, broken: &AtomicU8) {
        broken.fetch_or(self.bit(), Ordering::Relaxed);
    }
}

struct Session {
    id: String,
    /// The person who started it.
    user_id: String,
    item_id: String,
    source: PathBuf,
    dir: PathBuf,
    media: MediaSummary,
    options: StreamOptions,
    duration: f64,
    segments: u32,
    crf: u32,
    max_height: u32,
    encoder: VideoEncoder,
    realtime_input: bool,
    hw_start_check: Duration,
    hw_broken: Arc<AtomicU8>,
    last_access: Mutex<Instant>,
    stopped: AtomicBool,
    encoder_state: tokio::sync::Mutex<Option<Encoder>>,
}

/// A running (or finished) ffmpeg process for a session.
struct Encoder {
    child: Child,
    /// The video encoder this run uses.
    kind: VideoEncoder,
    /// First segment this run writes.
    start: u32,
    /// Highest segment written contiguously from `start` by this run.
    newest: Option<u32>,
    exit: Option<ExitStatus>,
    /// When this run started, rounded down to the second. Segment files older than this
    /// were left by an earlier run and say nothing about where this one is.
    launched: SystemTime,
}

enum Plan {
    Wait,
    Restart,
    PastEnd,
}

impl Encoder {
    /// Update the exit status, then the newest finished segment.
    async fn refresh(&mut self, dir: &Path) {
        if self.exit.is_none()
            && let Ok(Some(status)) = self.child.try_wait()
        {
            self.exit = Some(status);
        }
        let mut next = self.newest.map_or(self.start, |n| n + 1);
        while written_since(&segment_path(dir, next), self.launched).await {
            self.newest = Some(next);
            next += 1;
        }
    }

    /// A hardware run that died before writing anything: its encoder does not work here.
    /// Remember that, so the restart (and later sessions) use libx264 instead.
    fn note_hardware_failure(&self, broken: &AtomicU8) {
        let failed = matches!(self.exit, Some(status) if !status.success());
        if failed
            && self.newest.is_none()
            && self.kind != VideoEncoder::Software
            && !self.kind.is_broken(broken)
        {
            tracing::warn!(encoder = ?self.kind, "hardware encoding stopped before its first segment; using libx264");
            self.kind.mark_broken(broken);
        }
    }

    fn plan(&self, index: u32) -> Plan {
        let window_end = self.newest.map_or(i64::from(self.start) - 1, i64::from)
            + i64::from(LOOKAHEAD_SEGMENTS);
        let in_window = index >= self.start && i64::from(index) <= window_end;
        match self.exit {
            None if in_window => Plan::Wait,
            // ffmpeg finished cleanly from before this segment without writing it: past the real end.
            Some(status) if status.success() && index >= self.start => Plan::PastEnd,
            _ => Plan::Restart,
        }
    }
}

impl Session {
    fn touch(&self) {
        *self.last_access.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
    }

    fn last_access(&self) -> Instant {
        *self.last_access.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn idle_for(&self) -> Duration {
        self.last_access().elapsed()
    }

    async fn segment(&self, index: u32) -> Result<PathBuf> {
        let path = segment_path(&self.dir, index);
        if is_file(&path).await {
            return Ok(path);
        }
        let wait = {
            let mut state = self.encoder_state.lock().await;
            if self.stopped.load(Ordering::Relaxed) {
                return Err(CoreError::NotFound);
            }
            if is_file(&path).await {
                return Ok(path);
            }
            let plan = match state.as_mut() {
                Some(encoder) => {
                    encoder.refresh(&self.dir).await;
                    encoder.note_hardware_failure(&self.hw_broken);
                    encoder.plan(index)
                }
                None => Plan::Restart,
            };
            match plan {
                Plan::Wait => SEGMENT_WAIT,
                Plan::PastEnd => return Err(CoreError::NotFound),
                Plan::Restart => {
                    if is_file(&path).await {
                        return Ok(path);
                    }
                    tracing::debug!(session = %self.id, index, "restarting ffmpeg at the requested segment");
                    stop_encoder(state.take()).await;
                    *state = Some(self.launch(index).await?);
                    RESTART_WAIT
                }
            }
        };
        self.wait_for(index, &path, wait).await
    }

    /// Wait for a segment to appear; restart once if ffmpeg dies before writing it.
    async fn wait_for(&self, index: u32, path: &Path, limit: Duration) -> Result<PathBuf> {
        let mut deadline = Instant::now() + limit;
        let mut restarted = false;
        loop {
            if is_file(path).await {
                return Ok(path.to_path_buf());
            }
            if self.stopped.load(Ordering::Relaxed) {
                return Err(CoreError::NotFound);
            }
            if Instant::now() >= deadline {
                return Err(CoreError::external(
                    "This part of the video is still being prepared. Try again in a moment.",
                ));
            }
            tokio::time::sleep(POLL).await;
            // A player waiting on a slow segment is still using the session: never let
            // the wait make it look idle enough to be evicted.
            self.touch();

            let mut state = self.encoder_state.lock().await;
            if self.stopped.load(Ordering::Relaxed) {
                return Err(CoreError::NotFound);
            }
            let (running, plan) = match state.as_mut() {
                Some(encoder) => {
                    encoder.refresh(&self.dir).await;
                    encoder.note_hardware_failure(&self.hw_broken);
                    (encoder.exit.is_none(), encoder.plan(index))
                }
                None => (false, Plan::Restart),
            };
            if is_file(path).await {
                return Ok(path.to_path_buf());
            }
            match plan {
                Plan::Wait => {}
                Plan::PastEnd => return Err(CoreError::NotFound),
                // Another request restarted ffmpeg elsewhere (the player seeked away).
                Plan::Restart if running => {
                    return Err(CoreError::conflict(
                        "The player moved to another part of the video.",
                    ));
                }
                Plan::Restart if restarted => {
                    return Err(CoreError::external(
                        "This video could not be converted for streaming.",
                    ));
                }
                Plan::Restart => {
                    tracing::warn!(session = %self.id, index, "ffmpeg stopped early; restarting at the requested segment");
                    stop_encoder(state.take()).await;
                    *state = Some(self.launch(index).await?);
                    restarted = true;
                    deadline = Instant::now() + RESTART_WAIT;
                }
            }
        }
    }

    /// Start ffmpeg at `index`, falling back to libx264 when a hardware encoder fails to start.
    async fn launch(&self, index: u32) -> Result<Encoder> {
        let encoder = if self.encoder.is_broken(&self.hw_broken) {
            VideoEncoder::Software
        } else {
            self.encoder
        };
        if encoder != VideoEncoder::Software && self.media.video.is_some() {
            match self.spawn(index, encoder) {
                Ok(mut running) => {
                    if self.started_ok(&mut running).await {
                        return Ok(running);
                    }
                    tracing::warn!(session = %self.id, encoder = ?encoder, "hardware encoding failed to start; using libx264");
                }
                Err(err) => {
                    tracing::warn!(session = %self.id, encoder = ?encoder, error = %err, "hardware encoding failed to start; using libx264");
                }
            }
            encoder.mark_broken(&self.hw_broken);
        }
        self.spawn(index, VideoEncoder::Software)
    }

    /// True unless the process exits with an error before writing its first segment.
    async fn started_ok(&self, encoder: &mut Encoder) -> bool {
        let deadline = Instant::now() + self.hw_start_check;
        while Instant::now() < deadline {
            encoder.refresh(&self.dir).await;
            if encoder.newest.is_some() {
                return true;
            }
            if let Some(status) = encoder.exit {
                return status.success();
            }
            tokio::time::sleep(POLL).await;
        }
        true
    }

    fn spawn(&self, index: u32, encoder: VideoEncoder) -> Result<Encoder> {
        let args = ffmpeg_args(&EncodeJob {
            source: &self.source,
            dir: &self.dir,
            media: &self.media,
            options: &self.options,
            crf: self.crf,
            max_height: self.max_height,
            start_index: index,
            encoder,
            realtime_input: self.realtime_input,
        });
        let launched = whole_second(SystemTime::now());
        let mut cmd = Command::new(tools::FFMPEG);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| tools::spawn_error(tools::FFMPEG, e))?;
        if let Some(stderr) = child.stderr.take() {
            let session = self.id.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                let mut logged = 0;
                while let Ok(Some(line)) = lines.next_line().await {
                    if logged < 20 && !line.trim().is_empty() {
                        tracing::warn!(session = %session, "ffmpeg: {}", line.trim());
                        logged += 1;
                    }
                }
            });
        }
        Ok(Encoder {
            child,
            kind: encoder,
            start: index,
            newest: None,
            exit: None,
            launched,
        })
    }

    /// Kill ffmpeg and delete the folder.
    async fn shutdown(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        let encoder = self.encoder_state.lock().await.take();
        stop_encoder(encoder).await;
        if let Err(err) = tokio::fs::remove_dir_all(&self.dir).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(session = %self.id, error = %err, "could not delete transcode folder");
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // ffmpeg is killed by kill_on_drop; make sure no folder outlives its session.
        if self.dir.exists() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

async fn stop_encoder(encoder: Option<Encoder>) {
    if let Some(mut encoder) = encoder
        && encoder.exit.is_none()
    {
        let _ = encoder.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), encoder.child.wait()).await;
    }
}

async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok_and(|m| m.is_file())
}

/// A finished segment last written at or after `since`. Files from an earlier run keep
/// their old modification time until this run renames its own copy over them.
async fn written_since(path: &Path, since: SystemTime) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.is_file() => meta.modified().map_or(true, |t| t >= since),
        _ => false,
    }
}

/// `t` rounded down to the whole second, so file systems with coarse timestamps still
/// see a new segment as written after its run started.
fn whole_second(t: SystemTime) -> SystemTime {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => SystemTime::UNIX_EPOCH + Duration::from_secs(d.as_secs()),
        Err(_) => t,
    }
}

fn segment_path(dir: &Path, index: u32) -> PathBuf {
    dir.join(format!("{index}.ts"))
}

/// `12.ts` → 12. Anything else (paths, other extensions, signs, huge numbers) → None.
pub fn parse_segment_name(name: &str) -> Option<u32> {
    let digits = name.strip_suffix(".ts")?;
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Number of 6 s segments for a duration; a trailing sliver is folded into the last segment.
pub fn segment_count(duration_s: f64) -> u32 {
    let n = ((duration_s - MIN_LAST_SEGMENT) / SEGMENT_SECONDS).ceil();
    if n.is_finite() && n >= 1.0 {
        n.min(f64::from(u32::MAX)) as u32
    } else {
        1
    }
}

/// The complete VOD playlist; segment URLs are relative (`<n>.ts`).
pub fn build_playlist(duration_s: f64, segments: u32) -> String {
    let mut out = String::from(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MEDIA-SEQUENCE:0\n",
    );
    for n in 0..segments {
        let length = if n + 1 == segments {
            (duration_s - f64::from(n) * SEGMENT_SECONDS).max(0.001)
        } else {
            SEGMENT_SECONDS
        };
        out.push_str(&format!("#EXTINF:{length:.6},\n{n}.ts\n"));
    }
    out.push_str("#EXT-X-ENDLIST\n");
    out
}

/// Everything needed to build one ffmpeg command line.
pub struct EncodeJob<'a> {
    pub source: &'a Path,
    pub dir: &'a Path,
    pub media: &'a MediaSummary,
    pub options: &'a StreamOptions,
    pub crf: u32,
    /// Server-wide limit from settings.
    pub max_height: u32,
    pub start_index: u32,
    pub encoder: VideoEncoder,
    pub realtime_input: bool,
}

/// The short edge (the "1080" in 1080p) to encode at, None to keep the source size.
fn target_short_edge(job: &EncodeJob<'_>) -> Option<i64> {
    let video = job.media.video.as_ref()?;
    let short = video.width.min(video.height);
    let limit = i64::from(
        job.options
            .max_height
            .map_or(job.max_height, |m| m.min(job.max_height))
            .max(144),
    );
    (short > limit).then_some(limit / 2 * 2)
}

/// The video filter chain: scale, optional tone mapping, pixel format (and upload for VAAPI).
pub fn video_filter(job: &EncodeJob<'_>) -> String {
    let mut chain = Vec::new();
    let landscape = job.media.video.as_ref().is_none_or(|v| {
        let (w, h) = if v.rotation == 90 || v.rotation == 270 {
            (v.height, v.width)
        } else {
            (v.width, v.height)
        };
        w >= h
    });
    match target_short_edge(job) {
        Some(edge) if landscape => chain.push(format!("scale=-2:{edge}")),
        Some(edge) => chain.push(format!("scale={edge}:-2")),
        None => chain.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string()),
    }
    let hdr = job.media.video.as_ref().and_then(|v| v.hdr.as_deref());
    let tonemapped = match hdr.filter(|_| !job.options.hdr) {
        Some(kind) => {
            // The tone-mapping chain already ends in yuv420p.
            chain.push(filters::tonemap_chain(kind));
            true
        }
        None => false,
    };
    match job.encoder {
        VideoEncoder::Software | VideoEncoder::Nvenc if tonemapped => {}
        VideoEncoder::Software | VideoEncoder::Nvenc => chain.push("format=yuv420p".to_string()),
        VideoEncoder::Qsv => chain.push("format=nv12".to_string()),
        VideoEncoder::Vaapi => chain.push("format=nv12,hwupload".to_string()),
    }
    chain.join(",")
}

/// The full ffmpeg argument list for a session starting at `job.start_index`.
pub fn ffmpeg_args(job: &EncodeJob<'_>) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    push(&["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
    if job.encoder == VideoEncoder::Vaapi {
        push(&["-vaapi_device", tools::VAAPI_DEVICE]);
    }
    if job.realtime_input {
        push(&["-re"]);
    }
    let offset = f64::from(job.start_index) * SEGMENT_SECONDS;
    if job.start_index > 0 {
        push(&["-ss", &format!("{offset:.3}")]);
    }
    args.push("-i".into());
    args.push(tools::ffmpeg_input(job.source));

    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    match &job.media.video {
        Some(video) => {
            push(&["-map", &format!("0:{}", video.index)]);
        }
        None => push(&["-vn"]),
    }
    match playback::selected_audio(job.media, job.options.audio_index) {
        Some(audio) => push(&["-map", &format!("0:{}", audio.index)]),
        None => push(&["-an"]),
    }
    if job.media.video.is_some() {
        let crf = job.crf.to_string();
        push(&["-vf", &video_filter(job)]);
        match job.encoder {
            VideoEncoder::Software => {
                push(&[
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-crf",
                    &crf,
                    "-profile:v",
                    "high",
                ]);
            }
            // VAAPI turns forced keyframes into IDR frames by itself. QSV and NVENC make
            // plain I-frames unless told otherwise, and the HLS muxer only cuts at IDR
            // frames, so their segments would run past the 6 s the playlist promises.
            VideoEncoder::Vaapi => push(&["-c:v", "h264_vaapi", "-rc_mode", "CQP", "-qp", &crf]),
            VideoEncoder::Qsv => push(&[
                "-c:v",
                "h264_qsv",
                "-preset",
                "veryfast",
                "-global_quality",
                &crf,
                "-forced_idr",
                "1",
            ]),
            VideoEncoder::Nvenc => push(&[
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-rc",
                "vbr",
                "-cq",
                &crf,
                "-b:v",
                "0",
                "-forced-idr",
                "1",
            ]),
        }
        push(&["-force_key_frames", "expr:gte(t,n_forced*6)"]);
    }
    if playback::selected_audio(job.media, job.options.audio_index).is_some() {
        push(&["-c:a", "aac", "-ac", "2", "-b:a", "160k"]);
    }
    push(&[
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-map_chapters",
        "-1",
        "-max_muxing_queue_size",
        "4096",
    ]);
    push(&["-muxdelay", "0", "-muxpreload", "0"]);
    push(&[
        "-f",
        "hls",
        "-hls_time",
        "6",
        "-hls_list_size",
        "0",
        "-hls_segment_type",
        "mpegts",
    ]);
    push(&["-hls_flags", "temp_file+independent_segments"]);
    push(&[
        "-start_number",
        &job.start_index.to_string(),
        "-output_ts_offset",
        &format!("{:.3}", TIMESTAMP_BASE_S + offset),
    ]);

    // `%` in the folder path would be read as a pattern by the segment muxer.
    let dir = job.dir.to_string_lossy().replace('%', "%%");
    args.push("-hls_segment_filename".into());
    args.push(format!("{dir}/%d.ts").into());
    args.push(job.dir.join("ffmpeg.m3u8").into_os_string());
    args
}

async fn clear_leftovers(dir: PathBuf) {
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return;
    };
    let mut removed = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let result = match entry.file_type().await {
            Ok(t) if t.is_dir() => tokio::fs::remove_dir_all(&path).await,
            _ => tokio::fs::remove_file(&path).await,
        };
        match result {
            Ok(()) => removed += 1,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "could not delete old transcode files")
            }
        }
    }
    if removed > 0 {
        tracing::info!(removed, "deleted transcode folders left from the last run");
    }
}

fn clear_leftovers_blocking(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(err) = result {
            tracing::warn!(path = %path.display(), error = %err, "could not delete old transcode files");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command as StdCommand;

    use super::*;
    use crate::media::playback::{ClientCaps, PlaybackMode, decide};
    use crate::media::probe;
    use crate::models::{AudioStream, VideoStream};

    struct Env {
        _root: tempfile::TempDir,
        config_dir: tempfile::TempDir,
        config: Arc<Config>,
        settings: SettingsStore,
    }

    async fn env() -> Env {
        let root = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let config = Arc::new(Config::for_tests(root.path(), config_dir.path()));
        let db = crate::db::open_memory().await.unwrap();
        let settings = SettingsStore::load(db, HwAccel::None).await.unwrap();
        Env {
            _root: root,
            config_dir,
            config,
            settings,
        }
    }

    /// 20 s 640x360 test pattern with a sine tone, H.264 + AAC in `ext`'s container.
    fn make_clip(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let status = StdCommand::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=640x360:r=25:d=20",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:d=20",
            ])
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    /// (codec_type, codec_name, start_time) per stream.
    fn probe_segment(path: &Path) -> Vec<(String, String, f64)> {
        let out = StdCommand::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,codec_name,start_time",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let p: Vec<&str> = l.split(',').collect();
                (p[1].to_string(), p[0].to_string(), p[2].parse().unwrap())
            })
            .collect()
    }

    /// `expected_start` is the position in the video; segments carry it plus the base.
    fn assert_segment(path: &Path, expected_start: f64) {
        let expected_start = TIMESTAMP_BASE_S + expected_start;
        let streams = probe_segment(path);
        let video = streams
            .iter()
            .find(|s| s.0 == "video")
            .expect("video stream");
        let audio = streams
            .iter()
            .find(|s| s.0 == "audio")
            .expect("audio stream");
        assert_eq!(video.1, "h264");
        assert_eq!(audio.1, "aac");
        for s in [video, audio] {
            assert!(
                (s.2 - expected_start).abs() < 0.5,
                "{} starts at {} not {expected_start}",
                s.0,
                s.2
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_session_lifecycle() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mkv");
        let media = probe::probe(&clip).await.unwrap();
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone());

        let info = manager
            .start("item-1", clip.clone(), &media, StreamOptions::default())
            .await
            .unwrap();
        assert_eq!(
            info.url,
            format!("/api/stream/{}/index.m3u8", info.session_id)
        );
        assert_eq!(info.session_id.len(), 32);
        assert_eq!(
            manager.session_item_id(&info.session_id).as_deref(),
            Some("item-1")
        );

        let playlist = manager.playlist(&info.session_id).await.unwrap();
        assert!(playlist.starts_with(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-PLAYLIST-TYPE:VOD\n"
        ));
        assert_eq!(playlist.matches("#EXTINF:").count(), 4);
        assert!(playlist.contains("#EXTINF:6.000000,\n0.ts\n"));
        assert!(playlist.contains("3.ts\n#EXT-X-ENDLIST\n"));
        assert!(playlist.contains("#EXTINF:2."));

        let first = manager.segment(&info.session_id, "0.ts").await.unwrap();
        assert_segment(&first, 0.0);
        let last = manager.segment(&info.session_id, "3.ts").await.unwrap();
        assert_segment(&last, 18.0);
        for n in 1..3 {
            let seg = manager
                .segment(&info.session_id, &format!("{n}.ts"))
                .await
                .unwrap();
            assert_segment(&seg, f64::from(n) * 6.0);
        }

        // Bad names and out-of-range segments.
        for bad in [
            "4.ts",
            "../0.ts",
            "0.mp4",
            "-1.ts",
            "a.ts",
            ".ts",
            "0.ts/..",
            "1234567.ts",
        ] {
            assert!(
                matches!(
                    manager.segment(&info.session_id, bad).await,
                    Err(CoreError::NotFound)
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            manager.segment("nope", "0.ts").await,
            Err(CoreError::NotFound)
        ));
        assert!(matches!(
            manager.playlist("nope").await,
            Err(CoreError::NotFound)
        ));

        let dir = env.config.transcode_dir().join(&info.session_id);
        assert!(dir.exists());
        manager.stop(&info.session_id).await;
        assert!(!dir.exists(), "session folder is deleted");
        assert!(matches!(
            manager.segment(&info.session_id, "0.ts").await,
            Err(CoreError::NotFound)
        ));
        assert_eq!(manager.session_count(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn seeks_restart_ffmpeg() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        // Realtime input keeps ffmpeg behind the player, so jumps force restarts.
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone()).realtime();
        let info = manager
            .start("item", clip, &media, StreamOptions::default())
            .await
            .unwrap();
        assert_eq!(manager.encoder_start(&info.session_id).await, Some(0));

        // Far ahead of anything written: restart at 3.
        let seg3 = manager.segment(&info.session_id, "3.ts").await.unwrap();
        assert_eq!(manager.encoder_start(&info.session_id).await, Some(3));
        assert_segment(&seg3, 18.0);

        // Behind the running start: restart at 0.
        let seg0 = manager.segment(&info.session_id, "0.ts").await.unwrap();
        assert_eq!(manager.encoder_start(&info.session_id).await, Some(0));
        assert_segment(&seg0, 0.0);

        // Just ahead of the newest segment: wait, no restart.
        let seg1 = manager.segment(&info.session_id, "1.ts").await.unwrap();
        assert_eq!(manager.encoder_start(&info.session_id).await, Some(0));
        assert_segment(&seg1, 6.0);
        manager.stop(&info.session_id).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn start_offset_and_options() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone());
        let options = StreamOptions {
            start_s: Some(13.5),
            max_height: Some(240),
            audio_index: Some(1),
            hdr: false,
        };
        let info = manager
            .start("item", clip.clone(), &media, options)
            .await
            .unwrap();
        assert_eq!(manager.encoder_start(&info.session_id).await, Some(2));
        let seg = manager.segment(&info.session_id, "2.ts").await.unwrap();
        assert_segment(&seg, 12.0);
        let out = StdCommand::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&seg)
            .output()
            .unwrap();
        // MPEG-TS lists the stream again under its program; the first line is enough.
        let size = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        assert_eq!(size, "426,240");
        manager.stop(&info.session_id).await;

        let bad_track = StreamOptions {
            audio_index: Some(7),
            ..StreamOptions::default()
        };
        let err = manager
            .start("item", clip.clone(), &media, bad_track)
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "That audio track does not exist in this file."
        );
        let mut unknown = media.clone();
        unknown.duration_s = None;
        assert!(
            manager
                .start("item", clip, &unknown, StreamOptions::default())
                .await
                .is_err()
        );
        assert_eq!(manager.session_count(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_limit_and_eviction() {
        let env = env().await;
        env.settings
            .update(serde_json::json!({"transcode": {"max_sessions": 1}}))
            .await
            .unwrap();
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone());

        let a = manager
            .start("a", clip.clone(), &media, StreamOptions::default())
            .await
            .unwrap();
        let err = manager
            .start("b", clip.clone(), &media, StreamOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(&err, CoreError::Conflict(_)));
        assert_eq!(
            err.to_string(),
            "Too many videos are transcoding right now. Try again in a minute."
        );

        // Once A goes quiet, B takes its place.
        manager.make_idle(&a.session_id, ACTIVE_WINDOW + Duration::from_secs(1));
        let b = manager
            .start("b", clip.clone(), &media, StreamOptions::default())
            .await
            .unwrap();
        assert_eq!(manager.session_count(), 1);
        assert!(matches!(
            manager.playlist(&a.session_id).await,
            Err(CoreError::NotFound)
        ));
        assert!(!env.config.transcode_dir().join(&a.session_id).exists());
        assert!(manager.playlist(&b.session_id).await.is_ok());

        // Reaper: B idle past the timeout is stopped and deleted.
        assert_eq!(manager.reap_idle(IDLE_TIMEOUT).await, 0);
        manager.make_idle(&b.session_id, IDLE_TIMEOUT + Duration::from_secs(1));
        assert_eq!(manager.reap_idle(IDLE_TIMEOUT).await, 1);
        assert!(!env.config.transcode_dir().join(&b.session_id).exists());
        assert_eq!(manager.session_count(), 0);
    }

    #[tokio::test]
    async fn reaper_stops_everything_on_cancel() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        let manager = Arc::new(TranscodeManager::new(
            env.config.clone(),
            env.settings.clone(),
        ));
        let info = manager
            .start("a", clip, &media, StreamOptions::default())
            .await
            .unwrap();
        let cancel = CancellationToken::new();
        let reaper = manager.spawn_reaper(cancel.clone());
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(10), reaper)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(manager.session_count(), 0);
        assert!(!env.config.transcode_dir().join(&info.session_id).exists());
    }

    #[tokio::test]
    async fn leftovers_are_cleared_on_startup() {
        let env = env().await;
        let old = env.config.transcode_dir().join("0123abcd");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("5.ts"), b"x").unwrap();
        std::fs::write(env.config.transcode_dir().join("stray.tmp"), b"x").unwrap();
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone());
        manager.await_cleanup().await;
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_dir(env.config.transcode_dir())
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn new_works_without_a_runtime() {
        let root = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let config = Arc::new(Config::for_tests(root.path(), config_dir.path()));
        let old = config.transcode_dir().join("old");
        std::fs::create_dir_all(&old).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let settings = rt
            .block_on(async {
                let db = crate::db::open_memory().await.unwrap();
                SettingsStore::load(db, HwAccel::None).await
            })
            .unwrap();
        let manager = TranscodeManager::new(config, settings.clone());
        assert!(!old.exists());
        assert_eq!(manager.session_count(), 0);
        // No transcode dir at all is fine too.
        let config = Arc::new(Config::for_tests(
            root.path(),
            &config_dir.path().join("missing"),
        ));
        let other = TranscodeManager::new(config, settings.clone());
        // The database pool belongs to the runtime; drop everything inside it.
        rt.block_on(async move {
            drop(manager);
            drop(other);
            drop(settings);
        });
    }

    #[tokio::test]
    async fn direct_play_decisions_on_generated_clips() {
        let env = env().await;
        let mp4 = make_clip(env.config_dir.path(), "clip.mp4");
        let mkv = make_clip(env.config_dir.path(), "clip.mkv");
        let caps = ClientCaps {
            video_codecs: vec!["h264".into()],
            audio_codecs: vec!["aac".into()],
            ..ClientCaps::default()
        };
        let m = probe::probe(&mp4).await.unwrap();
        assert_eq!(decide(&m, "mp4", &caps), PlaybackMode::Direct);
        assert_eq!(
            decide(
                &m,
                "mp4",
                &ClientCaps {
                    max_height: Some(240),
                    ..caps.clone()
                }
            ),
            PlaybackMode::Hls {
                reason: "Lowered to 240p for this device".into()
            }
        );
        let k = probe::probe(&mkv).await.unwrap();
        assert_eq!(
            decide(&k, "mkv", &caps),
            PlaybackMode::Hls {
                reason: "MKV files cannot play directly in a browser".into()
            }
        );
        // An MKV renamed to .mp4 is caught by the probe.
        assert!(matches!(decide(&k, "mp4", &caps), PlaybackMode::Hls { .. }));
    }

    #[test]
    fn segment_names() {
        assert_eq!(parse_segment_name("0.ts"), Some(0));
        assert_eq!(parse_segment_name("123.ts"), Some(123));
        assert_eq!(parse_segment_name("999999.ts"), Some(999_999));
        for bad in [
            "",
            ".ts",
            "1.TS",
            "1.ts.tmp",
            "+1.ts",
            "-1.ts",
            "1 .ts",
            "../1.ts",
            "1/2.ts",
            "1234567.ts",
            "x.ts",
        ] {
            assert_eq!(parse_segment_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn segment_counts() {
        assert_eq!(segment_count(20.0), 4);
        assert_eq!(segment_count(18.0), 3);
        assert_eq!(segment_count(18.05), 3);
        assert_eq!(segment_count(18.2), 4);
        assert_eq!(segment_count(6.0), 1);
        assert_eq!(segment_count(0.05), 1);
        assert_eq!(segment_count(7080.042), 1180);
    }

    #[test]
    fn playlist_shape() {
        let p = build_playlist(20.0, segment_count(20.0));
        let expected = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MEDIA-SEQUENCE:0\n\
            #EXTINF:6.000000,\n0.ts\n#EXTINF:6.000000,\n1.ts\n#EXTINF:6.000000,\n2.ts\n#EXTINF:2.000000,\n3.ts\n#EXT-X-ENDLIST\n";
        assert_eq!(p, expected);
        let p = build_playlist(18.05, segment_count(18.05));
        assert!(p.contains("#EXTINF:6.050000,\n2.ts\n"));
    }

    fn job_media(width: i64, height: i64, rotation: i64, hdr: Option<&str>) -> MediaSummary {
        MediaSummary {
            duration_s: Some(60.0),
            video: Some(VideoStream {
                index: 0,
                codec: "hevc".into(),
                width,
                height,
                rotation,
                hdr: hdr.map(String::from),
                ..VideoStream::default()
            }),
            audio: vec![
                AudioStream {
                    index: 1,
                    codec: "eac3".into(),
                    ..AudioStream::default()
                },
                AudioStream {
                    index: 2,
                    codec: "aac".into(),
                    ..AudioStream::default()
                },
            ],
            ..MediaSummary::default()
        }
    }

    fn args_of(job: &EncodeJob<'_>) -> Vec<String> {
        ffmpeg_args(job)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn window(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    }

    #[test]
    fn ffmpeg_command_lines() {
        let media = job_media(3840, 2160, 0, Some("hdr10"));
        let options = StreamOptions {
            audio_index: Some(2),
            ..StreamOptions::default()
        };
        let job = EncodeJob {
            source: Path::new("/stash/Movies/A (2020)/A (2020).mkv"),
            dir: Path::new("/config/cache/transcode/abc"),
            media: &media,
            options: &options,
            crf: 23,
            max_height: 1080,
            start_index: 5,
            encoder: VideoEncoder::Software,
            realtime_input: false,
        };
        let args = args_of(&job);
        assert_eq!(window(&args, "-ss").as_deref(), Some("30.000"));
        let ss = args.iter().position(|a| a == "-ss").unwrap();
        let input = args.iter().position(|a| a == "-i").unwrap();
        assert!(ss < input, "-ss goes before -i for fast seeking");
        assert_eq!(args[input + 1], "file:/stash/Movies/A (2020)/A (2020).mkv");
        let maps: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > 0 && args[i - 1] == "-map")
            .map(|(_, a)| a)
            .collect();
        assert_eq!(maps, vec!["0:0", "0:2"]);
        assert_eq!(window(&args, "-c:v").as_deref(), Some("libx264"));
        assert_eq!(window(&args, "-preset").as_deref(), Some("veryfast"));
        assert_eq!(window(&args, "-crf").as_deref(), Some("23"));
        let vf = window(&args, "-vf").unwrap();
        assert!(vf.starts_with("scale=-2:1080,zscale="), "{vf}");
        assert!(vf.contains("tonemap=tonemap=hable"));
        assert!(vf.ends_with("format=yuv420p"));
        assert_eq!(
            window(&args, "-force_key_frames").as_deref(),
            Some("expr:gte(t,n_forced*6)")
        );
        assert_eq!(window(&args, "-c:a").as_deref(), Some("aac"));
        assert_eq!(window(&args, "-ac").as_deref(), Some("2"));
        assert_eq!(window(&args, "-b:a").as_deref(), Some("160k"));
        assert_eq!(window(&args, "-hls_time").as_deref(), Some("6"));
        assert_eq!(
            window(&args, "-hls_flags").as_deref(),
            Some("temp_file+independent_segments")
        );
        assert_eq!(window(&args, "-start_number").as_deref(), Some("5"));
        assert_eq!(
            window(&args, "-output_ts_offset").as_deref(),
            Some("40.000")
        );
        assert_eq!(
            window(&args, "-hls_segment_filename").as_deref(),
            Some("/config/cache/transcode/abc/%d.ts")
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("/config/cache/transcode/abc/ffmpeg.m3u8")
        );

        // HDR client: no tone mapping. First segment: no -ss.
        let options = StreamOptions {
            hdr: true,
            ..StreamOptions::default()
        };
        let job = EncodeJob {
            options: &options,
            start_index: 0,
            ..job
        };
        let args = args_of(&job);
        assert!(!args.contains(&"-ss".to_string()));
        assert!(!window(&args, "-vf").unwrap().contains("tonemap"));
        assert_eq!(
            window(&args, "-output_ts_offset").as_deref(),
            Some("10.000")
        );
        let maps: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > 0 && args[i - 1] == "-map")
            .map(|(_, a)| a)
            .collect();
        assert_eq!(maps, vec!["0:0", "0:1"], "default audio track");
    }

    #[test]
    fn scaling_rules() {
        let options = StreamOptions::default();
        let base = |media: &MediaSummary, max_height: u32, options: &StreamOptions| -> String {
            let job = EncodeJob {
                source: Path::new("/a.mkv"),
                dir: Path::new("/t"),
                media,
                options,
                crf: 23,
                max_height,
                start_index: 0,
                encoder: VideoEncoder::Software,
                realtime_input: false,
            };
            video_filter(&job)
        };
        // Smaller than the limit: keep size, force even dimensions.
        assert_eq!(
            base(&job_media(640, 360, 0, None), 1080, &options),
            "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p"
        );
        // Portrait phone video limits its width (the short edge).
        assert_eq!(
            base(&job_media(3840, 2160, 90, None), 1080, &options),
            "scale=1080:-2,format=yuv420p"
        );
        // Client limit below the server limit wins.
        let small = StreamOptions {
            max_height: Some(720),
            ..StreamOptions::default()
        };
        assert_eq!(
            base(&job_media(1920, 1080, 0, None), 1080, &small),
            "scale=-2:720,format=yuv420p"
        );
        // Client asking for more than the server allows is capped.
        let big = StreamOptions {
            max_height: Some(2160),
            ..StreamOptions::default()
        };
        assert_eq!(
            base(&job_media(3840, 2160, 0, None), 1080, &big),
            "scale=-2:1080,format=yuv420p"
        );
    }

    #[test]
    fn hardware_variants_and_audio_only() {
        let media = job_media(1920, 1080, 0, None);
        let options = StreamOptions::default();
        let job = |encoder| EncodeJob {
            source: Path::new("/a.mkv"),
            dir: Path::new("/t/100%"),
            media: &media,
            options: &options,
            crf: 21,
            max_height: 1080,
            start_index: 0,
            encoder,
            realtime_input: false,
        };
        let vaapi = args_of(&job(VideoEncoder::Vaapi));
        assert_eq!(
            window(&vaapi, "-vaapi_device").as_deref(),
            Some(tools::VAAPI_DEVICE)
        );
        assert_eq!(window(&vaapi, "-c:v").as_deref(), Some("h264_vaapi"));
        assert!(
            window(&vaapi, "-vf")
                .unwrap()
                .ends_with("format=nv12,hwupload")
        );
        assert_eq!(window(&vaapi, "-qp").as_deref(), Some("21"));
        assert_eq!(
            window(&vaapi, "-hls_segment_filename").as_deref(),
            Some("/t/100%%/%d.ts")
        );
        let qsv = args_of(&job(VideoEncoder::Qsv));
        assert_eq!(window(&qsv, "-c:v").as_deref(), Some("h264_qsv"));
        assert_eq!(window(&qsv, "-global_quality").as_deref(), Some("21"));
        let nvenc = args_of(&job(VideoEncoder::Nvenc));
        assert_eq!(window(&nvenc, "-c:v").as_deref(), Some("h264_nvenc"));
        assert_eq!(window(&nvenc, "-cq").as_deref(), Some("21"));

        let audio_only = MediaSummary {
            duration_s: Some(30.0),
            audio: vec![AudioStream {
                index: 0,
                codec: "flac".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let args = args_of(&EncodeJob {
            media: &audio_only,
            ..job(VideoEncoder::Software)
        });
        assert!(args.contains(&"-vn".to_string()));
        assert!(!args.contains(&"-vf".to_string()));
        assert_eq!(window(&args, "-c:a").as_deref(), Some("aac"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn broken_hardware_falls_back_to_software() {
        let env = env().await;
        env.settings
            .update(serde_json::json!({"transcode": {"hwaccel": "nvenc"}}))
            .await
            .unwrap();
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        // Skip detection so the session really tries NVENC, which fails on this host.
        let manager =
            TranscodeManager::new(env.config.clone(), env.settings.clone()).trust_hwaccel();
        let info = manager
            .start("a", clip, &media, StreamOptions::default())
            .await
            .unwrap();
        let seg = manager.segment(&info.session_id, "0.ts").await.unwrap();
        assert_segment(&seg, 0.0);
        assert!(VideoEncoder::Nvenc.is_broken(&manager.hw_broken));
        manager.stop(&info.session_id).await;
        // NVENC failing says nothing about QSV: switching the setting must try it.
        assert_eq!(
            manager.pick_encoder(HwAccel::Nvenc).await,
            VideoEncoder::Software
        );
        assert_eq!(manager.pick_encoder(HwAccel::Qsv).await, VideoEncoder::Qsv);
    }

    /// Run one ffmpeg command line from `start_index` to the end, outside any session.
    fn run_encode(source: &Path, media: &MediaSummary, dir: &Path, start_index: u32) {
        std::fs::create_dir_all(dir).unwrap();
        let options = StreamOptions::default();
        let args = ffmpeg_args(&EncodeJob {
            source,
            dir,
            media,
            options: &options,
            crf: 23,
            max_height: 1080,
            start_index,
            encoder: VideoEncoder::Software,
            realtime_input: false,
        });
        let out = StdCommand::new("ffmpeg").args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn video_start(path: &Path) -> f64 {
        probe_segment(path)
            .iter()
            .find(|s| s.0 == "video")
            .expect("video stream")
            .2
    }

    #[tokio::test]
    async fn restarted_runs_share_timestamps_with_the_first_run() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mkv");
        let media = probe::probe(&clip).await.unwrap();
        let first = env.config_dir.path().join("first");
        let restarted = env.config_dir.path().join("restarted");
        run_encode(&clip, &media, &first, 0);
        run_encode(&clip, &media, &restarted, 2);
        // The same segment number must hold the same timestamps whichever run wrote it,
        // or the player sees a jump each time it crosses from one run's files to another's.
        for n in 2..4 {
            let a = video_start(&segment_path(&first, n));
            let b = video_start(&segment_path(&restarted, n));
            assert!(
                (a - b).abs() < 0.005,
                "segment {n}: first run starts at {a}, restarted run at {b}"
            );
            let expected = TIMESTAMP_BASE_S + f64::from(n) * SEGMENT_SECONDS;
            assert!((b - expected).abs() < 0.005, "segment {n} starts at {b}");
        }
    }

    #[test]
    fn hardware_encoders_get_idr_keyframes_and_known_options() {
        let media = job_media(1920, 1080, 0, None);
        let options = StreamOptions::default();
        let cases: [(VideoEncoder, &str, &[&str]); 3] = [
            (
                VideoEncoder::Nvenc,
                "h264_nvenc",
                &["-preset", "-rc", "-cq", "-forced-idr"],
            ),
            (VideoEncoder::Qsv, "h264_qsv", &["-preset", "-forced_idr"]),
            (VideoEncoder::Vaapi, "h264_vaapi", &["-rc_mode", "-qp"]),
        ];
        for (encoder, name, private) in cases {
            let args = args_of(&EncodeJob {
                source: Path::new("/a.mkv"),
                dir: Path::new("/t"),
                media: &media,
                options: &options,
                crf: 23,
                max_height: 1080,
                start_index: 0,
                encoder,
                realtime_input: false,
            });
            // The HLS muxer only cuts at IDR frames; forced plain I-frames would
            // leave segments longer than the playlist says.
            if let Some(flag) = private.iter().find(|o| o.contains("idr")) {
                assert_eq!(window(&args, flag).as_deref(), Some("1"), "{name}");
            }
            let help = StdCommand::new("ffmpeg")
                .args(["-hide_banner", "-h", &format!("encoder={name}")])
                .output()
                .unwrap();
            let help = String::from_utf8_lossy(&help.stdout);
            for option in private {
                assert!(
                    args.iter().any(|a| a == option),
                    "{name} is missing {option}"
                );
                // Only checkable when this ffmpeg build includes the encoder.
                if help.contains(&format!("Encoder {name} ")) {
                    assert!(
                        help.contains(&format!("  {option} ")),
                        "{name} has no {option} option"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn segments_left_by_an_earlier_run_are_not_progress() {
        let dir = tempfile::tempdir().unwrap();
        let launched = whole_second(std::time::SystemTime::now());
        let child = Command::new("sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        // This run started at 4 and wrote 4 and 5; 6 to 9 are left from an earlier run.
        for n in 4..10 {
            std::fs::write(segment_path(dir.path(), n), b"x").unwrap();
        }
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        for n in 6..10 {
            std::fs::File::options()
                .write(true)
                .open(segment_path(dir.path(), n))
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        let mut encoder = Encoder {
            child,
            kind: VideoEncoder::Software,
            start: 4,
            newest: None,
            exit: None,
            launched,
        };
        encoder.refresh(dir.path()).await;
        assert_eq!(encoder.newest, Some(5));
        assert!(matches!(encoder.plan(8), Plan::Wait));
        // ffmpeg is at 6, so 9 is a seek: restart there instead of waiting.
        assert!(matches!(encoder.plan(9), Plan::Restart));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_waiting_request_keeps_its_session_active() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        let manager =
            Arc::new(TranscodeManager::new(env.config.clone(), env.settings.clone()).realtime());
        let info = manager
            .start("a", clip, &media, StreamOptions::default())
            .await
            .unwrap();
        // Realtime input needs about 12 s for segment 1, so this request waits.
        let waiting = {
            let manager = manager.clone();
            let id = info.session_id.clone();
            tokio::spawn(async move { manager.segment(&id, "1.ts").await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        // Pretend the wait has already lasted longer than the eviction window.
        manager.make_idle(&info.session_id, ACTIVE_WINDOW + Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(500)).await;
        let idle = manager
            .lock_sessions()
            .get(&info.session_id)
            .map(|s| s.idle_for())
            .unwrap();
        assert!(
            idle < ACTIVE_WINDOW,
            "a session serving a waiting request looked idle for {idle:?}"
        );
        manager.stop(&info.session_id).await;
        assert!(matches!(waiting.await.unwrap(), Err(CoreError::NotFound)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hardware_failing_after_the_start_check_still_falls_back() {
        let env = env().await;
        env.settings
            .update(serde_json::json!({"transcode": {"hwaccel": "nvenc"}}))
            .await
            .unwrap();
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let media = probe::probe(&clip).await.unwrap();
        // No start check: NVENC is "started" and only then fails, the way it does when a
        // slow seek delays the first frame past the check (or the GPU's session limit is hit).
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone())
            .trust_hwaccel()
            .no_hw_start_check();
        let info = manager
            .start("a", clip, &media, StreamOptions::default())
            .await
            .unwrap();
        let seg = manager.segment(&info.session_id, "0.ts").await.unwrap();
        assert_segment(&seg, 0.0);
        assert!(VideoEncoder::Nvenc.is_broken(&manager.hw_broken));
        manager.stop(&info.session_id).await;
    }

    #[tokio::test]
    async fn absurd_durations_are_refused() {
        let env = env().await;
        let clip = make_clip(env.config_dir.path(), "clip.mp4");
        let mut media = probe::probe(&clip).await.unwrap();
        // A damaged header can claim a length of years; the playlist would not fit in memory.
        media.duration_s = Some(1e12);
        let manager = TranscodeManager::new(env.config.clone(), env.settings.clone());
        let err = manager
            .start("a", clip, &media, StreamOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Invalid(_)), "{err}");
        assert_eq!(manager.session_count(), 0);
    }
}
