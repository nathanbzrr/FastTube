use crate::feed_prefs;
use crate::history::{self, History};
use crate::overlay;
use crate::process;
use crate::search::{self, ChannelTarget};
use crate::settings;
use crate::{
    ActivePlayback, BackupImportMode, BackupPreview, BrowseSection, BufferProfile,
    ChannelAboutFetchMessage, ChannelFetchMessage, ChannelMoreVideosFetchMessage, ChannelPage,
    ChannelPlaylistVideosFetchMessage, ChannelPlaylistsFetchMessage, ChannelTab,
    DecodingDiagnostics, FeedDerivedCache, FeedFilter, HISTORY_MIN_POSITION_DELTA_MS,
    HISTORY_SAVE_INTERVAL, HomeFetchMessage, ImportedUserData, PlaybackPreferences, Queue,
    QueuePlaybackCorrelation, SearchFetchMessage, SearchResult, Theme, ThemePalette, Video,
    apply_theme_visuals, buffer_profile_name, buffer_profile_summary, parse_duration_ms,
};
use eframe::egui::{self, Color32};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;

pub(crate) struct BrowseFeedState {
    pub(crate) query: String,
    pub(crate) videos: Vec<Video>,
    pub(crate) generation: u64,
    pub(crate) filter: FeedFilter,
    pub(crate) cache_dirty: bool,
    pub(crate) cache: FeedDerivedCache,
    pub(crate) search: SearchJob,
    pub(crate) home: HomeFeedState,
}

impl Default for BrowseFeedState {
    fn default() -> Self {
        Self {
            query: String::new(),
            videos: Vec::new(),
            generation: 0,
            filter: FeedFilter::All,
            cache_dirty: true,
            cache: FeedDerivedCache::default(),
            search: SearchJob::default(),
            home: HomeFeedState::default(),
        }
    }
}

#[derive(Default)]
pub(crate) struct SearchJob {
    pub(crate) rx: Option<Receiver<SearchFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) next_request_id: u64,
    pub(crate) active_request_id: Option<u64>,
}

#[derive(Default)]
pub(crate) struct HomeFeedState {
    pub(crate) rx: Option<Receiver<HomeFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) initial_public_started: bool,
    pub(crate) playlist_urls: Vec<String>,
    pub(crate) title: Option<String>,
    pub(crate) limit_reached: bool,
    pub(crate) subscription_channels: Vec<search::HomeChannel>,
    pub(crate) subscription_channels_loaded: bool,
    pub(crate) subscription_channels_error: Option<String>,
    pub(crate) pending_items: Vec<SearchResult>,
}

#[derive(Default)]
pub(crate) struct FeedPreferencesState {
    pub(crate) values: feed_prefs::FeedPreferences,
    pub(crate) path: Option<PathBuf>,
}

pub(crate) struct ChannelState {
    pub(crate) page: Option<ChannelPage>,
    pub(crate) view_open: bool,
    pub(crate) tab: ChannelTab,
    pub(crate) next_request_id: u64,
    pub(crate) loading_target: Option<crate::ChannelTarget>,
    pub(crate) last_target: Option<crate::ChannelTarget>,
    pub(crate) error: Option<String>,
    pub(crate) video_limit: usize,
    pub(crate) from_cache: bool,
    pub(crate) videos: ChannelVideosState,
    pub(crate) playlists: ChannelPlaylistsState,
    pub(crate) playlist_detail: ChannelPlaylistDetailState,
    pub(crate) about: ChannelAboutState,
}

impl ChannelState {
    fn with_defaults() -> Self {
        Self {
            page: None,
            view_open: false,
            tab: ChannelTab::Videos,
            next_request_id: 0,
            loading_target: None,
            last_target: None,
            error: None,
            video_limit: search::DEFAULT_CHANNEL_VIDEOS_LIMIT,
            from_cache: false,
            videos: ChannelVideosState::default(),
            playlists: ChannelPlaylistsState::default(),
            playlist_detail: ChannelPlaylistDetailState::default(),
            about: ChannelAboutState::default(),
        }
    }
}

