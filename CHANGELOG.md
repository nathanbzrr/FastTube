# Changelog

All notable changes to FastTube are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [0.4.0] - 2026-11-21

### Added

- Unified YouTube InnerTube (`youtubei/v1`) client (`src/innertube.rs`) with SWR Home feed caching, Explore Hub, Shorts shelf/viewer, notifications inbox, DPAPI-encrypted `YouTubeWebSession` (`web-session.dpapi`), and zero-quota authenticated mutations.
- Remote InnerTube watch history sync (`src/history_sync.rs`), remote playlists / Watch Later / Mix sync (`src/playlist_sync.rs`), and single-extraction `StreamManifest` (`src/stream_manifest.rs`) with bounded TTL LRU caching and speculative next-video prefetcher.
- Native Windows 11 desktop integration (`src/win11.rs`): DWM dark titlebar/rounded corners/Mica, Snap Layouts & Taskbar Thumbnail Toolbar, `ITaskbarList3` progress, `ICustomDestinationList` Jump Lists, theme registry observer, Windows Toasts, and `HKCU` `fasttube://` registration.
- Shared `AsyncTask<T>` / `AsyncStreamTask<T>` primitives (`src/async_task.rs`), pooled HTTP/2 + gzip + brotli `reqwest` client (`src/http_client.rs`), coalesced off-thread `PersistenceWorker` (`src/persistence.rs`), and Win32 `ProcessJobObject` child-process cleanup (`src/process.rs`).
- Multi-process FastTube vs. Chrome/Edge comparative benchmark script (`scripts/compare-chrome.ps1`).

### Changed

- Modularized `src/main.rs`, `src/watch_ui.rs`, `src/library_ui.rs`, `src/search.rs`, and `src/main_tests.rs` into focused submodule directories (`src/app/`, `src/watch_ui/`, `src/library_ui/`, `src/search/`, `src/main_tests/`) and updated `docs/architecture.md`.
- Kept `Player` (`src/player.rs`) warm across video transitions (`stop` + `loadfile`), switched to async `mpv_observe_property` updates into `PlayerSnapshot`, and played pre-resolved direct URLs (`ytdl=no`) from `StreamManifest` with automatic fallback.
- Pinned `shinchiro/mpv-winbuild-cmake` release archive SHA-256 and enforced `yt-dlp.exe` / `libmpv` SHA-256 verification in `.github/workflows/release.yml` and `scripts/build-installer.ps1`.
- Enabled `lto = "thin"`, `codegen-units = 1` for release builds, and `opt-level = 2` for dev dependencies in `Cargo.toml`.

## [0.3.6] - 2026-11-21

### Added

- Native streaming ISO BMFF MP4 muxer (`src/mp4_mux.rs`) for `ffmpeg`-free DASH video+audio downloads, `libmpv`-backed audio conversion, and expanded local `ffmpeg` lookup.

### Changed

- Faster video startup and lower session RAM usage by parsing resolutions, subtitles, and storyboards from `ytInitialPlayerResponse`, deferring fallback `yt-dlp` metadata queries until after playback starts, tuning `libmpv` fast-start/cache options, and pruning session caches.
- Simplified the player overlay settings menu.

### Fixed

- Subtitle track deduplication (`-orig` aliases and non-ASR HLS mirrors), native track prioritization over `tlang=` auto-translations, empty/HTML VTT rejection, fresh native subtitle URL refresh before `yt-dlp` fallback, and unconditional clearing of stale subtitles when switching tracks.
- Reliable YouTube downloads on DASH-only and multi-audio videos.

## [0.3.5] - 2026-11-20

### Added

- Mini Player controls for previous video, play/pause, next video, volume, and
  returning to the full player.
- Direct playback toggling by clicking anywhere on the Mini Player video.
- Project metadata, MIT/Apache-2.0 licensing, contribution, security, and
  third-party notices for distribution.

### Fixed

- Mini Player hover behavior and layout defects at the window edges.
- Playback state handling when switching between Mini Player and the full player.