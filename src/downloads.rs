//! Offline media download manager backed by yt-dlp with progress parsing and cancellation.

#[path = "mp4_mux.rs"]
mod mp4_mux;

use crate::process::Cancellation;
use crate::search::{self, Video};
use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};

const MAX_DOWNLOAD_HISTORY: usize = 50;
static STAGING_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
                    "bestaudio[ext=webm][protocol^=http]/bestaudio[ext=webm]/bestaudio[ext=opus]/bestaudio/best",
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
                "ba[ext=webm][protocol^=http]/ba[ext=webm]/ba/b",
                "-x",
                "--audio-format",
                "opus",
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
                Self::AudioOpus => vec!["-f", "ba/b", "-x", "--audio-format", "opus"],
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

pub fn find_ffmpeg() -> Option<PathBuf> {
    static CACHED: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHED
        .get_or_init(|| {
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
        })
        .clone()
}

pub fn has_ffmpeg() -> bool {
    find_ffmpeg().is_some()
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Running {
        progress_percent: f32,
        speed: Option<String>,
        eta: Option<String>,
    },
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
}

#[derive(Clone, Debug)]
pub struct DownloadItem {
    pub id: u64,
    pub video: Video,
    pub format: DownloadFormat,
    pub output_dir: PathBuf,
    pub status: DownloadStatus,
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

#[derive(Default)]
pub struct DownloadManager {
    pub items: Vec<DownloadItem>,
    next_id: u64,
    active_id: Option<u64>,
    active_cancel: Option<Cancellation>,
    rx: Option<Receiver<WorkerEvent>>,
    pending_output_paths: std::collections::HashMap<u64, PathBuf>,
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

    pub fn has_active(&self) -> bool {
        self.active_id.is_some()
            || self
                .items
                .iter()
                .any(|item| matches!(item.status, DownloadStatus::Queued))
    }

    pub fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| !item.status.is_terminal())
            .count()
    }

    pub fn enqueue(
        &mut self,
        video: Video,
        format: DownloadFormat,
        output_dir: PathBuf,
        ctx: &eframe::egui::Context,
    ) -> u64 {
        if self.items.len() >= MAX_DOWNLOAD_HISTORY
            && let Some(pos) = self.items.iter().position(|item| item.status.is_terminal())
        {
            self.items.remove(pos);
        }
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
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

    pub fn cancel(&mut self, id: u64, ctx: &eframe::egui::Context) {
        if self.active_id == Some(id) {
            if let Some(cancel) = self.active_cancel.take() {
                cancel.cancel();
            }
            if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
                item.status = DownloadStatus::Cancelled;
            }
            self.active_id = None;
            self.rx = None;
            self.pump_next(ctx);
            return;
        }
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id)
            && matches!(item.status, DownloadStatus::Queued)
        {
            item.status = DownloadStatus::Cancelled;
        }
    }

    pub fn cancel_all(&mut self) {
        if let Some(cancel) = self.active_cancel.take() {
            cancel.cancel();
        }
        self.active_id = None;
        self.rx = None;
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
            DownloadStatus::Failed(_) | DownloadStatus::Cancelled
        ) {
            return false;
        }
        item.status = DownloadStatus::Queued;
        self.pending_output_paths.remove(&id);
        self.pump_next(ctx);
        true
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if self.active_id == Some(id) {
            return false;
        }
        let Some(pos) = self
            .items
            .iter()
            .position(|item| item.id == id && item.status.is_terminal())
        else {
            return false;
        };
        self.items.remove(pos);
        self.pending_output_paths.remove(&id);
        true
    }

    pub fn poll(&mut self, ctx: &eframe::egui::Context) -> Option<(u64, Result<(), String>)> {
        let mut completed: Option<(u64, Result<(), String>)> = None;
        let mut disconnected = false;
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok(WorkerEvent::Progress {
                        id,
                        percent,
                        speed,
                        eta,
                    }) => {
                        if self.active_id == Some(id)
                            && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                        {
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
                        if self.active_id == Some(id) {
                            let resolved_path = self.pending_output_paths.remove(&id);
                            if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
                                match &result {
                                    Ok(()) => {
                                        let fallback_dir = item.output_dir.clone();
                                        item.status = DownloadStatus::Completed {
                                            output_path: Some(
                                                resolved_path.unwrap_or(fallback_dir),
                                            ),
                                        };
                                    }
                                    Err(err) => {
                                        item.status = DownloadStatus::Failed(err.clone());
                                    }
                                }
                            }
                            self.active_id = None;
                            self.active_cancel = None;
                            completed = Some((id, result));
                            disconnected = true;
                            break;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.rx = None;
            if let Some(id) = self.active_id.take()
                && let Some(item) = self.items.iter_mut().find(|i| i.id == id)
                && !item.status.is_terminal()
            {
                item.status = DownloadStatus::Failed("Processo de download encerrado.".to_owned());
            }
            self.active_cancel = None;
            self.pump_next(ctx);
        }
        completed
    }

    fn pump_next(&mut self, ctx: &eframe::egui::Context) {
        if self.active_id.is_some() {
            return;
        }
        let Some(next) = self
            .items
            .iter_mut()
            .find(|item| matches!(item.status, DownloadStatus::Queued))
        else {
            return;
        };
        let id = next.id;
        let url = next.video.url.clone();
        let format = next.format;
        let output_dir = next.output_dir.clone();
        next.status = DownloadStatus::Running {
            progress_percent: 0.0,
            speed: None,
            eta: None,
        };

        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();

        self.active_id = Some(id);
        self.active_cancel = Some(cancel);
        self.rx = Some(rx);

        std::thread::spawn(move || {
            let result =
                run_yt_dlp_download(id, &url, format, &output_dir, &worker_cancel, &tx, &wake);
            let _ = tx.send(WorkerEvent::Finished { id, result });
            wake.request_repaint();
        });
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

fn is_format_unavailable_error(stderr_lines: &[String]) -> bool {
    stderr_lines.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("requested format is not available")
            || lower.contains("only images are available for download")
    })
}

