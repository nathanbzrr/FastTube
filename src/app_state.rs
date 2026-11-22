use crate::async_task::{AsyncStreamTask, AsyncTask};
use crate::community_apis;
use crate::downloads;
use crate::feed_prefs;
use crate::followed_channels;
use crate::history::{self, FileFingerprint, History};
use crate::overlay;
use crate::persistence::{PersistenceSlot, PersistenceWorker};
use crate::playlist_sync;
use crate::process;
use crate::queue;
use crate::search::{self, ChannelTarget};
use crate::settings;
use crate::sponsorblock;
use crate::updater;
use crate::{
    ActivePlayback, BackupImportMode, BackupPreview, BrowseSection, BufferProfile,
    ChannelAboutFetchMessage, ChannelFetchMessage, ChannelMoreVideosFetchMessage, ChannelPage,
    ChannelPlaylistVideosFetchMessage, ChannelPlaylistsFetchMessage, ChannelSearchFetchMessage,
    ChannelSortedVideosFetchMessage, ChannelTab, ChannelTabVideosFetchMessage, DecodingDiagnostics,
    FeedDerivedCache, FeedFilter, HISTORY_MIN_POSITION_DELTA_MS, HISTORY_SAVE_INTERVAL,
    HomeBatchFetchMessage, HomeFetchMessage, ImportedUserData, LikedVideosFetchMessage,
    PlaybackPreferences, Queue, QueuePlaybackCorrelation, SearchFetchMessage, SearchResult, Theme,
    ThemePalette, Video, apply_theme_visuals, buffer_profile_name, buffer_profile_summary,
    parse_duration_ms,
};
use eframe::egui::{self, Color32};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;

pub(crate) const MAX_SEARCH_SUGGEST_PREFIX_CACHE: usize = 64;

pub(crate) struct InnerTubeHomeFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) append: bool,
    pub(crate) is_default_home: bool,
    pub(crate) result: Result<crate::innertube::InnerTubeHomePage, String>,
}

pub(crate) struct InnerTubeSubscriptionsFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) append: bool,
    pub(crate) result: Result<crate::innertube::InnerTubeSubscriptionsPage, String>,
}

pub(crate) struct SearchPageFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) append: bool,
    pub(crate) result: Result<search::SearchPageOutput, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChannelContinuationTarget {
    VideosAppend,
    VideosReplace,
    ShortsAppend,
    ShortsReplace,
    StreamsAppend,
    StreamsReplace,
    DynamicTab(crate::innertube::ChannelTabKind),
}

pub(crate) struct ChannelContinuationFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) mode: ChannelContinuationTarget,
    pub(crate) result: Result<search::ChannelTabPage, String>,
}

pub(crate) struct StreamManifestFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) video_id: String,
    pub(crate) video_url: String,
    pub(crate) resume_target_ms: Option<u64>,
    pub(crate) result: Result<std::sync::Arc<crate::stream_manifest::StreamManifest>, String>,
}

pub(crate) struct WatchNextFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) video_url: String,
    pub(crate) chip_index: Option<usize>,
    pub(crate) is_chip_reload: bool,
    pub(crate) result: Result<crate::innertube::InnerTubeWatchNext, String>,
}

pub(crate) struct InnerTubeCommentsFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) video_url: String,
    pub(crate) append: bool,
    pub(crate) result: Result<crate::innertube::InnerTubeCommentsPage, String>,
}

pub(crate) struct ZeroQuotaFeedbackMessage {
    pub(crate) request_id: u64,
    pub(crate) not_interested: bool,
    pub(crate) result: Result<(), String>,
}

pub(crate) struct NotificationsInboxFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) append: bool,
    pub(crate) web_session: Option<crate::comment_actions::YouTubeWebSession>,
    pub(crate) auth_expired: bool,
    pub(crate) needs_connect: bool,
    pub(crate) result: Result<crate::innertube::InnerTubeNotificationsMenu, String>,
}

pub(crate) struct ExploreHubFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) append: bool,
    pub(crate) result: Result<crate::innertube::ExploreHubPage, String>,
}

pub(crate) struct ShortsSequenceFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) result: Result<crate::innertube::ShortsSequencePage, String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CommentMutationKind {
    Edit,
    Delete,
}

pub(crate) struct CommentMutationFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) kind: CommentMutationKind,
    pub(crate) result: Result<(), String>,
}

pub(crate) struct LiveChatReplayFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) session_id: u64,
    pub(crate) result: Result<crate::live_chat::LiveChatBatch, String>,
}

pub(crate) struct BrowseFeedState {
    pub(crate) query: String,
    pub(crate) videos: Vec<Video>,
    pub(crate) generation: u64,
    pub(crate) signal_revision: u64,
    pub(crate) metadata_revision: u64,
    pub(crate) filter: FeedFilter,
    pub(crate) cache_dirty: bool,
    pub(crate) cache: FeedDerivedCache,
    pub(crate) search: SearchJob,
    pub(crate) home: HomeFeedState,
    pub(crate) search_filters: search::SearchFilters,
    pub(crate) search_chip: search::SearchChip,
    pub(crate) search_filters_open: bool,
    pub search_continuation: Option<String>,
    pub search_loading_more: bool,
    pub search_spelling: Option<crate::innertube::SearchSpellingCorrection>,
    pub search_filters_modal_open: bool,
    pub home_continuation: Option<String>,
    pub home_loading_more: bool,
    pub home_chips: Vec<crate::innertube::HomeFeedChip>,
    pub subscriptions_continuation: Option<String>,
    pub subscriptions_loading_more: bool,
    pub(crate) home_swr_hydrated: bool,
    pub(crate) home_innertube_task: AsyncTask<InnerTubeHomeFetchMessage>,
    pub(crate) subs_innertube_task: AsyncTask<InnerTubeSubscriptionsFetchMessage>,
    pub(crate) search_page_task: AsyncTask<SearchPageFetchMessage>,
    pub(crate) search_page_last_query: String,
    pub(crate) search_save_modal_video: Option<Video>,
    pub(crate) search_report_modal_video: Option<Video>,
    pub(crate) search_report_reason: Option<usize>,
    pub(crate) search_report_details: String,
    pub(crate) search_reported_urls: HashSet<String>,
    pub(crate) share_modal_video: Option<Video>,
    pub(crate) share_start_at_enabled: bool,
    pub(crate) share_start_at_text: String,
    pub(crate) share_embed_open: bool,
    pub(crate) share_targets_scroll_offset: f32,
    pub(crate) share_modal_copied_at: Option<Instant>,
    pub(crate) recent_searches: Vec<String>,
    pub(crate) search_suggest: SearchSuggestState,
    pub(crate) report_history: Vec<ReportedContentEntry>,
    pub(crate) report_history_filter: u8,
    pub(crate) report_history_path: Option<PathBuf>,
    pub(crate) notifications: Vec<crate::innertube::InnerTubeNotification>,
    pub(crate) notifications_loading: bool,
    pub(crate) notifications_loading_more: bool,
    pub(crate) notifications_loaded: bool,
    pub(crate) notifications_initial_sync_attempted: bool,
    pub(crate) notifications_continuation: Option<String>,
    pub(crate) notifications_error: Option<String>,
    pub(crate) notifications_needs_connect: bool,
    pub(crate) notifications_task: AsyncTask<NotificationsInboxFetchMessage>,
    pub(crate) explore_category: crate::innertube::ExploreCategory,
    pub(crate) explore_loading: bool,
    pub(crate) explore_loading_more: bool,
    pub(crate) explore_continuation: Option<String>,
    pub(crate) explore_loaded_category: Option<crate::innertube::ExploreCategory>,
    pub(crate) explore_task: AsyncTask<ExploreHubFetchMessage>,
    pub(crate) shorts_continuation: Option<String>,
    pub(crate) shorts_task: AsyncTask<ShortsSequenceFetchMessage>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReportedContentEntry {
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) channel: String,
    pub(crate) video_id: Option<String>,
    pub(crate) reason: String,
    pub(crate) timestamp_label: String,
    pub(crate) status: String,
}

impl Default for BrowseFeedState {
    fn default() -> Self {
        Self {
            query: String::new(),
            videos: Vec::new(),
            generation: 0,
            signal_revision: 0,
            metadata_revision: 0,
            filter: FeedFilter::All,
            cache_dirty: true,
            cache: FeedDerivedCache::default(),
            search: SearchJob::default(),
            home: HomeFeedState::default(),
            search_filters: search::SearchFilters::default(),
            search_chip: search::SearchChip::default(),
            search_filters_open: false,
            search_continuation: None,
            search_loading_more: false,
            search_spelling: None,
            search_filters_modal_open: false,
            home_continuation: None,
            home_loading_more: false,
            home_chips: Vec::new(),
            subscriptions_continuation: None,
            subscriptions_loading_more: false,
            home_swr_hydrated: false,
            home_innertube_task: AsyncTask::new(),
            subs_innertube_task: AsyncTask::new(),
            search_page_task: AsyncTask::new(),
            search_page_last_query: String::new(),
            search_save_modal_video: None,
            search_report_modal_video: None,
            search_report_reason: None,
            search_report_details: String::new(),
            search_reported_urls: HashSet::new(),
            share_modal_video: None,
            share_start_at_enabled: false,
            share_start_at_text: "0:00".to_owned(),
            share_embed_open: false,
            share_targets_scroll_offset: 0.0,
            share_modal_copied_at: None,
            recent_searches: Vec::new(),
            search_suggest: SearchSuggestState::default(),
            report_history: Vec::new(),
            report_history_filter: 0,
            report_history_path: None,
            notifications: Vec::new(),
            notifications_loading: false,
            notifications_loading_more: false,
            notifications_loaded: false,
            notifications_initial_sync_attempted: false,
            notifications_continuation: None,
            notifications_error: None,
            notifications_needs_connect: false,
            notifications_task: AsyncTask::new(),
            explore_category: crate::innertube::ExploreCategory::default(),
            explore_loading: false,
            explore_loading_more: false,
            explore_continuation: None,
            explore_loaded_category: None,
            explore_task: AsyncTask::new(),
            shorts_continuation: None,
            shorts_task: AsyncTask::new(),
        }
    }
}

pub(crate) struct SearchSuggestFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) query: String,
    pub(crate) result: Result<Vec<String>, String>,
}

