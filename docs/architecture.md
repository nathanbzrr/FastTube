# Architecture

FastTube is a native Windows YouTube desktop client built with `eframe`/`egui` (`glow` renderer), `libmpv` (embedded OpenGL render API with native `HWND` fallback), YouTube InnerTube (`youtubei/v1`), `yt-dlp` (stream manifest extraction and offline downloads), and optional Google OAuth / YouTube Data API v3 integration.

## State Layout

The root application struct `FastTube` in [`src/main.rs`](../src/main.rs) coordinates UI rendering, async polling, and domain workflows. It holds cross-cutting root fields (`video_metadata`, `selected_metadata`, `thumbnails`, `avatars`, `message`, `toasts`, `selected`, `scrolling`, `autoscroll`, `in_app_ui_pass`, `player`, `persistence`, `deferred_fonts_rx`, `desktop`, `desktop_win11`) and 14 domain substates defined in [`src/app_state.rs`](../src/app_state.rs):

| Field on `FastTube` | Type in `src/app_state.rs` | Domain |
| --- | --- | --- |
| `feed` | `BrowseFeedState` | Search query, active video list, feed filter, derived ranking cache, Home SWR & InnerTube `FEwhat_to_watch` / `FEsubscriptions` jobs, Explore Hub, recent searches |
| `feed_preferences` | `FeedPreferencesState` | Subscriptions page filter/sort preferences and persistence path |
| `channels` | `ChannelState` | Active channel page, tab selection, continuations, sort chips, channel search, public playlists, About info |
| `auth` | `AuthState` | OAuth `TokenSession`, DPAPI `YouTubeWebSession`, reauth/rating jobs, cached ratings, remote playlists, notifications inbox |
| `playback` | `PlaybackState` | Active playback correlation, `StreamManifest` cache & prefetcher, playback preferences, quality/EQ/aspect/storyboard state, decoding diagnostics |
| `settings` | `SettingsState` | Theme, buffer profile, playback defaults, JSON app preferences (`SponsorBlock`, `Keybindings`, `UiLanguage`, privacy, tray/updates, storage), app & `yt-dlp` updater jobs |
| `history` | `HistoryState` | Local watch history, remote InnerTube history sync (`RemoteHistoryFeedLru`), watchtime tracking, throttled save checkpointing |
| `queue` | `QueueState` | Persisted FIFO playback queue, repeat mode (`Off`/`All`/`One`), remote playlist loader, load confirmation state |
| `library` | `LibraryState` | OAuth-independent shelves, named collections, and timestamp bookmarks |
| `followed_channels` | `FollowedChannelsState` | OAuth-free local followed channels (`followed-channels.json`) and public YouTube RSS feed poller |
| `downloads` | `DownloadState` | Offline `yt-dlp` download queue, progress/status state, and local MP4 muxing |
| `backup` | `BackupState` | JSON v1 backup export (including `followed-channels.json`), import preview, merge/replace execution |
| `ui` | `NavigationUiState` | Active browse section, navigation history, sidebar drawer, command palette, shortcut modal, unified modals, clipboard quick-open |
| `watch` | `WatchState` | Watch-page sidebar tab, `next` related chips, transcript UI, OAuth-free & OAuth comments, Live Chat & Replay, SponsorBlock runtime, opt-in community extras cache |

## Directory & Module Map (`src/`)

### Modular Subdirectories

