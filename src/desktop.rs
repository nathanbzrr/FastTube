use eframe::egui;
use fastframe_autostart::{Autostart, Start, State as AutostartState};
use fastframe_instance::{Claim, Guard as InstanceGuard, Slot};
use fastframe_now_playing::{
    App as NowPlayingApp, Command as MediaCommand, Controls as MediaControls, NowPlaying,
    Playback as MediaPlayback, State as MediaState, Track as MediaTrack,
};
use fastframe_power::{Conditions as PowerConditions, Event as PowerEvent, KeepAwake, Power};
use fastframe_tray::{Config as TrayConfig, Event as TrayEvent, MenuItem as TrayMenuItem, Tray};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_ARTWORK_BYTES: usize = 4 * 1024 * 1024;
const ARTWORK_TIMEOUT: Duration = Duration::from_secs(8);
static ARTWORK_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn fasttube_icon_rgba(size: usize) -> Vec<u8> {
    let size = size.max(8);
    let mut rgba = vec![0u8; size * size * 4];
    let s = size as f32;
    let margin = s * 0.08;
    let min_x = margin;
    let max_x = s - margin;
    let min_y = s * 0.14;
    let max_y = s - s * 0.14;
    let radius = (max_y - min_y) * 0.24;

    // White play triangle vertices
    let ax = s * 0.40;
    let ay = s * 0.33;
    let bx = s * 0.40;
    let by = s * 0.67;
    let cx = s * 0.68;
    let cy = s * 0.50;

    let point_in_triangle = |px: f32, py: f32| -> bool {
        let sign = |x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32| {
            (x1 - x3) * (y2 - y3) - (x2 - x3) * (y1 - y3)
        };
        let d1 = sign(px, py, ax, ay, bx, by);
        let d2 = sign(px, py, bx, by, cx, cy);
        let d3 = sign(px, py, cx, cy, ax, ay);
        let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(has_neg && has_pos)
    };

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let inside_box = if px >= min_x && px <= max_x && py >= min_y && py <= max_y {
                let dx = if px < min_x + radius {
                    min_x + radius - px
                } else if px > max_x - radius {
                    px - (max_x - radius)
                } else {
                    0.0
                };
                let dy = if py < min_y + radius {
                    min_y + radius - py
                } else if py > max_y - radius {
                    py - (max_y - radius)
                } else {
                    0.0
                };
                (dx * dx + dy * dy) <= radius * radius
            } else {
                false
            };

            if inside_box {
                let idx = (y * size + x) * 4;
                if point_in_triangle(px, py) {
                    rgba[idx] = 255;
                    rgba[idx + 1] = 255;
                    rgba[idx + 2] = 255;
                    rgba[idx + 3] = 255;
                } else {
                    rgba[idx] = 229;
                    rgba[idx + 1] = 25;
                    rgba[idx + 2] = 43;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }
    rgba
}

pub(crate) fn window_icon_data() -> egui::IconData {
    egui::IconData {
        rgba: fasttube_icon_rgba(64),
        width: 64,
        height: 64,
    }
}

pub(crate) fn is_hidden_launch_flag(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.eq_ignore_ascii_case(fastframe_autostart::START_HIDDEN)
        || trimmed.eq_ignore_ascii_case("--hidden")
        || trimmed.eq_ignore_ascii_case("--minimized")
        || trimmed.eq_ignore_ascii_case("--tray")
}

pub(crate) fn has_hidden_launch_arg<I>(args: I) -> bool
where
    I: IntoIterator<Item = String>,
{
    args.into_iter()
        .skip(1)
        .any(|arg| is_hidden_launch_flag(&arg))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstanceCommand {
    Show,
    OpenUrl(String),
    OpenSection(String),
}

pub(crate) enum SingleInstanceOutcome {
    Primary(InstanceHandle),
    Delegated,
}

pub(crate) struct InstanceHandle {
    _guard: InstanceGuard,
    rx: Receiver<InstanceCommand>,
    wake_ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl InstanceHandle {
    pub(crate) fn attach_ctx(&self, ctx: &egui::Context) {
        if let Ok(mut guard) = self.wake_ctx.lock() {
            *guard = Some(ctx.clone());
        }
    }

    pub(crate) fn drain_commands(&self) -> Vec<InstanceCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = self.rx.try_recv() {
            commands.push(command);
        }
        commands
    }
}

pub(crate) fn is_supported_youtube_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let lower = host.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "youtube.com"
            | "www.youtube.com"
            | "m.youtube.com"
            | "music.youtube.com"
            | "youtu.be"
            | "youtube-nocookie.com"
            | "www.youtube-nocookie.com"
    )
}

fn strip_prefix_ignore_ascii_case<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let head = input.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        input.get(prefix.len()..)
    } else {
        None
    }
}

