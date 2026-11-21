# FastTube Agent & Contributor Guide

FastTube is a Windows-first Rust desktop YouTube client (`eframe`/`egui` + `libmpv` + `yt-dlp` + YouTube Data API v3).

## Architecture & Navigation

- Read [`docs/architecture.md`](docs/architecture.md) first for the 39-module map, the 14 domain substates in [`src/app_state.rs`](src/app_state.rs), and system invariants.
- [`src/main.rs`](src/main.rs) is ~13,200 lines. **Do not read it sequentially.**
- **Graphify decision rule (per prompt)**:
  - **Use Graphify (`graphify query --budget 1800`, `explain`, `affected`, `path`)** when the prompt involves cross-module call flows, impact/dependency analysis, architectural questions, or locating logic inside `src/main.rs` without a known symbol/line (see [`docs/graphify.md`](docs/graphify.md)).
  - **Skip Graphify (use targeted search / `view_file` directly)** when the prompt names a specific file, function, struct, UI string, or small localized fix, to avoid CLI overhead and save tokens.
  - Run `graphify update .` after structural Rust changes (or `graphify update . --force` if `graphify-out/` is missing).

## Core Invariants

Preserve these invariants across all changes:

1. **Atomic persistence & rollback**: Local JSON/settings files use temp-file + atomic replace. Corrupt files must remain untouched on disk and surface an error. Multi-file imports snapshot to `.fasttube-import-recovery` and roll back on failure or interrupted startup (`src/backup.rs`).
2. **No secret leakage**: OAuth secrets, tokens, and cookies must never be written to backups (`src/backup.rs`). Diagnostic reports (`src/diagnostics.rs`) must never expose raw error text, URLs, tokens, cookies, or filesystem paths.
3. **Async staleness & cancellation**: Background workers (`std::thread::spawn` + `mpsc`) must use `process::Cancellation` and validate monotonic `request_id`s before applying results to state.
4. **Queue confirmation**: Never remove a queued item until `libmpv` confirms loading that exact `QueuePlaybackCorrelation` occurrence ID.
5. **Isolated `cargo` targets**: Always use `--target-dir target\<name>` when building or testing locally if `fasttube.exe` may be running, to avoid Windows file-lock errors.

## Validation Commands

Run formatting, compilation, lints, and tests before concluding code changes:

```powershell
cargo fmt --check
cargo check --all-targets --locked --target-dir target\validation
cargo clippy --all-targets --locked --target-dir target\validation
cargo test --locked --target-dir target\validation
```

- CI runs on Windows MSVC (`stable`). For GNU builds (`+stable-x86_64-pc-windows-gnu`), configure `CC_x86_64_pc_windows_gnu`, `CXX_x86_64_pc_windows_gnu`, `AR_x86_64_pc_windows_gnu`, and `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` if MinGW is not already in `PATH`.
- PowerShell tooling tests: `.\scripts\test-portable-package.ps1`, `.\scripts\test-measure-process.ps1`, `.\scripts\test-measure-network.ps1`.

## Turn & Task Completion Discipline

- **Never end a turn while tasks are still running**: Do not finish a turn or reply with an interim "waiting for background task" message while any non-daemon `run_command` task, build, test, or subagent/delegate work is still in progress.
- **Drain background tasks within the same turn**: When a `run_command` call exceeds `WaitMsBeforeAsync` (10,000 ms) and backgrounds into a `taskId`, never choose the "update the user and end the turn" branch. Proceed with other independent work or check `manage_task` (`Action: "status"`) until the task reaches `DONE`, resolve any failures, and only end the turn after all tasks and validations have completely finished.
- **Prefer bounded synchronous steps**: Always pass `WaitMsBeforeAsync: 10000` for normal build/test/lint commands so incremental runs complete synchronously without leaving orphaned background tasks.

## Code & Documentation Style

- Write all code comments, commit messages, filenames, and documentation in **English** (user-facing UI strings follow the app's UI language).
- Keep code comments minimal and high-signal: document non-obvious invariants or *why* a constraint exists—never narrate step-by-step *what* the code does or restate identifiers.
- Do not commit dated session logs, line-by-line code walkthroughs, or one-off measurement/validation diaries to `docs/`.