struct StagingDirGuard {
    path: PathBuf,
}

impl StagingDirGuard {
    fn create(output_dir: &Path, id: u64) -> Result<Self, String> {
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = output_dir.join(format!(".fasttube-dl-{}-{id}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Não foi possível criar pasta temporária de download: {e}"))?;
        Ok(Self { path })
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
        let _ = std::fs::remove_dir_all(&self.path);
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
    cancel: &'a Cancellation,
    tx: &'a mpsc::Sender<WorkerEvent>,
    wake: &'a eframe::egui::Context,
}

struct YtDlpRunOptions<'a> {
    format_args: &'a [&'a str],
    extractor_args: Option<&'a str>,
    target_dir: &'a Path,
    output_template: &'a str,
    staged: bool,
    ffmpeg_path: Option<&'a Path>,
}

fn run_yt_dlp_once(
    job: &DownloadJob<'_>,
    opts: &YtDlpRunOptions<'_>,
) -> Result<Vec<DownloadedStream>, (String, Vec<String>)> {
    if job.cancel.cancelled() {
        return Err(("Download cancelado.".to_owned(), Vec::new()));
    }
    let mut cmd = search::yt_dlp_command();
    cmd.env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .arg("--newline")
        .arg("--no-playlist")
        .arg("--no-warnings");
    if opts.staged {
        cmd.arg("--no-simulate")
            .arg("--no-quiet")
            .arg("--progress")
            .arg("--print")
            .arg("after_move:FASTTUBE_DL:%(vcodec)s|%(acodec)s|%(ext)s|%(filepath)s");
    } else if let Some(ffmpeg) = opts.ffmpeg_path
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
    cmd.arg("--").arg(job.url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| (format!("Não foi possível iniciar yt-dlp: {e}"), Vec::new()))?;
    let err_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
                .filter(|line| !line.trim().is_empty())
                .collect::<Vec<_>>()
        })
    });

    let mut streams = Vec::new();
    let mut seen_destinations: usize = 0;

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if job.cancel.cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(("Download cancelado.".to_owned(), Vec::new()));
            }
            if opts.staged
                && let Some(stream) = parse_yt_dlp_printed_file_line(&line)
            {
                streams.push(stream);
                continue;
            }
            if line.trim_start().starts_with("[download] Destination: ") {
                seen_destinations += 1;
            }
            if let Some((raw_percent, speed, eta)) = parse_yt_dlp_progress_line(&line) {
                let mapped_percent = if opts.staged {
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
                } else {
                    raw_percent
                };
                let _ = job.tx.send(WorkerEvent::Progress {
                    id: job.id,
                    percent: mapped_percent,
                    speed,
                    eta,
                });
                job.wake.request_repaint();
            } else if !opts.staged
                && let Some(path) = parse_yt_dlp_output_path_line(&line)
            {
                let _ = job.tx.send(WorkerEvent::OutputPath { id: job.id, path });
            }
        }
    }

    let wait_res = child.wait();
    let stderr_lines = err_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let status =
        wait_res.map_err(|e| (format!("Erro aguardando yt-dlp: {e}"), stderr_lines.clone()))?;
    if job.cancel.cancelled() {
        return Err(("Download cancelado.".to_owned(), stderr_lines));
    }
    if status.success() {
        Ok(streams)
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
    let final_path = job.output_dir.join(format!("{base}.{ext}"));
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
    let streams = run_yt_dlp_once(
        job,
        &YtDlpRunOptions {
            format_args: &fallback_args,
            extractor_args: Some("youtube:player_client=default,android_vr"),
            target_dir: &staging.path,
            output_template: "%(title).120B [%(id)s]__ft_f%(format_id)s.%(ext)s",
            staged: true,
            ffmpeg_path: None,
        },
    )
    .map_err(|(err, _)| err)?;
    let Some(first) = streams.first() else {
        return Err("yt-dlp não produziu arquivos de saída na tentativa alternativa.".to_owned());
    };
    finish_single_staged_stream(job, first)
}