impl Default for ChannelState {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[derive(Default)]
pub(crate) struct ChannelVideosState {
    pub(crate) rx: Option<Receiver<ChannelFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) more_rx: Option<Receiver<ChannelMoreVideosFetchMessage>>,
    pub(crate) more_cancel: Option<process::Cancellation>,
    pub(crate) active_more_request_id: Option<u64>,
    pub(crate) more_error: Option<String>,
    pub(crate) has_more: bool,
    pub(crate) reached_max: bool,
}

#[derive(Default)]
pub(crate) struct ChannelPlaylistsState {
    pub(crate) items: Vec<search::ChannelPlaylist>,
    pub(crate) loaded: bool,
    pub(crate) rx: Option<Receiver<ChannelPlaylistsFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelPlaylistDetailState {
    pub(crate) selected: Option<search::ChannelPlaylist>,
    pub(crate) items: Vec<SearchResult>,
    pub(crate) loaded: bool,
    pub(crate) rx: Option<Receiver<ChannelPlaylistVideosFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelAboutState {
    pub(crate) info: Option<search::ChannelAboutInfo>,
    pub(crate) loaded: bool,
    pub(crate) rx: Option<Receiver<ChannelAboutFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) error: Option<String>,
}

pub(crate) type RatingResultReceiver = Receiver<(
    Option<crate::oauth::TokenSession>,
    Result<(String, Option<crate::oauth::VideoRating>), String>,
)>;

#[derive(Default)]
pub(crate) struct AuthState {
    pub(crate) reauth_rx: Option<Receiver<Result<crate::oauth::TokenSession, String>>>,
    pub(crate) reauth_cancel: Option<process::Cancellation>,
    pub(crate) authenticated: bool,
    pub(crate) session: Option<crate::oauth::TokenSession>,
    pub(crate) ratings: HashMap<String, crate::oauth::VideoRating>,
    pub(crate) rating_rx: Option<RatingResultReceiver>,
    pub(crate) rating_cancel: Option<process::Cancellation>,
    pub(crate) restore_attempted: bool,
}

pub(crate) struct PlaybackState {
    pub(crate) fullscreen: bool,
    pub(crate) theater: bool,
    pub(crate) window_visible: bool,
    pub(crate) active_buffer: Option<BufferProfile>,
    pub(crate) mini_player: bool,
    pub(crate) mini_player_restore_size: Option<eframe::egui::Vec2>,
    pub(crate) active: Option<ActivePlayback>,
    pub(crate) quality: Option<u32>,
    pub(crate) qualities: Vec<u32>,
    pub(crate) quality_rx: Option<Receiver<Result<Vec<u32>, String>>>,
    pub(crate) quality_cancel: Option<process::Cancellation>,
    pub(crate) quality_error: Option<String>,
    pub(crate) seek_drag: Option<f64>,
    pub(crate) overlay_last_activity: Instant,
    pub(crate) player_menu: Option<overlay::OverlayMenu>,
    pub(crate) last_subtitle_id: Option<String>,
    pub(crate) diagnostics: DecodingDiagnostics,
    pub(crate) last_diagnostics_poll_at: Option<Instant>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            fullscreen: false,
            theater: false,
            window_visible: false,
            active_buffer: None,
            mini_player: false,
            mini_player_restore_size: None,
            active: None,
            quality: None,
            qualities: Vec::new(),
            quality_rx: None,
            quality_cancel: None,
            quality_error: None,
            seek_drag: None,
            overlay_last_activity: Instant::now(),
            player_menu: None,
            last_subtitle_id: None,
            diagnostics: DecodingDiagnostics::unknown(Some(crate::DEFAULT_CONFIGURED_HWDEC)),
            last_diagnostics_poll_at: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct SettingsState {
    pub(crate) buffer: BufferSettingsState,
    pub(crate) playback: TrackedSettings<PlaybackPreferences>,
    pub(crate) theme: TrackedSettings<Theme>,
    pub(crate) applied_theme: Option<Theme>,
}

#[derive(Default)]
pub(crate) struct BufferSettingsState {
    pub(crate) profile: BufferProfile,
    pub(crate) path: Option<PathBuf>,
    pub(crate) error: Option<String>,
    pub(crate) notice: Option<String>,
}

pub(crate) struct TrackedSettings<T> {
    pub(crate) value: T,
    pub(crate) path: Option<PathBuf>,
    pub(crate) loaded_path: Option<PathBuf>,
    pub(crate) error: Option<String>,
}

impl<T: Default> Default for TrackedSettings<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            path: None,
            loaded_path: None,
            error: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct HistoryState {
    pub(crate) data: History,
    pub(crate) path: Option<PathBuf>,
    pub(crate) loaded_path: Option<PathBuf>,
    pub(crate) read_failed: bool,
    pub(crate) error: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) save: HistorySaveState,
}

#[derive(Default)]
pub(crate) struct HistorySaveState {
    pub(crate) last_at: Option<Instant>,
    pub(crate) last_position_ms: Option<u64>,
    pub(crate) last_duration_ms: Option<Option<u64>>,
}

pub(crate) struct QueueState {
    pub(crate) data: Queue,
    pub(crate) path: Option<PathBuf>,
    pub(crate) read_failed: bool,
    pub(crate) persistence_error: Option<String>,
    pub(crate) eof_handled: bool,
    pub(crate) error: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) last_rejected_video: Option<Video>,
    pub(crate) pending_video: Option<Video>,
    pub(crate) pending_entry_id: Option<u64>,
    pub(crate) correlation: QueuePlaybackCorrelation,
    pub(crate) pending_advance: bool,
}