pub(crate) fn parse_fasttube_section_uri(raw: &str) -> Option<&'static str> {
    let trimmed = raw.trim();
    let rest = strip_prefix_ignore_ascii_case(trimmed, "fasttube://")?.trim_start_matches('/');
    let section_raw = strip_prefix_ignore_ascii_case(rest, "section/")?;
    let section = section_raw.trim_matches('/').to_ascii_lowercase();
    match section.as_str() {
        "home" => Some("home"),
        "subscriptions" => Some("subscriptions"),
        "history" => Some("history"),
        "downloads" => Some("downloads"),
        "watch_later" | "watch-later" => Some("watch_later"),
        _ => None,
    }
}

pub(crate) fn normalize_fasttube_protocol_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if is_supported_youtube_url(trimmed) {
        return Some(trimmed.to_owned());
    }
    if let Some(section) = parse_fasttube_section_uri(trimmed) {
        return Some(format!("fasttube://section/{section}"));
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case("fasttube") {
        return None;
    }
    let rest = strip_prefix_ignore_ascii_case(trimmed, "fasttube://")?.trim_start_matches('/');
    if rest.is_empty() {
        return None;
    }
    if is_supported_youtube_url(rest) {
        return Some(rest.to_owned());
    }
    let candidate = format!("https://www.youtube.com/{rest}");
    is_supported_youtube_url(&candidate).then_some(candidate)
}

pub(crate) fn launch_argument_request<I>(args: I) -> String
where
    I: IntoIterator<Item = String>,
{
    for arg in args.into_iter().skip(1) {
        let trimmed = arg.trim();
        if is_hidden_launch_flag(trimmed) {
            continue;
        }
        if let Some(section) = parse_fasttube_section_uri(trimmed) {
            return format!("section {section}");
        }
        if let Some(url) = normalize_fasttube_protocol_url(trimmed) {
            return format!("open {url}");
        }
    }
    "show".to_owned()
}

pub(crate) fn parse_instance_request(line: &str) -> Option<InstanceCommand> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("show") {
        return Some(InstanceCommand::Show);
    }
    if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "section ") {
        let section = rest.trim().to_ascii_lowercase();
        let canonical = match section.as_str() {
            "home" => Some("home"),
            "subscriptions" => Some("subscriptions"),
            "history" => Some("history"),
            "downloads" => Some("downloads"),
            "watch_later" | "watch-later" => Some("watch_later"),
            _ => None,
        }?;
        return Some(InstanceCommand::OpenSection(canonical.to_owned()));
    }
    if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "open ") {
        let url = rest.trim();
        if let Some(section) = parse_fasttube_section_uri(url) {
            return Some(InstanceCommand::OpenSection(section.to_owned()));
        }
        if let Some(normalized) = normalize_fasttube_protocol_url(url) {
            return Some(InstanceCommand::OpenUrl(normalized));
        }
    }
    None
}

const MAX_INTERNET_SHORTCUT_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DroppedInput {
    YouTubeUrl(String),
    LocalMediaFile(PathBuf),
}

pub fn is_supported_local_media_ext(ext: &str) -> bool {
    let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    matches!(
        clean.as_str(),
        "mp4"
            | "mkv"
            | "webm"
            | "mov"
            | "avi"
            | "m4v"
            | "mp3"
            | "m4a"
            | "opus"
            | "ogg"
            | "wav"
            | "flac"
            | "aac"
    )
}

pub fn parse_internet_shortcut_url(contents: &str) -> Option<String> {
    let mut in_internet_shortcut = false;
    let mut saw_any_section = false;
    let mut fallback_url: Option<String> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim().trim_start_matches('\u{FEFF}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            saw_any_section = true;
            let section = line[1..line.len() - 1].trim();
            in_internet_shortcut = section.eq_ignore_ascii_case("InternetShortcut");
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("URL")
        {
            let cleaned = value.trim().trim_matches('"').trim();
            if cleaned.is_empty() {
                continue;
            }
            if in_internet_shortcut {
                return Some(cleaned.to_owned());
            }
            if !saw_any_section && fallback_url.is_none() {
                fallback_url = Some(cleaned.to_owned());
            }
        }
    }

    fallback_url
}

pub fn classify_dropped_path(path: &Path) -> Option<DroppedInput> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("url") {
        let file = fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_INTERNET_SHORTCUT_BYTES)
            .read_to_end(&mut bytes)
            .ok()?;
        let contents = String::from_utf8_lossy(&bytes);
        let raw_url = parse_internet_shortcut_url(&contents)?;
        let normalized = normalize_fasttube_protocol_url(&raw_url)?;
        return is_supported_youtube_url(&normalized)
            .then_some(DroppedInput::YouTubeUrl(normalized));
    }
    if is_supported_local_media_ext(ext) {
        return Some(DroppedInput::LocalMediaFile(path.to_path_buf()));
    }
    None
}

