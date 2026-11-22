//! Offline media download manager backed by yt-dlp with progress parsing and cancellation.

#[path = "mp4_mux.rs"]
mod mp4_mux;

use crate::process::{Cancellation, ProcessJobObject};
use crate::search::{self, Video};
use libloading::Library;
use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_DOWNLOAD_HISTORY: usize = 50;
const MAX_DOWNLOAD_RECORDS_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DOWNLOAD_RECORD_TEXT_BYTES: usize = 4096;
const MAX_STDERR_LINES: usize = 64;
const PROGRESS_REPAINT_MIN_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_REPAINT_MIN_PERCENT_DELTA: f32 = 1.0;
const CANCEL_WATCHER_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub const DEFAULT_MAX_CONCURRENT_DOWNLOADS: usize = 2;

pub fn can_start_another_download(active_count: usize, max_concurrent: usize) -> bool {
    active_count < max_concurrent.max(1)
}

static STAGING_SEQ: AtomicU64 = AtomicU64::new(1);
static FFMPEG_PATH_CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
static FFMPEG_PROBE_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFormat {
    #[default]
    VideoMp4Best,
    Video1080p,
    Video720p,
    Video480p,
    AudioM4a,
    AudioOpus,
    AudioMp3,
}

impl DownloadFormat {
    pub const ALL: [Self; 7] = [
        Self::VideoMp4Best,
        Self::Video1080p,
        Self::Video720p,
        Self::Video480p,
        Self::AudioM4a,
        Self::AudioOpus,
        Self::AudioMp3,
    ];

    pub fn is_audio(self) -> bool {
        matches!(self, Self::AudioM4a | Self::AudioOpus | Self::AudioMp3)
    }

    pub fn filename_tag(self) -> &'static str {
        match self {
            Self::VideoMp4Best => "best",
            Self::Video1080p => "1080p",
            Self::Video720p => "720p",
            Self::Video480p => "480p",
            Self::AudioM4a => "audio-m4a",
            Self::AudioOpus => "audio-opus",
            Self::AudioMp3 => "audio-mp3",
        }
    }

    pub fn output_template(self) -> String {
        format!("%(title).120B [%(id)s] [{}].%(ext)s", self.filename_tag())
    }

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::VideoMp4Best => "Vídeo MP4 (Melhor)",
            Self::Video1080p => "Vídeo MP4 (1080p)",
            Self::Video720p => "Vídeo MP4 (720p)",
            Self::Video480p => "Vídeo MP4 (480p)",
            Self::AudioM4a => "Áudio M4A",
            Self::AudioOpus => "Áudio Opus",
            Self::AudioMp3 => "Áudio MP3",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::VideoMp4Best => "Video MP4 (Best)",
            Self::Video1080p => "Video MP4 (1080p)",
            Self::Video720p => "Video MP4 (720p)",
            Self::Video480p => "Video MP4 (480p)",
            Self::AudioM4a => "Audio M4A",
            Self::AudioOpus => "Audio Opus",
            Self::AudioMp3 => "Audio MP3",
        }
    }

    #[cfg(test)]
    pub fn yt_dlp_args(self) -> Vec<&'static str> {
        self.yt_dlp_args_with_ffmpeg(true)
    }

    /// Selects format arguments for `yt-dlp`. When `ffmpeg` is absent, selects separate
    /// ISO BMFF DASH video+audio pairs (via comma selector) for built-in MP4 muxing or
    /// native audio streams for built-in `libmpv` audio conversion, with pre-muxed fallbacks.
    pub fn yt_dlp_args_with_ffmpeg(self, ffmpeg_available: bool) -> Vec<&'static str> {
        if !ffmpeg_available {
            return match self {
                Self::VideoMp4Best => vec![
                    "-f",
                    "(bestvideo[ext=mp4][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[ext=mp4],bestaudio[ext=m4a])/best[ext=mp4]/best",
                ],
                Self::Video1080p => vec![
                    "-f",
                    "(bestvideo[height<=1080][ext=mp4][vcodec^=avc1][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[height<=1080][ext=mp4][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[height<=1080][ext=mp4],bestaudio[ext=m4a])/best[height<=1080][ext=mp4]/best[height<=1080]/best[ext=mp4]/best",
                ],
                Self::Video720p => vec![
                    "-f",
                    "(bestvideo[height<=720][ext=mp4][vcodec^=avc1][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[height<=720][ext=mp4][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[height<=720][ext=mp4],bestaudio[ext=m4a])/best[height<=720][ext=mp4]/best[height<=720]/best[ext=mp4]/best",
                ],
                Self::Video480p => vec![
                    "-f",
                    "(bestvideo[height<=480][ext=mp4][vcodec^=avc1][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[height<=480][ext=mp4][protocol^=http],bestaudio[ext=m4a][protocol^=http])/(bestvideo[height<=480][ext=mp4],bestaudio[ext=m4a])/best[height<=480][ext=mp4]/best[height<=480]/best[ext=mp4]/best",
                ],
                Self::AudioM4a => vec![
                    "-f",
                    "bestaudio[ext=m4a][protocol^=http]/bestaudio[ext=m4a]/bestaudio[ext=mp4]/bestaudio/best",
                ],
                Self::AudioOpus => vec![
                    "-f",
                    "bestaudio[acodec*=opus][ext=webm][protocol^=http]/bestaudio[acodec*=opus]/bestaudio[ext=webm][protocol^=http]/bestaudio[ext=webm]/bestaudio[ext=opus]/bestaudio/best",
                ],
                Self::AudioMp3 => vec![
                    "-f",
                    "bestaudio[ext=m4a][protocol^=http]/bestaudio[protocol^=http]/bestaudio[ext=mp3]/bestaudio[ext=m4a]/bestaudio/best",
                ],
            };
        }
        match self {
            Self::VideoMp4Best => vec![
                "-f",
                "bv*[ext=mp4][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b[ext=mp4]/b",
                "--merge-output-format",
                "mp4",
            ],
            Self::Video1080p => vec![
                "-f",
                "bv*[height<=1080][ext=mp4][vcodec^=avc1][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[height<=1080][ext=mp4][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
                "--merge-output-format",
                "mp4",
            ],
            Self::Video720p => vec![
                "-f",
                "bv*[height<=720][ext=mp4][vcodec^=avc1][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[height<=720][ext=mp4][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[height<=720]+ba/b[height<=720]/bv*+ba/b",
                "--merge-output-format",
                "mp4",
            ],
            Self::Video480p => vec![
                "-f",
                "bv*[height<=480][ext=mp4][vcodec^=avc1][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[height<=480][ext=mp4][protocol^=http]+ba[ext=m4a][protocol^=http]/bv*[height<=480]+ba/b[height<=480]/bv*+ba/b",
                "--merge-output-format",
                "mp4",
            ],
            Self::AudioM4a => vec![
                "-f",
                "ba[ext=m4a][protocol^=http]/ba[ext=m4a]/ba/b",
                "-x",
                "--audio-format",
                "m4a",
            ],
            Self::AudioOpus => vec![
                "-f",
                "bestaudio[acodec*=opus][ext=webm][protocol^=http]/bestaudio[acodec*=opus]/ba[acodec*=opus]/ba[ext=webm][protocol^=http]/ba[ext=webm]/bestaudio/ba/b",
            ],
            Self::AudioMp3 => vec![
                "-f",
                "ba[protocol^=http]/ba/b",
                "-x",
                "--audio-format",
                "mp3",
            ],
        }
    }

    fn fallback_args(self, ffmpeg_available: bool) -> Vec<&'static str> {
        if ffmpeg_available {
            return match self {
                Self::VideoMp4Best | Self::Video1080p | Self::Video720p | Self::Video480p => {
                    vec!["-f", "bv*+ba/b", "--merge-output-format", "mp4"]
                }
                Self::AudioM4a => vec!["-f", "ba/b", "-x", "--audio-format", "m4a"],
                Self::AudioOpus => vec!["-f", "bestaudio[acodec*=opus]/bestaudio/ba/b"],
                Self::AudioMp3 => vec!["-f", "ba/b", "-x", "--audio-format", "mp3"],
            };
        }
        match self {
            Self::VideoMp4Best => vec!["-f", "best[ext=mp4]/best"],
            Self::Video1080p => {
                vec![
                    "-f",
                    "best[height<=1080][ext=mp4]/best[height<=1080]/best[ext=mp4]/best",
                ]
            }
            Self::Video720p => {
                vec![
                    "-f",
                    "best[height<=720][ext=mp4]/best[height<=720]/best[ext=mp4]/best",
                ]
            }
            Self::Video480p => {
                vec![
                    "-f",
                    "best[height<=480][ext=mp4]/best[height<=480]/best[ext=mp4]/best",
                ]
            }
            Self::AudioM4a | Self::AudioOpus | Self::AudioMp3 => vec!["-f", "bestaudio/best"],
        }
    }
}

fn probe_ffmpeg_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let exe_name = "ffmpeg.exe";
    #[cfg(not(windows))]
    let exe_name = "ffmpeg";

    let probe = |bin: &Path| {
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.status().map(|s| s.success()).unwrap_or(false)
    };

    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        let bin = PathBuf::from(local)
            .join("FastTube")
            .join("bin")
            .join(exe_name);
        if bin.is_file() && probe(&bin) {
            return Some(bin);
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let bin = dir.join(exe_name);
        if bin.is_file() && probe(&bin) {
            return Some(bin);
        }
    }
    let path_bin = Path::new("ffmpeg");
    if probe(path_bin) {
        return Some(PathBuf::from("ffmpeg"));
    }
    None
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    FFMPEG_PATH_CACHE.get_or_init(probe_ffmpeg_path).clone()
}

pub fn has_ffmpeg() -> bool {
    find_ffmpeg().is_some()
}

/// Returns the cached `ffmpeg` probe result without ever blocking or spawning a process.
pub fn has_ffmpeg_cached() -> Option<bool> {
    FFMPEG_PATH_CACHE.get().map(Option::is_some)
}

/// Spawns at most one background thread to probe `ffmpeg` availability and
/// wakes the UI via `ctx.request_repaint()` once the cache is populated.
pub fn trigger_ffmpeg_probe(ctx: eframe::egui::Context) {
    if FFMPEG_PATH_CACHE.get().is_some() {
        return;
    }
    if FFMPEG_PROBE_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let _ = has_ffmpeg();
        ctx.request_repaint();
    });
}

#[derive(Debug, Default)]
struct ProgressRepaintThrottle {
    last_repaint: Option<Instant>,
    last_percent: Option<f32>,
}