impl Default for QueueState {
    fn default() -> Self {
        Self {
            data: Queue::new(crate::DEFAULT_QUEUE_LIMIT),
            path: None,
            read_failed: false,
            persistence_error: None,
            eof_handled: false,
            error: None,
            notice: None,
            last_rejected_video: None,
            pending_video: None,
            pending_entry_id: None,
            correlation: QueuePlaybackCorrelation::default(),
            pending_advance: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct LibraryState {
    pub(crate) data: crate::library::Library,
    pub(crate) path: Option<PathBuf>,
    pub(crate) error: Option<String>,
    pub(crate) bookmark_note: String,
    pub(crate) collection_name: String,
}

#[derive(Default)]
pub(crate) struct BackupState {
    pub(crate) path_text: String,
    pub(crate) status: Option<String>,
    pub(crate) confirm_import: bool,
    pub(crate) import_mode: BackupImportMode,
    pub(crate) preview: Option<BackupPreview>,
    pub(crate) pending_import: Option<ImportedUserData>,
}

#[derive(Default)]
pub(crate) struct NavigationUiState {
    pub(crate) navigation_open: bool,
    pub(crate) search_results_mode: bool,
    pub(crate) settings_open: bool,
    pub(crate) shortcuts_open: bool,
    pub(crate) subscriptions_expanded: bool,
    pub(crate) browse_section: BrowseSection,
}

#[derive(Default)]
pub(crate) struct WatchState {
    pub(crate) extras: crate::watch_extras::WatchExtras,
    pub(crate) share_copied_at: Option<Instant>,
}

pub(crate) fn load_playback_settings_from_path(path: &Path) -> Result<PlaybackPreferences, String> {
    settings::load_playback(path).map_err(|error| {
        format!(
            "Erro ao ler preferências de reprodução em '{}' ({error}); usando 1× e qualidade automática.",
            path.display()
        )
    })
}

pub(crate) fn load_theme_settings_from_path(path: &Path) -> Result<Theme, String> {
    settings::load_theme(path).map_err(|error| {
        format!(
            "Erro ao ler tema da interface em '{}' ({error}); usando o tema Escuro padrão.",
            path.display()
        )
    })
}

pub(crate) fn load_history_from_path(path: &Path) -> Result<History, String> {
    history::load(path).map_err(|error| {
        format!(
            "Erro ao ler o histórico local em '{}' ({error}). O arquivo existente foi preservado sem sobrescrita.",
            path.display()
        )
    })
}

impl SettingsState {
    pub(crate) fn ensure_playback_synchronized(&mut self) {
        if self.playback.loaded_path == self.playback.path {
            return;
        }
        let had_previous_path = self.playback.loaded_path.is_some();
        self.playback.loaded_path = self.playback.path.clone();
        let Some(path) = self.playback.path.clone() else {
            return;
        };
        match load_playback_settings_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.playback.value == PlaybackPreferences::default() {
                    self.playback.value = loaded;
                }
                self.playback.error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.playback.value = PlaybackPreferences::default();
                }
                self.playback.error = Some(error);
            }
        }
    }

