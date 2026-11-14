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
- **Google Account Integration**: Subscriptions, playlists, comments, and Like/Dislike actions via YouTube Data API v3, while keeping public Home and local features functional when offline or quota-limited.
- **Search & `libmpv` Playback**: `yt-dlp` stream resolution and embedded OpenGL `libmpv` rendering (with native `HWND` fallback).
- **Shorts Stage**: Dedicated vertical layout with a centered player and side-by-side details on wide windows.
- **Persisted FIFO Queue**: Stored in `queue.json` (up to 50 items) with unique per-occurrence IDs so duplicate URLs behave cleanly. Items are popped only after `libmpv` confirms loading that exact occurrence.
- **Local Watch History & Resume**: Stored in `history.json` (up to 200 entries) with playback resume and manual completion toggling.
- **Local Library**: OAuth-independent storage in `library.json` with Watch Later, Favorites, named collections, and timestamped bookmarks with notes.
- **On-Demand Subtitles & Transcript**: Searchable WebVTT viewer (`<= 2 MiB`, `<= 20,000` cues) with active-cue auto-centering, click-to-seek, font sizing, and high-contrast mode without downloading the video.
- **Chapters & Bookmarks**: Navigate `libmpv` chapters and local bookmarks directly during playback.
- **Subscriptions Filters**: Hide watched videos, Shorts, or live streams, filter by channel, and sort by upload date when available.
- **Mini Player**: Resizes the active window and pins it always-on-top while preserving playback state, restoring previous window bounds on exit.
- **Redacted Diagnostics**: Classifies auth, quota, network, and `hwdec` decoding issues with actionable next steps and copies a redacted report free of raw errors, URLs, tokens, cookies, or local paths.
- **Backup & Import (v1 JSON)**: Exports library, bookmarks, queue, history, and preferences (max `8 MiB`; never exports OAuth secrets, tokens, or cookies). Imports show a preview and support **merge** or **replace**.
- **Accessibility**: Keyboard focus indicators, high-contrast captions, and an **F1** shortcuts overlay.

## Keyboard Shortcuts

| Keys | Action |
| --- | --- |
| `F1` | Open or close the shortcuts guide |
| `Tab` / `Shift+Tab` | Move focus across controls |
| `Enter` or `Space` | Activate focused control |
| `Alt+1` | Home |
| `Alt+2` | Explore |
| `Alt+3` | History |
| `Alt+4` | Queue |
| `Alt+5` | Library |
| `Space` or `K` | Play or pause |
| `←` / `→` | Seek back or forward 10 seconds |
| `M` | Mute or unmute |
| `T` | Toggle theater mode |
| `F` | Toggle fullscreen |
| `Esc` | Exit fullscreen or close open panel/modal |

## Data Storage & Recovery

Application data lives in `%LOCALAPPDATA%\FastTube`. Queue, history, library, and settings files are written via temporary files and atomic replacement. Corrupt data files are preserved untouched and reported as warnings.

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