impl ProgressRepaintThrottle {
    fn should_emit(&mut self, percent: f32, now: Instant) -> bool {
        let should = match (self.last_repaint, self.last_percent) {
            (None, _) | (_, None) => true,
            (Some(prev_time), Some(prev_pct)) => {
                now.duration_since(prev_time) >= PROGRESS_REPAINT_MIN_INTERVAL
                    || (percent - prev_pct).abs() >= PROGRESS_REPAINT_MIN_PERCENT_DELTA
                    || (percent >= 100.0 && prev_pct < 100.0)
            }
        };
        if should {
            self.last_repaint = Some(now);
            self.last_percent = Some(percent);
        }
        should
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DownloadOptions {
    pub embed_subs: bool,
    pub embed_chapters: bool,
    pub embed_thumbnail: bool,
    pub continue_partial: bool,
}

pub fn build_embed_args(opts: &DownloadOptions) -> Vec<String> {
    let mut args = Vec::new();
    if opts.continue_partial {
        args.push("--continue".to_owned());
    }
    if opts.embed_subs {
        args.push("--embed-subs".to_owned());
    }
    if opts.embed_chapters {
        args.push("--embed-chapters".to_owned());
    }
    if opts.embed_thumbnail {
        args.push("--embed-thumbnail".to_owned());
    }
    args
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchDownloadRequest {
    pub video: Video,
    pub format: DownloadFormat,
    pub output_dir: PathBuf,
    pub options: DownloadOptions,
}

pub fn build_batch_playlist_requests(
    videos: &[Video],
    format: DownloadFormat,
    output_dir: &Path,
    opts: DownloadOptions,
) -> Vec<BatchDownloadRequest> {
    let mut seen_keys = std::collections::HashSet::with_capacity(videos.len());
    let mut requests = Vec::with_capacity(videos.len());
    for video in videos {
        let trimmed_url = video.url.trim();
        if trimmed_url.is_empty() {
            continue;
        }
        let key =
            crate::history::youtube_video_id(trimmed_url).unwrap_or_else(|| trimmed_url.to_owned());
        if seen_keys.insert(key) {
            requests.push(BatchDownloadRequest {
                video: video.clone(),
                format,
                output_dir: output_dir.to_path_buf(),
                options: opts,
            });
        }
    }
    requests
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedDownloadStatus {
    Completed,
    Paused,
    Failed,
    #[default]
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedDownloadRecord {
    pub id: u64,
    pub video_id: Option<String>,
    pub title: String,
    pub channel: String,
    pub url: String,
    #[serde(default)]
    pub thumbnail_url: String,
    #[serde(default)]
    pub format: DownloadFormat,
    pub status: PersistedDownloadStatus,
    #[serde(default)]
    pub output_path: Option<PathBuf>,
    #[serde(default)]
    pub progress_percent: f32,
    #[serde(default)]
    pub file_size_bytes: Option<u64>,
    #[serde(default)]
    pub updated_at_unix: i64,
}

impl PersistedDownloadRecord {
    fn is_valid(&self) -> bool {
        !self.url.trim().is_empty()
            && self.url.len() <= MAX_DOWNLOAD_RECORD_TEXT_BYTES
            && self.title.len() <= MAX_DOWNLOAD_RECORD_TEXT_BYTES
            && self.channel.len() <= MAX_DOWNLOAD_RECORD_TEXT_BYTES
            && self.thumbnail_url.len() <= MAX_DOWNLOAD_RECORD_TEXT_BYTES
            && self.progress_percent.is_finite()
            && (0.0..=100.0).contains(&self.progress_percent)
    }

    pub fn into_download_item(self, default_output_dir: &Path) -> DownloadItem {
        let output_dir = self
            .output_path
            .as_deref()
            .and_then(|p| p.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_output_dir.to_path_buf());
        let status = match self.status {
            PersistedDownloadStatus::Completed => DownloadStatus::Completed {
                output_path: self.output_path,
            },
            PersistedDownloadStatus::Paused => DownloadStatus::Paused,
            PersistedDownloadStatus::Failed => {
                DownloadStatus::Failed("Download interrompido na sessão anterior.".to_owned())
            }
            PersistedDownloadStatus::Cancelled => DownloadStatus::Cancelled,
        };
        DownloadItem {
            id: self.id.max(1),
            video: Video {
                title: if self.title.trim().is_empty() {
                    self.video_id
                        .clone()
                        .unwrap_or_else(|| "Download".to_owned())
                } else {
                    self.title
                },
                channel: self.channel,
                url: self.url,
                thumbnail: self.thumbnail_url,
                duration: String::new(),
            },
            format: self.format,
            output_dir,
            status,
        }
    }
}

#[allow(dead_code)]
pub fn downloads_history_path() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        let root = PathBuf::from(local);
        if root.is_absolute() {
            return root.join("FastTube").join("downloads.json");
        }
    }
    PathBuf::from("downloads.json")
}

pub fn load_download_records_from(path: &Path) -> std::io::Result<Vec<PersistedDownloadRecord>> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut bytes = Vec::new();
    file.take(MAX_DOWNLOAD_RECORDS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid downloads.json");
    if bytes.len() as u64 > MAX_DOWNLOAD_RECORDS_FILE_BYTES {
        return Err(invalid());
    }
    let root: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let records_val = if let Some(arr) = root.as_array() {
        arr.clone()
    } else {
        if root.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err(invalid());
        }
        root.get("records")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(invalid)?
            .clone()
    };
    if records_val.len() > MAX_DOWNLOAD_HISTORY * 4 {
        return Err(invalid());
    }
    let mut records = Vec::with_capacity(records_val.len());
    for val in records_val {
        let rec: PersistedDownloadRecord = serde_json::from_value(val).map_err(|_| invalid())?;
        if !rec.is_valid() {
            return Err(invalid());
        }
        records.push(rec);
    }
    Ok(records)
}

pub fn save_download_records_to(
    path: &Path,
    records: &[PersistedDownloadRecord],
) -> std::io::Result<()> {
    if path.exists() {
        // Preserve corrupt files on disk untouched and surface InvalidData.
        let _ = load_download_records_from(path)?;
    }
    if records.iter().any(|r| !r.is_valid()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "download record exceeds validation limits",
        ));
    }
    #[derive(serde::Serialize)]
    struct DownloadsFile<'a> {
        version: u8,
        records: &'a [PersistedDownloadRecord],
    }
    let slice = if records.len() > MAX_DOWNLOAD_HISTORY {
        &records[records.len() - MAX_DOWNLOAD_HISTORY..]
    } else {
        records
    };
    let bytes = serde_json::to_vec(&DownloadsFile {
        version: 1,
        records: slice,
    })?;
    if bytes.len() as u64 > MAX_DOWNLOAD_RECORDS_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "downloads.json exceeds file size limit",
        ));
    }
    crate::history::write_atomic(path, &bytes)
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Running {
        progress_percent: f32,
        speed: Option<String>,
        eta: Option<String>,
    },
    Paused,
    Completed {
        output_path: Option<PathBuf>,
    },
    Failed(String),
    Cancelled,
}

impl DownloadStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed(_) | Self::Cancelled
        )
    }

    pub fn is_paused(&self) -> bool {
        matches!(self, Self::Paused)
    }
}

#[derive(Clone, Debug)]
pub struct DownloadItem {
    pub id: u64,
    pub video: Video,
    pub format: DownloadFormat,
    pub output_dir: PathBuf,
    pub status: DownloadStatus,
}

impl DownloadItem {
    pub fn to_persisted_record(
        &self,
        paused_progress: Option<f32>,
        now_unix: i64,
    ) -> PersistedDownloadRecord {
        let (status, output_path, progress_percent) = match &self.status {
            DownloadStatus::Completed { output_path } => (
                PersistedDownloadStatus::Completed,
                output_path.clone(),
                100.0,
            ),
            DownloadStatus::Paused => (
                PersistedDownloadStatus::Paused,
                None,
                paused_progress.unwrap_or(0.0).clamp(0.0, 100.0),
            ),
            DownloadStatus::Queued => (
                PersistedDownloadStatus::Paused,
                None,
                paused_progress.unwrap_or(0.0).clamp(0.0, 100.0),
            ),
            DownloadStatus::Running {
                progress_percent, ..
            } => (
                PersistedDownloadStatus::Paused,
                None,
                progress_percent.clamp(0.0, 100.0),
            ),
            DownloadStatus::Failed(_) => (
                PersistedDownloadStatus::Failed,
                None,
                paused_progress.unwrap_or(0.0).clamp(0.0, 100.0),
            ),
            DownloadStatus::Cancelled => (PersistedDownloadStatus::Cancelled, None, 0.0),
        };
        let file_size_bytes = output_path
            .as_deref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len());
        PersistedDownloadRecord {
            id: self.id,
            video_id: crate::history::youtube_video_id(&self.video.url),
            title: self.video.title.clone(),
            channel: self.video.channel.clone(),
            url: self.video.url.clone(),
            thumbnail_url: self.video.thumbnail.clone(),
            format: self.format,
            status,
            output_path,
            progress_percent,
            file_size_bytes,
            updated_at_unix: now_unix,
        }
    }
}

enum WorkerEvent {
    Progress {
        id: u64,
        percent: f32,
        speed: Option<String>,
        eta: Option<String>,
    },
    OutputPath {
        id: u64,
        path: PathBuf,
    },
    Finished {
        id: u64,
        result: Result<(), String>,
    },
}

struct ActiveWorkerSlot {
    cancel: Cancellation,
    pause_flag: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct DownloadManager {
    pub items: Vec<DownloadItem>,
    pub default_options: DownloadOptions,
    next_id: u64,
    max_concurrent: usize,
    active_id: Option<u64>,
    active_cancel: Option<Cancellation>,
    active_workers: std::collections::HashMap<u64, ActiveWorkerSlot>,
    tx: Option<mpsc::Sender<WorkerEvent>>,
    rx: Option<Receiver<WorkerEvent>>,
    pending_output_paths: std::collections::HashMap<u64, PathBuf>,
    item_options: std::collections::HashMap<u64, DownloadOptions>,
    paused_progress: std::collections::HashMap<u64, f32>,
    paused_staging_dirs: std::collections::HashMap<u64, PathBuf>,
}

impl DownloadManager {
    pub fn default_download_dir() -> PathBuf {
        if let Some(profile) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return PathBuf::from(profile).join("Downloads").join("FastTube");
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            return PathBuf::from(local).join("FastTube").join("Downloads");
        }
        PathBuf::from("Downloads")
    }

    pub fn effective_download_dir(override_dir: Option<&str>) -> PathBuf {
        override_dir
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(Self::default_download_dir)
    }

    pub fn effective_max_concurrent(&self) -> usize {
        if self.max_concurrent == 0 {
            DEFAULT_MAX_CONCURRENT_DOWNLOADS
        } else {
            self.max_concurrent
        }
    }

    pub fn set_max_concurrent(&mut self, max_concurrent: usize) {
        self.max_concurrent = max_concurrent.max(1);
    }