| Directory | Submodules | Responsibility |
| --- | --- | --- |
| [`src/app/`](../src/app/mod.rs) | `playback`, `feeds`, `channels_playlists`, `auth_sync`, `watch_actions`, `shell_modals`, `persistence_desktop` | `impl FastTube` domain coordinators split by responsibility: playback/manifest/queue, Home/Subscriptions/Search/Explore/Shorts feeds, channels/playlists/library, auth/history sync, watch actions/live chat/comments/reports/share, masthead/shortcuts/command palette/modals, persistence/backup/updaters/desktop/Win11 |
| [`src/watch_ui/`](../src/watch_ui/mod.rs) | `player_stage`, `player_overlay`, `metadata_sidebar`, `comments`, `live_chat_ui`, `transcript_ui`, `mini_player`, `shorts`, `tests` | Watch player stage, OSD & in-player overlay bindings, metadata/EQ/aspect/storyboard, comments, live chat & replay UI, transcript search, in-app mini-player, Shorts view |
| [`src/library_ui/`](../src/library_ui/mod.rs) | `queue_view`, `history_view`, `playlists_view`, `downloads_view`, `insights_view`, `tests` | Queue, local/remote History, Playlists/Watch Later/Liked/Library shelves, Offline Downloads, and Local Watch Insights views |
| [`src/search/`](../src/search/mod.rs) | `query`, `filters`, `channel`, `playlist`, `tests` | Search queries/filters/suggestions, channel pages/tabs, public playlist extraction, and `yt-dlp` fallback helpers |
| [`src/main_tests/`](../src/main_tests.rs) | `helpers`, `browse_channel`, `desktop_settings`, `playback_queue`, `watch_library`, `waves_integration` | Integration and headless `egui` UI test suite |

### Core & Feature Modules

