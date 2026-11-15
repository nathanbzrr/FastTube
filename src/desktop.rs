use eframe::egui;
use fastframe_autostart::{Autostart, Start, State as AutostartState};
use fastframe_instance::{Claim, Guard as InstanceGuard, Slot};
use fastframe_now_playing::{
    App as NowPlayingApp, Command as MediaCommand, Controls as MediaControls, NowPlaying,
    Playback as MediaPlayback, State as MediaState, Track as MediaTrack,
};
use fastframe_power::{Conditions as PowerConditions, Event as PowerEvent, KeepAwake, Power};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_ARTWORK_BYTES: usize = 4 * 1024 * 1024;
const ARTWORK_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstanceCommand {
    Show,
    OpenUrl(String),
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

pub(crate) fn launch_argument_request<I>(args: I) -> String
where
    I: IntoIterator<Item = String>,
{
    for arg in args.into_iter().skip(1) {
        let trimmed = arg.trim();
        if is_supported_youtube_url(trimmed) {
            return format!("open {trimmed}");
        }
    }
    "show".to_owned()
}

pub(crate) fn parse_instance_request(line: &str) -> Option<InstanceCommand> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("show") {
        return Some(InstanceCommand::Show);
    }
    if let Some(rest) = trimmed
        .strip_prefix("open ")
        .or_else(|| trimmed.strip_prefix("OPEN "))
    {
        let url = rest.trim();
        if is_supported_youtube_url(url) {
            return Some(InstanceCommand::OpenUrl(url.to_owned()));
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
            if let Some(InstanceCommand::OpenUrl(url)) = parse_instance_request(&request) {
                let _ = tx.send(InstanceCommand::OpenUrl(url));
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

fn download_artwork_to(url: &str, dest: &PathBuf) -> Option<PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .timeout(ARTWORK_TIMEOUT)
        .build()
        .ok()?;
    let response = client.get(url).send().ok()?;
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
    fs::write(dest, &bytes).ok()?;
    Some(dest.clone())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AutostartUiState {
    Loading,
    Unsupported,
    Disabled,
    Enabled,
    NeedsApproval,
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

    pub(crate) fn set_enabled(&mut self, enabled: bool, ctx: &egui::Context) {
        self.busy = true;
        self.error = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let autostart = Autostart::new("fasttube", "FastTube");
            let target = if enabled { Some(Start::Window) } else { None };
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
        AutostartState::On(_) => AutostartUiState::Enabled,
        AutostartState::NeedsApproval(_) => AutostartUiState::NeedsApproval,
    }
}

#[derive(Default)]
pub(crate) struct DesktopIntegrations {
    instance: Option<InstanceHandle>,
    now_playing: Option<NowPlaying>,
    artwork: ArtworkCache,
    power: Option<Power>,
    keep_awake: Option<KeepAwake>,
    pub(crate) autostart: AutostartController,
}

impl DesktopIntegrations {
    pub(crate) fn init(&mut self, ctx: &egui::Context, instance: Option<InstanceHandle>) {
        if let Some(handle) = instance {
            handle.attach_ctx(ctx);
            self.instance = Some(handle);
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
        if let Some(controls) = &mut self.now_playing
            && position_secs.is_finite()
            && position_secs >= 0.0
        {
            controls.seeked(Duration::from_secs_f64(position_secs));
        }
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
                "https://www.youtube.com/watch?v=abc".into()
            ]),
            "open https://www.youtube.com/watch?v=abc"
        );
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
}