    pub fn running_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item.status, DownloadStatus::Running { .. }))
            .count()
            .max(self.active_workers.len())
    }

    pub fn paused_progress(&self, id: u64) -> f32 {
        self.paused_progress.get(&id).copied().unwrap_or(0.0)
    }

    pub fn has_active(&self) -> bool {
        self.active_id.is_some()
            || !self.active_workers.is_empty()
            || self.items.iter().any(|item| {
                matches!(
                    item.status,
                    DownloadStatus::Queued | DownloadStatus::Running { .. }
                )
            })
    }

    pub fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| !item.status.is_terminal() && !item.status.is_paused())
            .count()
    }

    pub fn to_persisted_records(&self, now_unix: i64) -> Vec<PersistedDownloadRecord> {
        self.items
            .iter()
            .map(|item| {
                item.to_persisted_record(self.paused_progress.get(&item.id).copied(), now_unix)
            })
            .collect()
    }

    pub fn restore_from_records(
        &mut self,
        records: Vec<PersistedDownloadRecord>,
        default_output_dir: &Path,
    ) {
        for rec in records {
            let id = rec.id.max(1);
            let progress = rec.progress_percent.clamp(0.0, 100.0);
            let is_paused = rec.status == PersistedDownloadStatus::Paused;
            let item = rec.into_download_item(default_output_dir);
            if is_paused {
                self.paused_progress.insert(id, progress);
                let mut opts = self.default_options;
                opts.continue_partial = true;
                self.item_options.insert(id, opts);
            }
            self.next_id = self.next_id.max(id);
            if !self.items.iter().any(|existing| existing.id == id) {
                self.items.push(item);
            }
        }
    }

    pub fn enqueue(
        &mut self,
        video: Video,
        format: DownloadFormat,
        output_dir: PathBuf,
        ctx: &eframe::egui::Context,
    ) -> u64 {
        let opts = self.default_options;
        self.enqueue_with_options(video, format, output_dir, opts, ctx)
    }

    pub fn enqueue_with_options(
        &mut self,
        video: Video,
        format: DownloadFormat,
        output_dir: PathBuf,
        options: DownloadOptions,
        ctx: &eframe::egui::Context,
    ) -> u64 {
        if self.items.len() >= MAX_DOWNLOAD_HISTORY
            && let Some(pos) = self.items.iter().position(|item| item.status.is_terminal())
        {
            let removed = self.items.remove(pos);
            self.cleanup_item_aux(removed.id, true);
        }
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.item_options.insert(id, options);
        self.items.push(DownloadItem {
            id,
            video,
            format,
            output_dir,
            status: DownloadStatus::Queued,
        });
        self.pump_next(ctx);
        id
    }

    pub fn enqueue_batch(
        &mut self,
        requests: Vec<BatchDownloadRequest>,
        ctx: &eframe::egui::Context,
    ) -> Vec<u64> {
        let mut ids = Vec::with_capacity(requests.len());
        for req in requests {
            ids.push(self.enqueue_with_options(
                req.video,
                req.format,
                req.output_dir,
                req.options,
                ctx,
            ));
        }
        ids
    }

    fn cleanup_item_aux(&mut self, id: u64, remove_staging: bool) {
        self.pending_output_paths.remove(&id);
        self.item_options.remove(&id);
        self.paused_progress.remove(&id);
        if let Some(staging_dir) = self.paused_staging_dirs.remove(&id)
            && remove_staging
        {
            let _ = std::fs::remove_dir_all(staging_dir);
        }
    }

    fn sync_legacy_active_fields(&mut self) {
        if let Some((&id, slot)) = self.active_workers.iter().next() {
            self.active_id = Some(id);
            self.active_cancel = Some(slot.cancel.clone());
        } else {
            self.active_id = None;
            self.active_cancel = None;
        }
    }

    pub fn pause(&mut self, id: u64, ctx: &eframe::egui::Context) -> bool {
        let mut current_pct = self.paused_progress.get(&id).copied().unwrap_or(0.0);
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        match item.status {
            DownloadStatus::Running {
                progress_percent, ..
            } => {
                current_pct = progress_percent;
            }
            DownloadStatus::Queued => {}
            _ => return false,
        }
        item.status = DownloadStatus::Paused;
        self.paused_progress.insert(id, current_pct);
        self.item_options.entry(id).or_default().continue_partial = true;

        if let Some(slot) = self.active_workers.remove(&id) {
            slot.pause_flag.store(true, Ordering::Release);
            slot.cancel.cancel();
        } else if self.active_id == Some(id)
            && let Some(cancel) = self.active_cancel.take()
        {
            cancel.cancel();
        }
        self.sync_legacy_active_fields();
        self.pump_next(ctx);
        true
    }

    pub fn resume(&mut self, id: u64, ctx: &eframe::egui::Context) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        if !matches!(item.status, DownloadStatus::Paused) {
            return false;
        }
        item.status = DownloadStatus::Queued;
        self.item_options.entry(id).or_default().continue_partial = true;
        self.pump_next(ctx);
        true
    }

    pub fn cancel(&mut self, id: u64, ctx: &eframe::egui::Context) {
        let was_active = if let Some(slot) = self.active_workers.remove(&id) {
            slot.cancel.cancel();
            true
        } else if self.active_id == Some(id) {
            if let Some(cancel) = self.active_cancel.take() {
                cancel.cancel();
            }
            true
        } else {
            false
        };
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id)
            && (was_active
                || matches!(item.status, DownloadStatus::Queued | DownloadStatus::Paused))
        {
            item.status = DownloadStatus::Cancelled;
        }
        self.cleanup_item_aux(id, true);
        self.sync_legacy_active_fields();
        if self.active_workers.is_empty() {
            self.tx = None;
            self.rx = None;
        }
        if was_active {
            self.pump_next(ctx);
        }
    }

    pub fn cancel_all(&mut self) {
        for (_, slot) in self.active_workers.drain() {
            slot.cancel.cancel();
        }
        if let Some(cancel) = self.active_cancel.take() {
            cancel.cancel();
        }
        self.active_id = None;
        self.tx = None;
        self.rx = None;
        for (_, staging_dir) in self.paused_staging_dirs.drain() {
            let _ = std::fs::remove_dir_all(staging_dir);
        }
        self.paused_progress.clear();
        for item in &mut self.items {
            if !item.status.is_terminal() {
                item.status = DownloadStatus::Cancelled;
            }
        }
    }

    pub fn clear_finished(&mut self) {
        self.items.retain(|item| !item.status.is_terminal());
    }

    pub fn retry(&mut self, id: u64, ctx: &eframe::egui::Context) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        if !matches!(
            item.status,
            DownloadStatus::Failed(_) | DownloadStatus::Cancelled | DownloadStatus::Paused
        ) {
            return false;
        }
        let was_paused = matches!(item.status, DownloadStatus::Paused);
        item.status = DownloadStatus::Queued;
        self.pending_output_paths.remove(&id);
        if was_paused {
            self.item_options.entry(id).or_default().continue_partial = true;
        }
        self.pump_next(ctx);
        true
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if self.active_id == Some(id) || self.active_workers.contains_key(&id) {
            return false;
        }
        let Some(pos) = self.items.iter().position(|item| {
            item.id == id && (item.status.is_terminal() || item.status.is_paused())
        }) else {
            return false;
        };
        self.items.remove(pos);
        self.cleanup_item_aux(id, true);
        true
    }

    pub fn poll(&mut self, ctx: &eframe::egui::Context) -> Option<(u64, Result<(), String>)> {
        let mut completed: Option<(u64, Result<(), String>)> = None;
        let mut disconnected = false;
        let mut slot_freed = false;
        if let Some(rx) = self.rx.take() {
            let mut keep_rx = true;
            loop {
                match rx.try_recv() {
                    Ok(WorkerEvent::Progress {
                        id,
                        percent,
                        speed,
                        eta,
                    }) => {
                        if (self.active_workers.contains_key(&id) || self.active_id == Some(id))
                            && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                            && !matches!(
                                item.status,
                                DownloadStatus::Paused | DownloadStatus::Cancelled
                            )
                        {
                            self.paused_progress.insert(id, percent);
                            item.status = DownloadStatus::Running {
                                progress_percent: percent,
                                speed,
                                eta,
                            };
                        }
                    }
                    Ok(WorkerEvent::OutputPath { id, path }) => {
                        if let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                            && let DownloadStatus::Completed { output_path } = &mut item.status
                        {
                            *output_path = Some(path);
                        } else {
                            self.pending_output_paths.insert(id, path);
                        }
                    }
                    Ok(WorkerEvent::Finished { id, result }) => {
                        let was_tracked =
                            self.active_workers.remove(&id).is_some() || self.active_id == Some(id);
                        self.sync_legacy_active_fields();
                        if !was_tracked {
                            continue;
                        }
                        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
                            if matches!(
                                item.status,
                                DownloadStatus::Paused | DownloadStatus::Cancelled
                            ) {
                                slot_freed = true;
                                continue;
                            }
                            let resolved_path = self.pending_output_paths.remove(&id);
                            match &result {
                                Ok(()) => {
                                    let fallback_dir = item.output_dir.clone();
                                    item.status = DownloadStatus::Completed {
                                        output_path: Some(resolved_path.unwrap_or(fallback_dir)),
                                    };
                                }
                                Err(err) => {
                                    item.status = DownloadStatus::Failed(err.clone());
                                }
                            }
                        }
                        self.cleanup_item_aux(id, true);
                        completed = Some((id, result));
                        slot_freed = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        keep_rx = false;
                        break;
                    }
                }
            }
            if keep_rx {
                self.rx = Some(rx);
            }
        }
        if disconnected {
            self.tx = None;
            self.rx = None;
            let abandoned: Vec<u64> = self
                .active_workers
                .drain()
                .map(|(id, _)| id)
                .chain(self.active_id.take())
                .collect();
            self.active_cancel = None;
            for id in abandoned {
                if let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                    && !item.status.is_terminal()
                    && !item.status.is_paused()
                {
                    item.status =
                        DownloadStatus::Failed("Processo de download encerrado.".to_owned());
                }
            }
            self.pump_next(ctx);
        } else if slot_freed {
            if self.active_workers.is_empty() {
                self.tx = None;
                self.rx = None;
            }
            self.pump_next(ctx);
        }
        completed
    }

    fn pump_next(&mut self, ctx: &eframe::egui::Context) {
        let max_conc = self.effective_max_concurrent();
        while can_start_another_download(self.active_workers.len(), max_conc) {
            let Some(next_idx) = self
                .items
                .iter()
                .position(|item| matches!(item.status, DownloadStatus::Queued))
            else {
                break;
            };
            let id = self.items[next_idx].id;
            let url = self.items[next_idx].video.url.clone();
            let format = self.items[next_idx].format;
            let output_dir = self.items[next_idx].output_dir.clone();
            let initial_pct = self.paused_progress.get(&id).copied().unwrap_or(0.0);
            self.items[next_idx].status = DownloadStatus::Running {
                progress_percent: initial_pct,
                speed: None,
                eta: None,
            };

            let options = self
                .item_options
                .get(&id)
                .copied()
                .unwrap_or(self.default_options);
            let staging_dir = self
                .paused_staging_dirs
                .entry(id)
                .or_insert_with(|| {
                    let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
                    output_dir.join(format!(".fasttube-dl-{}-{id}-{seq}", std::process::id()))
                })
                .clone();

            let cancel = Cancellation::default();
            let worker_cancel = cancel.clone();
            let pause_flag = Arc::new(AtomicBool::new(false));
            let worker_pause_flag = Arc::clone(&pause_flag);

            if self.tx.is_none() || self.rx.is_none() {
                let (tx, rx) = mpsc::channel();
                self.tx = Some(tx);
                self.rx = Some(rx);
            }
            let tx = self.tx.as_ref().unwrap().clone();
            let wake = ctx.clone();

            self.active_workers
                .insert(id, ActiveWorkerSlot { cancel, pause_flag });
            self.sync_legacy_active_fields();

            std::thread::spawn(move || {
                let result = run_yt_dlp_download_with_options(
                    id,
                    &url,
                    format,
                    &output_dir,
                    options,
                    Some((staging_dir, worker_pause_flag)),
                    &worker_cancel,
                    &tx,
                    &wake,
                );
                let _ = tx.send(WorkerEvent::Finished { id, result });
                wake.request_repaint();
            });
        }
    }
}

pub fn parse_yt_dlp_output_path_line(line: &str) -> Option<PathBuf> {
    let trimmed = line.trim();
    if let Some(dest) = trimmed.strip_prefix("[download] Destination: ") {
        let p = dest.trim();
        return (!p.is_empty()).then(|| PathBuf::from(p));
    }
    if let Some(dest) = trimmed.strip_prefix("[ExtractAudio] Destination: ") {
        let p = dest.trim();
        return (!p.is_empty()).then(|| PathBuf::from(p));
    }
    if let Some(rest) = trimmed.strip_prefix("[Merger] Merging formats into ") {
        let unquoted = rest.trim().trim_matches('"').trim();
        return (!unquoted.is_empty()).then(|| PathBuf::from(unquoted));
    }
    if let Some(rest) = trimmed.strip_prefix("[download] ")
        && let Some(path_part) = rest.strip_suffix(" has already been downloaded")
    {
        let p = path_part.trim();
        return (!p.is_empty()).then(|| PathBuf::from(p));
    }
    None
}

