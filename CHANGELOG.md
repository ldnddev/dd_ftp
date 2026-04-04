# Changelog

All notable changes to this project will be documented in this file.

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