pub fn classify_dropped_text(text: &str) -> Option<DroppedInput> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    for candidate in std::iter::once(trimmed)
        .chain(trimmed.lines().map(str::trim))
        .chain(trimmed.split_whitespace())
    {
        let stripped = candidate.trim_matches(|c| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')'));
        if let Some(normalized) = normalize_fasttube_protocol_url(stripped)
            && is_supported_youtube_url(&normalized)
        {
            return Some(DroppedInput::YouTubeUrl(normalized));
        }
    }
    None
}

pub(crate) fn claim_single_instance<I>(args: I) -> SingleInstanceOutcome
where
    I: IntoIterator<Item = String>,
{
    let request = launch_argument_request(args);
    let slot = Slot::new("FastTube");
    let (tx, rx) = mpsc::channel();
    let wake_ctx: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    let wake_for_handler = Arc::clone(&wake_ctx);
    let handler_tx = tx.clone();

    match slot.claim(&request, move |incoming| {
        if incoming.trim().eq_ignore_ascii_case("ping") {
            return Some("ok".to_owned());
        }
        let command = parse_instance_request(incoming)?;
        let _ = handler_tx.send(command);
        if let Ok(guard) = wake_for_handler.lock()
            && let Some(ctx) = guard.as_ref()
        {
            ctx.request_repaint();
        }
        Some("ok".to_owned())
    }) {
        Claim::First(guard) => {
            if let Some(cmd @ (InstanceCommand::OpenUrl(_) | InstanceCommand::OpenSection(_))) =
                parse_instance_request(&request)
            {
                let _ = tx.send(cmd);
            }
            SingleInstanceOutcome::Primary(InstanceHandle {
                _guard: guard,
                rx,
                wake_ctx,
            })
        }
        Claim::Running(_) | Claim::Declined | Claim::Unanswered => SingleInstanceOutcome::Delegated,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NowPlayingAction {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    SeekByMs(i64),
    SeekTo(Duration),
    SetVolume(f64),
    Raise,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NowPlayingSnapshot {
    pub(crate) video_id: String,
    pub(crate) title: String,
    pub(crate) channel: String,
    pub(crate) thumbnail_url: String,
    pub(crate) playing: bool,
    pub(crate) ended: bool,
    pub(crate) position_secs: f64,
    pub(crate) duration_secs: Option<f64>,
    pub(crate) volume_fraction: Option<f64>,
    pub(crate) can_next: bool,
    pub(crate) can_previous: bool,
}

struct ArtworkCache {
    current_url: Option<String>,
    current_file: Option<PathBuf>,
    request_id: u64,
    rx: Option<Receiver<(u64, String, Option<PathBuf>)>>,
    temp_path: PathBuf,
}

impl Default for ArtworkCache {
    fn default() -> Self {
        Self {
            current_url: None,
            current_file: None,
            request_id: 0,
            rx: None,
            temp_path: std::env::temp_dir().join(format!(
                "fasttube-now-playing-art-{}.img",
                std::process::id()
            )),
        }
    }
}

impl Drop for ArtworkCache {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp_path);
    }
}

impl ArtworkCache {
    fn sync(&mut self, thumbnail_url: Option<&str>, ctx: &egui::Context) -> Option<PathBuf> {
        if let Some(rx) = &self.rx {
            while let Ok((id, url, path)) = rx.try_recv() {
                if id == self.request_id && self.current_url.as_deref() == Some(url.as_str()) {
                    self.current_file = path;
                }
            }
        }

        let clean_url = thumbnail_url
            .map(str::trim)
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"));

        if self.current_url.as_deref() != clean_url {
            self.current_url = clean_url.map(str::to_owned);
            self.current_file = None;
            self.request_id = self.request_id.wrapping_add(1);
            if let Some(url) = clean_url {
                let id = self.request_id;
                let url_owned = url.to_owned();
                let dest = self.temp_path.clone();
                let ctx_clone = ctx.clone();
                let (tx, rx) = mpsc::channel();
                self.rx = Some(rx);
                std::thread::spawn(move || {
                    let result = download_artwork_to(&url_owned, &dest);
                    let _ = tx.send((id, url_owned, result));
                    ctx_clone.request_repaint();
                });
            } else {
                self.rx = None;
            }
        }

        self.current_file.clone()
    }
}

fn write_artwork_atomically(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = dest
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "artwork path has no parent directory",
            )
        })?;
    fs::create_dir_all(parent)?;
    let (temp_path, mut file) = loop {
        let id = ARTWORK_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".fasttube-now-playing-art-{}-{id}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        drop(file);
        fs::rename(&temp_path, dest)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn download_artwork_to(url: &str, dest: &Path) -> Option<PathBuf> {
    let response = crate::http_client::shared()
        .get(url)
        .timeout(ARTWORK_TIMEOUT)
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_ARTWORK_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_ARTWORK_BYTES {
        return None;
    }
    write_artwork_atomically(dest, &bytes).ok()?;
    Some(dest.to_path_buf())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutostartUiState {
    Loading,
    Unsupported,
    Disabled,
    Enabled { hidden: bool },
    NeedsApproval,
}

impl AutostartUiState {
    pub(crate) fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    pub(crate) fn is_hidden(self) -> bool {
        matches!(self, Self::Enabled { hidden: true })
    }
}

enum AutostartResponse {
    State(AutostartUiState),
    Error(String),
}

pub(crate) struct AutostartController {
    pub(crate) state: AutostartUiState,
    pub(crate) busy: bool,
    pub(crate) error: Option<String>,
    initialized: bool,
    rx: Option<Receiver<AutostartResponse>>,
}

impl Default for AutostartController {
    fn default() -> Self {
        Self {
            state: AutostartUiState::Loading,
            busy: false,
            error: None,
            initialized: false,
            rx: None,
        }
    }
}

impl AutostartController {
    pub(crate) fn ensure_loaded(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.refresh(ctx);
    }

    pub(crate) fn refresh(&mut self, ctx: &egui::Context) {
        self.busy = true;
        self.error = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let autostart = Autostart::new("fasttube", "FastTube");
            let mapped = map_autostart_state(autostart.state());
            let _ = tx.send(AutostartResponse::State(mapped));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, hidden: bool, ctx: &egui::Context) {
        self.busy = true;
        self.error = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let autostart = Autostart::new("fasttube", "FastTube");
            let target = if enabled {
                Some(if hidden { Start::Hidden } else { Start::Window })
            } else {
                None
            };
            let response = match autostart.set(target) {
                Ok(state) => AutostartResponse::State(map_autostart_state(state)),
                Err(err) => AutostartResponse::Error(format!(
                    "Não foi possível alterar a inicialização automática: {err}"
                )),
            };
            let _ = tx.send(response);
            ctx_clone.request_repaint();
        });
    }

    #[cfg(test)]
    pub(crate) fn set_state_for_test(&mut self, state: AutostartUiState) {
        self.state = state;
        self.initialized = true;
        self.busy = false;
        self.error = None;
        self.rx = None;
    }

    pub(crate) fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(AutostartResponse::State(state)) => {
                self.state = state;
                self.busy = false;
                self.rx = None;
            }
            Ok(AutostartResponse::Error(error)) => {
                self.error = Some(error);
                self.busy = false;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

fn map_autostart_state(state: AutostartState) -> AutostartUiState {
    match state {
        AutostartState::Unsupported => AutostartUiState::Unsupported,
        AutostartState::Off => AutostartUiState::Disabled,
        AutostartState::On(start) => AutostartUiState::Enabled {
            hidden: start == Start::Hidden,
        },
        AutostartState::NeedsApproval(_) => AutostartUiState::NeedsApproval,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Show,
    ToggleVisibility,
    PlayPause,
    CheckUpdates,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayMenuSnapshot {
    hidden_to_tray: bool,
    has_video: bool,
    playing: bool,
    pt: bool,
    update_version: Option<String>,
    video_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NowPlayingDiffKey {
    pub(crate) video_id: String,
    pub(crate) title: String,
    pub(crate) channel: String,
    pub(crate) artwork_url: String,
    pub(crate) art_file: Option<PathBuf>,
    pub(crate) is_playing: bool,
    pub(crate) ended: bool,
    pub(crate) timeline_secs_bucket: i64,
    pub(crate) duration_secs_bucket: Option<i64>,
    pub(crate) volume_pct: Option<u8>,
    pub(crate) can_next: bool,
    pub(crate) can_previous: bool,
}

#[derive(Default)]
pub(crate) struct DesktopIntegrations {
    instance: Option<InstanceHandle>,
    now_playing: Option<NowPlaying>,
    artwork: ArtworkCache,
    last_now_playing_key: Option<Option<NowPlayingDiffKey>>,
    now_playing_push_count: u64,
    power: Option<Power>,
    keep_awake: Option<KeepAwake>,
    tray: Option<Tray>,
    last_tray_snapshot: Option<TrayMenuSnapshot>,
    pub(crate) hidden_to_tray: bool,
    pub(crate) tray_quit_requested: bool,
    pub(crate) autostart: AutostartController,
}

impl DesktopIntegrations {
    pub(crate) fn init(
        &mut self,
        ctx: &egui::Context,
        instance: Option<InstanceHandle>,
        start_hidden: bool,
    ) {
        if let Some(handle) = instance {
            handle.attach_ctx(ctx);
            self.instance = Some(handle);
        }
        if self.tray.is_none() {
            let wake_ctx = ctx.clone();
            let config = TrayConfig {
                id: "fasttube",
                title: "FastTube".into(),
                icon: fasttube_icon_rgba,
                template_icon: None,
                themed_icon: false,
                menu: vec![
                    TrayMenuItem::action("show", "Abrir FastTube"),
                    TrayMenuItem::action("play_pause", "Reproduzir / Pausar").visible(false),
                    TrayMenuItem::Separator,
                    TrayMenuItem::action("check_update", "Verificar atualizações..."),
                    TrayMenuItem::Separator,
                    TrayMenuItem::action("quit", "Sair"),
                ],
            };
            if let Some(mut tray) = Tray::spawn(config, move || {
                wake_ctx.request_repaint();
            }) {
                tray.attach();
                self.tray = Some(tray);
            }
        }
        if start_hidden && self.tray_active() {
            self.hidden_to_tray = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if self.now_playing.is_none() {
            let app = NowPlayingApp::new("fasttube", "FastTube");
            let wake_ctx = ctx.clone();
            self.now_playing = Some(NowPlaying::start(app, move || {
                wake_ctx.request_repaint();
            }));
        }
        if self.power.is_none() {
            let wake_ctx = ctx.clone();
            self.power = Some(Power::spawn(move || {
                wake_ctx.request_repaint();
            }));
        }
        self.autostart.ensure_loaded(ctx);
    }

    pub(crate) fn tray_active(&self) -> bool {
        self.tray.as_ref().is_some_and(Tray::is_shown)
    }

    pub(crate) fn poll_tray_actions(&self) -> Vec<TrayAction> {
        let Some(tray) = &self.tray else {
            return Vec::new();
        };
        tray.events()
            .into_iter()
            .filter_map(|event| match event {
                TrayEvent::Show => Some(TrayAction::Show),
                TrayEvent::Toggle => Some(TrayAction::ToggleVisibility),
                TrayEvent::Menu("show") => Some(if self.hidden_to_tray {
                    TrayAction::Show
                } else {
                    TrayAction::ToggleVisibility
                }),
                TrayEvent::Menu("play_pause") => Some(TrayAction::PlayPause),
                TrayEvent::Menu("check_update") => Some(TrayAction::CheckUpdates),
                TrayEvent::Menu("quit") => Some(TrayAction::Quit),
                TrayEvent::Menu(_) => None,
            })
            .collect()
    }

    pub(crate) fn sync_tray(
        &mut self,
        hidden_to_tray: bool,
        has_video: bool,
        playing: bool,
        pt: bool,
        update_version: Option<&str>,
        video_title: Option<&str>,
    ) {
        let Some(tray) = &mut self.tray else {
            return;
        };
        let snapshot = TrayMenuSnapshot {
            hidden_to_tray,
            has_video,
            playing,
            pt,
            update_version: update_version.map(str::to_owned),
            video_title: video_title.map(str::to_owned),
        };
        if self.last_tray_snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        self.last_tray_snapshot = Some(snapshot);

        let show_label = match (hidden_to_tray, pt) {
            (true, true) => "Abrir FastTube",
            (true, false) => "Show FastTube",
            (false, true) => "Minimizar para a bandeja",
            (false, false) => "Minimize to tray",
        };
        tray.set_label("show", show_label);

        tray.set_visible("play_pause", has_video);
        let play_label = match (playing, pt) {
            (true, true) => "Pausar vídeo",
            (true, false) => "Pause video",
            (false, true) => "Reproduzir vídeo",
            (false, false) => "Play video",
        };
        tray.set_label("play_pause", play_label);

        let update_label = match (update_version, pt) {
            (Some(ver), true) => format!("Atualização disponível (v{ver})"),
            (Some(ver), false) => format!("Update available (v{ver})"),
            (None, true) => "Verificar atualizações...".to_owned(),
            (None, false) => "Check for updates...".to_owned(),
        };
        tray.set_label("check_update", update_label);
        tray.set_label("quit", if pt { "Sair" } else { "Quit" });

        let mut tooltip = String::from("FastTube");
        if let Some(title) = video_title.filter(|t| !t.trim().is_empty()) {
            tooltip.push('\n');
            let truncated: String = title.chars().take(64).collect();
            tooltip.push_str(&truncated);
        } else if let Some(ver) = update_version {
            tooltip.push_str(&format!(
                "\nv{ver} {}",
                if pt { "disponível" } else { "available" }
            ));
        }
        tray.set_tooltip(tooltip);
    }

    pub(crate) fn drain_instance_commands(&self) -> Vec<InstanceCommand> {
        self.instance
            .as_ref()
            .map(InstanceHandle::drain_commands)
            .unwrap_or_default()
    }

    pub(crate) fn poll_now_playing_commands(&mut self) -> Vec<NowPlayingAction> {
        let Some(controls) = &mut self.now_playing else {
            return Vec::new();
        };
        controls
            .commands()
            .into_iter()
            .filter_map(|cmd| match cmd {
                MediaCommand::Play => Some(NowPlayingAction::Play),
                MediaCommand::Pause => Some(NowPlayingAction::Pause),
                MediaCommand::PlayPause => Some(NowPlayingAction::PlayPause),
                MediaCommand::Stop => Some(NowPlayingAction::Stop),
                MediaCommand::Next => Some(NowPlayingAction::Next),
                MediaCommand::Previous => Some(NowPlayingAction::Previous),
                MediaCommand::SeekBy(ms) => Some(NowPlayingAction::SeekByMs(ms)),
                MediaCommand::SetPosition { position, .. } => {
                    Some(NowPlayingAction::SeekTo(position))
                }
                MediaCommand::SetVolume(vol) => Some(NowPlayingAction::SetVolume(vol)),
                MediaCommand::Raise => Some(NowPlayingAction::Raise),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn notify_seeked(&mut self, position_secs: f64) {
        self.last_now_playing_key = None;
        if let Some(controls) = &mut self.now_playing
            && position_secs.is_finite()
            && position_secs >= 0.0
        {
            controls.seeked(Duration::from_secs_f64(position_secs));
        }
    }

    pub(crate) fn now_playing_diff_key(
        snapshot: Option<&NowPlayingSnapshot>,
        art_file: Option<&PathBuf>,
    ) -> Option<NowPlayingDiffKey> {
        let snap = snapshot?;
        let timeline_secs_bucket = if snap.position_secs.is_finite() && snap.position_secs >= 0.0 {
            snap.position_secs.floor() as i64
        } else {
            0
        };
        let duration_secs_bucket = snap
            .duration_secs
            .filter(|d| d.is_finite() && *d > 0.0)
            .map(|d| d.round() as i64);
        let volume_pct = snap
            .volume_fraction
            .filter(|v| v.is_finite())
            .map(|v| (v.clamp(0.0, 1.0) * 100.0).round() as u8);
        Some(NowPlayingDiffKey {
            video_id: snap.video_id.clone(),
            title: snap.title.clone(),
            channel: snap.channel.clone(),
            artwork_url: snap.thumbnail_url.clone(),
            art_file: art_file.cloned(),
            is_playing: snap.playing,
            ended: snap.ended,
            timeline_secs_bucket,
            duration_secs_bucket,
            volume_pct,
            can_next: snap.can_next,
            can_previous: snap.can_previous,
        })
    }

    #[cfg(test)]
    pub(crate) fn now_playing_push_count(&self) -> u64 {
        self.now_playing_push_count
    }

    pub(crate) fn sync_now_playing(
        &mut self,
        snapshot: Option<NowPlayingSnapshot>,
        ctx: &egui::Context,
    ) {
        let art_file = self.artwork.sync(
            snapshot.as_ref().map(|snap| snap.thumbnail_url.as_str()),
            ctx,
        );
        let key = Self::now_playing_diff_key(snapshot.as_ref(), art_file.as_ref());
        if self.last_now_playing_key.as_ref() == Some(&key) {
            return;
        }
        self.last_now_playing_key = Some(key);
        self.now_playing_push_count = self.now_playing_push_count.saturating_add(1);
        let Some(controls) = &mut self.now_playing else {
            return;
        };
        let state = match snapshot {
            Some(snap) => {
                let playback = if snap.ended {
                    MediaPlayback::Paused
                } else if snap.playing {
                    MediaPlayback::Playing
                } else {
                    MediaPlayback::Paused
                };
                let duration = snap
                    .duration_secs
                    .filter(|d| d.is_finite() && *d > 0.0)
                    .map(Duration::from_secs_f64);
                let position = Duration::from_secs_f64(snap.position_secs.max(0.0));
                let artists = if snap.channel.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![snap.channel]
                };
                let track = MediaTrack {
                    id: snap.video_id,
                    title: snap.title,
                    artists,
                    duration,
                    art_file,
                    art_url: (!snap.thumbnail_url.trim().is_empty()).then_some(snap.thumbnail_url),
                    ..MediaTrack::default()
                };
                MediaState {
                    playback,
                    position,
                    track: Some(track),
                    volume: snap.volume_fraction,
                    controls: MediaControls {
                        play: true,
                        pause: true,
                        next: snap.can_next,
                        previous: snap.can_previous,
                        seek: duration.is_some(),
                    },
                    ..MediaState::default()
                }
            }
            None => MediaState::default(),
        };
        controls.update(state);
    }

    pub(crate) fn sync_power(
        &mut self,
        actively_playing: bool,
        window_minimized: bool,
    ) -> Vec<PowerEvent> {
        let Some(power) = &mut self.power else {
            return Vec::new();
        };
        let conditions: PowerConditions = power.conditions();
        let should_background =
            !actively_playing && (window_minimized || conditions.display_off || conditions.locked);
        power.set_background(should_background);
        if actively_playing {
            self.keep_awake
                .get_or_insert_with(|| power.keep_awake("Reproduzindo vídeo no FastTube"));
        } else {
            self.keep_awake = None;
        }
        power.events()
    }

    pub(crate) fn release_keep_awake(&mut self) {
        self.keep_awake = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_supported_youtube_urls() {
        assert!(is_supported_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(is_supported_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(is_supported_youtube_url(
            "https://music.youtube.com/watch?v=abc"
        ));
        assert!(!is_supported_youtube_url("file:///C:/test.mp4"));
        assert!(!is_supported_youtube_url("https://example.com/watch?v=1"));
        assert!(!is_supported_youtube_url("not-a-url"));
    }

    #[test]
    fn parses_instance_requests_and_launch_args() {
        assert_eq!(parse_instance_request("show"), Some(InstanceCommand::Show));
        assert_eq!(parse_instance_request("ping"), None);
        assert_eq!(
            parse_instance_request("open https://youtu.be/abc123"),
            Some(InstanceCommand::OpenUrl("https://youtu.be/abc123".into()))
        );
        assert_eq!(
            parse_instance_request("open https://evil.example/video"),
            None
        );

        assert_eq!(
            launch_argument_request(["fasttube.exe".into(), "--flag".into()]),
            "show"
        );
        assert_eq!(
            launch_argument_request([
                "fasttube.exe".into(),
                "--hidden".into(),
                "https://www.youtube.com/watch?v=abc".into()
            ]),
            "open https://www.youtube.com/watch?v=abc"
        );
        assert!(has_hidden_launch_arg([
            "fasttube.exe".into(),
            fastframe_autostart::START_HIDDEN.into()
        ]));
        assert!(has_hidden_launch_arg([
            "fasttube.exe".into(),
            "--hidden".into()
        ]));
        assert!(has_hidden_launch_arg([
            "fasttube.exe".into(),
            "--minimized".into()
        ]));
        assert!(!has_hidden_launch_arg(["fasttube.exe".into()]));
    }

    #[test]
    fn generates_tray_and_window_icon_and_maps_autostart() {
        let icon = window_icon_data();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        assert!(icon.rgba.contains(&255));

        assert_eq!(
            map_autostart_state(AutostartState::On(Start::Hidden)),
            AutostartUiState::Enabled { hidden: true }
        );
        assert_eq!(
            map_autostart_state(AutostartState::On(Start::Window)),
            AutostartUiState::Enabled { hidden: false }
        );
        assert!(AutostartUiState::Enabled { hidden: true }.is_enabled());
        assert!(AutostartUiState::Enabled { hidden: true }.is_hidden());
        assert!(!AutostartUiState::Enabled { hidden: false }.is_hidden());
    }

    #[test]
    fn power_fixed_tracks_keep_awake_and_events() {
        let power = Power::fixed(PowerConditions::default());
        let ack = fastframe_power::Ack::default();
        power.send(PowerEvent::Suspending(ack.clone()));
        let events = power.events();
        assert_eq!(events.len(), 1);
        if let PowerEvent::Suspending(answer) = &events[0] {
            answer.done();
        } else {
            panic!("expected suspending event");
        }
        assert!(ack.is_done());
    }

    #[test]
    fn normalizes_fasttube_protocol_urls_case_insensitively() {
        let expected_watch = Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned());
        assert_eq!(
            normalize_fasttube_protocol_url("fasttube://watch?v=dQw4w9WgXcQ"),
            expected_watch
        );
        assert_eq!(
            normalize_fasttube_protocol_url("FASTTUBE://watch?v=dQw4w9WgXcQ"),
            expected_watch
        );
        assert_eq!(
            normalize_fasttube_protocol_url("FastTube://watch?v=dQw4w9WgXcQ"),
            expected_watch
        );
        assert_eq!(
            normalize_fasttube_protocol_url("FaStTuBe:///watch?v=dQw4w9WgXcQ"),
            expected_watch
        );
        assert_eq!(
            normalize_fasttube_protocol_url("FastTube://https://youtu.be/dQw4w9WgXcQ"),
            Some("https://youtu.be/dQw4w9WgXcQ".to_owned())
        );
        assert_eq!(
            parse_instance_request("Open FastTube://watch?v=dQw4w9WgXcQ"),
            Some(InstanceCommand::OpenUrl(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into()
            ))
        );
        assert_eq!(normalize_fasttube_protocol_url("FastTube://"), None);
        assert_eq!(
            normalize_fasttube_protocol_url("FastTube:watch?v=dQw4w9WgXcQ"),
            None
        );
    }

    #[test]
    fn writes_smtc_artwork_atomically_and_leaves_no_temp_files() {
        struct TempArtDir(PathBuf);
        impl Drop for TempArtDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        let dir = TempArtDir(std::env::temp_dir().join(format!(
            "fasttube-desktop-art-test-{}-{}",
            std::process::id(),
            ARTWORK_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        )));
        let _ = fs::remove_dir_all(&dir.0);
        fs::create_dir_all(&dir.0).expect("create temp artwork dir");
        let dest = dir.0.join("now-playing-art.img");

        let first = b"first-artwork-payload";
        write_artwork_atomically(&dest, first).expect("initial atomic artwork write");
        assert_eq!(fs::read(&dest).expect("read initial artwork"), first);

        let second = b"second-artwork-payload-replacing-first";
        write_artwork_atomically(&dest, second).expect("atomic artwork replace");
        assert_eq!(fs::read(&dest).expect("read replaced artwork"), second);

        let leftovers: Vec<String> = fs::read_dir(&dir.0)
            .expect("list temp artwork dir")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "unexpected temp artwork files left behind: {leftovers:?}"
        );
        assert!(write_artwork_atomically(Path::new("orphan.img"), first).is_err());
    }

    #[test]
    fn parses_fasttube_section_uris_for_jump_lists() {
        for section in [
            "home",
            "subscriptions",
            "history",
            "downloads",
            "watch_later",
        ] {
            let uri = format!("fasttube://section/{section}");
            assert_eq!(parse_fasttube_section_uri(&uri), Some(section));
            assert_eq!(normalize_fasttube_protocol_url(&uri), Some(uri.clone()));
            assert_eq!(
                launch_argument_request(["fasttube.exe".into(), uri.clone()]),
                format!("section {section}")
            );
            assert_eq!(
                parse_instance_request(&format!("section {section}")),
                Some(InstanceCommand::OpenSection(section.to_owned()))
            );
            assert_eq!(
                parse_instance_request(&format!("open {uri}")),
                Some(InstanceCommand::OpenSection(section.to_owned()))
            );
        }

        assert_eq!(
            parse_fasttube_section_uri("FastTube://Section/Watch-Later/"),
            Some("watch_later")
        );
        assert_eq!(
            normalize_fasttube_protocol_url("FASTTUBE:///section/Downloads"),
            Some("fasttube://section/downloads".to_owned())
        );
        assert_eq!(
            parse_fasttube_section_uri("fasttube://section/unknown"),
            None
        );
        assert_eq!(parse_instance_request("section unknown"), None);
    }

    #[test]
    fn classifies_dropped_paths_internet_shortcuts_and_text() {
        for ext in [
            "mp4", "MKV", ".webm", "mov", "avi", "m4v", "mp3", "m4a", "opus", "ogg", "wav", "flac",
            "aac",
        ] {
            assert!(
                is_supported_local_media_ext(ext),
                "expected {ext} supported"
            );
        }
        assert!(!is_supported_local_media_ext("exe"));
        assert!(!is_supported_local_media_ext("txt"));
        assert!(!is_supported_local_media_ext(""));

        let shortcut_body = "[DEFAULT]\r\nBASEURL=https://example.com\r\n[InternetShortcut]\r\nURL=https://www.youtube.com/watch?v=dQw4w9WgXcQ\r\nIconIndex=0\r\n";
        assert_eq!(
            parse_internet_shortcut_url(shortcut_body),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned())
        );
        assert_eq!(
            parse_internet_shortcut_url("URL=https://youtu.be/abc123\n"),
            Some("https://youtu.be/abc123".to_owned())
        );
        assert_eq!(
            parse_internet_shortcut_url("[Other]\nURL=https://youtu.be/abc"),
            None
        );

        assert_eq!(
            classify_dropped_path(Path::new(r"C:\Videos\demo.mkv")),
            Some(DroppedInput::LocalMediaFile(PathBuf::from(
                r"C:\Videos\demo.mkv"
            )))
        );
        assert_eq!(classify_dropped_path(Path::new(r"C:\notes.txt")), None);

        struct TempDropDir(PathBuf);
        impl Drop for TempDropDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let dir = TempDropDir(std::env::temp_dir().join(format!(
            "fasttube-drop-test-{}-{}",
            std::process::id(),
            ARTWORK_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir_all(&dir.0).expect("create temp drop dir");
        let yt_shortcut = dir.0.join("clip.url");
        fs::write(&yt_shortcut, shortcut_body).expect("write yt shortcut");
        assert_eq!(
            classify_dropped_path(&yt_shortcut),
            Some(DroppedInput::YouTubeUrl(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()
            ))
        );

        let other_shortcut = dir.0.join("other.url");
        fs::write(
            &other_shortcut,
            "[InternetShortcut]\nURL=https://example.com\n",
        )
        .expect("write non-yt shortcut");
        assert_eq!(classify_dropped_path(&other_shortcut), None);

        assert_eq!(
            classify_dropped_text("  <https://youtu.be/dQw4w9WgXcQ>  "),
            Some(DroppedInput::YouTubeUrl(
                "https://youtu.be/dQw4w9WgXcQ".to_owned()
            ))
        );
        assert_eq!(
            classify_dropped_text("FastTube://watch?v=dQw4w9WgXcQ"),
            Some(DroppedInput::YouTubeUrl(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()
            ))
        );
        assert_eq!(classify_dropped_text("https://example.com/video"), None);
        assert_eq!(classify_dropped_text("fasttube://section/home"), None);
    }
}
