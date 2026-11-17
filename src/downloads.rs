//! Offline media download manager backed by yt-dlp with progress parsing and cancellation.

use crate::process::Cancellation;
use crate::search::{self, Video};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver};

const MAX_DOWNLOAD_HISTORY: usize = 50;

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

    pub fn yt_dlp_args(self) -> Vec<&'static str> {
        match self {
            Self::VideoMp4Best => vec![
                "-f",
                "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best",
                "--merge-output-format",
                "mp4",
            ],
            Self::Video1080p => vec![
                "-f",
                "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/bestvideo[height<=1080]+bestaudio/best[height<=1080]",
                "--merge-output-format",
                "mp4",
            ],
            Self::Video720p => vec![
                "-f",
                "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/bestvideo[height<=720]+bestaudio/best[height<=720]",
                "--merge-output-format",
                "mp4",
            ],
            Self::Video480p => vec![
                "-f",
                "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/bestvideo[height<=480]+bestaudio/best[height<=480]",
                "--merge-output-format",
                "mp4",
            ],
            Self::AudioM4a => vec!["-x", "--audio-format", "m4a"],
            Self::AudioOpus => vec!["-x", "--audio-format", "opus"],
            Self::AudioMp3 => vec!["-x", "--audio-format", "mp3"],
        }
    }
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
    let mut cmd = search::yt_dlp_command();
    cmd.arg("--newline")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("-P")
        .arg(output_dir)
        .arg("-o")
        .arg("%(title).120B [%(id)s].%(ext)s");
    for arg in format.yt_dlp_args() {
        cmd.arg(arg);
    }
    cmd.arg("--").arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Não foi possível iniciar yt-dlp: {e}"))?;
    let stdout = child.stdout.take();
    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if cancel.cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Download cancelado.".to_owned());
            }
            if let Some((percent, speed, eta)) = parse_yt_dlp_progress_line(&line) {
                let _ = tx.send(WorkerEvent::Progress {
                    id,
                    percent,
                    speed,
                    eta,
                });
                wake.request_repaint();
            } else if let Some(dest) = line.strip_prefix("[download] Destination: ") {
                let _ = tx.send(WorkerEvent::OutputPath {
                    id,
                    path: PathBuf::from(dest.trim()),
                });
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("Erro aguardando yt-dlp: {e}"))?;
    if cancel.cancelled() {
        return Err("Download cancelado.".to_owned());
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!("yt-dlp terminou com código {:?}.", status.code()))
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
}
