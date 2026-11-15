# Architecture

FastTube is a native Windows YouTube desktop client built with `eframe`/`egui` (`glow` renderer), `libmpv` (embedded OpenGL render API with native `HWND` fallback), `yt-dlp` (public search, channel, and stream extraction), and the YouTube Data API v3 (optional Google OAuth account features).

## State Layout

The root application struct `FastTube` in [`src/main.rs`](../src/main.rs) acts as the UI and workflow coordinator. It holds 9 cross-cutting root fields (`video_metadata`, `selected_metadata`, `thumbnails`, `message`, `selected`, `scrolling`, `in_app_ui_pass`, `player`, `desktop`) and 12 domain substates defined in [`src/app_state.rs`](../src/app_state.rs):

| Field on `FastTube` | Type in `src/app_state.rs` | Domain |
| --- | --- | --- |
| `feed` | `BrowseFeedState` | Search query, active video list, feed filter, derived ranking cache, async search/home jobs, recent searches |
| `feed_preferences` | `FeedPreferencesState` | Subscriptions page filter/sort preferences and persistence path |
| `channels` | `ChannelState` | Active channel page, tab selection, video/shorts/streams pagination, public playlists, About info |
| `auth` | `AuthState` | OAuth `TokenSession`, reauth job, rating submission job, cached video ratings |
| `playback` | `PlaybackState` | Active playback correlation, playback preferences, quality fetch job, overlay/subtitles/player UI state, decoding diagnostics |
| `settings` | `SettingsState` | Theme, buffer profile, playback defaults, JSON app preferences (`SponsorBlock`, `hwdec`, privacy, storage), and active Settings tab state |
| `history` | `HistoryState` | Local watch history and throttled save checkpointing |
| `queue` | `QueueState` | Persisted FIFO playback queue and load confirmation state |
| `library` | `LibraryState` | OAuth-independent shelves, named collections, and timestamp bookmarks |
| `backup` | `BackupState` | JSON v1 backup export, import preview, merge/replace execution |
| `ui` | `NavigationUiState` | Active browse section, sidebar drawer state, shortcut modal, clipboard/status flags |
| `watch` | `WatchState` | Watch-page sidebar tab, transcript UI state, live comments pagination/posting, and SponsorBlock runtime state |

## Module Map (`src/`)

| File | Responsibility |
| --- | --- |
| [`src/main.rs`](../src/main.rs) | `FastTube` coordinator, `eframe::App` loop, async task polling, top-level UI views |
| [`src/main_tests.rs`](../src/main_tests.rs) | Integration and headless `egui` UI tests |
| [`src/app_state.rs`](../src/app_state.rs) | Domain substate definitions and local state transition helpers |
| [`src/desktop.rs`](../src/desktop.rs) | Windows desktop integrations (`fastframe-instance`, `fastframe-now-playing`, `fastframe-power`, `fastframe-autostart`) |
| [`src/player.rs`](../src/player.rs) | `libmpv` dynamic loading, OpenGL render API context, Win32 child `HWND` fallback |
| [`src/overlay.rs`](../src/overlay.rs) | In-player YouTube-style controls, progress bar, chapters, and popup menus |
| [`src/layout.rs`](../src/layout.rs) | Responsive grid, watch columns, header, and rail geometry calculations |
| [`src/search.rs`](../src/search.rs) | `yt-dlp` subprocess queries for search, Home, channels, playlists, and quality formats |
| [`src/recommendations.rs`](../src/recommendations.rs) | Local `UserProfile` learning, feed chip filtering, Home & watch-sidebar ranking |
| [`src/thumbnails.rs`](../src/thumbnails.rs) | Bounded viewport-aware thumbnail scheduler and `egui` texture eviction |
| [`src/watch_extras.rs`](../src/watch_extras.rs) | On-demand subtitle/WebVTT fetching and watch-page metadata helpers |
| [`src/transcript.rs`](../src/transcript.rs) | Bounded WebVTT parser (`<= 2 MiB`, `<= 20,000` cues) and active-cue binary search |
| [`src/queue.rs`](../src/queue.rs) | Persisted FIFO queue (`queue.json`, max 50 items) with per-occurrence IDs |
| [`src/history.rs`](../src/history.rs) | Local watch history (`history.json`, max 200 entries) and resume threshold logic |
| [`src/library.rs`](../src/library.rs) | Local shelves (Watch Later, Favorites), collections, and timestamp bookmarks (`library.json`) |
| [`src/backup.rs`](../src/backup.rs) | Non-secret JSON v1 backup envelope and `.fasttube-import-recovery` atomic rollback |
| [`src/settings.rs`](../src/settings.rs) | Theme, buffer profile, playback defaults, JSON `app-preferences.json`, and cache directory helpers |
| [`src/settings_ui.rs`](../src/settings_ui.rs) | Dedicated 6-tab Settings page UI (`CentralPanel`), category rail, and backup import modal |
| [`src/sponsorblock.rs`](../src/sponsorblock.rs) | SponsorBlock API client, segment filtering/markers, and auto-skip runtime state |
| [`src/feed_prefs.rs`](../src/feed_prefs.rs) | Subscriptions feed filter and sort persistence |
| [`src/oauth.rs`](../src/oauth.rs) | Desktop OAuth flow, DPAPI/local token storage, YouTube Data API calls |
| [`src/session.rs`](../src/session.rs) | Dedicated isolated browser profile launcher for Google OAuth sign-in |
| [`src/process.rs`](../src/process.rs) | Subprocess spawning with cancellation and timeout handling |
| [`src/diagnostics.rs`](../src/diagnostics.rs) | `hwdec`/`hwdec-current` classification and redacted diagnostic report builder |

## Navigating `src/main.rs`

`src/main.rs` is large (~16k lines). Avoid reading it sequentially; search by symbol name or query the Graphify index ([`docs/graphify.md`](graphify.md)):

- `struct FastTube` — root state and domain composition.
- `impl FastTube` — async pollers (`poll_*`), playback lifecycle (`open_video`, `close_video`), queue/history/library coordination, and decomposed UI helpers (`watch_primary_ui`, `watch_page_ui`, `navigation_ui`, `browse_ui`, `home_grid_ui`, `channel_section_ui`, and `settings_page_ui` in [`src/settings_ui.rs`](../src/settings_ui.rs)).
- `impl eframe::App for FastTube` — frame loop (`update`), keyboard shortcuts, and `on_exit` cleanup.

## System Invariants

- **Atomic persistence & import rollback**: `queue.json`, `history.json`, `library.json`, and settings files are written via temp-file + atomic replace. Corrupt files must be preserved on disk and surfaced as errors rather than silently overwritten. Imports snapshot affected files to `.fasttube-import-recovery` before writing and roll back on failure or interrupted startup.
- **No secrets in backups or diagnostics**: OAuth client secrets, tokens, and cookies are never included in backups (`src/backup.rs`). Diagnostic reports (`src/diagnostics.rs`) must never leak raw error strings, URLs, tokens, cookies, or filesystem paths.
- **Async staleness & cancellation**: Background workers (`std::thread::spawn` + `mpsc`) pair `process::Cancellation` with monotonic `request_id` checks so stale or canceled responses are discarded before mutating state.
- **Queue occurrence correlation**: `QueuePlaybackCorrelation` ensures a queued item is removed only after `libmpv` confirms loading that exact occurrence ID; load failures and manual replacements leave the queue intact.
- **On-demand `libmpv` lifecycle**: `Player` is instantiated only when opening a video and destroyed on `close_video` or app exit. Buffer profile changes apply on the next `Player` creation.