    pub(crate) fn apply_playback_preferences(&mut self, preferences: PlaybackPreferences) {
        self.ensure_playback_synchronized();
        self.playback.value = preferences;
        let save_result = match self.playback.path.clone().ok_or_else(|| {
            if cfg!(test) {
                Err("Caminho de preferências de reprodução não configurado no teste".to_owned())
            } else {
                settings::playback_path().map_err(|error| error.to_string())
            }
        }) {
            Ok(path) => settings::save_playback(&path, preferences).map_err(|error| {
                format!(
                    "Não foi possível salvar preferências de reprodução em '{}' ({error}); a escolha vale apenas nesta sessão.",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save_playback(&path, preferences).map_err(|error| {
                    format!(
                        "Não foi possível salvar preferências de reprodução em '{}' ({error}); a escolha vale apenas nesta sessão.",
                        path.display()
                    )
                });
                self.playback.path = Some(path.clone());
                self.playback.loaded_path = Some(path);
                res
            }
            Err(Err(error)) => Err(format!(
                "Não foi possível salvar preferências de reprodução ({error}); a escolha vale apenas nesta sessão."
            )),
        };
        self.playback.error = save_result.err();
    }

    pub(crate) fn ensure_theme_synchronized(&mut self) {
        if self.theme.loaded_path == self.theme.path {
            return;
        }
        let had_previous_path = self.theme.loaded_path.is_some();
        self.theme.loaded_path = self.theme.path.clone();
        let Some(path) = self.theme.path.clone() else {
            return;
        };
        match load_theme_settings_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.theme.value == Theme::default() {
                    self.theme.value = loaded;
                    self.applied_theme = None;
                }
                self.theme.error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.theme.value = Theme::default();
                    self.applied_theme = None;
                }
                self.theme.error = Some(error);
            }
        }
    }

    pub(crate) fn sync_theme_visuals(&mut self, ctx: &egui::Context) {
        self.ensure_theme_synchronized();
        let want_dark = self.theme.value == Theme::Dark;
        if self.applied_theme != Some(self.theme.value)
            || ctx.global_style().visuals.dark_mode != want_dark
        {
            apply_theme_visuals(ctx, self.theme.value);
            self.applied_theme = Some(self.theme.value);
        }
    }

    pub(crate) fn apply_theme(&mut self, theme: Theme) {
        self.ensure_theme_synchronized();
        self.theme.value = theme;
        self.applied_theme = None;
        let save_result = match self.theme.path.clone().ok_or_else(|| {
            if cfg!(test) {
                Err("Caminho de configurações de tema não configurado no teste".to_owned())
            } else {
                settings::theme_path().map_err(|error| error.to_string())
            }
        }) {
            Ok(path) => settings::save_theme(&path, theme).map_err(|error| {
                format!(
                    "Não foi possível salvar o tema em '{}' ({error}); a escolha vale apenas nesta sessão.",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save_theme(&path, theme).map_err(|error| {
                    format!(
                        "Não foi possível salvar o tema em '{}' ({error}); a escolha vale apenas nesta sessão.",
                        path.display()
                    )
                });
                self.theme.path = Some(path.clone());
                self.theme.loaded_path = Some(path);
                res
            }
            Err(Err(error)) => Err(format!(
                "Não foi possível salvar o tema ({error}); a escolha vale apenas nesta sessão."
            )),
        };
        self.theme.error = save_result.err();
    }

    pub(crate) fn palette(&self) -> ThemePalette {
        ThemePalette::for_theme(self.theme.value)
    }

    pub(crate) fn overlay_style(&self) -> overlay::OverlayStyle {
        let _ = self.theme.value;
        overlay::OverlayStyle {
            panel: Color32::from_rgba_unmultiplied(20, 20, 22, 238),
            icon: Color32::from_rgb(248, 248, 250),
            text: Color32::from_rgb(244, 244, 246),
            accent: Color32::from_rgb(255, 0, 51),
        }
    }

    pub(crate) fn apply_buffer_profile(
        &mut self,
        profile: BufferProfile,
        pending_restart: bool,
    ) -> bool {
        self.buffer.profile = profile;
        let save_result = match self.buffer.path.clone().ok_or_else(|| {
            settings::settings_path().map_err(|error| {
                format!("Não foi possível determinar o caminho para salvar configurações: {error}")
            })
        }) {
            Ok(path) => settings::save(&path, profile).map_err(|error| {
                format!(
                    "Não foi possível salvar as configurações em '{}': {error}",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save(&path, profile).map_err(|error| {
                    format!(
                        "Não foi possível salvar as configurações em '{}': {error}",
                        path.display()
                    )
                });
                self.buffer.path = Some(path);
                res
            }
            Err(Err(message)) => Err(message),
        };

        match save_result {
            Ok(()) => {
                self.buffer.error = None;
                self.buffer.notice = Some(if pending_restart {
                    format!(
                        "Perfil {} salvo. Como o player já está ativo, a alteração entrará em vigor após fechar o vídeo atual (reiniciar o player) e abrir o próximo vídeo.",
                        buffer_profile_summary(profile)
                    )
                } else {
                    format!(
                        "Perfil {} salvo e pronto para ser aplicado ao iniciar o player no próximo vídeo.",
                        buffer_profile_summary(profile)
                    )
                });
                true
            }
            Err(error) => {
                self.buffer.error = Some(format!(
                    "{error} O perfil {} permanece selecionado apenas nesta sessão.",
                    buffer_profile_name(profile)
                ));
                self.buffer.notice = if pending_restart {
                    Some(
                        "O player já está ativo: feche o vídeo atual (reinicie o player) para aplicar o novo perfil na sessão."
                            .into(),
                    )
                } else {
                    None
                };
                false
            }
        }
    }
}

impl HistoryState {
    pub(crate) fn ensure_synchronized(&mut self) -> bool {
        if self.loaded_path == self.path {
            return false;
        }
        let had_previous_path = self.loaded_path.is_some();
        self.loaded_path = self.path.clone();
        let Some(path) = self.path.clone() else {
            self.read_failed = false;
            return false;
        };
        let mut invalidated = false;
        match load_history_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.data.entries().is_empty() {
                    self.data = loaded;
                    invalidated = true;
                }
                self.read_failed = false;
                self.error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.data = History::default();
                    invalidated = true;
                }
                self.read_failed = true;
                self.error = Some(error);
            }
        }
        invalidated
    }

    pub(crate) fn save_to_disk(
        &mut self,
        now: Instant,
        active: Option<&ActivePlayback>,
    ) -> (bool, bool) {
        let invalidated = self.ensure_synchronized();
        if self.read_failed {
            return (false, invalidated);
        }
        let path = match self.path.clone().ok_or_else(|| {
            history::history_path().map_err(|error| {
                format!("Não foi possível determinar o caminho para salvar o histórico: {error}")
            })
        }) {
            Ok(path) => path,
            Err(Ok(path)) => {
                let mut loaded = match load_history_from_path(&path) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        self.path = Some(path.clone());
                        self.loaded_path = Some(path);
                        self.read_failed = true;
                        self.error = Some(error);
                        return (false, invalidated);
                    }
                };
                for entry in self.data.entries().iter().rev() {
                    let _ = loaded.record(entry.clone());
                }
                self.data = loaded;
                self.path = Some(path.clone());
                self.loaded_path = Some(path.clone());
                path
            }
            Err(Err(message)) => {
                self.error = Some(message);
                return (false, invalidated);
            }
        };
        if let Err(error) = load_history_from_path(&path) {
            self.read_failed = true;
            self.error = Some(error);
            return (false, invalidated);
        }
        let saved = match history::save(&path, &self.data) {
            Ok(()) => {
                self.error = None;
                self.dirty = false;
                self.save.last_at = Some(now);
                if let Some(active) = active {
                    self.save.last_position_ms = Some(active.position_ms);
                    self.save.last_duration_ms = Some(active.duration_ms);
                }
                true
            }
            Err(error) => {
                self.error = Some(format!(
                    "Não foi possível salvar o histórico em '{}': {error}",
                    path.display()
                ));
                false
            }
        };
        (saved, invalidated)
    }

    pub(crate) fn planned_resume_ms(&mut self, video: &Video) -> (Option<u64>, bool) {
        let invalidated = self.ensure_synchronized();
        let Some(video_id) = history::youtube_video_id(&video.url) else {
            return (None, invalidated);
        };
        let fallback_duration_ms = parse_duration_ms(&video.duration);
        let Some(entry) = self.data.get(&video_id) else {
            return (None, invalidated);
        };
        let resume = if entry.duration_ms.is_some() {
            self.data.resume_position_ms(&video_id)
        } else {
            history::should_resume(entry.position_ms, fallback_duration_ms)
        };
        (resume, invalidated)
    }

    pub(crate) fn should_persist_now(
        &self,
        active: Option<&ActivePlayback>,
        now: Instant,
        force: bool,
    ) -> bool {
        if !self.dirty || self.read_failed {
            return false;
        }
        if force {
            return true;
        }
        let Some(last_save) = self.save.last_at else {
            return true;
        };
        if now.saturating_duration_since(last_save) < HISTORY_SAVE_INTERVAL {
            return false;
        }
        let Some(active) = active else {
            return true;
        };
        let pos_changed = self.save.last_position_ms.is_none_or(|saved| {
            active.position_ms.abs_diff(saved) >= HISTORY_MIN_POSITION_DELTA_MS
        });
        let dur_changed = self
            .save
            .last_duration_ms
            .is_none_or(|saved| saved != active.duration_ms);
        pos_changed || dur_changed
    }

    #[cfg(test)]
    pub(crate) fn record_video(
        &mut self,
        video: &Video,
        position_ms: u64,
        duration_ms: Option<u64>,
    ) -> (std::io::Result<()>, bool) {
        let invalidated = self.ensure_synchronized();
        let Some(video_id) = history::youtube_video_id(&video.url) else {
            return (
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "apenas URLs de vídeo do YouTube com ID validado são aceitas no histórico",
                )),
                invalidated,
            );
        };
        let duration_ms = duration_ms
            .or_else(|| parse_duration_ms(&video.duration))
            .filter(|&dur| dur > 0);
        let position_ms = duration_ms
            .map(|dur| position_ms.min(dur))
            .unwrap_or(position_ms);
        let entry = crate::history::Entry {
            video_id: video_id.clone(),
            title: crate::sanitize_history_title(&video.title, &video_id),
            channel: crate::sanitize_history_channel(&video.channel),
            position_ms,
            duration_ms,
        };
        match self.data.record(entry) {
            Ok(()) => {
                self.dirty = true;
                (Ok(()), true)
            }
            Err(error) => (Err(error), invalidated),
        }
    }
}