fn run_yt_dlp_download(
    id: u64,
    url: &str,
    format: DownloadFormat,
    output_dir: &Path,
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
        cancel,
        tx,
        wake,
    };
    let ffmpeg_path = find_ffmpeg();
    let ffmpeg_available = ffmpeg_path.is_some();

    if ffmpeg_available || format == DownloadFormat::AudioM4a {
        let primary_args = format.yt_dlp_args_with_ffmpeg(ffmpeg_available);
        match run_yt_dlp_once(
            &job,
            &YtDlpRunOptions {
                format_args: &primary_args,
                extractor_args: None,
                target_dir: output_dir,
                output_template: "%(title).120B [%(id)s].%(ext)s",
                staged: false,
                ffmpeg_path: ffmpeg_path.as_deref(),
            },
        ) {
            Ok(_) => return Ok(()),
            Err((err, stderr_lines)) => {
                if cancel.cancelled() || !is_format_unavailable_error(&stderr_lines) {
                    return Err(err);
                }
                let fallback_args = format.fallback_args(ffmpeg_available);
                run_yt_dlp_once(
                    &job,
                    &YtDlpRunOptions {
                        format_args: &fallback_args,
                        extractor_args: Some("youtube:player_client=default,android_vr"),
                        target_dir: output_dir,
                        output_template: "%(title).120B [%(id)s].%(ext)s",
                        staged: false,
                        ffmpeg_path: ffmpeg_path.as_deref(),
                    },
                )
                .map(|_| ())
                .map_err(|(fallback_err, _)| fallback_err)
            }
        }?;
        return Ok(());
    }

    let staging = StagingDirGuard::create(output_dir, id)?;
    let primary_args = format.yt_dlp_args_with_ffmpeg(false);
    let streams = match run_yt_dlp_once(
        &job,
        &YtDlpRunOptions {
            format_args: &primary_args,
            extractor_args: None,
            target_dir: &staging.path,
            output_template: "%(title).120B [%(id)s]__ft_f%(format_id)s.%(ext)s",
            staged: true,
            ffmpeg_path: None,
        },
    ) {
        Ok(streams) => streams,
        Err((err, stderr_lines)) => {
            if cancel.cancelled() || !is_format_unavailable_error(&stderr_lines) {
                return Err(err);
            }
            staging.reset()?;
            let fallback_args = format.fallback_args(false);
            run_yt_dlp_once(
                &job,
                &YtDlpRunOptions {
                    format_args: &fallback_args,
                    extractor_args: Some("youtube:player_client=default,android_vr"),
                    target_dir: &staging.path,
                    output_template: "%(title).120B [%(id)s]__ft_f%(format_id)s.%(ext)s",
                    staged: true,
                    ffmpeg_path: None,
                },
            )
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
                let mut last_mux_pct = 95.0f32;
                let mux_result =
                    mp4_mux::mux_mp4(&video.path, &audio.path, &merged_temp, cancel, |p| {
                        let mapped = (95.0 + (p * 0.05)).clamp(95.0, 100.0);
                        if mapped - last_mux_pct >= 0.5 || mapped >= 100.0 {
                            last_mux_pct = mapped;
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
                        let final_path = output_dir.join(format!("{base}.mp4"));
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
        DownloadFormat::AudioOpus => ("opus", "libopus", "opus"),
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
            let final_path = output_dir.join(format!("{base}.{target_ext}"));
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
}
