# Changelog

All notable changes to this project will be documented in this file.

## [1.2.0] - 2026-09-04

### Added
- Unique cancel key (`Ctrl+C`); `C` stays directory compare; F1/F2 toggle closed; Enter opens dirs and queues files
- Recursive folder upload/download with overwrite/skip/rename prompts (default skip)
- SFTP host-key TOFU against `~/.ssh/known_hosts`; path-escape guards on local and remote names
- File list columns (name, size, date, permissions); disconnected remote empty state; error toasts
- Multi-select (Space), SFTP chmod (`p`), sort (`s`/`S`), hide-dotfiles (`.`), transfer speed/ETA
- Help overlay and README generated from a single `KEYMAP` table; compare badges in existing lists

### Changed
- Selection indexes the visible (filtered) list; FTP downloads use `cwd + name` instead of the raw LIST line
- Connect, list, and file operations run off the UI thread; UI SFTP session is persistent; FTP transfers stream with progress/cancel
- CLI split into modules; live protocol sessions moved out of `AppState` into a CLI `Runtime`; `RemoteSession` implemented for FTP/FTPS

### Fixed
- Dead cancel keybind shadowed by compare; empty remote refresh leaving stale listings; post-transfer SFTP-only relist
- Filter highlight/action desync; leftover DEBUG delete status; theme reloaded from disk every frame

## [Unreleased] - 2026-06-16

### Added
- Remote file operations now fully wired over the `RemoteSession` trait for SFTP and FTP/FTPS:
  - rename remote files/directories
  - delete remote files and directories (`rmdir` vs `unlink`/`rm` chosen by entry kind)
  - create remote directories (`mkdir`)
- New trait methods on `dd_ftp_core::RemoteSession`: `rename`, `remove_file`, `remove_dir`, `create_dir` (implemented in `SftpSession` via `ssh2`, and as inherent methods on `UnifiedFtpSession` via `async_ftp`)
- Mouse support in input fields: click-to-position caret and click+drag text selection, with cursor and selection rendering
- Keyboard cursor editing in input fields (arrows, Home/End, word delete, selection)
- `TextField` cursor + selection model now backs the prompt and Quick Connect fields
- Toast notifications and visual alignment to the shared `LDNDDEV_TUI_VISUAL_STANDARD.md` (theme tokens, layout, spacing)

### Changed
- New-item prompt: `Ctrl+n` opens a single prompt; `Tab` toggles between File and Folder (replaces `Ctrl+Shift+N`, which conflicted with terminal emulators such as WezTerm)
- Remote create-file now uploads the staged empty file synchronously before cleanup, so the file reliably exists on the server
- Remote create/rename/delete refresh the remote pane listing on success

### Fixed
- Remote rename was a no-op stub ("not implemented") — now performs the operation
- Remote delete was a no-op stub — now deletes files and directories
- Remote folder creation was a no-op stub — now creates the directory
- Remote file creation race: the temp source file was deleted immediately after queuing, so the async worker uploaded nothing; create now completes before the temp is removed
- Remote paths for create/rename/delete are built from `remote_cwd` + entry name (correct for FTP, whose `FileEntry.path` holds the raw `LIST` line) instead of relying on `entry.path`
- Scrollbar drag includes cancelled jobs in the queue total; mouse-wheel list scroll is guarded behind open modals

## [v0.4.0-phase4] - 2026-04-04

### Added
- FTPS explicit TLS support in unified crate `dd_ftp_ftp` using `async_ftp` secure mode (`AUTH TLS` upgrade before auth)
- FTPS transfer path now supports:
  - connect
  - list (`NLST`)
  - upload
  - download
- FTPS root trust setup with compatible rustls stack (`tokio-rustls 0.23`, `webpki-roots 0.22`)

### Changed
- Unified FTP/FTPS crate is now the primary runtime path for FTP-family transfers
- Removed active FTP/FTPS implementations from `dd_ftp_protocols` exports (SFTP-only exports remain)
- Dependency alignment for FTPS compatibility:
  - `async_ftp` now uses `secure` feature
  - `tokio-rustls` pinned to `0.23` to match `async_ftp` secure API
  - `webpki-roots` pinned to `0.22`
- Linux keyring backend now explicitly enabled via `keyring` feature set (`linux-native-sync-persistent`) for durable password persistence

### Added
- Parallel transfer worker scheduling in CLI runtime (up to `worker_max_concurrency`, default `2`)
- Queue header now reports active worker usage (`active/max`)
- Queue pane now renders per-job rows (active/pending/failed) with compact progress bars and truncation
- Queue pane expands when focused to show more rows (A/P/F) with improved density
- Theme file support in UI with lookup order:
  - `./dd_ftp_theme.yml`
  - `~/.config/ldnddev/dd_ftp_theme.yml`

### Fixed
- FTPS compile/runtime blockers from mixed rustls API generations (0.20 vs 0.23+ styles)
- Type inference issue in `retr(...)` callback return type for FTPS downloads
- Bookmark edit/save flow now updates existing bookmark records instead of returning "Bookmark already exists"
- Bookmark password persistence now survives app restarts on Linux with keyring backend enabled
- Added keyring health check UX + error modal diagnostics to expose backend failures clearly

## [v0.3.0-phase3] - 2026-04-04

### Added
- Real SFTP upload/download operations in `dd_ftp_protocols::SftpSession`
- Async background transfer worker with queue-driven processing
- Transfer progress events (bytes + optional percent)
- Queue lifecycle states: pending, active, completed, failed, cancelled
- Queue controls: clear pending (`X`), retry last failed (`R`), cancel active (`C`)
- F1 help modal with dim backdrop + Esc close
- Quick Connect modal (`o`) with fields:
  - Name/Label
  - Host
  - Port
  - Username
  - Password (masked)
  - Protocol
  - Initial Path
- Bookmark modal (`m`) with actions:
  - connect (`c`)
  - load/edit (`e` / Enter)
  - delete (`d`)
  - set default (`D`)
- Bookmark persistence to `~/.config/dd_ftp/sites.toml`
- `default_site` support in bookmark storage

### Changed
- Connection flow now bookmark-first with quick-connect fallback
- Header action now context-aware: `[c] connect` / `[c] disconnect`
- `c` key now toggles connect/disconnect when already connected
- Quick Connect save key changed to `Ctrl+S` (prevents conflict while typing `s`)
- Quick Connect now defaults focus to Name/Label field
- Bookmark display now prefers Name/Label over host/IP
- Queue panel now shows worker state + active/next/failed summaries

### Fixed
- Queue not processing automatically (worker now starts automatically)
- Multiple dependency/import build blockers across workspace crates
- Modal input conflict with save shortcut
- Quick Connect field focus behavior

### Known Limitations
- FTP/FTPS protocol runtime support remains deferred (SFTP is production path)
- Multi-worker parallel transfer scheduling not yet enabled
- Bookmark inline edit form is still mediated through Quick Connect modal