impl ChannelState {
    pub(crate) fn is_loading(&self) -> bool {
        self.videos.active_request_id.is_some()
    }

    pub(crate) fn is_more_videos_loading(&self) -> bool {
        self.videos.active_more_request_id.is_some()
    }

    pub(crate) fn is_playlists_loading(&self) -> bool {
        self.playlists.active_request_id.is_some()
    }

    pub(crate) fn is_playlist_videos_loading(&self) -> bool {
        self.playlist_detail.active_request_id.is_some()
    }

    pub(crate) fn is_about_loading(&self) -> bool {
        self.about.active_request_id.is_some()
    }

    pub(crate) fn current_target(&self) -> Option<ChannelTarget> {
        self.loading_target
            .clone()
            .or_else(|| self.page.as_ref().map(|page| page.target.clone()))
            .or_else(|| self.last_target.clone())
    }

    pub(crate) fn cancel_subrequests(&mut self) {
        if let Some(cancel) = self.videos.more_cancel.take() {
            cancel.cancel();
        }
        self.videos.more_rx = None;
        self.videos.active_more_request_id = None;
        if let Some(cancel) = self.playlists.cancel.take() {
            cancel.cancel();
        }
        self.playlists.rx = None;
        self.playlists.active_request_id = None;
        if let Some(cancel) = self.playlist_detail.cancel.take() {
            cancel.cancel();
        }
        self.playlist_detail.rx = None;
        self.playlist_detail.active_request_id = None;
        if let Some(cancel) = self.about.cancel.take() {
            cancel.cancel();
        }
        self.about.rx = None;
        self.about.active_request_id = None;
    }