#[derive(Default)]
pub(crate) struct SearchSuggestState {
    pub(crate) open: bool,
    pub(crate) last_input_query: String,
    pub(crate) last_input_at: Option<Instant>,
    pub(crate) pending_fetch_query: Option<String>,
    pub(crate) last_fetched_query: String,
    pub(crate) remote_suggestions: Vec<String>,
    pub(crate) prefix_cache: HashMap<String, Vec<String>>,
    pub(crate) prefix_lru: VecDeque<String>,
    pub(crate) recent_revision: u64,
    pub(crate) remote_revision: u64,
    pub(crate) merged_cache_key: Option<(String, u64, u64, usize)>,
    pub(crate) merged_cache: Vec<search::SearchSuggestionItem>,
    pub(crate) highlighted_index: Option<usize>,
    pub(crate) task: AsyncTask<SearchSuggestFetchMessage>,
}

impl SearchSuggestState {
    fn normalize_prefix_key(prefix: &str) -> String {
        prefix.trim().to_ascii_lowercase()
    }

    pub(crate) fn set_remote_suggestions(&mut self, items: Vec<String>) {
        if self.remote_suggestions != items {
            self.remote_suggestions = items;
            self.remote_revision = self.remote_revision.wrapping_add(1);
        }
    }

    pub(crate) fn clear_remote_suggestions(&mut self) {
        if !self.remote_suggestions.is_empty() {
            self.remote_suggestions.clear();
            self.remote_revision = self.remote_revision.wrapping_add(1);
        }
    }

    pub(crate) fn bump_recent_revision(&mut self) {
        self.recent_revision = self.recent_revision.wrapping_add(1);
    }

    pub(crate) fn get_cached_prefix(&mut self, prefix: &str) -> Option<Vec<String>> {
        let key = Self::normalize_prefix_key(prefix);
        let hit = self.prefix_cache.get(&key)?.clone();
        self.prefix_lru.retain(|existing| existing != &key);
        self.prefix_lru.push_back(key);
        Some(hit)
    }

    pub(crate) fn insert_cached_prefix(&mut self, prefix: &str, items: Vec<String>) {
        let key = Self::normalize_prefix_key(prefix);
        if key.is_empty() {
            return;
        }
        self.prefix_lru.retain(|existing| existing != &key);
        self.prefix_lru.push_back(key.clone());
        self.prefix_cache.insert(key, items);
        while self.prefix_cache.len() > MAX_SEARCH_SUGGEST_PREFIX_CACHE {
            let Some(victim) = self.prefix_lru.pop_front() else {
                break;
            };
            self.prefix_cache.remove(&victim);
        }
    }

    pub(crate) fn merged_suggestions(
        &mut self,
        query: &str,
        recent_searches: &[String],
        limit: usize,
    ) -> &[search::SearchSuggestionItem] {
        let cache_hit = self.merged_cache_key.as_ref().is_some_and(
            |(cached_query, cached_recent, cached_remote, cached_limit)| {
                cached_query == query
                    && *cached_recent == self.recent_revision
                    && *cached_remote == self.remote_revision
                    && *cached_limit == limit
            },
        );
        if !cache_hit {
            self.merged_cache = search::merge_search_suggestions(
                query,
                recent_searches,
                &self.remote_suggestions,
                limit,
            );
            self.merged_cache_key = Some((
                query.to_owned(),
                self.recent_revision,
                self.remote_revision,
                limit,
            ));
        }
        &self.merged_cache
    }
}

#[derive(Default)]
pub(crate) struct SearchJob {
    pub(crate) task: AsyncTask<SearchFetchMessage>,
}

pub(crate) type SubscriptionChannelsPayload = (
    Option<crate::oauth::TokenSession>,
    Result<Vec<search::HomeChannel>, String>,
);

#[derive(Default)]
pub(crate) struct HomeFeedState {
    pub(crate) stream_task: AsyncStreamTask<HomeFetchMessage>,
    pub(crate) batch_task: AsyncTask<HomeBatchFetchMessage>,
    pub(crate) channels_task: AsyncTask<SubscriptionChannelsPayload>,
    pub(crate) initial_public_started: bool,
    pub(crate) playlist_urls: Vec<String>,
    pub(crate) playlist_total_results: Option<u64>,
    pub(crate) title: Option<String>,
    pub(crate) limit_reached: bool,
    pub(crate) subscription_channels: Vec<search::HomeChannel>,
    pub(crate) subscription_channels_loaded: bool,
    pub(crate) subscription_channels_error: Option<String>,
    pub(crate) subscriptions_cached_items: Vec<SearchResult>,
    pub(crate) subscriptions_cached_loaded: bool,
    pub(crate) subscriptions_cached_at: Option<Instant>,
    pub(crate) subscriptions_disk_cache_loaded: bool,
    pub(crate) subscriptions_disk_cache_path: Option<PathBuf>,
    pub(crate) subscriptions_rss_cache: HashMap<String, crate::oauth::ChannelRssCacheEntry>,
    pub(crate) subscriptions_progressive_streaming: bool,
    pub(crate) subscriptions_refreshing_in_background: bool,
    pub(crate) pending_items: Vec<SearchResult>,
    pub(crate) cached_items: Vec<SearchResult>,
    pub(crate) cached_playlists: Vec<String>,
    pub(crate) cached_loaded: bool,
    pub(crate) page: usize,
    pub(crate) has_more: bool,
    pub(crate) channel_avatars: HashMap<String, String>,
    pub(crate) shorts_shelf_hidden: bool,
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
    pub(crate) about_modal_open: bool,
    pub(crate) loading_target: Option<crate::ChannelTarget>,
    pub(crate) last_target: Option<crate::ChannelTarget>,
    pub(crate) error: Option<String>,
    pub(crate) video_limit: usize,
    pub(crate) from_cache: bool,
    pub(crate) videos: ChannelVideosState,
    pub(crate) shorts: ChannelTabFeedState,
    pub(crate) streams: ChannelTabFeedState,
    pub(crate) playlists: ChannelPlaylistsState,
    pub(crate) playlist_detail: ChannelPlaylistDetailState,
    pub(crate) about: ChannelAboutState,
    pub(crate) search: ChannelSearchState,
    pub(crate) community_posts: Vec<crate::innertube::ChannelCommunityPost>,
    pub(crate) upcoming_notify_urls: HashSet<String>,
    pub(crate) upcoming_start_unix_by_url: HashMap<String, i64>,
}

