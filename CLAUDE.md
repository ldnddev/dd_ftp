# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Terminal FTP/SFTP/FTPS client (Rust + ratatui + tokio). Cargo workspace, binary is `dd_ftp_cli` (installed as `dd_ftp`).

## Common Commands

```bash
# Run TUI (debug)
cargo run -p dd_ftp_cli

# Build single crate
cargo build -p dd_ftp_app

# Release build + install to ~/.local/bin/dd_ftp
./install.sh

# Workspace-wide
cargo check
cargo build
cargo test
cargo test -p dd_ftp_app          # one crate
cargo test -p dd_ftp_app -- name  # one test by name substring
cargo clippy --workspace --all-targets
cargo fmt --all
```

Pre-populate connection at launch via env: `DD_FTP_HOST`, `DD_FTP_PORT`, `DD_FTP_USER`, `DD_FTP_PASS` or `DD_FTP_KEY`, `DD_FTP_PATH` (see README controls section for keybinds).

## Architecture

Workspace crates form a one-way dependency graph; respect it when adding code:

```
dd_ftp_cli  →  dd_ftp_app, dd_ftp_ui, dd_ftp_protocols, dd_ftp_ftp, dd_ftp_storage, dd_ftp_core
dd_ftp_app  →  dd_ftp_core, dd_ftp_transfer
dd_ftp_ui   →  dd_ftp_app, dd_ftp_core, dd_ftp_transfer
dd_ftp_protocols / dd_ftp_ftp / dd_ftp_transfer / dd_ftp_storage  →  dd_ftp_core
dd_ftp_core →  (no internal deps; defines traits + types)
```

### Crate roles
- `dd_ftp_core` — shared types: `ConnectionInfo`, `Protocol`, `FileEntry`, `TransferJob/Status/Direction`, and the `RemoteSession` trait (async). All higher crates speak this vocabulary.
- `dd_ftp_protocols` — `SftpSession` (ssh2-backed) implementing `RemoteSession`.
- `dd_ftp_ftp` — `UnifiedFtpSession` (async_ftp + tokio-rustls) covering plain FTP and FTPS via `FtpVariant`.
- `dd_ftp_transfer` — `TransferQueue` (pending/active/completed/failed lists, retry, progress).
- `dd_ftp_storage` — `SiteManager` (bookmark TOML config) and `SecretStore` (OS keyring; passwords never persisted in site config).
- `dd_ftp_app` — Redux-style core: `AppState` + `Action` enum + pure `reduce(state, action)`. No IO. Sub-state owns the queue. Live sockets live in CLI `Runtime`, not `AppState`.
- `dd_ftp_ui` — ratatui render layer + theme loader. Read-only over `AppState`.
- `dd_ftp_cli` — terminal lifecycle, crossterm event loop, async glue, spawns transfer workers. `Runtime` owns `enum SessionHandle { Sftp(SftpSession), Ftp(UnifiedFtpSession) }` (no `dyn RemoteSession`), generation, cancel flags, worker handles, `in_flight`, and `pending_scan`. IO requests (`request_list` / `request_fs` / `connect_off_thread`) never `.await` in the key handler.

### Event / IO flow
1. CLI captures crossterm events → translates to `Action`s → calls `reduce(&mut state, action)` (pure state mutation).
2. `dd_ftp_ui::render(frame, &state)` draws each frame.
3. Transfers: when state has pending jobs and active < `worker_max_concurrency`, CLI spawns a `tokio::spawn` worker per job. Worker owns its own session (`SftpSession` or `UnifiedFtpSession`), reports `WorkerMessage::Progress` / `Done` over an `mpsc` channel. Main loop drains the channel and dispatches `UpdateTransferProgress` / `MarkTransferCompleted|Failed|Cancelled`.
4. Cancellation = per-worker `Arc<AtomicBool>`; protocol code polls it during chunk loops.
5. Passwords: not stored in `SiteConfig`. Hydrated on demand from `SecretStore` (keyring) keyed by `(name, username, host, port)`.

### Adding a protocol
Implement `dd_ftp_core::RemoteSession` in a new crate, extend `Protocol` enum in `dd_ftp_core::connection`, branch on it in `dd_ftp_cli::main` worker spawn (the `match protocol { Sftp => ..., Ftp | Ftps => ... }` block).

### Adding UI state
1. Field on `AppState` (with `Default`).
2. Variant on `Action`.
3. Arm in `reduce`.
4. Render in `dd_ftp_ui::render`.
Keep `reduce` pure — no IO, no async. IO belongs in `dd_ftp_cli` and protocol crates.

## Theming

`THEME_STRUCTURE_STANDARD.md` is the canonical token schema shared across ldnddev TUIs. Lookup order:
1. `./dd_ftp_theme.yml`
2. `~/.config/ldnddev/dd_ftp_theme.yml`
3. Built-in defaults

When adding a UI element that needs a new color, add the token to `THEME_STRUCTURE_STANDARD.md` first, then `dd_ftp_ui::theme`, then consume in `render`. Do not hardcode colors past the theme load. `F2` toggles in-app theme debug overlay.
