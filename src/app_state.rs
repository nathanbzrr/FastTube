use crate::feed_prefs;
use crate::process;
use crate::search;
use crate::{
    ActivePlayback, BackupImportMode, BackupPreview, BrowseSection, BufferProfile,
    ChannelAboutFetchMessage, ChannelFetchMessage, ChannelMoreVideosFetchMessage, ChannelPage,
    ChannelPlaylistVideosFetchMessage, ChannelPlaylistsFetchMessage, ChannelTab,
    DecodingDiagnostics, FeedDerivedCache, FeedFilter, History, HomeFetchMessage, ImportedUserData,
    PlaybackPreferences, Queue, QueuePlaybackCorrelation, SearchFetchMessage, SearchResult, Theme,
    Video,
};
use std::collections::HashMap;
use std::path::PathBuf;
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

#[derive(Default)]
pub(crate) struct AuthState {
    pub(crate) reauth_rx: Option<Receiver<Result<crate::oauth::TokenSession, String>>>,
    pub(crate) reauth_cancel: Option<process::Cancellation>,
    pub(crate) authenticated: bool,
    pub(crate) session: Option<crate::oauth::TokenSession>,
    pub(crate) ratings: HashMap<String, crate::oauth::VideoRating>,
    pub(crate) rating_rx:
        Option<Receiver<Result<(String, Option<crate::oauth::VideoRating>), String>>>,
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
}
