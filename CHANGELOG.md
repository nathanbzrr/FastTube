# Changelog

All notable changes to FastTube are documented here. The project follows
[Semantic Versioning](https://semver.org/).

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