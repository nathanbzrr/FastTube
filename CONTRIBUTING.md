# Contributing to FastTube

FastTube is a Windows-first Rust desktop application. Contributions should keep
the application local-first, keep account data private, and preserve the existing
persistence and playback guarantees documented in
[`docs/architecture.md`](docs/architecture.md).

## Development setup

1. Install Rust 1.98 or newer with a Windows x86-64 toolchain.
2. Provide `yt-dlp` and a compatible `libmpv` build for running the application.
3. Read the architecture guide before changing cross-module flows or persisted
   state.
4. Use an isolated Cargo target directory when `fasttube.exe` may be running.

## Validation

Run the repository's complete Windows validation sequence before opening a pull
request:

```powershell
cargo fmt --check
cargo check --all-targets --locked --target-dir target\validation
cargo clippy --all-targets --locked --target-dir target\validation
cargo test --locked --target-dir target\validation
```

Packaging changes should also run the relevant PowerShell tests under `scripts`.

## Pull requests

Describe the user-visible behavior and the reason for the change. Keep commits
focused, add tests for behavior that can regress, and avoid including OAuth
secrets, cookies, tokens, local paths, generated binaries, or personal data.
Documentation and code comments are written in English; user-facing UI text
follows the supported application languages.

## Security and privacy

Do not open a public issue containing credentials, cookies, raw diagnostic
exports, private URLs, or filesystem paths. Follow [`SECURITY.md`](SECURITY.md)
for private reporting.