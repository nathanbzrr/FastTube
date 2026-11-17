# FastTube

Native Windows desktop YouTube client built with `eframe`/`egui`, embedded `libmpv` playback, `yt-dlp` stream/metadata resolution, and local-first discovery.

## Requirements

- Rust 1.98 or later with a Windows x86-64 toolchain (MSVC or GNU).
- `yt-dlp` on `PATH` or alongside the portable executable.
- `libmpv-2.dll` or `mpv-2.dll` and any companion runtime DLLs.
- Optional: a Google Cloud **Desktop app** OAuth client for account features (subscriptions, playlists, likes/dislikes, and comments). Public browsing and all local features work without signing in.

### OAuth Setup

1. Create a **Desktop app** OAuth client in Google Cloud Console.
2. Enable the **YouTube Data API v3** for the project.
3. Save the downloaded JSON file as `%LOCALAPPDATA%\FastTube\oauth-client.json`.
4. Launch FastTube, click **Sign in**, and complete authorization in the browser.
5. If access expires, sign in again; tokens and cookies are never included in backups.

See [`scripts/build-portable.ps1`](scripts/build-portable.ps1) and [`docs/benchmarking.md`](docs/benchmarking.md) to assemble a portable distribution. The packager does not download binaries or bundle credentials; comply with the licenses of all redistributed binaries.

## Features

- **Public Home & Explainable Local Feed**: Topic-based discovery and a local ranking engine combining watch history, queue signals, recency decay, channel diversification, and visible recommendation reasons.
- **Explore**: Shortcuts for trending, music, gaming, podcasts, live streams, mixes, and new releases.
- **OAuth-Free Local Followed Channels & RSS**: Follow any channel locally without signing in (`followed-channels.json`), fetch uploads via public YouTube RSS feeds, and track unread `"New"` uploads.
- **Google Account Integration**: Optional subscriptions, playlists, comments, and Like/Dislike actions via YouTube Data API v3, while keeping public Home and all local features functional when offline or signed out.
- **Search, Public Playlists & `libmpv` Playback**: `yt-dlp` stream resolution, embedded OpenGL `libmpv` rendering (with native `HWND` fallback), and public playlist pages (`playlist?list=...`) with **Play All**, **Queue All**, and **Save as Collection**.
- **Player Visual/Audio Controls**: Storyboard hover scrub, multi-audio track selector, aspect ratio/zoom presets, video EQ (brightness/contrast/saturation/gamma), and 10-band/preset audio equalizer.
- **Offline Download Manager**: Download videos or audio (`MP4 Best/1080p/720p/480p`, `M4A`, `Opus`, `MP3`) via `yt-dlp` directly from cards or the watch page, with per-item progress, cancellation, and a configurable downloads folder.
- **Local Watch Insights**: Aggregate local statistics (total watch time, completed count, top channels, learned topic interests, and library/follow counters) computed strictly on-device.
- **Opt-In Community APIs**: Optional Return YouTube Dislike counts and DeArrow titles (`ryd_enabled` and `dearrow_enabled`, both **disabled by default**—zero requests are made unless explicitly enabled in Settings → Privacy).
- **Shorts Stage**: Dedicated vertical layout with a centered player and side-by-side details on wide windows.
- **Persisted FIFO Queue & Repeat Modes**: Stored in `queue.json` (up to 50 items) with unique per-occurrence IDs and `Off` / `Repeat All` / `Repeat One` playback modes. Items are popped only after `libmpv` confirms loading that exact occurrence.
- **Local Watch History & Resume**: Stored in `history.json` (up to 200 entries) with playback resume and manual completion toggling.
- **Local Library**: OAuth-independent storage in `library.json` with Watch Later, Favorites, named collections, and timestamped bookmarks with notes.
- **On-Demand Subtitles & Transcript**: Searchable WebVTT viewer (`<= 2 MiB`, `<= 20,000` cues) with active-cue auto-centering, click-to-seek, font sizing, and high-contrast mode without downloading the video.
- **Chapters & Bookmarks**: Navigate `libmpv` chapters and local bookmarks directly during playback.
- **Subscriptions Filters**: Hide watched videos, Shorts, or live streams, filter by channel, and sort by upload date when available.
- **Mini Player & Auto Mini Player**: Resizes the active window and pins it always-on-top while preserving playback state, optionally activating automatically when navigating away from the watch page.
- **Bilingual UI & Custom Keybindings**: Switch instantly between `Português (Brasil)` and `English`, and customize player shortcuts live in Settings → General.
- **Built-in FastTube & `yt-dlp` Updaters (`GitHub Releases`)**: Automatically checks `nathanbzrr/FastTube` on GitHub at startup (opt-outable), verifies downloads against `SHA256SUMS.txt`, and seamlessly updates both Inno Setup and portable installations, alongside atomic `yt-dlp` updates in Settings → Storage.
- **System Tray & Windows Startup**: Minimize or close to the Windows notification area with playback/update tray controls, toggle **Start with Windows**, and optionally launch hidden to the tray (`--start-hidden`).
- **`fasttube://` Protocol**: Launch YouTube links directly via `fasttube://` URI registration in the Windows installer.
- **Redacted Diagnostics**: Classifies auth, quota, network, and `hwdec` decoding issues with actionable next steps and copies a redacted report free of raw errors, URLs, tokens, cookies, or local paths.
- **Backup & Import (v1 JSON)**: Exports library, bookmarks, queue, history, and preferences (max `8 MiB`; never exports OAuth secrets, tokens, or cookies). Imports show a preview and support **merge** or **replace**.
- **Accessibility**: Keyboard focus indicators, high-contrast captions, and an **F1** shortcuts overlay.

