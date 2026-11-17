# Architecture

FastTube is a native Windows YouTube desktop client built with `eframe`/`egui` (`glow` renderer), `libmpv` (embedded OpenGL render API with native `HWND` fallback), `yt-dlp` (public search, channel, and stream extraction), and the YouTube Data API v3 (optional Google OAuth account features).

## State Layout

The root application struct `FastTube` in [`src/main.rs`](../src/main.rs) acts as the UI and workflow coordinator. It holds 9 cross-cutting root fields (`video_metadata`, `selected_metadata`, `thumbnails`, `avatars`, `message`, `selected`, `scrolling`, `player`, `desktop`) and 14 domain substates defined in [`src/app_state.rs`](../src/app_state.rs):

| Field on `FastTube` | Type in `src/app_state.rs` | Domain |
| --- | --- | --- |
| `feed` | `BrowseFeedState` | Search query, active video list, feed filter, derived ranking cache, async search/home jobs, recent searches |
| `feed_preferences` | `FeedPreferencesState` | Subscriptions page filter/sort preferences and persistence path |
| `channels` | `ChannelState` | Active channel page, tab selection, video/shorts/streams pagination, public playlists, About info |
| `auth` | `AuthState` | OAuth `TokenSession`, reauth job, rating submission job, cached video ratings, remote playlists |
| `playback` | `PlaybackState` | Active playback correlation, playback preferences, quality/EQ/aspect/storyboard state, decoding diagnostics |
| `settings` | `SettingsState` | Theme, buffer profile, playback defaults, JSON app preferences (`SponsorBlock`, `Keybindings`, `UiLanguage`, privacy, tray/updates, storage), app & `yt-dlp` updater jobs |
| `history` | `HistoryState` | Local watch history and throttled save checkpointing |
| `queue` | `QueueState` | Persisted FIFO playback queue, repeat mode (`Off`/`All`/`One`), remote playlist loader, load confirmation state |
| `library` | `LibraryState` | OAuth-independent shelves, named collections, and timestamp bookmarks |
| `backup` | `BackupState` | JSON v1 backup export, import preview, merge/replace execution |
| `ui` | `NavigationUiState` | Active browse section (including `Downloads` and `Insights`), sidebar drawer state, command palette, shortcut modal, clipboard target |
| `watch` | `WatchState` | Watch-page sidebar tab, transcript UI state, live comments pagination/posting, SponsorBlock runtime, opt-in community extras cache |
| `followed_channels` | `FollowedChannelsState` | OAuth-free local followed channels (`followed-channels.json`) and public YouTube RSS feed poller |
| `downloads` | `DownloadState` | Offline `yt-dlp` download queue and progress/status state |

## Module Map (`src/`)