| File | Responsibility |
| --- | --- |
| [`src/main.rs`](../src/main.rs) | Crate entry point, module wiring, `ThemePalette` & visual helpers, shared message/UI types, `FastTube` struct & `Default`, `eframe::App` frame loop (`ui`, `on_exit`), startup prewarm |
| [`src/app_state.rs`](../src/app_state.rs) | Domain substate definitions, async message payloads, and local state transition helpers |
| [`src/async_task.rs`](../src/async_task.rs) | Generic `AsyncTask<T>` and `AsyncStreamTask<T>` primitives with monotonic `request_id` validation, cooperative cancellation, cancel-on-drop, and `RepaintGuard` disconnect handling |
| [`src/http_client.rs`](../src/http_client.rs) | Shared `OnceLock`-pooled `reqwest::blocking::Client` instances with HTTP/2, gzip, and brotli |
| [`src/innertube.rs`](../src/innertube.rs) | Unified YouTube InnerTube (`youtubei/v1`) client, DPAPI-encrypted `YouTubeWebSession`, SWR Home cache, feed/search/channel/watch parsers, and zero-quota authenticated mutations |
| [`src/stream_manifest.rs`](../src/stream_manifest.rs) | Single-extraction `StreamManifest` (`yt-dlp -J`), codec/HDR/audio selector, bounded TTL LRU cache, and single-slot speculative next-video prefetcher |
| [`src/persistence.rs`](../src/persistence.rs) | Coalesced single-threaded background `PersistenceWorker` for `History`, `Library`, and `Queue` with `(mtime, len)` `FileFingerprint` external-edit validation |
| [`src/history_sync.rs`](../src/history_sync.rs) | InnerTube remote watch history pagination, `RemoteHistoryFeedLru`, watchtime heartbeat tracking, and remote removal/clear mutations |
| [`src/playlist_sync.rs`](../src/playlist_sync.rs) | InnerTube remote playlists, Watch Later, Mix, and playlist detail sync plus add/remove/create mutations |
| [`src/mp4_mux.rs`](../src/mp4_mux.rs) | Pure-Rust streaming ISO BMFF (MP4) muxer for merging separate DASH video and audio tracks |
| [`src/win11.rs`](../src/win11.rs) | Native Windows 11 integration: DWM dark titlebar/rounded corners/Mica, Snap Layouts & Taskbar Thumbnail Toolbar subclassing, `ITaskbarList3` progress, `ICustomDestinationList` Jump Lists, theme registry observer, Windows Toasts, HKCU `fasttube://` registration |
| [`src/browse_ui.rs`](../src/browse_ui.rs) | Navigation drawer, home/search/explore grids, Shorts shelf, remote playlist banner, share modal, and subscriptions header |
| [`src/channel_ui.rs`](../src/channel_ui.rs) | Channel page header, local/OAuth follow button, dynamic tabs, sort chips, shelves, and About modal |
| [`src/settings_ui.rs`](../src/settings_ui.rs) | Dedicated 6-tab Settings page UI (`CentralPanel`), keybinding capture, tray/autostart/app updater, and backup modal |
| [`src/ui_icons.rs`](../src/ui_icons.rs) | Custom vector icons, sidebar items, card action structs, and header badges |
| [`src/skeleton.rs`](../src/skeleton.rs) | Shimmer skeleton placeholders for feeds, search, channel pages, watch sidebar, comments, transcript, and live chat |
| [`src/command_palette.rs`](../src/command_palette.rs) | `Ctrl+K` command palette, direct YouTube URL/playlist/channel parser, quick jump actions |
| [`src/comment_actions.rs`](../src/comment_actions.rs) | Watch-page comment composer, replies, and like/edit/delete actions |
| [`src/community_apis.rs`](../src/community_apis.rs) | Opt-in Return YouTube Dislike and DeArrow API clients (strictly idle unless enabled) |
| [`src/desktop.rs`](../src/desktop.rs) | Windows integrations (`fastframe-instance`, `fastframe-tray`, `fastframe-autostart` with `--start-hidden`, `fasttube://` protocol normalization, now playing, power) |
| [`src/player.rs`](../src/player.rs) | `libmpv` dynamic loading, warm `Player` instance, `mpv_observe_property` / `PlayerSnapshot`, OpenGL render API context, Win32 child `HWND` fallback, filters/EQ, direct `ytdl=no` stream loading |
| [`src/overlay.rs`](../src/overlay.rs) | In-player YouTube-style controls, progress bar, chapters, storyboard preview, and popup menus |
| [`src/layout.rs`](../src/layout.rs) | Responsive grid, watch columns, header, and rail geometry calculations |
| [`src/recommendations.rs`](../src/recommendations.rs) | Local `UserProfile` learning, feed chip filtering, Home & watch-sidebar ranking |
| [`src/thumbnails.rs`](../src/thumbnails.rs) | Bounded viewport-aware thumbnail/avatar scheduler, disk cache, and `egui` texture eviction |
| [`src/toast.rs`](../src/toast.rs) | Non-blocking transient toast notification queue |
| [`src/watch_extras.rs`](../src/watch_extras.rs) | On-demand subtitle/WebVTT fetching and watch-page metadata helpers |
| [`src/live_chat.rs`](../src/live_chat.rs) | Live stream chat and replay polling/parsing and message state |
| [`src/transcript.rs`](../src/transcript.rs) | Bounded WebVTT parser (`<= 2 MiB`, `<= 20,000` cues) and active-cue binary search |
| [`src/queue.rs`](../src/queue.rs) | Persisted FIFO queue (`queue.json`, max 50 items), `RepeatMode`, and per-occurrence IDs |
| [`src/history.rs`](../src/history.rs) | Local watch history (`history.json`, max 200 entries), `FileFingerprint`, atomic writer, and resume threshold logic |
| [`src/library.rs`](../src/library.rs) | Local shelves (Watch Later, Favorites), collections, and timestamp bookmarks (`library.json`) |
| [`src/followed_channels.rs`](../src/followed_channels.rs) | OAuth-free local followed channels (`followed-channels.json`) and public YouTube RSS feed poller |
| [`src/downloads.rs`](../src/downloads.rs) | Offline `yt-dlp` download queue, progress parser, cancellation, and format profiles |
| [`src/insights.rs`](../src/insights.rs) | Local watch insights aggregator computed from history, library, profile, and followed channels |
| [`src/backup.rs`](../src/backup.rs) | Non-secret JSON v1 backup envelope and `.fasttube-import-recovery` atomic rollback |
| [`src/settings.rs`](../src/settings.rs) | Theme, buffer profile, playback defaults, JSON `app-preferences.json` (`Keybindings`, opt-ins, tray/update flags, dirs) |
| [`src/i18n.rs`](../src/i18n.rs) | Bilingual UI language enum (`UiLanguage::{PtBr, En}`) and localized string selector |
| [`src/updater.rs`](../src/updater.rs) | FastTube GitHub Releases self-updater (`SHA256SUMS.txt` verified installer/portable staging) and `yt-dlp` atomic updater |
| [`src/sponsorblock.rs`](../src/sponsorblock.rs) | SponsorBlock API client, segment filtering/markers, and auto-skip runtime state |
| [`src/feed_prefs.rs`](../src/feed_prefs.rs) | Subscriptions feed filter and sort persistence |
| [`src/oauth.rs`](../src/oauth.rs) | Desktop OAuth flow, DPAPI/local token storage, YouTube Data API calls, Atom RSS parser |
| [`src/session.rs`](../src/session.rs) | Dedicated isolated browser profile launcher for Google OAuth / web session sign-in |
| [`src/process.rs`](../src/process.rs) | Subprocess spawning with `Cancellation`, timeout handling, and Win32 `ProcessJobObject` child-process cleanup |
| [`src/diagnostics.rs`](../src/diagnostics.rs) | `hwdec`/`hwdec-current` classification and redacted diagnostic report builder |