    pub(crate) fn cancel_loading(&mut self) {
        if let Some(cancel) = self.videos.cancel.take() {
            cancel.cancel();
        }
        self.videos.active_request_id = None;
        self.loading_target = None;
        self.videos.rx = None;
        self.cancel_subrequests();
    }

    pub(crate) fn close_view(&mut self) {
        if let Some(cancel) = self.videos.cancel.take() {
            cancel.cancel();
        }
        self.cancel_subrequests();
        self.view_open = false;
        self.videos.active_request_id = None;
        self.loading_target = None;
        self.videos.rx = None;
    }

    pub(crate) fn cancel_more_videos(&mut self) {
        if let Some(cancel) = self.videos.more_cancel.take() {
            cancel.cancel();
        }
        self.videos.more_rx = None;
        self.videos.active_more_request_id = None;
    }

    pub(crate) fn cancel_playlists(&mut self) {
        if let Some(cancel) = self.playlists.cancel.take() {
            cancel.cancel();
        }
        self.playlists.rx = None;
        self.playlists.active_request_id = None;
    }

    pub(crate) fn cancel_playlist_videos(&mut self) {
        if let Some(cancel) = self.playlist_detail.cancel.take() {
            cancel.cancel();
        }
        self.playlist_detail.rx = None;
        self.playlist_detail.active_request_id = None;
    }