| File | Responsibility |
| --- | --- |
| [`src/main.rs`](../src/main.rs) | `FastTube` coordinator, `eframe::App` loop, async task polling, top-level navigation and modals |
| [`src/main_tests.rs`](../src/main_tests.rs) | Integration and headless `egui` UI tests |
| [`src/app_state.rs`](../src/app_state.rs) | Domain substate definitions, async message payloads, and local state transition helpers |
| [`src/browse_ui.rs`](../src/browse_ui.rs) | Navigation drawer, home/search grids, Shorts shelf, remote playlist banner, and subscriptions header |
| [`src/channel_ui.rs`](../src/channel_ui.rs) | Channel page header, local/OAuth follow button, tabs, shelves, and About modal |
| [`src/library_ui.rs`](../src/library_ui.rs) | Queue/history/library views, Offline Downloads view, and Local Watch Insights panel |
| [`src/watch_ui.rs`](../src/watch_ui.rs) | Watch player stage, video/audio EQ, aspect/zoom, storyboard scrub, comments, live chat, and transcript |
| [`src/ui_icons.rs`](../src/ui_icons.rs) | Custom vector icons, sidebar items, card action structs, and header badges |
| [`src/command_palette.rs`](../src/command_palette.rs) | `Ctrl+K` command palette, direct YouTube URL/playlist/channel parser, quick jump actions |
| [`src/comment_actions.rs`](../src/comment_actions.rs) | Watch-page comment composer, replies, and like/delete actions |
| [`src/community_apis.rs`](../src/community_apis.rs) | Opt-in Return YouTube Dislike and DeArrow API clients (strictly idle unless enabled) |
| [`src/desktop.rs`](../src/desktop.rs) | Windows integrations (`fastframe-instance`, `fastframe-tray`, `fastframe-autostart` with `--start-hidden`, `fasttube://` protocol normalization, now playing, power) |
| [`src/player.rs`](../src/player.rs) | `libmpv` dynamic loading, OpenGL render API context, Win32 child `HWND` fallback, filters/EQ |
| [`src/overlay.rs`](../src/overlay.rs) | In-player YouTube-style controls, progress bar, chapters, storyboard preview, and popup menus |
| [`src/layout.rs`](../src/layout.rs) | Responsive grid, watch columns, header, and rail geometry calculations |
| [`src/search.rs`](../src/search.rs) | `yt-dlp` subprocess queries for search, Home, channels, public playlists, and quality formats |
| [`src/recommendations.rs`](../src/recommendations.rs) | Local `UserProfile` learning, feed chip filtering, Home & watch-sidebar ranking |
| [`src/thumbnails.rs`](../src/thumbnails.rs) | Bounded viewport-aware thumbnail scheduler and `egui` texture eviction |
| [`src/toast.rs`](../src/toast.rs) | Non-blocking transient toast notification queue |
| [`src/watch_extras.rs`](../src/watch_extras.rs) | On-demand subtitle/WebVTT fetching and watch-page metadata helpers |
| [`src/live_chat.rs`](../src/live_chat.rs) | Live stream chat polling and message rendering |
| [`src/transcript.rs`](../src/transcript.rs) | Bounded WebVTT parser (`<= 2 MiB`, `<= 20,000` cues) and active-cue binary search |
| [`src/queue.rs`](../src/queue.rs) | Persisted FIFO queue (`queue.json`, max 50 items), `RepeatMode`, and per-occurrence IDs |
| [`src/history.rs`](../src/history.rs) | Local watch history (`history.json`, max 200 entries) and resume threshold logic |
| [`src/library.rs`](../src/library.rs) | Local shelves (Watch Later, Favorites), collections, and timestamp bookmarks (`library.json`) |
| [`src/followed_channels.rs`](../src/followed_channels.rs) | OAuth-free local followed channels (`followed-channels.json`) and public YouTube RSS feed poller |
| [`src/downloads.rs`](../src/downloads.rs) | Offline `yt-dlp` download queue, progress parser, cancellation, and format profiles |
| [`src/insights.rs`](../src/insights.rs) | Local watch insights aggregator computed from history, library, profile, and followed channels |
| [`src/backup.rs`](../src/backup.rs) | Non-secret JSON v1 backup envelope and `.fasttube-import-recovery` atomic rollback |
| [`src/settings.rs`](../src/settings.rs) | Theme, buffer profile, playback defaults, JSON `app-preferences.json` (`Keybindings`, opt-ins, tray/update flags, dirs) |
| [`src/settings_ui.rs`](../src/settings_ui.rs) | Dedicated 6-tab Settings page UI (`CentralPanel`), keybinding capture, tray/autostart/app updater, and backup modal |
| [`src/i18n.rs`](../src/i18n.rs) | Bilingual UI language enum (`UiLanguage::{PtBr, En}`) and localized string selector |
| [`src/updater.rs`](../src/updater.rs) | FastTube GitHub Releases self-updater (`SHA256SUMS.txt` verified installer/portable staging) and `yt-dlp` atomic updater |
| [`src/sponsorblock.rs`](../src/sponsorblock.rs) | SponsorBlock API client, segment filtering/markers, and auto-skip runtime state |
| [`src/feed_prefs.rs`](../src/feed_prefs.rs) | Subscriptions feed filter and sort persistence |
| [`src/oauth.rs`](../src/oauth.rs) | Desktop OAuth flow, DPAPI/local token storage, YouTube Data API calls, Atom RSS parser |
| [`src/session.rs`](../src/session.rs) | Dedicated isolated browser profile launcher for Google OAuth sign-in |
| [`src/process.rs`](../src/process.rs) | Subprocess spawning with cancellation and timeout handling |
| [`src/diagnostics.rs`](../src/diagnostics.rs) | `hwdec`/`hwdec-current` classification and redacted diagnostic report builder |

## Navigating `src/main.rs`

`src/main.rs` coordinates root state; UI sections live in focused modules ([`src/browse_ui.rs`](../src/browse_ui.rs), [`src/channel_ui.rs`](../src/channel_ui.rs), [`src/library_ui.rs`](../src/library_ui.rs), [`src/watch_ui.rs`](../src/watch_ui.rs), [`src/settings_ui.rs`](../src/settings_ui.rs)). Search by symbol name or query the Graphify index ([`docs/graphify.md`](graphify.md)):

- `struct FastTube` — root state and domain composition.
- `impl FastTube` — async pollers (`poll_*`), playback lifecycle (`open_video`, `close_video`), queue/history/library/download coordination.
- `impl eframe::App for FastTube` — frame loop (`update`), customizable keyboard shortcuts (`Keybindings`), and `on_exit` cleanup.

## System Invariants

- **Atomic persistence & import rollback**: `queue.json`, `history.json`, `library.json`, and settings files are written via temp-file + atomic replace. Corrupt files must be preserved on disk and surfaced as errors rather than silently overwritten. Imports snapshot affected files to `.fasttube-import-recovery` before writing and roll back on failure or interrupted startup.
- **No secrets in backups or diagnostics**: OAuth client secrets, tokens, and cookies are never included in backups (`src/backup.rs`). Diagnostic reports (`src/diagnostics.rs`) must never leak raw error strings, URLs, tokens, cookies, or filesystem paths.
- **Async staleness & cancellation**: Background workers (`std::thread::spawn` + `mpsc`) pair `process::Cancellation` with monotonic `request_id` checks so stale or canceled responses are discarded before mutating state.
- **Queue occurrence correlation**: `QueuePlaybackCorrelation` ensures a queued item is removed only after `libmpv` confirms loading that exact occurrence ID; load failures and manual replacements leave the queue intact.
- **On-demand `libmpv` lifecycle**: `Player` is instantiated only when opening a video and destroyed on `close_video` or app exit. Buffer profile changes apply on the next `Player` creation.