## System Invariants

- **Coalesced atomic persistence & import rollback**: `queue.json`, `history.json`, and `library.json` writes run off the UI thread through `PersistenceWorker` (`src/persistence.rs`), coalescing rapid updates per slot. Each write compares `(mtime, len)` via `FileFingerprint` and validates modified files before temp-file + atomic replace so corrupt files on disk are preserved untouched and surfaced as errors. Multi-file imports snapshot affected files to `.fasttube-import-recovery` (`src/backup.rs`) and roll back on failure or interrupted startup.
- **DPAPI session security & no secret leakage**: `YouTubeWebSession` (`src/innertube.rs`) is persisted as Windows DPAPI-encrypted `web-session.dpapi`, zeroized on drop, and redacted from `Debug` and error output. OAuth client secrets, tokens, and cookies are never written to backups (`src/backup.rs`). Diagnostic reports (`src/diagnostics.rs`) never expose raw error strings, URLs, tokens, cookies, or filesystem paths.
- **Async staleness, cancellation & disconnect visibility**: Background workers use `AsyncTask<T>` / `AsyncStreamTask<T>` (`src/async_task.rs`) pairing `process::Cancellation` with monotonic `request_id` validation, automatic cancel-on-drop, and `RepaintGuard` so stale results are discarded and unexpected worker exits/panics surface cleanly.
- **Win32 `ProcessJobObject` cleanup**: Spawned child processes (`yt-dlp`, etc.) are assigned to a Win32 `ProcessJobObject` configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`src/process.rs`) so no orphaned helper processes survive app exit or crash.
- **Warm `Player` lifecycle & `ytdl=no` `StreamManifest`**: `Player` (`src/player.rs`) is kept warm across video transitions (`stop` + `loadfile` instead of tearing down `libmpv` on every navigation, destroyed only on `hwdec`/buffer restart or exit), streams property updates asynchronously via `mpv_observe_property` into `PlayerSnapshot`, and plays pre-resolved direct URLs (`ytdl=no`) supplied by `StreamManifest` (`src/stream_manifest.rs`) with automatic fallback if direct streaming fails.
- **Queue occurrence correlation**: `QueuePlaybackCorrelation` ensures a queued item is removed only after `libmpv` confirms loading that exact occurrence ID; load failures and manual replacements leave the queue intact.
- **No `panic = "abort"` in `Cargo.toml`**: Release and dev profiles must retain stack unwinding (never set `panic = "abort"`), because `RepaintGuard`, `AsyncTask` cancel-on-drop, `PersistenceWorker` flush/shutdown, and `ProcessJobObject` cleanup rely on Rust drop semantics.