    pub(crate) fn cancel_about(&mut self) {
        if let Some(cancel) = self.about.cancel.take() {
            cancel.cancel();
        }
        self.about.rx = None;
        self.about.active_request_id = None;
    }

    pub(crate) fn begin_more_videos_request(
        &mut self,
    ) -> Option<(u64, ChannelTarget, usize, usize)> {
        if self.is_loading() || self.is_more_videos_loading() {
            return None;
        }
        let page = self.page.as_ref()?;
        let current_len = page.videos.len();
        if current_len >= search::MAX_CHANNEL_VIDEOS_LIMIT {
            self.videos.reached_max = true;
            self.videos.has_more = false;
            return None;
        }
        let start = current_len + 1;
        let remaining = search::MAX_CHANNEL_VIDEOS_LIMIT.saturating_sub(current_len);
        let count = remaining.min(search::CHANNEL_VIDEOS_PAGE_STEP);
        if count == 0 {
            self.videos.reached_max = true;
            self.videos.has_more = false;
            return None;
        }
        let target = page.target.clone();
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        self.videos.active_more_request_id = Some(request_id);
        self.videos.more_error = None;
        Some((request_id, target, start, count))
    }

    pub(crate) fn begin_playlists_request(&mut self) -> Option<(u64, ChannelTarget)> {
        if self.is_playlists_loading() {
            return None;
        }
        let target = self
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_target())?;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        self.playlists.active_request_id = Some(request_id);
        self.playlists.error = None;
        Some((request_id, target))
    }

    pub(crate) fn begin_about_request(&mut self) -> Option<(u64, ChannelTarget)> {
        if self.is_about_loading() {
            return None;
        }
        let target = self
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_target())?;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = self.next_request_id;
        self.about.active_request_id = Some(request_id);
        self.about.error = None;
        Some((request_id, target))
    }
}

impl QueueState {
    pub(crate) fn persist(&mut self) {
        if self.read_failed {
            return;
        }
        if let Some(path) = &self.path {
            if let Err(error) = self.data.save(path) {
                self.persistence_error = Some(format!("Não foi possível salvar a fila: {error}"));
            } else {
                self.persistence_error = None;
            }
        }
    }
}