impl ChannelState {
    fn with_defaults() -> Self {
        Self {
            page: None,
            view_open: false,
            tab: ChannelTab::Home,
            about_modal_open: false,
            loading_target: None,
            last_target: None,
            error: None,
            video_limit: search::DEFAULT_CHANNEL_VIDEOS_LIMIT,
            from_cache: false,
            videos: ChannelVideosState::default(),
            shorts: ChannelTabFeedState::default(),
            streams: ChannelTabFeedState::default(),
            playlists: ChannelPlaylistsState::default(),
            playlist_detail: ChannelPlaylistDetailState::default(),
            about: ChannelAboutState::default(),
            search: ChannelSearchState::default(),
            community_posts: Vec::new(),
            upcoming_notify_urls: HashSet::new(),
            upcoming_start_unix_by_url: HashMap::new(),
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
    pub(crate) task: AsyncTask<ChannelFetchMessage>,
    pub(crate) more_task: AsyncTask<ChannelMoreVideosFetchMessage>,
    pub(crate) continuation_task: AsyncTask<ChannelContinuationFetchMessage>,
    pub(crate) more_error: Option<String>,
    pub(crate) has_more: bool,
    pub(crate) reached_max: bool,
    pub(crate) sort: search::ChannelVideosSort,
    pub(crate) sorted_items: Vec<SearchResult>,
    pub(crate) sorted_task: AsyncTask<ChannelSortedVideosFetchMessage>,
    pub(crate) sorted_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelSearchState {
    pub(crate) input_open: bool,
    pub(crate) query: String,
    pub(crate) items: Vec<SearchResult>,
    pub(crate) loaded: bool,
    pub(crate) task: AsyncTask<ChannelSearchFetchMessage>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelTabFeedState {
    pub(crate) items: Vec<SearchResult>,
    pub(crate) loaded: bool,
    pub(crate) task: AsyncTask<ChannelTabVideosFetchMessage>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelPlaylistsState {
    pub(crate) items: Vec<search::ChannelPlaylist>,
    pub(crate) loaded: bool,
    pub(crate) task: AsyncTask<ChannelPlaylistsFetchMessage>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelPlaylistDetailState {
    pub(crate) selected: Option<search::ChannelPlaylist>,
    pub(crate) items: Vec<SearchResult>,
    pub(crate) loaded: bool,
    pub(crate) task: AsyncTask<ChannelPlaylistVideosFetchMessage>,
    pub(crate) error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ChannelAboutState {
    pub(crate) info: Option<search::ChannelAboutInfo>,
    pub(crate) loaded: bool,
    pub(crate) task: AsyncTask<ChannelAboutFetchMessage>,
    pub(crate) error: Option<String>,
}

pub(crate) type RatingResultPayload = (
    Option<crate::oauth::TokenSession>,
    Result<(String, Option<crate::oauth::VideoRating>), String>,
);

pub(crate) type SubscriptionResultPayload = (
    Option<crate::oauth::TokenSession>,
    Result<(String, crate::oauth::SubscriptionStatus), String>,
);

pub(crate) type MyChannelResultPayload = (
    Option<crate::oauth::TokenSession>,
    Result<crate::oauth::MyChannelProfile, String>,
);

pub(crate) type RemotePlaylistsResultPayload = (
    Option<crate::oauth::TokenSession>,
    Result<Vec<playlist_sync::SyncedPlaylistCard>, String>,
);

pub(crate) type PlaylistActionResultPayload = (
    Option<crate::oauth::TokenSession>,
    Result<(Option<(String, String)>, String), String>,
);

pub(crate) type ReportResultPayload = (Option<crate::oauth::TokenSession>, Result<String, String>);

#[derive(Default)]
pub(crate) struct AuthState {
    pub(crate) reauth_task: AsyncTask<Result<crate::oauth::TokenSession, String>>,
    pub(crate) authenticated: bool,
    pub(crate) session: Option<crate::oauth::TokenSession>,
    pub(crate) ratings: HashMap<String, crate::oauth::VideoRating>,
    pub(crate) rating_task: AsyncTask<RatingResultPayload>,
    pub(crate) subscriptions: HashMap<String, crate::oauth::SubscriptionStatus>,
    pub(crate) sub_check_task: AsyncTask<SubscriptionResultPayload>,
    pub(crate) sub_toggle_task: AsyncTask<SubscriptionResultPayload>,
    pub(crate) my_channel: Option<ChannelTarget>,
    pub(crate) my_channel_title: Option<String>,
    pub(crate) my_channel_avatar_url: Option<String>,
    pub(crate) my_channel_subscribers: Option<u64>,
    pub(crate) my_channel_open_when_ready: bool,
    pub(crate) my_channel_fetch_attempted: bool,
    pub(crate) my_channel_task: AsyncTask<MyChannelResultPayload>,
    pub(crate) remote_playlists: Vec<(String, String)>,
    pub(crate) remote_playlists_task: AsyncTask<RemotePlaylistsResultPayload>,
    pub(crate) playlist_action_task: AsyncTask<PlaylistActionResultPayload>,
    pub(crate) report_task: AsyncTask<ReportResultPayload>,
    pub(crate) youtube_web_session: Option<crate::comment_actions::YouTubeWebSession>,
    pub(crate) youtube_web_session_checked: bool,
    pub(crate) cached_has_saved_profile: bool,
    pub(crate) restore_attempted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MiniPlayerPendingApply {
    pub(crate) target_size: egui::Vec2,
    pub(crate) target_pos: Option<egui::Pos2>,
    pub(crate) frames_left: u8,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PlayerNerdsStats {
    pub(crate) audio_codec: Option<String>,
    pub(crate) fps: Option<f64>,
    pub(crate) dropped_frames: Option<u64>,
    pub(crate) video_bitrate_bps: Option<f64>,
    pub(crate) audio_bitrate_bps: Option<f64>,
}

pub(crate) const AUTOSCROLL_DEADZONE_PX: f32 = 14.0;
pub(crate) const AUTOSCROLL_MAX_SPEED_PX_PER_SEC: f32 = 2400.0;

#[derive(Clone, Copy, Debug)]
struct ActiveAutoscroll {
    anchor: egui::Pos2,
    cursor_pos: egui::Pos2,
    middle_held: bool,
    moved_beyond_deadzone: bool,
}

#[derive(Default)]
pub(crate) struct AutoscrollState {
    active: Option<ActiveAutoscroll>,
    swallow_release_button: Option<egui::PointerButton>,
    last_pointer_pos: Option<egui::Pos2>,
}

impl AutoscrollState {
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn anchor(&self) -> Option<egui::Pos2> {
        self.active.map(|state| state.anchor)
    }

    pub(crate) fn cancel(&mut self) {
        self.active = None;
    }

    pub(crate) fn velocity_and_cursor(&self) -> Option<(egui::Vec2, egui::CursorIcon)> {
        let active = self.active?;
        let offset = active.cursor_pos - active.anchor;
        if offset.length() <= AUTOSCROLL_DEADZONE_PX {
            return Some((egui::Vec2::ZERO, egui::CursorIcon::AllScroll));
        }

        let speed_for = |excess: f32| {
            (excess * 4.5 + excess * excess * 0.035).min(AUTOSCROLL_MAX_SPEED_PX_PER_SEC)
        };
        let vy = if offset.y.abs() > AUTOSCROLL_DEADZONE_PX {
            let excess = offset.y.abs() - AUTOSCROLL_DEADZONE_PX;
            // egui subtracts smooth_scroll_delta from ScrollArea::offset, so down-drag needs negative delta.
            -offset.y.signum() * speed_for(excess)
        } else {
            0.0
        };
        let vx = if offset.x.abs() > 28.0 && offset.x.abs() > offset.y.abs() * 1.5 {
            let excess = offset.x.abs() - 28.0;
            -offset.x.signum() * speed_for(excess)
        } else {
            0.0
        };

        let angle = offset.y.atan2(offset.x).to_degrees();
        let cursor = if (-22.5..22.5).contains(&angle) {
            egui::CursorIcon::ResizeEast
        } else if (22.5..67.5).contains(&angle) {
            egui::CursorIcon::ResizeSouthEast
        } else if (67.5..112.5).contains(&angle) {
            egui::CursorIcon::ResizeSouth
        } else if (112.5..157.5).contains(&angle) {
            egui::CursorIcon::ResizeSouthWest
        } else if !(-157.5..157.5).contains(&angle) {
            egui::CursorIcon::ResizeWest
        } else if (-157.5..-112.5).contains(&angle) {
            egui::CursorIcon::ResizeNorthWest
        } else if (-112.5..-67.5).contains(&angle) {
            egui::CursorIcon::ResizeNorth
        } else {
            egui::CursorIcon::ResizeNorthEast
        };

        Some((egui::vec2(vx, vy), cursor))
    }

    pub(crate) fn is_scrolling(&self) -> bool {
        self.velocity_and_cursor()
            .is_some_and(|(vel, _)| vel != egui::Vec2::ZERO)
    }

    pub(crate) fn preprocess_raw_input(
        &mut self,
        raw_input: &mut egui::RawInput,
        allow_activate: bool,
        header_guard_y: f32,
    ) {
        if !allow_activate {
            self.active = None;
        }
        let events = std::mem::take(&mut raw_input.events);
        let mut out = Vec::with_capacity(events.len() + 1);

        for event in events {
            match &event {
                egui::Event::PointerMoved(pos) => self.last_pointer_pos = Some(*pos),
                egui::Event::PointerButton { pos, .. } => self.last_pointer_pos = Some(*pos),
                _ => {}
            }

            if self.active.is_none()
                && let egui::Event::PointerButton {
                    button,
                    pressed: false,
                    ..
                } = event
                && self.swallow_release_button == Some(button)
            {
                self.swallow_release_button = None;
                continue;
            }
            if self.active.is_none()
                && matches!(event, egui::Event::PointerButton { pressed: true, .. })
            {
                self.swallow_release_button = None;
            }

            if self.active.is_none() {
                if allow_activate
                    && let egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Middle,
                        pressed: true,
                        ..
                    } = event
                    && pos.y >= header_guard_y
                {
                    self.active = Some(ActiveAutoscroll {
                        anchor: pos,
                        cursor_pos: pos,
                        middle_held: true,
                        moved_beyond_deadzone: false,
                    });
                    out.push(egui::Event::PointerMoved(pos));
                    continue;
                }
                out.push(event);
                continue;
            }

            match event {
                egui::Event::PointerMoved(pos) => {
                    if let Some(active) = self.active.as_mut() {
                        active.cursor_pos = pos;
                        if (pos - active.anchor).length() >= AUTOSCROLL_DEADZONE_PX {
                            active.moved_beyond_deadzone = true;
                        }
                    }
                }
                egui::Event::PointerGone => {}
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Middle,
                    pressed: false,
                    ..
                } => {
                    let release_exits = if let Some(active) = self.active.as_mut() {
                        active.cursor_pos = pos;
                        active.middle_held = false;
                        active.moved_beyond_deadzone
                    } else {
                        false
                    };
                    if release_exits {
                        self.active = None;
                        out.push(egui::Event::PointerMoved(pos));
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    ..
                } => {
                    self.active = None;
                    self.swallow_release_button = Some(button);
                    out.push(egui::Event::PointerMoved(pos));
                }
                egui::Event::MouseWheel { .. } => {
                    let restore_pos = self
                        .active
                        .map_or(self.last_pointer_pos, |a| Some(a.cursor_pos));
                    self.active = None;
                    if let Some(pos) = restore_pos {
                        out.push(egui::Event::PointerMoved(pos));
                    }
                    out.push(event);
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    let restore_pos = self
                        .active
                        .map_or(self.last_pointer_pos, |a| Some(a.cursor_pos));
                    self.active = None;
                    if let Some(pos) = restore_pos {
                        out.push(egui::Event::PointerMoved(pos));
                    }
                }
                egui::Event::WindowFocused(false) => {
                    self.active = None;
                    out.push(event);
                }
                other => out.push(other),
            }
        }

        if let Some(active) = self.active {
            // Keep egui's pointer anchored inside the target ScrollArea even if the real pointer crosses other regions.
            out.push(egui::Event::PointerMoved(active.anchor));
        }

        raw_input.events = out;
    }
}

pub(crate) struct PlaybackState {
    pub(crate) fullscreen: bool,
    pub(crate) fullscreen_restore_maximized: bool,
    pub(crate) fullscreen_sync_retries: u8,
    pub(crate) theater: bool,
    pub(crate) window_visible: bool,
    pub(crate) active_buffer: Option<BufferProfile>,
    pub(crate) active_hwdec: Option<settings::HwdecMode>,
    pub(crate) mini_player: bool,
    pub(crate) mini_player_restore_size: Option<eframe::egui::Vec2>,
    pub(crate) mini_player_restore_pos: Option<eframe::egui::Pos2>,
    pub(crate) mini_player_restore_maximized: bool,
    pub(crate) mini_player_saved_size: Option<eframe::egui::Vec2>,
    pub(crate) mini_player_saved_pos: Option<eframe::egui::Pos2>,
    pub(crate) mini_player_pending_enter: Option<MiniPlayerPendingApply>,
    pub(crate) mini_player_settled: bool,
    /// Docked in-app miniplayer shown in the bottom-right corner while browsing other tabs.
    pub(crate) in_app_mini_player: bool,
    pub(crate) video_back_stack: Vec<Video>,
    pub(crate) video_forward_stack: Vec<Video>,
    pub(crate) pending_open_keep_mini_player: bool,
    pub(crate) pending_open_suppress_back_stack: bool,
    pub(crate) pending_open_keep_playlist: bool,
    pub(crate) mini_player_paused: bool,
    pub(crate) mini_player_muted: bool,
    pub(crate) mini_player_volume: f32,
    pub(crate) active: Option<ActivePlayback>,
    pub(crate) quality: Option<u32>,
    pub(crate) qualities: Vec<u32>,
    pub(crate) quality_task: AsyncTask<Result<search::VideoPlaybackMetadata, String>>,
    pub(crate) quality_error: Option<String>,
    pub(crate) quality_fallback_url: Option<String>,
    pub(crate) current_file_loaded: bool,
    pub(crate) cached_raw_path: Option<String>,
    pub(crate) cached_raw_path_poll: Option<Instant>,
    pub(crate) seek_drag: Option<f64>,
    pub(crate) overlay_last_activity: Instant,
    pub(crate) overlay_last_pointer_pos: Option<egui::Pos2>,
    pub(crate) player_menu: Option<overlay::OverlayMenu>,
    pub(crate) player_context_menu: Option<egui::Pos2>,
    pub(crate) stats_for_nerds_open: bool,
    pub(crate) stats_for_nerds_last_poll: Option<Instant>,
    pub(crate) stats_for_nerds: PlayerNerdsStats,
    pub(crate) last_subtitle_id: Option<String>,
    pub(crate) autoplay: bool,
    pub(crate) loop_video: bool,
    pub(crate) loop_a_secs: Option<f64>,
    pub(crate) loop_b_secs: Option<f64>,
    pub(crate) ab_loop_last_seek_at: Option<Instant>,
    pub(crate) audio_only: bool,
    pub(crate) speed: f64,
    pub(crate) autoplay_eof_handled: bool,
    pub(crate) save_menu_open: bool,
    pub(crate) new_remote_playlist_title: String,
    pub(crate) pending_open_video: Option<Video>,
    pub(crate) pending_open_seek_ms: Option<u64>,
    pub(crate) diagnostics: DecodingDiagnostics,
    pub(crate) last_diagnostics_poll_at: Option<Instant>,
    pub(crate) last_video_size_url: Option<String>,
    pub(crate) last_video_size: Option<crate::VideoSize>,
    pub(crate) auto_subtitle_attempted: bool,
    pub(crate) audio_eq: overlay::AudioEqPreset,
    pub(crate) brightness: i32,
    pub(crate) contrast: i32,
    pub(crate) saturation: i32,
    pub(crate) audio_tracks: Vec<overlay::OverlayAudioTrack>,
    pub(crate) audio_tracks_video_url: Option<String>,
    pub(crate) audio_tracks_last_poll: Option<Instant>,
    pub(crate) storyboard: Option<search::StoryboardSpec>,
    pub(crate) storyboard_video_url: Option<String>,
    pub stream_manifest_cache: std::sync::Arc<crate::stream_manifest::StreamManifestCache>,
    pub stream_prefetcher: crate::stream_manifest::StreamPrefetcher,
    pub current_manifest: Option<std::sync::Arc<crate::stream_manifest::StreamManifest>>,
    pub preferred_audio_lang: Option<String>,
    pub(crate) manifest_task: AsyncTask<StreamManifestFetchMessage>,
    pub(crate) active_direct_stream_url: Option<String>,
    pub(crate) direct_stream_fallback_attempted: bool,
    pub(crate) pending_open_queue_entry_id: Option<u64>,
    #[cfg(windows)]
    pub(crate) frame_snapshot: Option<crate::player::PlayerSnapshot>,
    pub(crate) frame_events_polled_pass: Option<u64>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        let stream_manifest_cache =
            std::sync::Arc::new(crate::stream_manifest::StreamManifestCache::default());
        let stream_prefetcher = crate::stream_manifest::StreamPrefetcher::new(
            std::sync::Arc::clone(&stream_manifest_cache),
        );
        Self {
            fullscreen: false,
            fullscreen_restore_maximized: false,
            fullscreen_sync_retries: 0,
            theater: false,
            window_visible: false,
            active_buffer: None,
            active_hwdec: None,
            mini_player: false,
            mini_player_restore_size: None,
            mini_player_restore_pos: None,
            mini_player_restore_maximized: false,
            mini_player_saved_size: None,
            mini_player_saved_pos: None,
            mini_player_pending_enter: None,
            mini_player_settled: false,
            in_app_mini_player: false,
            video_back_stack: Vec::new(),
            video_forward_stack: Vec::new(),
            pending_open_keep_mini_player: false,
            pending_open_suppress_back_stack: false,
            pending_open_keep_playlist: false,
            mini_player_paused: false,
            mini_player_muted: false,
            mini_player_volume: 100.0,
            active: None,
            quality: None,
            qualities: Vec::new(),
            quality_task: AsyncTask::new(),
            quality_error: None,
            quality_fallback_url: None,
            current_file_loaded: false,
            cached_raw_path: None,
            cached_raw_path_poll: None,
            seek_drag: None,
            overlay_last_activity: Instant::now(),
            overlay_last_pointer_pos: None,
            player_menu: None,
            player_context_menu: None,
            stats_for_nerds_open: false,
            stats_for_nerds_last_poll: None,
            stats_for_nerds: PlayerNerdsStats::default(),
            last_subtitle_id: None,
            autoplay: true,
            loop_video: false,
            loop_a_secs: None,
            loop_b_secs: None,
            ab_loop_last_seek_at: None,
            audio_only: false,
            speed: 1.0,
            autoplay_eof_handled: false,
            save_menu_open: false,
            new_remote_playlist_title: String::new(),
            pending_open_video: None,
            pending_open_seek_ms: None,
            diagnostics: DecodingDiagnostics::unknown(Some(crate::DEFAULT_CONFIGURED_HWDEC)),
            last_diagnostics_poll_at: None,
            last_video_size_url: None,
            last_video_size: None,
            auto_subtitle_attempted: false,
            audio_eq: overlay::AudioEqPreset::Flat,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            audio_tracks: Vec::new(),
            audio_tracks_video_url: None,
            audio_tracks_last_poll: None,
            storyboard: None,
            storyboard_video_url: None,
            stream_manifest_cache,
            stream_prefetcher,
            current_manifest: None,
            preferred_audio_lang: None,
            manifest_task: AsyncTask::new(),
            active_direct_stream_url: None,
            direct_stream_fallback_attempted: false,
            pending_open_queue_entry_id: None,
            #[cfg(windows)]
            frame_snapshot: None,
            frame_events_polled_pass: None,
        }
    }
}

impl PlaybackState {
    pub(crate) fn set_loop_a(&mut self, pos_secs: f64) {
        let a = if pos_secs.is_finite() && pos_secs >= 0.0 {
            pos_secs
        } else {
            0.0
        };
        self.loop_a_secs = Some(a);
        if self.loop_b_secs.is_some_and(|b| !b.is_finite() || b <= a) {
            self.loop_b_secs = None;
        }
        self.ab_loop_last_seek_at = None;
    }

    pub(crate) fn set_loop_b(&mut self, pos_secs: f64) {
        let b = if pos_secs.is_finite() && pos_secs >= 0.0 {
            pos_secs
        } else {
            0.0
        };
        self.loop_b_secs = Some(b);
        if self.loop_a_secs.is_some_and(|a| !a.is_finite() || b <= a) {
            self.loop_a_secs = None;
        }
        self.ab_loop_last_seek_at = None;
    }

    pub(crate) fn clear_ab_loop(&mut self) {
        self.loop_a_secs = None;
        self.loop_b_secs = None;
        self.ab_loop_last_seek_at = None;
    }

    pub(crate) fn ab_loop_active(&self) -> Option<(f64, f64)> {
        match (self.loop_a_secs, self.loop_b_secs) {
            (Some(a), Some(b)) if a.is_finite() && b.is_finite() && a >= 0.0 && a < b => {
                Some((a, b))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Playback,
    SponsorBlock,
    Privacy,
    Storage,
    BackupDiagnostics,
}

impl SettingsTab {
    pub(crate) const ALL: [Self; 6] = [
        Self::General,
        Self::Playback,
        Self::SponsorBlock,
        Self::Privacy,
        Self::Storage,
        Self::BackupDiagnostics,
    ];

    pub(crate) fn label_pt(self) -> &'static str {
        match self {
            Self::General => "Geral e aparência",
            Self::Playback => "Reprodução e desempenho",
            Self::SponsorBlock => "SponsorBlock",
            Self::Privacy => "Conta e privacidade",
            Self::Storage => "Cache e armazenamento",
            Self::BackupDiagnostics => "Backup e diagnóstico",
        }
    }

    pub(crate) fn label_en(self) -> &'static str {
        match self {
            Self::General => "General & appearance",
            Self::Playback => "Playback & performance",
            Self::SponsorBlock => "SponsorBlock",
            Self::Privacy => "Account & privacy",
            Self::Storage => "Cache & storage",
            Self::BackupDiagnostics => "Backup & diagnostics",
        }
    }

    pub(crate) fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }

    pub(crate) fn subtitle_pt(self) -> &'static str {
        match self {
            Self::General => {
                "Personalize tema, densidade dos cartões, comportamento inicial e atalhos."
            }
            Self::Playback => {
                "Configure velocidade, qualidade, áudio, legendas, hardware e buffer."
            }
            Self::SponsorBlock => {
                "Pule ou destaque automaticamente segmentos enviados pela comunidade."
            }
            Self::Privacy => {
                "Gerencie sua conta Google, OAuth, histórico local e tópicos recomendados."
            }
            Self::Storage => {
                "Escolha a pasta de cache, acompanhe o espaço usado e limpe dados temporários."
            }
            Self::BackupDiagnostics => {
                "Exporte ou restaure backups locais e inspecione diagnósticos do libmpv."
            }
        }
    }

    pub(crate) fn subtitle_en(self) -> &'static str {
        match self {
            Self::General => "Customize theme, card density, startup behavior, and shortcuts.",
            Self::Playback => "Configure speed, quality, audio, subtitles, hardware, and buffer.",
            Self::SponsorBlock => "Automatically skip or highlight community-submitted segments.",
            Self::Privacy => "Manage your Google account, OAuth, local history, and feed topics.",
            Self::Storage => "Choose cache folder, inspect disk usage, and clear temporary data.",
            Self::BackupDiagnostics => {
                "Export or restore local backups and inspect libmpv diagnostics."
            }
        }
    }

    pub(crate) fn subtitle(self, pt: bool) -> &'static str {
        if pt {
            self.subtitle_pt()
        } else {
            self.subtitle_en()
        }
    }
}

pub(crate) type BackupPickerResult = (bool, Result<Option<PathBuf>, String>);

pub(crate) enum YtDlpUpdaterMessage {
    Checked(Result<updater::YtDlpUpdateCheck, String>),
    Updated {
        version: String,
        result: Result<PathBuf, String>,
    },
}

pub(crate) enum AppUpdaterMessage {
    Checked {
        silent: bool,
        result: Result<updater::AppUpdateCheck, String>,
    },
    Prepared(Result<updater::AppUpdatePrepared, String>),
}

pub(crate) struct SettingsState {
    pub(crate) buffer: BufferSettingsState,
    pub(crate) playback: TrackedSettings<PlaybackPreferences>,
    pub(crate) theme: TrackedSettings<Theme>,
    pub(crate) prefs: TrackedSettings<settings::AppPreferences>,
    pub(crate) applied_theme: Option<Theme>,
    pub(crate) active_tab: SettingsTab,
    pub(crate) search_query: String,
    pub(crate) confirm_clear_history: bool,
    pub(crate) confirm_reset_tab: Option<SettingsTab>,
    pub(crate) cache_dir_input: String,
    pub(crate) cache_dir_input_synced: bool,
    pub(crate) cache_picker_task: AsyncTask<Result<Option<PathBuf>, String>>,
    pub(crate) download_dir_input: String,
    pub(crate) download_dir_input_synced: bool,
    pub(crate) download_picker_task: AsyncTask<Result<Option<PathBuf>, String>>,
    pub(crate) backup_picker_task: AsyncTask<BackupPickerResult>,
    pub(crate) oauth_picker_task: AsyncTask<Result<Option<PathBuf>, String>>,
    pub(crate) oauth_import_status: Option<(bool, String)>,
    pub(crate) cache_status: Option<String>,
    pub(crate) storage_action_status: Option<(bool, String)>,
    pub(crate) cache_disk_usage: Option<(PathBuf, settings::DirUsage, Instant)>,
    pub(crate) custom_buffer_max_mib: u32,
    pub(crate) custom_buffer_back_mib: u32,
    pub(crate) custom_buffer_cache_secs: u32,
    pub(crate) custom_buffer_synced: bool,
    pub(crate) pending_reload_video: bool,
    pub(crate) ytdlp_check: Option<updater::YtDlpUpdateCheck>,
    pub(crate) ytdlp_status: Option<(bool, String)>,
    pub(crate) ytdlp_task: AsyncTask<YtDlpUpdaterMessage>,
    pub(crate) app_update_check: Option<updater::AppUpdateCheck>,
    pub(crate) app_update_prepared: Option<updater::AppUpdatePrepared>,
    pub(crate) app_update_status: Option<(bool, String)>,
    pub(crate) app_update_task: AsyncTask<AppUpdaterMessage>,
    pub(crate) app_update_auto_checked: bool,
    pub(crate) app_update_banner_dismissed: bool,
    pub(crate) capturing_keybinding: Option<&'static str>,
}

impl Default for SettingsState {
    fn default() -> Self {
        let defaults = BufferProfile::Default.values();
        Self {
            buffer: BufferSettingsState::default(),
            playback: TrackedSettings::default(),
            theme: TrackedSettings::default(),
            prefs: TrackedSettings::default(),
            applied_theme: None,
            active_tab: SettingsTab::default(),
            search_query: String::new(),
            confirm_clear_history: false,
            confirm_reset_tab: None,
            cache_dir_input: String::new(),
            cache_dir_input_synced: false,
            cache_picker_task: AsyncTask::new(),
            download_dir_input: String::new(),
            download_dir_input_synced: false,
            download_picker_task: AsyncTask::new(),
            backup_picker_task: AsyncTask::new(),
            oauth_picker_task: AsyncTask::new(),
            oauth_import_status: None,
            cache_status: None,
            storage_action_status: None,
            cache_disk_usage: None,
            custom_buffer_max_mib: defaults.max_mib,
            custom_buffer_back_mib: defaults.back_mib,
            custom_buffer_cache_secs: defaults.cache_seconds,
            custom_buffer_synced: false,
            pending_reload_video: false,
            ytdlp_check: None,
            ytdlp_status: None,
            ytdlp_task: AsyncTask::new(),
            app_update_check: None,
            app_update_prepared: None,
            app_update_status: None,
            app_update_task: AsyncTask::new(),
            app_update_auto_checked: false,
            app_update_banner_dismissed: false,
            capturing_keybinding: None,
        }
    }
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
pub(crate) struct RemoteHistoryFeed {
    pub(crate) sections: Vec<crate::history_sync::RemoteHistorySection>,
    pub(crate) seen_ids: HashSet<String>,
    pub(crate) next_continuation: Option<String>,
    pub(crate) loaded: bool,
    pub(crate) complete: bool,
    pub(crate) loading_more: bool,
    pub(crate) page_count: usize,
}

#[derive(Default)]
pub(crate) struct RemoteHistoryState {
    pub(crate) feeds: HashMap<history::HistoryFilter, RemoteHistoryFeed>,
    pub(crate) feed_lru: crate::history_sync::RemoteHistoryFeedLru,
    pub(crate) chips: HashMap<history::HistoryFilter, crate::history_sync::HistoryChipAction>,
    pub(crate) feedback_tokens: HashMap<String, String>,
    pub(crate) actions: crate::history_sync::RemoteHistoryActions,
    pub(crate) sync_rx: Option<Receiver<crate::history_sync::HistorySyncMessage>>,
    pub(crate) sync_cancel: Option<process::Cancellation>,
    pub(crate) sync_next_request_id: u64,
    pub(crate) sync_active_request_id: Option<u64>,
    pub(crate) chip_rx: Option<Receiver<crate::history_sync::HistorySyncMessage>>,
    pub(crate) chip_cancel: Option<process::Cancellation>,
    pub(crate) chip_next_request_id: u64,
    pub(crate) chip_active_request_id: Option<u64>,
    pub(crate) chip_active_filter: Option<history::HistoryFilter>,
    pub(crate) feedback_rx: Option<Receiver<crate::history_sync::RemoteHistoryFeedbackEvent>>,
    pub(crate) feedback_cancel: Option<process::Cancellation>,
    pub(crate) initial_sync_attempted: bool,
    pub(crate) loaded: bool,
    pub(crate) stale: bool,
    pub(crate) needs_connect: bool,
    pub(crate) synced_at: Option<Instant>,
    pub(crate) error: Option<String>,
    pub(crate) confirm_remote_clear: bool,
    pub(crate) shorts_shelf_offsets: HashMap<String, usize>,
    pub(crate) watch_session_id: u64,
    pub(crate) watch_init_rx: Option<Receiver<crate::history_sync::RemoteWatchInitEvent>>,
    pub(crate) watch_init_cancel: Option<process::Cancellation>,
    pub(crate) watch_tracker: Option<crate::history_sync::RemoteWatchTracker>,
}

impl RemoteHistoryState {
    pub(crate) fn reset(&mut self) {
        if let Some(cancel) = self.sync_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.chip_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feedback_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.watch_init_cancel.take() {
            cancel.cancel();
        }
        self.sync_rx = None;
        self.sync_active_request_id = None;
        self.chip_rx = None;
        self.chip_active_request_id = None;
        self.chip_active_filter = None;
        self.feedback_rx = None;
        self.watch_init_rx = None;
        self.watch_tracker = None;
        self.feeds.clear();
        self.feed_lru.clear();
        self.chips.clear();
        self.feedback_tokens.clear();
        self.actions = crate::history_sync::RemoteHistoryActions::default();
        self.initial_sync_attempted = false;
        self.loaded = false;
        self.stale = false;
        self.needs_connect = false;
        self.synced_at = None;
        self.error = None;
        self.confirm_remote_clear = false;
        self.shorts_shelf_offsets.clear();
    }
}

#[derive(Default)]
pub(crate) struct HistoryState {
    pub(crate) data: History,
    pub(crate) path: Option<PathBuf>,
    pub(crate) loaded_path: Option<PathBuf>,
    pub(crate) loaded_fingerprint: Option<FileFingerprint>,
    pub(crate) read_failed: bool,
    pub(crate) error: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) save: HistorySaveState,
    pub(crate) search_query: String,
    pub(crate) filter: history::HistoryFilter,
    pub(crate) remote: RemoteHistoryState,
}

#[derive(Default)]
pub(crate) struct HistorySaveState {
    pub(crate) last_at: Option<Instant>,
    pub(crate) last_position_ms: Option<u64>,
    pub(crate) last_duration_ms: Option<Option<u64>>,
}

pub(crate) struct RemotePlaylistFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) playlist_id: String,
    pub(crate) result: Result<Vec<search::SearchResult>, String>,
}

pub(crate) struct QueueState {
    pub(crate) data: Queue,
    pub(crate) path: Option<PathBuf>,
    pub(crate) loaded_fingerprint: Option<FileFingerprint>,
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
    pub(crate) pending_play_entry_id: Option<u64>,
    pub(crate) save_collection_name: String,
    pub(crate) repeat_mode: queue::RepeatMode,
    pub(crate) remote_playlist_id: Option<String>,
    pub(crate) remote_playlist_task: AsyncTask<RemotePlaylistFetchMessage>,
    pub(crate) remote_playlist_save_name: String,
}

impl Default for QueueState {
    fn default() -> Self {
        Self {
            data: Queue::new(crate::DEFAULT_QUEUE_LIMIT),
            path: None,
            loaded_fingerprint: None,
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
            pending_play_entry_id: None,
            save_collection_name: String::new(),
            repeat_mode: queue::RepeatMode::Off,
            remote_playlist_id: None,
            remote_playlist_task: AsyncTask::new(),
            remote_playlist_save_name: String::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct LikedVideosState {
    pub(crate) items: Vec<SearchResult>,
    pub(crate) seen_keys: HashSet<String>,
    pub(crate) total_results: Option<u64>,
    pub(crate) revision: u64,
    pub(crate) loaded: bool,
    pub(crate) error: Option<String>,
    pub(crate) rx: Option<Receiver<LikedVideosFetchMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) next_request_id: u64,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) merged_cache_key: Option<(u64, usize, u64, usize)>,
    pub(crate) merged_videos: Vec<Video>,
    pub(crate) merged_is_short: Vec<bool>,
    pub(crate) extra_local_count: usize,
}

impl LikedVideosState {
    pub(crate) fn reset(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.rx = None;
        self.active_request_id = None;
        self.items.clear();
        self.seen_keys.clear();
        self.total_results = None;
        self.loaded = false;
        self.error = None;
        self.merged_videos.clear();
        self.merged_is_short.clear();
        self.extra_local_count = 0;
        self.merged_cache_key = None;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Default)]
pub(crate) struct RemotePlaylistsSyncState {
    pub(crate) items: Vec<playlist_sync::SyncedPlaylistCard>,
    pub(crate) all_items: Vec<playlist_sync::SyncedPlaylistCard>,
    pub(crate) oauth_items: Vec<playlist_sync::SyncedPlaylistCard>,
    pub(crate) chips:
        HashMap<playlist_sync::PlaylistCategoryFilter, playlist_sync::PlaylistChipAction>,
    pub(crate) sort_options: Vec<playlist_sync::PlaylistSortOption>,
    pub(crate) active_filter: playlist_sync::PlaylistCategoryFilter,
    pub(crate) active_sort: playlist_sync::PlaylistSort,
    pub(crate) server_sorted_for: Option<playlist_sync::PlaylistSort>,
    pub(crate) next_continuation: Option<String>,
    pub(crate) loaded: bool,
    pub(crate) initial_sync_attempted: bool,
    pub(crate) needs_connect: bool,
    pub(crate) error: Option<String>,
    pub(crate) rx: Option<Receiver<playlist_sync::PlaylistsSyncMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) next_request_id: u64,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) revision: u64,
    pub(crate) action_rx: Option<Receiver<playlist_sync::RemotePlaylistActionEvent>>,
    pub(crate) action_cancel: Option<process::Cancellation>,
}

impl RemotePlaylistsSyncState {
    pub(crate) fn cancel_active(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.rx = None;
        self.active_request_id = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn reset(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.action_cancel.take() {
            cancel.cancel();
        }
        self.rx = None;
        self.action_rx = None;
        self.active_request_id = None;
        self.items.clear();
        self.all_items.clear();
        self.oauth_items.clear();
        self.chips.clear();
        self.sort_options.clear();
        self.active_filter = playlist_sync::PlaylistCategoryFilter::All;
        self.active_sort = playlist_sync::PlaylistSort::RecentlyAdded;
        self.server_sorted_for = None;
        self.next_continuation = None;
        self.loaded = false;
        self.initial_sync_attempted = false;
        self.needs_connect = false;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Default)]
pub(crate) struct SyncedPlaylistDetailState {
    pub(crate) playlist_id: Option<String>,
    pub(crate) meta: Option<playlist_sync::RemotePlaylistDetailMetadata>,
    pub(crate) items: Vec<playlist_sync::RemotePlaylistDetailItem>,
    pub(crate) seen_keys: HashSet<String>,
    pub(crate) total_results: Option<u64>,
    pub(crate) loaded: bool,
    pub(crate) needs_connect: bool,
    pub(crate) error: Option<String>,
    pub(crate) rx: Option<Receiver<playlist_sync::PlaylistDetailSyncMessage>>,
    pub(crate) cancel: Option<process::Cancellation>,
    pub(crate) next_request_id: u64,
    pub(crate) active_request_id: Option<u64>,
    pub(crate) revision: u64,
    pub(crate) merged_cache_key: Option<(u64, usize, u64, usize)>,
    pub(crate) merged_videos: Vec<Video>,
    pub(crate) merged_is_short: Vec<bool>,
    pub(crate) extra_local_count: usize,
}

impl SyncedPlaylistDetailState {
    pub(crate) fn cancel_active(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.rx = None;
        self.active_request_id = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn reset(&mut self) {
        self.cancel_active();
        self.playlist_id = None;
        self.meta = None;
        self.items.clear();
        self.seen_keys.clear();
        self.total_results = None;
        self.loaded = false;
        self.needs_connect = false;
        self.error = None;
        self.merged_videos.clear();
        self.merged_is_short.clear();
        self.extra_local_count = 0;
        self.merged_cache_key = None;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Default)]
pub(crate) struct LibraryState {
    pub(crate) data: crate::library::Library,
    pub(crate) revision: u64,
    pub(crate) path: Option<PathBuf>,
    pub(crate) loaded_fingerprint: Option<FileFingerprint>,
    pub(crate) read_failed: bool,
    pub(crate) error: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) bookmark_note: String,
    pub(crate) collection_name: String,
    pub(crate) search_query: String,
    pub(crate) bookmark_filter: String,
    pub(crate) renaming_collection: Option<usize>,
    pub(crate) rename_draft: String,
    pub(crate) confirm_delete_collection: Option<usize>,
    pub(crate) editing_bookmark_idx: Option<usize>,
    pub(crate) editing_bookmark_note: String,
    pub(crate) pending_bookmark_seek: Option<(String, u64)>,
    pub(crate) selected_collection_idx: Option<usize>,
    pub(crate) active_remote_playlist_title: Option<String>,
    pub(crate) detail_chip_filter: u8,
    pub(crate) liked_videos: LikedVideosState,
    pub(crate) remote_playlists: RemotePlaylistsSyncState,
    pub(crate) watch_later_remote: SyncedPlaylistDetailState,
    pub(crate) active_playlist_detail: SyncedPlaylistDetailState,
    pub(crate) new_playlist_composer_open: bool,
}

#[derive(Default)]
pub(crate) struct FollowedChannelsState {
    pub(crate) data: followed_channels::FollowedChannels,
    pub(crate) path: Option<PathBuf>,
    pub(crate) error: Option<String>,
    pub(crate) feed_items: Vec<search::SearchResult>,
    pub(crate) new_video_urls: HashSet<String>,
    pub(crate) feed_task: AsyncTask<Result<followed_channels::LocalFeedOutput, String>>,
    pub(crate) loaded_at: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct Win11RuntimeState {
    pub(crate) hwnd: Option<isize>,
    pub(crate) theme_observer: Option<crate::win11::SystemThemeObserver>,
    pub(crate) taskbar: crate::win11::TaskbarController,
    pub(crate) thumb_rx: Option<Receiver<crate::win11::TaskbarThumbCommand>>,
    pub(crate) last_dwm_state: Option<(bool, bool)>,
    pub(crate) last_jump_list_lang: Option<crate::i18n::UiLanguage>,
    pub(crate) subclass_installed: bool,
    pub(crate) maximize_button_rect: Option<egui::Rect>,
    #[cfg(test)]
    pub(crate) test_system_dark_override: Option<bool>,
}

#[derive(Default)]
pub(crate) struct DownloadState {
    pub(crate) manager: downloads::DownloadManager,
    pub(crate) notice: Option<String>,
    pub(crate) direct_url_input: String,
    pub(crate) selected_format: downloads::DownloadFormat,
    pub(crate) filter_tab: u8,
    pub(crate) history_path: Option<PathBuf>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NavigationRoute {
    Browse {
        section: BrowseSection,
        search_query: Option<String>,
    },
    Channel(ChannelTarget),
    Watch(Video),
    Settings(SettingsTab),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandPaletteCacheKey {
    pub(crate) pt: bool,
    pub(crate) theme: Theme,
    pub(crate) repeat_mode: queue::RepeatMode,
    pub(crate) queue_len: usize,
    pub(crate) queue_first_entry_id: Option<u64>,
    pub(crate) feed_generation: u64,
    pub(crate) history_len: usize,
    pub(crate) history_first_id: Option<String>,
    pub(crate) history_first_pos_ms: Option<u64>,
    pub(crate) library_revision: u64,
    pub(crate) authenticated: bool,
    pub(crate) sub_channels_len: usize,
    pub(crate) followed_channels_len: usize,
    pub(crate) clipboard_raw: Option<String>,
    pub(crate) query: String,
}

#[derive(Default)]
pub(crate) struct NavigationUiState {
    pub(crate) navigation_open: bool,
    pub(crate) initial_sidebar_synced: bool,
    pub(crate) search_results_mode: bool,
    pub(crate) settings_open: bool,
    pub(crate) shortcuts_open: bool,
    pub(crate) subscriptions_expanded: bool,
    pub(crate) explore_expanded: bool,
    pub(crate) browse_section: BrowseSection,
    pub(crate) chips_scroll_offset: f32,
    pub(crate) nav_back_stack: Vec<NavigationRoute>,
    pub(crate) nav_forward_stack: Vec<NavigationRoute>,
    pub(crate) nav_last_route: Option<NavigationRoute>,
    pub(crate) nav_suppress_record: bool,
    pub(crate) command_palette: crate::command_palette::CommandPaletteState,
    pub(crate) command_palette_cache_key: Option<CommandPaletteCacheKey>,
    pub(crate) command_palette_cached_items: Vec<crate::command_palette::PaletteItem>,
    pub(crate) command_palette_cached_ranked: Vec<crate::command_palette::RankedPaletteEntry>,
    pub(crate) command_palette_cached_has_direct: bool,
    pub(crate) command_palette_rebuilds: u64,
    pub(crate) clipboard_last_checked_at: Option<Instant>,
    pub(crate) last_clipboard_seq: u32,
    pub(crate) clipboard_last_raw: Option<String>,
    pub(crate) clipboard_youtube_target: Option<crate::command_palette::DirectYouTubeTarget>,
    pub(crate) clipboard_dismissed_raw: Option<String>,
    pub(crate) incognito_mode: bool,
    pub(crate) subscriptions_list_view: bool,
    pub(crate) subscriptions_filters_open: bool,
    pub(crate) subscriptions_top_expanded: bool,
    pub(crate) subscriptions_shorts_expanded: bool,
    pub(crate) manage_subscriptions_open: bool,
    pub(crate) manage_subscriptions_query: String,
    pub(crate) pending_unsubscribe_channel: Option<(String, String)>,
    pub(crate) notifications_open: bool,
    pub(crate) focus_search_requested: bool,
}

impl NavigationUiState {
    pub(crate) fn can_go_back(&self) -> bool {
        !self.nav_back_stack.is_empty()
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        !self.nav_forward_stack.is_empty()
    }

    pub(crate) fn dismiss_clipboard_target(&mut self) {
        if let Some(raw) = self.clipboard_last_raw.clone() {
            self.clipboard_dismissed_raw = Some(raw);
        }
        self.clipboard_youtube_target = None;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CachedMpvSubtitleTrack {
    pub(crate) id: String,
    pub(crate) label: String,
}

#[derive(Default)]
pub(crate) struct OverlayCache {
    pub(crate) chapters_video_url: Option<String>,
    pub(crate) chapters_count: Option<i64>,
    pub(crate) chapters_last_poll: Option<Instant>,
    pub(crate) chapters: Vec<overlay::OverlayChapter>,
    pub(crate) mpv_subs_video_url: Option<String>,
    pub(crate) mpv_subs_track_count: Option<i64>,
    pub(crate) mpv_subs_yt_empty: bool,
    pub(crate) mpv_subs_last_poll: Option<Instant>,
    pub(crate) mpv_subs: Vec<CachedMpvSubtitleTrack>,
    pub(crate) force_refresh: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CachedSidebarCard {
    pub(crate) video: Video,
    pub(crate) is_playlist: bool,
    pub(crate) channel_target: Option<ChannelTarget>,
    pub(crate) progress_ratio: Option<f32>,
    pub(crate) in_history: bool,
    pub(crate) is_music: bool,
    pub(crate) is_new: bool,
    pub(crate) title_truncated: String,
    pub(crate) channel_label: Option<String>,
    pub(crate) subline: Option<String>,
    pub(crate) measured_height: Option<(i32, f32)>,
}

#[derive(Default)]
pub(crate) struct WatchSidebarCache {
    pub(crate) valid: bool,
    pub(crate) feed_generation: u64,
    pub(crate) feed_signal_revision: u64,
    pub(crate) metadata_revision: u64,
    pub(crate) feed_videos_len: usize,
    pub(crate) feed_first_url: Option<String>,
    pub(crate) channel_target_key: Option<String>,
    pub(crate) channel_videos_len: usize,
    pub(crate) selected_url: Option<String>,
    pub(crate) sidebar_tab: crate::watch_extras::SidebarTab,
    pub(crate) playlist_urls_len: usize,
    pub(crate) cards: Vec<CachedSidebarCard>,
}

#[derive(Default)]
pub(crate) struct WatchMetadataUiCache {
    pub(crate) valid: bool,
    pub(crate) video_url: Option<String>,
    pub(crate) metadata_revision: u64,
    pub(crate) live_info_revision: u64,
    pub(crate) community_revision: u64,
    pub(crate) library_revision: u64,
    pub(crate) is_live: bool,
    pub(crate) channel_target: Option<ChannelTarget>,
    pub(crate) channel_id_hint: Option<String>,
    pub(crate) channel_avatar_url: Option<String>,
    pub(crate) subscriber_count_text: Option<String>,
    pub(crate) saved_in_library: bool,
    pub(crate) stats: Vec<String>,
    pub(crate) cleaned_description: Option<String>,
    pub(crate) description_lines: Option<Vec<crate::watch_extras::CachedTimestampLine>>,
    pub(crate) description_chapters: Vec<crate::watch_extras::DescriptionChapter>,
    pub(crate) description_expanded: bool,
}

pub(crate) struct CommunityExtrasFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) outcome: community_apis::CommunityExtrasOutcome,
}

#[derive(Clone, Debug)]
pub(crate) struct ActivePlaylistPlayback {
    pub(crate) playlist_id: String,
    pub(crate) title: String,
    pub(crate) owner: String,
    pub(crate) privacy: playlist_sync::PlaylistPrivacy,
    pub(crate) is_mix: bool,
    pub(crate) videos: Vec<Video>,
    pub(crate) total_count: Option<usize>,
    pub(crate) play_order: Vec<usize>,
    pub(crate) current_pos: usize,
    pub(crate) loop_playlist: bool,
    pub(crate) shuffle: bool,
    pub(crate) collapsed: bool,
    pub(crate) scroll_to_current: bool,
    pub(crate) loading_more: bool,
    pub(crate) local_collection_idx: Option<usize>,
}

fn shuffle_usize_slice(slice: &mut [usize], mut seed: u64) {
    if slice.len() <= 1 {
        return;
    }
    if seed == 0 {
        seed = 0x9E37_79B9_7F4A_7C15;
    }
    for i in (1..slice.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed as usize) % (i + 1);
        slice.swap(i, j);
    }
}

impl ActivePlaylistPlayback {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        playlist_id: String,
        title: String,
        owner: String,
        privacy: playlist_sync::PlaylistPrivacy,
        is_mix: bool,
        videos: Vec<Video>,
        total_count: Option<usize>,
        start_index: usize,
        shuffle: bool,
        seed: u64,
        loading_more: bool,
        local_collection_idx: Option<usize>,
    ) -> Self {
        let len = videos.len();
        let safe_start = if len == 0 {
            0
        } else {
            start_index.min(len - 1)
        };
        let mut play_order: Vec<usize> = (0..len).collect();
        let current_pos = if shuffle && len > 1 {
            play_order.swap(0, safe_start);
            shuffle_usize_slice(&mut play_order[1..], seed);
            0
        } else {
            safe_start
        };
        Self {
            playlist_id,
            title,
            owner,
            privacy,
            is_mix,
            videos,
            total_count,
            play_order,
            current_pos,
            loop_playlist: false,
            shuffle,
            collapsed: false,
            scroll_to_current: true,
            loading_more,
            local_collection_idx,
        }
    }

    pub(crate) fn current_video(&self) -> Option<&Video> {
        self.video_at_order_pos(self.current_pos)
    }

    pub(crate) fn video_at_order_pos(&self, pos: usize) -> Option<&Video> {
        let idx = *self.play_order.get(pos)?;
        self.videos.get(idx)
    }

    pub(crate) fn total_display_count(&self) -> usize {
        self.total_count
            .unwrap_or(self.videos.len())
            .max(self.videos.len())
    }

    pub(crate) fn peek_next(&self) -> Option<(usize, Video)> {
        if self.play_order.is_empty() {
            return None;
        }
        if self.current_pos + 1 < self.play_order.len() {
            let next_pos = self.current_pos + 1;
            let vid = self.videos.get(self.play_order[next_pos])?.clone();
            Some((next_pos, vid))
        } else if self.loop_playlist {
            let vid = self.videos.get(self.play_order[0])?.clone();
            Some((0, vid))
        } else {
            None
        }
    }

    pub(crate) fn peek_previous(&self) -> Option<(usize, Video)> {
        if self.play_order.is_empty() {
            return None;
        }
        if self.current_pos > 0 {
            let prev_pos = self.current_pos - 1;
            let vid = self.videos.get(self.play_order[prev_pos])?.clone();
            Some((prev_pos, vid))
        } else if self.loop_playlist {
            let last_pos = self.play_order.len() - 1;
            let vid = self.videos.get(self.play_order[last_pos])?.clone();
            Some((last_pos, vid))
        } else {
            None
        }
    }

    pub(crate) fn has_next(&self) -> bool {
        self.peek_next().is_some()
    }

    pub(crate) fn has_previous(&self) -> bool {
        self.peek_previous().is_some()
    }

    pub(crate) fn select_pos(&mut self, pos: usize) -> Option<Video> {
        let vid = self.video_at_order_pos(pos)?.clone();
        self.current_pos = pos;
        self.scroll_to_current = true;
        Some(vid)
    }

    pub(crate) fn toggle_loop(&mut self) {
        self.loop_playlist = !self.loop_playlist;
    }

    pub(crate) fn toggle_shuffle(&mut self, seed: u64) {
        let current_vid_idx = self.play_order.get(self.current_pos).copied();
        self.shuffle = !self.shuffle;
        if self.videos.is_empty() {
            self.play_order.clear();
            self.current_pos = 0;
            return;
        }
        if self.shuffle {
            if let Some(cur_idx) = current_vid_idx {
                let mut rest: Vec<usize> =
                    (0..self.videos.len()).filter(|&i| i != cur_idx).collect();
                shuffle_usize_slice(&mut rest, seed);
                self.play_order.clear();
                self.play_order.push(cur_idx);
                self.play_order.extend(rest);
                self.current_pos = 0;
            } else {
                self.play_order = (0..self.videos.len()).collect();
                shuffle_usize_slice(&mut self.play_order, seed);
                self.current_pos = 0;
            }
        } else {
            self.play_order = (0..self.videos.len()).collect();
            self.current_pos = current_vid_idx
                .unwrap_or(0)
                .min(self.videos.len().saturating_sub(1));
        }
        self.scroll_to_current = true;
    }

    pub(crate) fn append_videos(
        &mut self,
        incoming: &[Video],
        total_count: Option<usize>,
        loading_more: bool,
    ) {
        let mut key_to_idx: HashMap<String, usize> = HashMap::with_capacity(self.videos.len());
        for (idx, vid) in self.videos.iter().enumerate() {
            let key = history::youtube_video_id(&vid.url).unwrap_or_else(|| vid.url.clone());
            key_to_idx.entry(key).or_insert(idx);
        }
        for vid in incoming {
            let key = history::youtube_video_id(&vid.url).unwrap_or_else(|| vid.url.clone());
            if let Some(&existing_idx) = key_to_idx.get(&key) {
                if let Some(existing) = self.videos.get_mut(existing_idx)
                    && !vid.title.trim().is_empty()
                    && (existing.title.trim().is_empty()
                        || existing.title.starts_with("YouTube (")
                        || existing.channel.trim().is_empty())
                {
                    *existing = vid.clone();
                }
            } else {
                let new_idx = self.videos.len();
                key_to_idx.insert(key, new_idx);
                self.videos.push(vid.clone());
                self.play_order.push(new_idx);
            }
        }
        if let Some(total) = total_count {
            self.total_count = Some(total);
        }
        self.loading_more = loading_more;
    }

    pub(crate) fn update_metadata(
        &mut self,
        title: Option<String>,
        owner: Option<String>,
        privacy: Option<playlist_sync::PlaylistPrivacy>,
        total_count: Option<usize>,
        loading_more: bool,
    ) {
        if let Some(t) = title.filter(|s| !s.trim().is_empty()) {
            self.title = t;
        }
        if let Some(o) = owner.filter(|s| !s.trim().is_empty()) {
            self.owner = o;
        }
        if let Some(p) = privacy.filter(|&p| p != playlist_sync::PlaylistPrivacy::Unknown) {
            self.privacy = p;
        }
        if let Some(total) = total_count {
            self.total_count = Some(total);
        }
        self.loading_more = loading_more;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ShortsSidePanel {
    #[default]
    None,
    Description,
    Comments,
}

#[derive(Default)]
pub(crate) struct WatchState {
    pub(crate) extras: crate::watch_extras::WatchExtras,
    pub(crate) live: crate::live_chat::LiveWatchState,
    pub(crate) sponsorblock: sponsorblock::SponsorBlockRuntime,
    pub(crate) auto_skipped_chapter_indices: HashSet<usize>,
    pub(crate) share_copied_at: Option<Instant>,
    pub(crate) shorts_side_panel: ShortsSidePanel,
    pub(crate) sidebar_cache: WatchSidebarCache,
    pub(crate) metadata_cache: WatchMetadataUiCache,
    pub(crate) overlay_cache: OverlayCache,
    pub(crate) community_cache: community_apis::CommunityExtrasCache,
    pub(crate) community_revision: u64,
    pub(crate) community_task: AsyncTask<CommunityExtrasFetchMessage>,
    pub(crate) show_original_title: bool,
    pub(crate) active_playlist: Option<ActivePlaylistPlayback>,
    pub(crate) mix_rx: Option<Receiver<playlist_sync::PlaylistDetailSyncMessage>>,
    pub(crate) mix_cancel: Option<process::Cancellation>,
    pub(crate) mix_next_request_id: u64,
    pub(crate) mix_active_request_id: Option<u64>,
    pub(crate) next_task: AsyncTask<WatchNextFetchMessage>,
    pub(crate) innertube_comments_task: AsyncTask<InnerTubeCommentsFetchMessage>,
    pub(crate) feedback_task: AsyncTask<ZeroQuotaFeedbackMessage>,
    pub(crate) comment_mutation_task: AsyncTask<CommentMutationFetchMessage>,
    pub(crate) replay_chat_task: AsyncTask<LiveChatReplayFetchMessage>,
    pub(crate) replay_chat_last_poll_at: Option<Instant>,
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

pub(crate) fn load_app_preferences_from_path(
    path: &Path,
) -> Result<settings::AppPreferences, String> {
    settings::load_app_preferences(path).map_err(|error| {
        format!(
            "Erro ao ler preferências em '{}' ({error}); usando os padrões.",
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

pub(crate) fn load_history_with_fingerprint_from_path(
    path: &Path,
) -> Result<(History, Option<FileFingerprint>), String> {
    history::load_with_fingerprint(path).map_err(|error| {
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

    pub(crate) fn sync_theme_visuals(&mut self, ctx: &egui::Context, system_is_dark: bool) {
        self.ensure_theme_synchronized();
        let effective = self.theme.value.resolve(system_is_dark).to_concrete_theme();
        let want_dark = effective == Theme::Dark;
        if self.applied_theme != Some(effective)
            || ctx.global_style().visuals.dark_mode != want_dark
        {
            apply_theme_visuals(ctx, effective);
            self.applied_theme = Some(effective);
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

    pub(crate) fn palette_for(&self, system_is_dark: bool) -> ThemePalette {
        ThemePalette::for_theme(self.theme.value.resolve(system_is_dark).to_concrete_theme())
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

    pub(crate) fn ensure_prefs_synchronized(&mut self) {
        if self.prefs.loaded_path == self.prefs.path {
            return;
        }
        let had_previous_path = self.prefs.loaded_path.is_some();
        self.prefs.loaded_path = self.prefs.path.clone();
        let Some(path) = self.prefs.path.clone() else {
            return;
        };
        match load_app_preferences_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.prefs.value == settings::AppPreferences::default() {
                    self.prefs.value = loaded;
                    self.cache_dir_input_synced = false;
                }
                self.prefs.error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.prefs.value = settings::AppPreferences::default();
                    self.cache_dir_input_synced = false;
                }
                self.prefs.error = Some(error);
            }
        }
    }

    pub(crate) fn apply_app_preferences(&mut self, next: settings::AppPreferences) {
        self.ensure_prefs_synchronized();
        let normalized = next.normalized();
        if let Err(validation_err) = normalized.validate() {
            self.prefs.error = Some(validation_err.to_owned());
            return;
        }
        self.prefs.value = normalized.clone();
        let save_result = match self.prefs.path.clone().ok_or_else(|| {
            if cfg!(test) {
                Err("Caminho de preferências gerais não configurado no teste".to_owned())
            } else {
                settings::app_preferences_path().map_err(|error| error.to_string())
            }
        }) {
            Ok(path) => settings::save_app_preferences(&path, &normalized).map_err(|error| {
                format!(
                    "Não foi possível salvar preferências em '{}' ({error}); a escolha vale apenas nesta sessão.",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save_app_preferences(&path, &normalized).map_err(|error| {
                    format!(
                        "Não foi possível salvar preferências em '{}' ({error}); a escolha vale apenas nesta sessão.",
                        path.display()
                    )
                });
                self.prefs.path = Some(path.clone());
                self.prefs.loaded_path = Some(path);
                res
            }
            Err(Err(error)) => Err(format!(
                "Não foi possível salvar preferências ({error}); a escolha vale apenas nesta sessão."
            )),
        };
        self.prefs.error = save_result.err();
    }
}

impl HistoryState {
    pub(crate) fn ensure_synchronized(&mut self, persistence: &PersistenceWorker) -> bool {
        if self.loaded_path == self.path {
            return false;
        }
        let had_previous_path = self.loaded_path.is_some();
        self.loaded_path = self.path.clone();
        let Some(path) = self.path.clone() else {
            self.loaded_fingerprint = None;
            self.read_failed = false;
            return false;
        };
        let mut invalidated = false;
        match load_history_with_fingerprint_from_path(&path) {
            Ok((loaded, fingerprint)) => {
                if had_previous_path || self.data.entries().is_empty() {
                    self.data = loaded;
                    invalidated = true;
                }
                self.loaded_fingerprint = fingerprint;
                persistence.record_loaded_fingerprint(PersistenceSlot::History, path, fingerprint);
                self.read_failed = false;
                self.error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.data = History::default();
                    invalidated = true;
                }
                self.loaded_fingerprint = None;
                self.read_failed = true;
                self.error = Some(error);
            }
        }
        invalidated
    }

    pub(crate) fn save_to_disk(
        &mut self,
        persistence: &PersistenceWorker,
        now: Instant,
        active: Option<&ActivePlayback>,
    ) -> (bool, bool) {
        let invalidated = self.ensure_synchronized(persistence);
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
                let (mut loaded, fingerprint) = match load_history_with_fingerprint_from_path(&path)
                {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        self.path = Some(path.clone());
                        self.loaded_path = Some(path);
                        self.loaded_fingerprint = None;
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
                self.loaded_fingerprint = fingerprint;
                persistence.record_loaded_fingerprint(
                    PersistenceSlot::History,
                    path.clone(),
                    fingerprint,
                );
                path
            }
            Err(Err(message)) => {
                self.error = Some(message);
                return (false, invalidated);
            }
        };
        let expected_fp = persistence
            .last_fingerprint(PersistenceSlot::History)
            .or(self.loaded_fingerprint);
        // Fast `(mtime, len)` guard: skip reading/parsing disk JSON unless the file
        // changed externally since our last known good load or write.
        let current_fp = history::file_fingerprint(&path);
        if current_fp != expected_fp && current_fp.is_some() {
            if let Err(error) = load_history_from_path(&path) {
                self.read_failed = true;
                self.error = Some(error);
                return (false, invalidated);
            }
            self.loaded_fingerprint = current_fp;
            persistence.record_loaded_fingerprint(
                PersistenceSlot::History,
                path.clone(),
                current_fp,
            );
        }
        persistence.schedule_history(path, self.data.clone(), expected_fp);
        self.error = None;
        self.dirty = false;
        self.save.last_at = Some(now);
        if let Some(active) = active {
            self.save.last_position_ms = Some(active.position_ms);
            self.save.last_duration_ms = Some(active.duration_ms);
        }
        (true, invalidated)
    }

    pub(crate) fn planned_resume_ms(
        &mut self,
        video: &Video,
        persistence: &PersistenceWorker,
    ) -> (Option<u64>, bool) {
        let invalidated = self.ensure_synchronized(persistence);
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
        persistence: &PersistenceWorker,
    ) -> (std::io::Result<()>, bool) {
        let invalidated = self.ensure_synchronized(persistence);
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
            watched_at_ms: Some(crate::history::current_unix_epoch_ms()),
            ..Default::default()
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
        self.videos.task.is_active()
    }

    pub(crate) fn is_more_videos_loading(&self) -> bool {
        self.videos.more_task.is_active() || self.videos.continuation_task.is_active()
    }

    pub(crate) fn is_shorts_loading(&self) -> bool {
        self.shorts.task.is_active()
    }

    pub(crate) fn is_streams_loading(&self) -> bool {
        self.streams.task.is_active()
    }

    pub(crate) fn is_playlists_loading(&self) -> bool {
        self.playlists.task.is_active()
    }

    pub(crate) fn is_playlist_videos_loading(&self) -> bool {
        self.playlist_detail.task.is_active()
    }

    pub(crate) fn is_about_loading(&self) -> bool {
        self.about.task.is_active()
    }

    pub(crate) fn is_sorted_videos_loading(&self) -> bool {
        self.videos.sorted_task.is_active()
    }

    pub(crate) fn is_channel_search_loading(&self) -> bool {
        self.search.task.is_active()
    }

    pub(crate) fn current_target(&self) -> Option<ChannelTarget> {
        self.loading_target
            .clone()
            .or_else(|| self.page.as_ref().map(|page| page.target.clone()))
            .or_else(|| self.last_target.clone())
    }

    pub(crate) fn cancel_subrequests(&mut self) {
        self.videos.more_task.cancel();
        self.videos.continuation_task.cancel();
        self.cancel_sorted_videos();
        self.cancel_shorts();
        self.cancel_streams();
        self.cancel_channel_search();
        self.playlists.task.cancel();
        self.playlist_detail.task.cancel();
        self.about.task.cancel();
    }

    pub(crate) fn cancel_loading(&mut self) {
        self.videos.task.cancel();
        self.loading_target = None;
        self.cancel_subrequests();
    }

    pub(crate) fn close_view(&mut self) {
        self.videos.task.cancel();
        self.cancel_subrequests();
        self.view_open = false;
        self.about_modal_open = false;
        self.loading_target = None;
    }

    pub(crate) fn cancel_more_videos(&mut self) {
        self.videos.more_task.cancel();
        self.videos.continuation_task.cancel();
    }

    pub(crate) fn cancel_sorted_videos(&mut self) {
        self.videos.sorted_task.cancel();
    }

    pub(crate) fn cancel_channel_search(&mut self) {
        self.search.task.cancel();
    }

    pub(crate) fn cancel_shorts(&mut self) {
        self.shorts.task.cancel();
    }

    pub(crate) fn cancel_streams(&mut self) {
        self.streams.task.cancel();
    }

    pub(crate) fn cancel_playlists(&mut self) {
        self.playlists.task.cancel();
    }

    pub(crate) fn cancel_playlist_videos(&mut self) {
        self.playlist_detail.task.cancel();
    }

    pub(crate) fn cancel_about(&mut self) {
        self.about.task.cancel();
    }

    pub(crate) fn begin_sorted_videos_request(&mut self) -> Option<(u64, ChannelTarget)> {
        self.cancel_sorted_videos();
        let target = self
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_target())?;
        let request_id = self.videos.sorted_task.begin_reserved();
        self.videos.sorted_error = None;
        Some((request_id, target))
    }

    pub(crate) fn begin_channel_search_request(&mut self) -> Option<(u64, ChannelTarget)> {
        self.cancel_channel_search();
        let target = self
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_target())?;
        let request_id = self.search.task.begin_reserved();
        self.search.error = None;
        Some((request_id, target))
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
        let request_id = self.videos.more_task.begin_reserved();
        self.videos.more_error = None;
        Some((request_id, target, start, count))
    }

    pub(crate) fn begin_shorts_request(&mut self) -> Option<(u64, ChannelTarget)> {
        if self.is_shorts_loading() {
            return None;
        }
        let target = self
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_target())?;
        let request_id = self.shorts.task.begin_reserved();
        self.shorts.error = None;
        Some((request_id, target))
    }

    pub(crate) fn begin_streams_request(&mut self) -> Option<(u64, ChannelTarget)> {
        if self.is_streams_loading() {
            return None;
        }
        let target = self
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_target())?;
        let request_id = self.streams.task.begin_reserved();
        self.streams.error = None;
        Some((request_id, target))
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
        let request_id = self.playlists.task.begin_reserved();
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
        let request_id = self.about.task.begin_reserved();
        self.about.error = None;
        Some((request_id, target))
    }
}

impl QueueState {
    pub(crate) fn persist(&mut self, persistence: &PersistenceWorker) {
        if self.read_failed {
            return;
        }
        if let Some(path) = self.path.clone() {
            let expected_fp = persistence
                .last_fingerprint(PersistenceSlot::Queue)
                .or(self.loaded_fingerprint);
            let current_fp = history::file_fingerprint(&path);
            if current_fp != expected_fp && current_fp.is_some() {
                if let Err(error) = Queue::load(&path, self.data.limit()) {
                    self.read_failed = true;
                    self.persistence_error = Some(format!(
                        "Não foi possível salvar a fila (arquivo externo inválido preservado): {error}"
                    ));
                    return;
                }
                self.loaded_fingerprint = current_fp;
                persistence.record_loaded_fingerprint(
                    PersistenceSlot::Queue,
                    path.clone(),
                    current_fp,
                );
            }
            persistence.schedule_queue(path, self.data.clone(), expected_fp);
            self.persistence_error = None;
        }
    }
}