## Keyboard Shortcuts

Default shortcuts (customizable in **Settings → General → Keyboard Shortcuts**):

| Keys | Action |
| --- | --- |
| `F1` | Open or close the shortcuts guide |
| `Ctrl+K` | Open command palette / jump to URL or playlist |
| `Tab` / `Shift+Tab` | Move focus across controls |
| `Enter` or `Space` | Activate focused control |
| `Alt+1` | Home |
| `Alt+2` | Explore |
| `Alt+3` | History |
| `Alt+4` | Queue |
| `Alt+5` | Library |
| `Space` or `K` | Play or pause |
| `←` / `→` | Seek back or forward (configurable step) |
| `M` | Mute or unmute |
| `T` | Toggle theater mode |
| `F` | Toggle fullscreen |
| `I` | Toggle Mini Player |
| `Esc` | Exit fullscreen or close open panel/modal |

## Data Storage & Recovery

Application data lives in `%LOCALAPPDATA%\FastTube` (`settings.json`, `app-preferences.json`, `history.json`, `library.json`, `queue.json`, `feed-preferences.json`, and `followed-channels.json`). Data files are written via temporary files and atomic replacement; corrupt files are preserved untouched and reported as warnings.

Before applying an import, FastTube snapshots affected files into `.fasttube-import-recovery`. If any write fails—or if the process terminates mid-import—FastTube rolls back to the snapshot before loading application state on next startup.

## Development & Validation

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) targets standard Windows MSVC (`stable`). To avoid overwriting or locking a running `fasttube.exe`, pass an isolated `--target-dir`:

```powershell
cargo fmt --check
cargo check --all-targets --locked --target-dir target\validation
cargo clippy --all-targets --locked --target-dir target\validation
cargo test --locked --target-dir target\validation
```

If building with the GNU toolchain (`+stable-x86_64-pc-windows-gnu`), ensure MinGW/WinLibs is on `PATH` and `CC_x86_64_pc_windows_gnu`, `CXX_x86_64_pc_windows_gnu`, `AR_x86_64_pc_windows_gnu`, and `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` point to your toolchain binaries.

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — Domain state layout, module map, and system invariants.
- [`docs/graphify.md`](docs/graphify.md) — Generating and querying the local AST code graph.
- [`docs/benchmarking.md`](docs/benchmarking.md) — Portable packaging and process/network sampling scripts.