pub fn parse_yt_dlp_progress_line(line: &str) -> Option<(f32, Option<String>, Option<String>)> {
    let trimmed = line.trim();
    if !trimmed.starts_with("[download]") {
        return None;
    }
    let pct_idx = trimmed.find('%')?;
    let before = trimmed[..pct_idx].trim_end();
    let num_token = before.split_whitespace().last()?;
    let percent = num_token.parse::<f32>().ok()?.clamp(0.0, 100.0);

    let speed = trimmed.split(" at ").nth(1).and_then(|rest| {
        let token = rest.split_whitespace().next()?.trim();
        (!token.is_empty() && token != "Unknown").then(|| token.to_owned())
    });
    let eta = trimmed.split(" ETA ").nth(1).and_then(|rest| {
        let token = rest.split_whitespace().next()?.trim();
        (!token.is_empty() && token != "Unknown").then(|| token.to_owned())
    });
    Some((percent, speed, eta))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DownloadedStream {
    vcodec: String,
    acodec: String,
    ext: String,
    path: PathBuf,
}

impl DownloadedStream {
    fn has_video(&self) -> bool {
        !self.vcodec.is_empty()
            && !self.vcodec.eq_ignore_ascii_case("none")
            && !self.vcodec.eq_ignore_ascii_case("NA")
    }

    fn has_audio(&self) -> bool {
        !self.acodec.is_empty()
            && !self.acodec.eq_ignore_ascii_case("none")
            && !self.acodec.eq_ignore_ascii_case("NA")
    }

    fn is_iso_bmff(&self) -> bool {
        matches!(
            self.ext.to_ascii_lowercase().as_str(),
            "mp4" | "m4a" | "m4v" | "mov" | "mp4a"
        )
    }
}

fn parse_yt_dlp_printed_file_line(line: &str) -> Option<DownloadedStream> {
    let rest = line.trim().strip_prefix("FASTTUBE_DL:")?;
    let mut parts = rest.splitn(4, '|');
    let vcodec = parts.next()?.trim().to_owned();
    let acodec = parts.next()?.trim().to_owned();
    let ext = parts.next()?.trim().to_owned();
    let filepath = parts.next()?.trim();
    if filepath.is_empty() {
        return None;
    }
    Some(DownloadedStream {
        vcodec,
        acodec,
        ext,
        path: PathBuf::from(filepath),
    })
}

fn extract_sanitized_base_name(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    let stem = match file_name.rsplit_once('.') {
        Some((base, _ext)) if !base.is_empty() => base,
        _ => file_name,
    };
    let clean = match stem.rsplit_once("__ft_f") {
        Some((prefix, _fmt)) if !prefix.is_empty() => prefix,
        _ => stem,
    };
    let trimmed = clean.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn build_final_output_path(
    output_dir: &Path,
    base: &str,
    format: DownloadFormat,
    ext: &str,
) -> PathBuf {
    let tag = format!("[{}]", format.filename_tag());
    let trimmed_base = base.trim();
    let stem = if trimmed_base.ends_with(&tag) {
        trimmed_base.to_owned()
    } else if trimmed_base.is_empty() {
        format!("download {tag}")
    } else {
        format!("{trimmed_base} {tag}")
    };
    let clean_ext = ext.trim().trim_start_matches('.');
    if clean_ext.is_empty() {
        output_dir.join(stem)
    } else {
        output_dir.join(format!("{stem}.{clean_ext}"))
    }
}

fn is_format_unavailable_error(stderr_lines: &[String]) -> bool {
    stderr_lines.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("requested format is not available")
            || lower.contains("only images are available for download")
    })
}

struct StagingDirGuard {
    path: PathBuf,
    preserve_on_drop: Option<Arc<AtomicBool>>,
}

impl StagingDirGuard {
    fn create(output_dir: &Path, id: u64) -> Result<Self, String> {
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = output_dir.join(format!(".fasttube-dl-{}-{id}-{seq}", std::process::id()));
        Self::open_or_create_at(path, None)
    }

    fn open_or_create_at(
        path: PathBuf,
        preserve_on_drop: Option<Arc<AtomicBool>>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Não foi possível criar pasta temporária de download: {e}"))?;
        Ok(Self {
            path,
            preserve_on_drop,
        })
    }

    fn reset(&self) -> Result<(), String> {
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path)
                .map_err(|e| format!("Erro limpando pasta temporária de download: {e}"))?;
        }
        std::fs::create_dir_all(&self.path)
            .map_err(|e| format!("Erro recriando pasta temporária de download: {e}"))?;
        Ok(())
    }
}

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        if self
            .preserve_on_drop
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return;
        }
        for attempt in 0..4 {
            if std::fs::remove_dir_all(&self.path).is_ok() || !self.path.exists() {
                break;
            }
            if attempt < 3 {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn replace_file_atomically(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Não foi possível preparar pasta de destino: {e}"))?;
    }
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        fn to_wide_z(p: &OsStr) -> Vec<u16> {
            p.encode_wide().chain(std::iter::once(0)).collect()
        }

        let src_w = to_wide_z(src.as_os_str());
        let dst_w = to_wide_z(dst.as_os_str());
        let ok = unsafe {
            MoveFileExW(
                src_w.as_ptr(),
                dst_w.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok != 0 {
            return Ok(());
        }
        let move_err = std::io::Error::last_os_error();
        let parent = dst.parent().unwrap_or_else(|| Path::new("."));
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_dst = parent.join(format!(".fasttube-part-{}-{seq}.tmp", std::process::id()));
        if let Err(copy_err) = (|| -> std::io::Result<()> {
            std::fs::copy(src, &temp_dst)?;
            if let Ok(file) = std::fs::OpenOptions::new().read(true).open(&temp_dst) {
                let _ = file.sync_all();
            }
            let temp_w = to_wide_z(temp_dst.as_os_str());
            let replaced = unsafe {
                MoveFileExW(
                    temp_w.as_ptr(),
                    dst_w.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if replaced == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let _ = std::fs::remove_file(src);
            Ok(())
        })() {
            let _ = std::fs::remove_file(&temp_dst);
            return Err(format!(
                "Não foi possível mover o arquivo baixado para o destino ({move_err}; {copy_err})"
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if std::fs::rename(src, dst).is_ok() {
            return Ok(());
        }
        let parent = dst.parent().unwrap_or_else(|| Path::new("."));
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_dst = parent.join(format!(".fasttube-part-{}-{seq}.tmp", std::process::id()));
        let res = (|| -> std::io::Result<()> {
            std::fs::copy(src, &temp_dst)?;
            if let Ok(file) = std::fs::OpenOptions::new().read(true).open(&temp_dst) {
                let _ = file.sync_all();
            }
            std::fs::rename(&temp_dst, dst)?;
            let _ = std::fs::remove_file(src);
            Ok(())
        })();
        if let Err(err) = res {
            let _ = std::fs::remove_file(&temp_dst);
            return Err(format!(
                "Não foi possível mover o arquivo baixado para o destino: {err}"
            ));
        }
        Ok(())
    }
}

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *const c_void,
}

#[repr(C)]
struct MpvEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: u64,
    _playlist_insert_id: u64,
}

type MpvCreateFn = unsafe extern "C" fn() -> *mut c_void;
type MpvSetOptionStringFn =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type MpvInitializeFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type MpvCommandFn = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type MpvWaitEventFn = unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent;
type MpvTerminateDestroyFn = unsafe extern "C" fn(*mut c_void);
type MpvErrorStringFn = unsafe extern "C" fn(c_int) -> *const c_char;

struct MpvHandleGuard {
    handle: *mut c_void,
    terminate: MpvTerminateDestroyFn,
}

impl Drop for MpvHandleGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.terminate)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

fn load_mpv_library() -> Result<Library, String> {
    for name in ["libmpv-2.dll", "mpv-2.dll"] {
        let mut candidates = Vec::new();
        if let Ok(executable) = std::env::current_exe() {
            let mut current = executable.parent();
            for _ in 0..4 {
                let Some(directory) = current else { break };
                candidates.push(directory.join(name));
                current = directory.parent();
            }
        }
        if let Ok(directory) = std::env::current_dir() {
            candidates.push(directory.join(name));
        }
        candidates.push(name.into());
        for candidate in candidates {
            if let Ok(found) = unsafe { Library::new(&candidate) } {
                return Ok(found);
            }
        }
    }
    Err("libmpv não disponível para conversão de áudio.".to_owned())
}

fn transcode_audio_with_libmpv(
    input: &Path,
    output: &Path,
    target_of: &str,
    target_oac: &str,
    cancel: &Cancellation,
) -> Result<(), String> {
    if cancel.cancelled() {
        return Err("Download cancelado.".to_owned());
    }
    let input_str = input
        .to_str()
        .ok_or_else(|| "Caminho de áudio de entrada inválido.".to_owned())?;
    let output_str = output
        .to_str()
        .ok_or_else(|| "Caminho de áudio de saída inválido.".to_owned())?;

    let lib = load_mpv_library()?;
    let (create, set_opt, init, command, wait_event, terminate, error_string) = unsafe {
        let create = *lib
            .get::<MpvCreateFn>(b"mpv_create\0")
            .map_err(|e| format!("Falha ao carregar mpv_create: {e}"))?;
        let set_opt = *lib
            .get::<MpvSetOptionStringFn>(b"mpv_set_option_string\0")
            .map_err(|e| format!("Falha ao carregar mpv_set_option_string: {e}"))?;
        let init = *lib
            .get::<MpvInitializeFn>(b"mpv_initialize\0")
            .map_err(|e| format!("Falha ao carregar mpv_initialize: {e}"))?;
        let command = *lib
            .get::<MpvCommandFn>(b"mpv_command\0")
            .map_err(|e| format!("Falha ao carregar mpv_command: {e}"))?;
        let wait_event = *lib
            .get::<MpvWaitEventFn>(b"mpv_wait_event\0")
            .map_err(|e| format!("Falha ao carregar mpv_wait_event: {e}"))?;
        let terminate = *lib
            .get::<MpvTerminateDestroyFn>(b"mpv_terminate_destroy\0")
            .map_err(|e| format!("Falha ao carregar mpv_terminate_destroy: {e}"))?;
        let error_string = *lib
            .get::<MpvErrorStringFn>(b"mpv_error_string\0")
            .map_err(|e| format!("Falha ao carregar mpv_error_string: {e}"))?;
        (
            create,
            set_opt,
            init,
            command,
            wait_event,
            terminate,
            error_string,
        )
    };

    let describe = |code: c_int| -> String {
        unsafe {
            let ptr = error_string(code);
            if ptr.is_null() {
                format!("erro {code}")
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        }
    };

    let handle = unsafe { create() };
    if handle.is_null() {
        return Err("Não foi possível inicializar o codificador do libmpv.".to_owned());
    }
    let guard = MpvHandleGuard { handle, terminate };

    let options = [
        ("terminal", "no"),
        ("config", "no"),
        ("vid", "no"),
        ("o", output_str),
        ("of", target_of),
        ("oac", target_oac),
    ];
    for (k, v) in options {
        let kc = CString::new(k).map_err(|_| "Opção libmpv inválida.".to_owned())?;
        let vc = CString::new(v).map_err(|_| "Valor de opção libmpv inválido.".to_owned())?;
        let rc = unsafe { set_opt(guard.handle, kc.as_ptr(), vc.as_ptr()) };
        if rc < 0 {
            return Err(format!(
                "Falha configurando '{k}' no libmpv: {}",
                describe(rc)
            ));
        }
    }

    let init_rc = unsafe { init(guard.handle) };
    if init_rc < 0 {
        return Err(format!(
            "Falha ao iniciar libmpv para conversão de áudio: {}",
            describe(init_rc)
        ));
    }

    let cmd_c = CString::new("loadfile").unwrap();
    let path_c = CString::new(input_str).map_err(|_| "Caminho de entrada inválido.".to_owned())?;
    let mode_c = CString::new("replace").unwrap();
    let argv = [
        cmd_c.as_ptr(),
        path_c.as_ptr(),
        mode_c.as_ptr(),
        std::ptr::null(),
    ];
    let cmd_rc = unsafe { command(guard.handle, argv.as_ptr()) };
    if cmd_rc < 0 {
        return Err(format!(
            "Falha ao carregar áudio para conversão: {}",
            describe(cmd_rc)
        ));
    }

    loop {
        if cancel.cancelled() {
            return Err("Download cancelado.".to_owned());
        }
        let ev_ptr = unsafe { wait_event(guard.handle, 0.1) };
        if ev_ptr.is_null() {
            continue;
        }
        let ev = unsafe { &*ev_ptr };
        match ev.event_id {
            0 => continue,
            1 => break,
            7 => {
                if !ev.data.is_null() {
                    let end = unsafe { &*(ev.data as *const MpvEndFile) };
                    if end.reason == 4 || end.error < 0 {
                        let detail = describe(if end.error < 0 { end.error } else { ev.error });
                        return Err(format!("Falha na conversão de áudio pelo libmpv: {detail}"));
                    }
                } else if ev.error < 0 {
                    return Err(format!(
                        "Falha na conversão de áudio pelo libmpv: {}",
                        describe(ev.error)
                    ));
                }
                break;
            }
            _ => {}
        }
    }

    // Explicitly terminate mpv so the muxer closes and flushes the output file handle.
    drop(guard);

    let size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        let _ = std::fs::remove_file(output);
        return Err("Arquivo de áudio convertido resultou vazio.".to_owned());
    }
    Ok(())
}

struct DownloadJob<'a> {
    id: u64,
    url: &'a str,
    format: DownloadFormat,
    output_dir: &'a Path,
    options: DownloadOptions,
    cancel: &'a Cancellation,
    tx: &'a mpsc::Sender<WorkerEvent>,
    wake: &'a eframe::egui::Context,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgressMode {
    Direct,
    StagedPostprocess,
}

struct YtDlpRunOptions<'a> {
    format_args: &'a [&'a str],
    extractor_args: Option<&'a str>,
    target_dir: &'a Path,
    output_template: &'a str,
    print_after_move: bool,
    progress_mode: ProgressMode,
    ffmpeg_path: Option<&'a Path>,
}

struct YtDlpRunOutput {
    streams: Vec<DownloadedStream>,
    last_output_path: Option<PathBuf>,
}

struct CancelWatcherGuard {
    signal: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for CancelWatcherGuard {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.signal;
        if let Ok(mut done) = lock.lock() {
            *done = true;
            cvar.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn spawn_cancel_watcher(
    cancel: Cancellation,
    process_job: Option<Arc<ProcessJobObject>>,
    child: Arc<Mutex<std::process::Child>>,
) -> CancelWatcherGuard {
    let signal = Arc::new((Mutex::new(false), Condvar::new()));
    let thread_signal = Arc::clone(&signal);
    let handle = std::thread::spawn(move || {
        let (lock, cvar) = &*thread_signal;
        let mut done = lock.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            if cancel.cancelled() {
                if let Some(ref pj) = process_job {
                    pj.terminate();
                }
                if let Ok(mut guard) = child.lock() {
                    let _ = guard.kill();
                }
                break;
            }
            let (guard, _) = cvar
                .wait_timeout(done, CANCEL_WATCHER_POLL_INTERVAL)
                .unwrap_or_else(|e| e.into_inner());
            done = guard;
        }
    });
    CancelWatcherGuard {
        signal,
        handle: Some(handle),
    }
}

fn resolve_staged_output_file(
    staging_dir: &Path,
    streams: &[DownloadedStream],
    last_parsed_path: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(stream) = streams.iter().rev().find(|s| s.path.is_file()) {
        return Some(stream.path.clone());
    }
    if let Some(path) = last_parsed_path.filter(|p| p.is_file()) {
        return Some(path.to_path_buf());
    }
    let entries = std::fs::read_dir(staging_dir).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let is_temp = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("part")
                    || ext.eq_ignore_ascii_case("ytdl")
                    || ext.eq_ignore_ascii_case("tmp")
                    || ext.eq_ignore_ascii_case("temp")
            });
        if is_temp {
            continue;
        }
        let len = meta.len();
        if best.as_ref().is_none_or(|(best_len, _)| len >= *best_len) {
            best = Some((len, path));
        }
    }
    best.map(|(_, path)| path)
}

fn move_staged_output_to_final(
    staged_file: &Path,
    output_dir: &Path,
    format: DownloadFormat,
) -> Result<PathBuf, String> {
    let base = extract_sanitized_base_name(staged_file).unwrap_or_else(|| "download".to_owned());
    let ext = staged_file
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .unwrap_or(if format.is_audio() { "m4a" } else { "mp4" });
    let final_path = build_final_output_path(output_dir, &base, format, ext);
    replace_file_atomically(staged_file, &final_path)?;
    Ok(final_path)
}

fn run_yt_dlp_once(
    job: &DownloadJob<'_>,
    opts: &YtDlpRunOptions<'_>,
) -> Result<YtDlpRunOutput, (String, Vec<String>)> {
    if job.cancel.cancelled() {
        return Err(("Download cancelado.".to_owned(), Vec::new()));
    }
    let mut cmd = search::yt_dlp_command();
    cmd.env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .arg("--newline")
        .arg("--no-playlist")
        .arg("--no-warnings");
    if opts.print_after_move {
        cmd.arg("--no-simulate")
            .arg("--no-quiet")
            .arg("--progress")
            .arg("--print")
            .arg("after_move:FASTTUBE_DL:%(vcodec)s|%(acodec)s|%(ext)s|%(filepath)s");
    }
    if let Some(ffmpeg) = opts.ffmpeg_path
        && ffmpeg != Path::new("ffmpeg")
    {
        cmd.arg("--ffmpeg-location").arg(ffmpeg);
    }
    if let Some(extra) = opts.extractor_args {
        cmd.arg("--extractor-args").arg(extra);
    }
    cmd.arg("-P")
        .arg(opts.target_dir)
        .arg("-o")
        .arg(opts.output_template);
    for arg in opts.format_args {
        cmd.arg(arg);
    }
    let embed_opts = if opts.ffmpeg_path.is_some() {
        job.options
    } else {
        DownloadOptions {
            continue_partial: job.options.continue_partial,
            ..Default::default()
        }
    };
    for arg in build_embed_args(&embed_opts) {
        cmd.arg(arg);
    }
    cmd.arg("--").arg(job.url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let process_job = ProcessJobObject::new().map(Arc::new);
    let mut child = cmd
        .spawn()
        .map_err(|e| (format!("Não foi possível iniciar yt-dlp: {e}"), Vec::new()))?;
    if let Some(ref pj) = process_job {
        pj.assign_child(&child);
    }
    let stdout = child.stdout.take();
    let err_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut ring = VecDeque::with_capacity(MAX_STDERR_LINES);
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                if ring.len() >= MAX_STDERR_LINES {
                    ring.pop_front();
                }
                ring.push_back(line);
            }
            ring.into_iter().collect::<Vec<_>>()
        })
    });

    let child = Arc::new(Mutex::new(child));
    let _watcher =
        spawn_cancel_watcher(job.cancel.clone(), process_job.clone(), Arc::clone(&child));

    let mut streams = Vec::new();
    let mut last_output_path = None;
    let mut seen_destinations: usize = 0;
    let mut progress_throttle = ProgressRepaintThrottle::default();

    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if job.cancel.cancelled() {
                if let Some(ref pj) = process_job {
                    pj.terminate();
                }
                if let Ok(mut guard) = child.lock() {
                    let _ = guard.kill();
                    let _ = guard.wait();
                }
                return Err(("Download cancelado.".to_owned(), Vec::new()));
            }
            if opts.print_after_move
                && let Some(stream) = parse_yt_dlp_printed_file_line(&line)
            {
                streams.push(stream);
                continue;
            }
            if line.trim_start().starts_with("[download] Destination: ") {
                seen_destinations += 1;
            }
            if let Some((raw_percent, speed, eta)) = parse_yt_dlp_progress_line(&line) {
                let mapped_percent = match opts.progress_mode {
                    ProgressMode::StagedPostprocess => {
                        if job.format.is_audio() {
                            (raw_percent * 0.90).clamp(0.0, 90.0)
                        } else {
                            let stream_idx = seen_destinations.saturating_sub(1).max(streams.len());
                            if stream_idx == 0 {
                                (raw_percent * 0.80).clamp(0.0, 80.0)
                            } else {
                                (80.0 + raw_percent * 0.15).clamp(80.0, 95.0)
                            }
                        }
                    }
                    ProgressMode::Direct => raw_percent,
                };
                if progress_throttle.should_emit(mapped_percent, Instant::now()) {
                    let _ = job.tx.send(WorkerEvent::Progress {
                        id: job.id,
                        percent: mapped_percent,
                        speed,
                        eta,
                    });
                    job.wake.request_repaint();
                }
            } else if let Some(path) = parse_yt_dlp_output_path_line(&line) {
                last_output_path = Some(path);
            }
        }
    }

    // Reap without holding the mutex across a blocking wait so the cancel
    // watcher can still issue `kill()` if stdout closed early.
    let wait_res = loop {
        let step = {
            let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
            guard.try_wait()
        };
        match step {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if job.cancel.cancelled() {
                    if let Some(ref pj) = process_job {
                        pj.terminate();
                    }
                    if let Ok(mut guard) = child.lock() {
                        let _ = guard.kill();
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => break Err(err),
        }
    };
    let stderr_lines = err_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let status =
        wait_res.map_err(|e| (format!("Erro aguardando yt-dlp: {e}"), stderr_lines.clone()))?;
    if job.cancel.cancelled() {
        return Err(("Download cancelado.".to_owned(), stderr_lines));
    }
    if status.success() {
        Ok(YtDlpRunOutput {
            streams,
            last_output_path,
        })
    } else {
        let message = stderr_lines
            .iter()
            .rev()
            .find(|line| line.trim_start().starts_with("ERROR:"))
            .cloned()
            .or_else(|| stderr_lines.last().cloned())
            .unwrap_or_else(|| format!("yt-dlp terminou com código {:?}.", status.code()));
        Err((message, stderr_lines))
    }
}

fn finish_single_staged_stream(
    job: &DownloadJob<'_>,
    stream: &DownloadedStream,
) -> Result<(), String> {
    let base = extract_sanitized_base_name(&stream.path).unwrap_or_else(|| "download".to_owned());
    let ext = if stream.ext.is_empty() {
        stream
            .path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .unwrap_or("mp4")
    } else {
        stream.ext.as_str()
    };
    let final_path = build_final_output_path(job.output_dir, &base, job.format, ext);
    replace_file_atomically(&stream.path, &final_path)?;
    let _ = job.tx.send(WorkerEvent::Progress {
        id: job.id,
        percent: 100.0,
        speed: None,
        eta: None,
    });
    let _ = job.tx.send(WorkerEvent::OutputPath {
        id: job.id,
        path: final_path,
    });
    job.wake.request_repaint();
    Ok(())
}

fn run_premuxed_staged_fallback(
    job: &DownloadJob<'_>,
    staging: &StagingDirGuard,
) -> Result<(), String> {
    staging.reset()?;
    let fallback_args = job.format.fallback_args(false);
    let run_out = run_yt_dlp_once(
        job,
        &YtDlpRunOptions {
            format_args: &fallback_args,
            extractor_args: Some("youtube:player_client=default,android_vr"),
            target_dir: &staging.path,
            output_template: "%(title).120B [%(id)s]__ft_f%(format_id)s.%(ext)s",
            print_after_move: true,
            progress_mode: ProgressMode::StagedPostprocess,
            ffmpeg_path: None,
        },
    )
    .map_err(|(err, _)| err)?;
    let Some(first) = run_out.streams.first() else {
        return Err("yt-dlp não produziu arquivos de saída na tentativa alternativa.".to_owned());
    };
    finish_single_staged_stream(job, first)
}

pub fn is_native_opus_stream(ext: &str, acodec: &str) -> bool {
    let ext_lower = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let acodec_lower = acodec.trim().to_ascii_lowercase();
    ext_lower == "webm" || ext_lower == "opus" || acodec_lower.contains("opus")
}

#[allow(dead_code)]
fn run_yt_dlp_download(
    id: u64,
    url: &str,
    format: DownloadFormat,
    output_dir: &Path,
    cancel: &Cancellation,
    tx: &mpsc::Sender<WorkerEvent>,
    wake: &eframe::egui::Context,
) -> Result<(), String> {
    run_yt_dlp_download_with_options(
        id,
        url,
        format,
        output_dir,
        DownloadOptions::default(),
        None,
        cancel,
        tx,
        wake,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_yt_dlp_download_with_options(
    id: u64,
    url: &str,
    format: DownloadFormat,
    output_dir: &Path,
    options: DownloadOptions,
    resumable_staging: Option<(PathBuf, Arc<AtomicBool>)>,
    cancel: &Cancellation,
    tx: &mpsc::Sender<WorkerEvent>,
    wake: &eframe::egui::Context,
) -> Result<(), String> {
    if cancel.cancelled() {
        return Err("Download cancelado.".to_owned());
    }
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Não foi possível criar a pasta de downloads: {e}"))?;

    let job = DownloadJob {
        id,
        url,
        format,
        output_dir,
        options,
        cancel,
        tx,
        wake,
    };
    let ffmpeg_path = find_ffmpeg();
    let ffmpeg_available = ffmpeg_path.is_some();
    let staging = match resumable_staging {
        Some((path, pause_flag)) => StagingDirGuard::open_or_create_at(path, Some(pause_flag))?,
        None => StagingDirGuard::create(output_dir, id)?,
    };

    if ffmpeg_available || format == DownloadFormat::AudioM4a || format == DownloadFormat::AudioOpus
    {
        let primary_args = format.yt_dlp_args_with_ffmpeg(ffmpeg_available);
        let output_template = format.output_template();
        let run_out = match run_yt_dlp_once(
            &job,
            &YtDlpRunOptions {
                format_args: &primary_args,
                extractor_args: None,
                target_dir: &staging.path,
                output_template: &output_template,
                print_after_move: true,
                progress_mode: ProgressMode::Direct,
                ffmpeg_path: ffmpeg_path.as_deref(),
            },
        ) {
            Ok(out) => out,
            Err((err, stderr_lines)) => {
                if cancel.cancelled() || !is_format_unavailable_error(&stderr_lines) {
                    return Err(err);
                }
                if !options.continue_partial {
                    staging.reset()?;
                }
                let fallback_args = format.fallback_args(ffmpeg_available);
                run_yt_dlp_once(
                    &job,
                    &YtDlpRunOptions {
                        format_args: &fallback_args,
                        extractor_args: Some("youtube:player_client=default,android_vr"),
                        target_dir: &staging.path,
                        output_template: &output_template,
                        print_after_move: true,
                        progress_mode: ProgressMode::Direct,
                        ffmpeg_path: ffmpeg_path.as_deref(),
                    },
                )
                .map_err(|(fallback_err, _)| fallback_err)?
            }
        };
        let staged_file = resolve_staged_output_file(
            &staging.path,
            &run_out.streams,
            run_out.last_output_path.as_deref(),
        )
        .ok_or_else(|| "yt-dlp não produziu arquivos de saída.".to_owned())?;
        let final_path = move_staged_output_to_final(&staged_file, output_dir, format)?;
        let _ = tx.send(WorkerEvent::Progress {
            id,
            percent: 100.0,
            speed: None,
            eta: None,
        });
        let _ = tx.send(WorkerEvent::OutputPath {
            id,
            path: final_path,
        });
        wake.request_repaint();
        return Ok(());
    }

    let primary_args = format.yt_dlp_args_with_ffmpeg(false);
    let streams = match run_yt_dlp_once(
        &job,
        &YtDlpRunOptions {
            format_args: &primary_args,
            extractor_args: None,
            target_dir: &staging.path,
            output_template: "%(title).120B [%(id)s]__ft_f%(format_id)s.%(ext)s",
            print_after_move: true,
            progress_mode: ProgressMode::StagedPostprocess,
            ffmpeg_path: None,
        },
    ) {
        Ok(out) => out.streams,
        Err((err, stderr_lines)) => {
            if cancel.cancelled() || !is_format_unavailable_error(&stderr_lines) {
                return Err(err);
            }
            if !options.continue_partial {
                staging.reset()?;
            }
            let fallback_args = format.fallback_args(false);
            run_yt_dlp_once(
                &job,
                &YtDlpRunOptions {
                    format_args: &fallback_args,
                    extractor_args: Some("youtube:player_client=default,android_vr"),
                    target_dir: &staging.path,
                    output_template: "%(title).120B [%(id)s]__ft_f%(format_id)s.%(ext)s",
                    print_after_move: true,
                    progress_mode: ProgressMode::StagedPostprocess,
                    ffmpeg_path: None,
                },
            )
            .map(|out| out.streams)
            .map_err(|(fallback_err, _)| fallback_err)?
        }
    };

    if streams.is_empty() {
        return Err("yt-dlp não produziu arquivos de saída.".to_owned());
    }

    if !format.is_audio() {
        let video_stream = streams
            .iter()
            .find(|s| s.has_video() && !s.has_audio())
            .or_else(|| streams.iter().find(|s| s.has_video()));
        let audio_stream = video_stream.and_then(|v| {
            streams
                .iter()
                .find(|s| !std::ptr::eq(*s, v) && s.has_audio() && !s.has_video())
                .or_else(|| {
                    streams
                        .iter()
                        .find(|s| !std::ptr::eq(*s, v) && s.has_audio())
                })
        });

        if let (Some(video), Some(audio)) = (video_stream, audio_stream) {
            if video.is_iso_bmff() && audio.is_iso_bmff() {
                let base = extract_sanitized_base_name(&video.path)
                    .or_else(|| extract_sanitized_base_name(&audio.path))
                    .unwrap_or_else(|| "download".to_owned());
                let merged_temp = staging.path.join(format!("{base}.__merged.mp4"));
                let mut mux_throttle = ProgressRepaintThrottle::default();
                let mux_result =
                    mp4_mux::mux_mp4(&video.path, &audio.path, &merged_temp, cancel, |p| {
                        let mapped = (95.0 + (p * 0.05)).clamp(95.0, 100.0);
                        if mux_throttle.should_emit(mapped, Instant::now()) {
                            let _ = tx.send(WorkerEvent::Progress {
                                id,
                                percent: mapped,
                                speed: None,
                                eta: None,
                            });
                            wake.request_repaint();
                        }
                    });
                match mux_result {
                    Ok(()) => {
                        let final_path = build_final_output_path(output_dir, &base, format, "mp4");
                        replace_file_atomically(&merged_temp, &final_path)?;
                        let _ = tx.send(WorkerEvent::Progress {
                            id,
                            percent: 100.0,
                            speed: None,
                            eta: None,
                        });
                        let _ = tx.send(WorkerEvent::OutputPath {
                            id,
                            path: final_path,
                        });
                        wake.request_repaint();
                        return Ok(());
                    }
                    Err(mux_err) => {
                        if cancel.cancelled() {
                            return Err(mux_err);
                        }
                        return run_premuxed_staged_fallback(&job, &staging).map_err(|_| mux_err);
                    }
                }
            }
            if cancel.cancelled() {
                return Err("Download cancelado.".to_owned());
            }
            return run_premuxed_staged_fallback(&job, &staging);
        }

        let single = &streams[0];
        return finish_single_staged_stream(&job, single);
    }

    let audio_stream = streams
        .iter()
        .find(|s| s.has_audio())
        .unwrap_or(&streams[0]);
    let base =
        extract_sanitized_base_name(&audio_stream.path).unwrap_or_else(|| "download".to_owned());
    let (target_of, target_oac, target_ext) = match format {
        DownloadFormat::AudioMp3 => ("mp3", "libmp3lame", "mp3"),
        DownloadFormat::AudioOpus => {
            if is_native_opus_stream(&audio_stream.ext, &audio_stream.acodec) {
                return finish_single_staged_stream(&job, audio_stream);
            }
            ("opus", "libopus", "opus")
        }
        _ => return finish_single_staged_stream(&job, audio_stream),
    };

    if audio_stream.ext.eq_ignore_ascii_case(target_ext) {
        return finish_single_staged_stream(&job, audio_stream);
    }

    let _ = tx.send(WorkerEvent::Progress {
        id,
        percent: 90.0,
        speed: None,
        eta: None,
    });
    wake.request_repaint();

    let temp_out = staging
        .path
        .join(format!("{base}.__transcoded.{target_ext}"));
    match transcode_audio_with_libmpv(&audio_stream.path, &temp_out, target_of, target_oac, cancel)
    {
        Ok(()) => {
            let final_path = build_final_output_path(output_dir, &base, format, target_ext);
            replace_file_atomically(&temp_out, &final_path)?;
            let _ = tx.send(WorkerEvent::Progress {
                id,
                percent: 100.0,
                speed: None,
                eta: None,
            });
            let _ = tx.send(WorkerEvent::OutputPath {
                id,
                path: final_path,
            });
            wake.request_repaint();
            Ok(())
        }
        Err(err) => {
            if cancel.cancelled() {
                return Err(err);
            }
            // Fall back to saving the native downloaded audio stream if libmpv encoding is unavailable.
            finish_single_staged_stream(&job, audio_stream)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yt_dlp_progress_lines() {
        let line = "[download]  42.7% of ~  120.50MiB at    3.42MiB/s ETA 00:21 (frag 12/28)";
        let parsed = parse_yt_dlp_progress_line(line).unwrap();
        assert!((parsed.0 - 42.7).abs() < 0.01);
        assert_eq!(parsed.1.as_deref(), Some("3.42MiB/s"));
        assert_eq!(parsed.2.as_deref(), Some("00:21"));
        assert!(parse_yt_dlp_progress_line("[info] Downloading 1 format").is_none());
    }

    #[test]
    fn parses_yt_dlp_output_paths_and_supports_retry_and_remove() {
        assert_eq!(
            parse_yt_dlp_output_path_line("[download] Destination: C:\\Temp\\video.f137.mp4"),
            Some(PathBuf::from("C:\\Temp\\video.f137.mp4"))
        );
        assert_eq!(
            parse_yt_dlp_output_path_line(
                "[Merger] Merging formats into \"C:\\Temp\\video [abc].mp4\""
            ),
            Some(PathBuf::from("C:\\Temp\\video [abc].mp4"))
        );
        assert_eq!(
            parse_yt_dlp_output_path_line("[ExtractAudio] Destination: C:\\Temp\\audio [abc].mp3"),
            Some(PathBuf::from("C:\\Temp\\audio [abc].mp3"))
        );
        assert_eq!(
            parse_yt_dlp_output_path_line(
                "[download] C:\\Temp\\already.mp4 has already been downloaded"
            ),
            Some(PathBuf::from("C:\\Temp\\already.mp4"))
        );

        let ctx = eframe::egui::Context::default();
        let video = Video {
            title: "Test".into(),
            channel: "Chan".into(),
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            thumbnail: String::new(),
            duration: "1:00".into(),
        };
        let mut manager = DownloadManager {
            items: vec![
                DownloadItem {
                    id: 1,
                    video: video.clone(),
                    format: DownloadFormat::Video720p,
                    output_dir: PathBuf::from("C:\\Temp"),
                    status: DownloadStatus::Failed("erro".into()),
                },
                DownloadItem {
                    id: 2,
                    video,
                    format: DownloadFormat::AudioMp3,
                    output_dir: PathBuf::from("C:\\Temp"),
                    status: DownloadStatus::Completed {
                        output_path: Some(PathBuf::from("C:\\Temp\\out.mp3")),
                    },
                },
            ],
            next_id: 2,
            ..Default::default()
        };
        assert!(!manager.retry(2, &ctx));
        assert!(manager.remove(2));
        assert_eq!(manager.items.len(), 1);
        assert!(manager.retry(1, &ctx));
        assert!(!manager.remove(1));
        manager.cancel_all();
        assert!(manager.remove(1));
        assert!(manager.items.is_empty());
    }

    #[test]
    fn fallback_download_args_avoid_ffmpeg_postprocessing() {
        let video_args = DownloadFormat::Video1080p.yt_dlp_args_with_ffmpeg(false);
        assert!(!video_args.contains(&"--merge-output-format"));
        assert!(
            video_args
                .iter()
                .any(|arg| arg.contains("best[height<=1080]"))
        );

        let audio_args = DownloadFormat::AudioMp3.yt_dlp_args_with_ffmpeg(false);
        assert!(!audio_args.contains(&"-x"));
        assert!(audio_args.iter().any(|arg| arg.contains("bestaudio")));
    }

    #[test]
    fn parses_printed_download_stream_lines_with_pipes_and_unicode() {
        let line = r"FASTTUBE_DL:avc1.64002A|none|mp4|D:\Downloads\GTA 6 - #11_ Essa missão foi uma PORCARIA 🐷🐽🐖｜Gameplay [TedrETAJeNM]__ft_f299.mp4";
        let parsed = parse_yt_dlp_printed_file_line(line).unwrap();
        assert_eq!(parsed.vcodec, "avc1.64002A");
        assert_eq!(parsed.acodec, "none");
        assert_eq!(parsed.ext, "mp4");
        assert!(parsed.has_video());
        assert!(!parsed.has_audio());
        assert!(parsed.is_iso_bmff());
        assert_eq!(
            parsed.path,
            PathBuf::from(
                r"D:\Downloads\GTA 6 - #11_ Essa missão foi uma PORCARIA 🐷🐽🐖｜Gameplay [TedrETAJeNM]__ft_f299.mp4"
            )
        );

        let audio_line =
            r"FASTTUBE_DL:none|mp4a.40.2|m4a|D:\Downloads\Track [TedrETAJeNM]__ft_f140-1.m4a";
        let audio = parse_yt_dlp_printed_file_line(audio_line).unwrap();
        assert!(!audio.has_video());
        assert!(audio.has_audio());
        assert!(audio.is_iso_bmff());

        assert!(parse_yt_dlp_printed_file_line("FASTTUBE_DL:a|b|c|").is_none());
        assert!(parse_yt_dlp_printed_file_line("[download] 100%").is_none());
    }

    #[test]
    fn extracts_sanitized_base_name_from_staged_filename() {
        assert_eq!(
            extract_sanitized_base_name(Path::new(
                r"C:\Temp\.fasttube-dl-1-1\GTA 6 - #11_ PORCARIA 🐷🐽🐖 [TedrETAJeNM]__ft_f299.mp4"
            ))
            .as_deref(),
            Some("GTA 6 - #11_ PORCARIA 🐷🐽🐖 [TedrETAJeNM]")
        );
        assert_eq!(
            extract_sanitized_base_name(Path::new(r"C:\Temp\Title [TedrETAJeNM]__ft_f140-1.m4a"))
                .as_deref(),
            Some("Title [TedrETAJeNM]")
        );
        assert_eq!(
            extract_sanitized_base_name(Path::new(r"C:\Temp\plain_name.mp4")).as_deref(),
            Some("plain_name")
        );
    }

    #[test]
    fn format_args_include_dash_pairs_without_ffmpeg_and_universal_fallbacks() {
        for fmt in DownloadFormat::ALL {
            let no_ff = fmt.yt_dlp_args_with_ffmpeg(false);
            let with_ff = fmt.yt_dlp_args_with_ffmpeg(true);
            let sel_no_ff = no_ff[1];
            let sel_with_ff = with_ff[1];
            if !fmt.is_audio() {
                assert!(sel_no_ff.contains("bestvideo"));
                assert!(sel_no_ff.contains("bestaudio"));
                assert!(sel_no_ff.ends_with("/best"));
                assert!(sel_with_ff.contains("bv*"));
                assert!(sel_with_ff.ends_with("/b"));
            } else {
                assert!(sel_no_ff.contains("bestaudio"));
                assert!(sel_no_ff.ends_with("/best"));
                assert!(sel_with_ff.contains("ba"));
                assert!(sel_with_ff.ends_with("/b"));
            }
        }
    }

    #[test]
    fn detects_requested_format_unavailable_errors() {
        assert!(is_format_unavailable_error(&[
            "WARNING: something".to_owned(),
            "ERROR: [youtube] TedrETAJeNM: Requested format is not available. Use --list-formats for a list of available formats".to_owned(),
        ]));
        assert!(is_format_unavailable_error(&[
            "WARNING: Only images are available for download. use --list-formats to see them"
                .to_owned(),
        ]));
        assert!(!is_format_unavailable_error(&[
            "ERROR: HTTP Error 403: Forbidden".to_owned(),
        ]));
    }

    #[test]
    fn download_output_templates_and_resolved_paths_are_distinct_across_qualities() {
        let mut tags = std::collections::HashSet::new();
        let mut templates = std::collections::HashSet::new();
        for fmt in DownloadFormat::ALL {
            let tag = fmt.filename_tag();
            assert!(!tag.is_empty());
            assert!(tags.insert(tag), "duplicate filename tag: {tag}");
            let template = fmt.output_template();
            assert!(template.contains("%(title).120B [%(id)s]"));
            assert!(template.contains(&format!("[{tag}]")));
            assert!(
                templates.insert(template.clone()),
                "duplicate output template: {template}"
            );
        }

        let dir = Path::new(r"C:\Downloads");
        let base = "Sample Video [dQw4w9WgXcQ]";
        let video_formats = [
            DownloadFormat::VideoMp4Best,
            DownloadFormat::Video1080p,
            DownloadFormat::Video720p,
            DownloadFormat::Video480p,
        ];
        let mut video_paths = std::collections::HashSet::new();
        for fmt in video_formats {
            let path = build_final_output_path(dir, base, fmt, "mp4");
            assert!(
                video_paths.insert(path.clone()),
                "video quality collision for {fmt:?}: {}",
                path.display()
            );
        }

        assert_eq!(
            build_final_output_path(dir, base, DownloadFormat::Video1080p, "mp4"),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [1080p].mp4")
        );
        assert_eq!(
            build_final_output_path(dir, base, DownloadFormat::Video480p, "mp4"),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [480p].mp4")
        );
        assert_eq!(
            build_final_output_path(dir, base, DownloadFormat::VideoMp4Best, "mp4"),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [best].mp4")
        );
        assert_eq!(
            build_final_output_path(dir, base, DownloadFormat::AudioMp3, "mp3"),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [audio-mp3].mp3")
        );
        assert_eq!(
            build_final_output_path(dir, base, DownloadFormat::AudioOpus, "opus"),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [audio-opus].opus")
        );
        assert_eq!(
            build_final_output_path(dir, base, DownloadFormat::AudioM4a, "m4a"),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [audio-m4a].m4a")
        );

        // Even when an audio profile falls back to a native .m4a stream, paths stay distinct.
        assert_ne!(
            build_final_output_path(dir, base, DownloadFormat::AudioMp3, "m4a"),
            build_final_output_path(dir, base, DownloadFormat::AudioM4a, "m4a")
        );

        // Idempotency: an already-tagged base is not tagged twice.
        assert_eq!(
            build_final_output_path(
                dir,
                "Sample Video [dQw4w9WgXcQ] [1080p]",
                DownloadFormat::Video1080p,
                "mp4"
            ),
            PathBuf::from(r"C:\Downloads\Sample Video [dQw4w9WgXcQ] [1080p].mp4")
        );
    }

    #[test]
    fn progress_repaint_throttle_limits_frequency_and_passes_large_deltas() {
        let mut throttle = ProgressRepaintThrottle::default();
        let t0 = Instant::now();
        assert!(throttle.should_emit(0.0, t0));
        assert!(!throttle.should_emit(0.3, t0 + Duration::from_millis(20)));
        assert!(!throttle.should_emit(0.9, t0 + Duration::from_millis(80)));
        assert!(throttle.should_emit(1.0, t0 + Duration::from_millis(85)));
        assert!(!throttle.should_emit(1.2, t0 + Duration::from_millis(120)));
        assert!(throttle.should_emit(1.2, t0 + Duration::from_millis(186)));
        // Reaching 100% always passes immediately even for a tiny delta.
        assert!(throttle.should_emit(99.7, t0 + Duration::from_millis(290)));
        assert!(throttle.should_emit(100.0, t0 + Duration::from_millis(295)));
    }

    #[test]
    fn staging_dir_guard_moves_output_atomically_and_cleans_up_on_drop() {
        let root_guard = TempDirGuard(std::env::temp_dir().join(format!(
            "fasttube-dl-staging-test-{}-{}",
            std::process::id(),
            STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
        )));
        let out_dir = &root_guard.0;
        std::fs::create_dir_all(out_dir).unwrap();

        let staging_path;
        let final_path;
        {
            let staging = StagingDirGuard::create(out_dir, 42).unwrap();
            staging_path = staging.path.clone();
            assert!(staging_path.is_dir());
            assert_eq!(staging_path.parent(), Some(out_dir.as_path()));

            // Partial files must be ignored in favor of the completed merged output.
            std::fs::write(staging_path.join("video.f299.mp4.part"), b"partial").unwrap();
            let merged = staging_path.join("Clip [abc12345678] [1080p].mp4");
            std::fs::write(&merged, b"completed-mp4-bytes").unwrap();

            let resolved = resolve_staged_output_file(&staging_path, &[], None).unwrap();
            assert_eq!(resolved, merged);

            final_path =
                move_staged_output_to_final(&resolved, out_dir, DownloadFormat::Video1080p)
                    .unwrap();
            assert_eq!(final_path, out_dir.join("Clip [abc12345678] [1080p].mp4"));
            assert_eq!(std::fs::read(&final_path).unwrap(), b"completed-mp4-bytes");
        }

        // Guard drop removes the staging dir and all leftover temp files.
        assert!(!staging_path.exists());
        assert!(final_path.is_file());
    }

    #[test]
    fn ffmpeg_cached_probe_state_is_non_blocking_and_consistent() {
        let before = has_ffmpeg_cached();
        let actual = has_ffmpeg();
        let after = has_ffmpeg_cached();
        assert_eq!(after, Some(actual));
        if let Some(cached_before) = before {
            assert_eq!(cached_before, actual);
        }
        // Once cached, trigger_ffmpeg_probe is an immediate no-op.
        trigger_ffmpeg_probe(eframe::egui::Context::default());
        assert_eq!(has_ffmpeg_cached(), Some(actual));
    }

    #[cfg(windows)]
    #[test]
    fn cancel_watcher_terminates_stalled_child_immediately() {
        use std::os::windows::process::CommandExt;

        let job = ProcessJobObject::new().map(Arc::new);
        let mut cmd = std::process::Command::new("ping");
        cmd.args(["-n", "10", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        let child = cmd.spawn().expect("spawn ping");
        if let Some(ref pj) = job {
            assert!(pj.assign_child(&child));
        }

        let cancel = Cancellation::default();
        let child = Arc::new(Mutex::new(child));
        let _watcher = spawn_cancel_watcher(cancel.clone(), job, Arc::clone(&child));

        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(80));
        cancel.cancel();

        let status = loop {
            let step = child.lock().unwrap().try_wait().unwrap();
            if let Some(status) = step {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "cancel watcher did not terminate stalled child promptly"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(!status.success());
    }

    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct MediaProbe {
        video_codec: Option<String>,
        audio_codec: Option<String>,
        width: u32,
        height: u32,
        duration_secs: f64,
    }

    fn probe_media_with_libmpv(path: &Path, test_seek: bool) -> MediaProbe {
        type MpvGetPropertyStringFn =
            unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
        type MpvFreeFn = unsafe extern "C" fn(*mut c_void);

        let lib = load_mpv_library().expect("libmpv-2.dll must load for live probe");
        let (create, set_opt, init, command, wait_event, terminate, get_prop_str, free_fn) = unsafe {
            (
                *lib.get::<MpvCreateFn>(b"mpv_create\0").unwrap(),
                *lib.get::<MpvSetOptionStringFn>(b"mpv_set_option_string\0")
                    .unwrap(),
                *lib.get::<MpvInitializeFn>(b"mpv_initialize\0").unwrap(),
                *lib.get::<MpvCommandFn>(b"mpv_command\0").unwrap(),
                *lib.get::<MpvWaitEventFn>(b"mpv_wait_event\0").unwrap(),
                *lib.get::<MpvTerminateDestroyFn>(b"mpv_terminate_destroy\0")
                    .unwrap(),
                *lib.get::<MpvGetPropertyStringFn>(b"mpv_get_property_string\0")
                    .unwrap(),
                *lib.get::<MpvFreeFn>(b"mpv_free\0").unwrap(),
            )
        };

        let handle = unsafe { create() };
        assert!(!handle.is_null());
        let guard = MpvHandleGuard { handle, terminate };

        for (k, v) in [
            ("terminal", "no"),
            ("config", "no"),
            ("vo", "null"),
            ("ao", "null"),
            ("pause", "yes"),
        ] {
            let kc = CString::new(k).unwrap();
            let vc = CString::new(v).unwrap();
            assert!(unsafe { set_opt(guard.handle, kc.as_ptr(), vc.as_ptr()) } >= 0);
        }
        assert!(unsafe { init(guard.handle) } >= 0);

        let cmd_load = CString::new("loadfile").unwrap();
        let path_c = CString::new(path.to_str().unwrap()).unwrap();
        let mode_c = CString::new("replace").unwrap();
        let argv = [
            cmd_load.as_ptr(),
            path_c.as_ptr(),
            mode_c.as_ptr(),
            std::ptr::null(),
        ];
        assert!(unsafe { command(guard.handle, argv.as_ptr()) } >= 0);

        let mut loaded = false;
        for _ in 0..200 {
            let ev_ptr = unsafe { wait_event(guard.handle, 0.05) };
            if ev_ptr.is_null() {
                continue;
            }
            let ev = unsafe { &*ev_ptr };
            if ev.event_id == 8 {
                loaded = true;
                break;
            }
            if ev.event_id == 7 && !ev.data.is_null() {
                let end = unsafe { &*(ev.data as *const MpvEndFile) };
                panic!(
                    "libmpv failed to load '{}': reason={} error={}",
                    path.display(),
                    end.reason,
                    end.error
                );
            }
        }
        assert!(loaded, "timed out waiting for libmpv FILE_LOADED");

        let read_prop = |name: &str| -> Option<String> {
            let nc = CString::new(name).unwrap();
            let ptr = unsafe { get_prop_str(guard.handle, nc.as_ptr()) };
            if ptr.is_null() {
                return None;
            }
            let val = unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .trim()
                .to_owned();
            unsafe { free_fn(ptr.cast()) };
            (!val.is_empty()).then_some(val)
        };

        let video_codec = read_prop("video-codec");
        let audio_codec = read_prop("audio-codec");
        let width = read_prop("width")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let height = read_prop("height")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let duration_secs = read_prop("duration")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        if test_seek {
            let cmd_seek = CString::new("seek").unwrap();
            let sec_c = CString::new("100").unwrap();
            let abs_c = CString::new("absolute").unwrap();
            let seek_argv = [
                cmd_seek.as_ptr(),
                sec_c.as_ptr(),
                abs_c.as_ptr(),
                std::ptr::null(),
            ];
            let seek_rc = unsafe { command(guard.handle, seek_argv.as_ptr()) };
            assert!(
                seek_rc >= 0,
                "seek failed on '{}': rc={seek_rc}",
                path.display()
            );
        }

        drop(guard);
        MediaProbe {
            video_codec,
            audio_codec,
            width,
            height,
            duration_secs,
        }
    }

    #[test]
    #[ignore]
    fn live_downloads_and_muxes_real_youtube_video_and_mp3() {
        let dir_guard = TempDirGuard(
            std::env::temp_dir().join(format!("fasttube-live-dl-{}", std::process::id())),
        );
        let dir = &dir_guard.0;
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();

        let ctx = eframe::egui::Context::default();
        let cancel = Cancellation::default();
        let (tx, rx) = mpsc::channel();

        // 1. Live 480p MP4 download + pure-Rust mp4_mux
        run_yt_dlp_download(
            1,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            DownloadFormat::Video480p,
            dir,
            &cancel,
            &tx,
            &ctx,
        )
        .unwrap();

        let mut mp4_out = None;
        while let Ok(ev) = rx.try_recv() {
            if let WorkerEvent::OutputPath { path, .. } = ev {
                mp4_out = Some(path);
            }
        }
        let mp4_path = mp4_out.expect("expected OutputPath for MP4 download");
        assert!(mp4_path.exists());
        assert_eq!(mp4_path.extension().and_then(|e| e.to_str()), Some("mp4"));
        let mp4_size = std::fs::metadata(&mp4_path).unwrap().len();
        assert!(mp4_size > 1_000_000, "unexpectedly small mp4: {mp4_size}");
        let mp4_probe = probe_media_with_libmpv(&mp4_path, true);
        assert!(
            mp4_probe.video_codec.is_some(),
            "muxed MP4 missing video track"
        );
        assert!(
            mp4_probe.audio_codec.is_some(),
            "muxed MP4 missing audio track"
        );
        assert!(mp4_probe.width > 0 && mp4_probe.height > 0);
        assert!(
            mp4_probe.duration_secs > 180.0,
            "unexpected MP4 duration: {}",
            mp4_probe.duration_secs
        );

        // 2. Live MP3 download + libmpv conversion
        run_yt_dlp_download(
            2,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            DownloadFormat::AudioMp3,
            dir,
            &cancel,
            &tx,
            &ctx,
        )
        .unwrap();

        let mut mp3_out = None;
        while let Ok(ev) = rx.try_recv() {
            if let WorkerEvent::OutputPath { path, .. } = ev {
                mp3_out = Some(path);
            }
        }
        let mp3_path = mp3_out.expect("expected OutputPath for MP3 download");
        assert!(mp3_path.exists());
        assert_eq!(mp3_path.extension().and_then(|e| e.to_str()), Some("mp3"));
        let mp3_size = std::fs::metadata(&mp3_path).unwrap().len();
        assert!(mp3_size > 500_000, "unexpectedly small mp3: {mp3_size}");
        let mp3_probe = probe_media_with_libmpv(&mp3_path, false);
        assert!(mp3_probe.audio_codec.is_some(), "MP3 missing audio track");
        assert!(
            mp3_probe.duration_secs > 180.0,
            "unexpected MP3 duration: {}",
            mp3_probe.duration_secs
        );

        // 3. Live Opus download + libmpv conversion
        run_yt_dlp_download(
            3,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            DownloadFormat::AudioOpus,
            dir,
            &cancel,
            &tx,
            &ctx,
        )
        .unwrap();

        let mut opus_out = None;
        while let Ok(ev) = rx.try_recv() {
            if let WorkerEvent::OutputPath { path, .. } = ev {
                opus_out = Some(path);
            }
        }
        let opus_path = opus_out.expect("expected OutputPath for Opus download");
        assert!(opus_path.exists());
        assert_eq!(opus_path.extension().and_then(|e| e.to_str()), Some("opus"));
        let opus_size = std::fs::metadata(&opus_path).unwrap().len();
        assert!(opus_size > 500_000, "unexpectedly small opus: {opus_size}");
        let opus_probe = probe_media_with_libmpv(&opus_path, false);
        assert!(opus_probe.audio_codec.is_some(), "Opus missing audio track");
        assert!(
            opus_probe.duration_secs > 180.0,
            "unexpected Opus duration: {}",
            opus_probe.duration_secs
        );
    }

    #[test]
    fn persisted_download_records_round_trip_and_preserve_corrupt_file() {
        let dir_guard = TempDirGuard(std::env::temp_dir().join(format!(
            "fasttube-dl-persist-test-{}-{}",
            std::process::id(),
            STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
        )));
        let dir = &dir_guard.0;
        std::fs::create_dir_all(dir).unwrap();
        let json_path = dir.join("downloads.json");

        assert!(load_download_records_from(&json_path).unwrap().is_empty());

        let records = vec![
            PersistedDownloadRecord {
                id: 1,
                video_id: Some("dQw4w9WgXcQ".into()),
                title: "Completed Track".into(),
                channel: "Artist".into(),
                url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
                thumbnail_url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into(),
                format: DownloadFormat::AudioOpus,
                status: PersistedDownloadStatus::Completed,
                output_path: Some(dir.join("Completed Track [dQw4w9WgXcQ] [audio-opus].webm")),
                progress_percent: 100.0,
                file_size_bytes: Some(4_200_000),
                updated_at_unix: 1_700_000_000,
            },
            PersistedDownloadRecord {
                id: 2,
                video_id: Some("9bZkp7q19f0".into()),
                title: "Paused Video".into(),
                channel: "Creator".into(),
                url: "https://www.youtube.com/watch?v=9bZkp7q19f0".into(),
                thumbnail_url: String::new(),
                format: DownloadFormat::Video1080p,
                status: PersistedDownloadStatus::Paused,
                output_path: None,
                progress_percent: 47.5,
                file_size_bytes: None,
                updated_at_unix: 1_700_000_120,
            },
        ];

        save_download_records_to(&json_path, &records).unwrap();
        let loaded = load_download_records_from(&json_path).unwrap();
        assert_eq!(loaded, records);

        let mut manager = DownloadManager::default();
        manager.restore_from_records(loaded, dir);
        assert_eq!(manager.items.len(), 2);
        assert!(matches!(
            manager.items[0].status,
            DownloadStatus::Completed { .. }
        ));
        assert_eq!(manager.items[1].status, DownloadStatus::Paused);
        assert!((manager.paused_progress(2) - 47.5).abs() < 0.01);

        // Corrupt file must remain untouched on disk and surface an error on load/save.
        std::fs::write(&json_path, b"{corrupt-downloads-json").unwrap();
        assert!(load_download_records_from(&json_path).is_err());
        assert!(save_download_records_to(&json_path, &records).is_err());
        assert_eq!(
            std::fs::read(&json_path).unwrap(),
            b"{corrupt-downloads-json"
        );
    }

    #[test]
    fn audio_opus_avoids_ffmpeg_reencode_and_build_embed_args_maps_flags() {
        let opus_with_ff = DownloadFormat::AudioOpus.yt_dlp_args_with_ffmpeg(true);
        let opus_no_ff = DownloadFormat::AudioOpus.yt_dlp_args_with_ffmpeg(false);
        assert!(!opus_with_ff.contains(&"-x"));
        assert!(!opus_with_ff.contains(&"--audio-format"));
        assert!(opus_with_ff[1].contains("bestaudio[acodec*=opus]"));
        assert!(opus_no_ff[1].contains("bestaudio[acodec*=opus]"));
        assert!(is_native_opus_stream("webm", "opus"));
        assert!(is_native_opus_stream("opus", "none"));
        assert!(!is_native_opus_stream("m4a", "mp4a.40.2"));

        let opts = DownloadOptions {
            embed_subs: true,
            embed_chapters: true,
            embed_thumbnail: true,
            continue_partial: true,
        };
        let args = build_embed_args(&opts);
        assert_eq!(
            args,
            vec![
                "--continue".to_owned(),
                "--embed-subs".to_owned(),
                "--embed-chapters".to_owned(),
                "--embed-thumbnail".to_owned(),
            ]
        );
        assert!(build_embed_args(&DownloadOptions::default()).is_empty());
    }

    #[test]
    fn pause_preserves_staging_part_file_and_batch_playlist_helpers_deduplicate() {
        let dir_guard = TempDirGuard(std::env::temp_dir().join(format!(
            "fasttube-dl-pause-test-{}-{}",
            std::process::id(),
            STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
        )));
        let out_dir = &dir_guard.0;
        std::fs::create_dir_all(out_dir).unwrap();

        let staging_path = out_dir.join(".fasttube-dl-paused-1");
        let part_file = staging_path.join("video.f137.mp4.part");
        let pause_flag = Arc::new(AtomicBool::new(true));
        {
            let guard = StagingDirGuard::open_or_create_at(
                staging_path.clone(),
                Some(Arc::clone(&pause_flag)),
            )
            .unwrap();
            std::fs::write(&part_file, b"partial-bytes-preserved").unwrap();
            assert_eq!(guard.path, staging_path);
        }
        // When pause_flag is true, Drop must preserve the staging directory and .part file.
        assert!(part_file.is_file());
        assert_eq!(
            std::fs::read(&part_file).unwrap(),
            b"partial-bytes-preserved"
        );

        // When pause_flag is cleared and guard drops, staging directory is cleaned up.
        pause_flag.store(false, Ordering::Release);
        {
            let _guard =
                StagingDirGuard::open_or_create_at(staging_path.clone(), Some(pause_flag)).unwrap();
        }
        assert!(!staging_path.exists());

        assert!(can_start_another_download(
            0,
            DEFAULT_MAX_CONCURRENT_DOWNLOADS
        ));
        assert!(can_start_another_download(
            1,
            DEFAULT_MAX_CONCURRENT_DOWNLOADS
        ));
        assert!(!can_start_another_download(
            2,
            DEFAULT_MAX_CONCURRENT_DOWNLOADS
        ));

        let v1 = Video {
            title: "One".into(),
            channel: "Ch".into(),
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            thumbnail: String::new(),
            duration: "3:00".into(),
        };
        let v1_dup = Video {
            title: "One Short URL".into(),
            channel: "Ch".into(),
            url: "https://youtu.be/dQw4w9WgXcQ".into(),
            thumbnail: String::new(),
            duration: "3:00".into(),
        };
        let v2 = Video {
            title: "Two".into(),
            channel: "Ch".into(),
            url: "https://www.youtube.com/watch?v=9bZkp7q19f0".into(),
            thumbnail: String::new(),
            duration: "4:00".into(),
        };
        let opts = DownloadOptions {
            embed_subs: true,
            ..Default::default()
        };
        let batch = build_batch_playlist_requests(
            &[v1, v1_dup, v2],
            DownloadFormat::Video720p,
            out_dir,
            opts,
        );
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].video.title, "One");
        assert_eq!(batch[1].video.title, "Two");
        assert_eq!(batch[0].format, DownloadFormat::Video720p);
        assert!(batch[0].options.embed_subs);
    }
}
