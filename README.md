# dd_ftp

Terminal-based FTP client built with Rust + ratatui.

## Status

Scaffold complete (architecture + crate layout).

### Included
- Workspace with 8 crates
- Core domain models + `RemoteSession` trait
- Protocol stubs (SFTP/FTP/FTPS)
- Transfer queue model
- App state + reducer/event scaffolding
- Minimal ratatui renderer (dual pane + queue + status)
- Storage scaffold for site manager (`serde` + `toml`)
- `dd_ftp_cli` runner crate with working terminal loop + key handling

### Current Phase 1 progress (Phase 1.3 complete)
- ✅ Added runner binary crate (`dd_ftp_cli`) and wired tokio + ratatui event loop
- ✅ Connected keyboard actions to reducer (`Tab`, `j/k`, `h/l`, `r`, `c`, `q`)
- ✅ Added selectable local/remote panes with highlighted cursor row
- ✅ Added local filesystem listing refresh
- ✅ Implemented real SFTP connect + remote `list_dir` using `ssh2`
- ✅ Wired `c` to connect and immediately populate remote pane
- ✅ Wired `r` to refresh both local + remote (when connected)
- ✅ Added transfer queue lifecycle (pending -> active -> completed/failed/cancelled)
- ✅ Added `u` (queue upload), `d` (queue download)
- ✅ Implemented async background queue worker (continuous processing)
- ✅ Added queue controls: `X` clear pending, `R` retry last failed, `C` cancel active
- ✅ Added progress events (bytes + optional percent) into active queue job state
- ✅ Added true cancellation token checks in transfer loops
- ✅ Enhanced queue panel with worker state + active/next/failed transfer summaries
- ✅ Added F1 help modal with dim backdrop and Esc close
- ✅ Added bookmark/site manager persistence (`~/.config/dd_ftp/sites.toml`)
- ✅ Passwords moved out of TOML into OS keyring via `keyring` crate
- ✅ Added bookmark controls: `b` cycle saved sites, `B` save current quick-connect as bookmark
- ✅ Added Quick Connect modal (`o`) and Bookmarks modal (`m`)
- ✅ Added bookmark modal actions: edit (`e`), delete (`d`), set default (`D`)
- ✅ FTP connect/list/upload/download routed via unified crate
- ✅ FTPS explicit TLS connect/list/upload/download routed via unified crate
- ✅ Multi-worker parallel transfer processing (default concurrency: 2)
- ✅ Queue pane now shows per-job rows (active/pending/failed) with progress bars

### Run
```bash
cargo run -p dd_ftp_cli
```

### SFTP env vars
Set these before running:

```bash
export DD_FTP_HOST=your.server.com
export DD_FTP_PORT=22
export DD_FTP_USER=your_username
# pick one auth method:
export DD_FTP_PASS='your_password'
# OR
export DD_FTP_KEY="$HOME/.ssh/id_rsa"

# optional
export DD_FTP_PATH=/
```

Controls:
- `F1` toggle controls/help modal (`Esc` closes modal too)
- `F2` toggle theme debug overlay
- `q` quit
- `1` focus Local pane
- `2` focus Remote pane
- `3` focus Queue pane
- `Tab` cycle pane focus
- `j/k` move selection in focused pane (↓/↑ also supported)
- `l` enter selected directory in focused pane
- `h` move to parent directory in focused pane
- `r` refresh local listing (and remote when connected)
- `b` cycle bookmarks
- `B` save current quick-connect as bookmark (password saved to keyring)
- `Ctrl+K` run keyring health check
- `o` open Quick Connect modal (includes Name/Label field)
- `m` open Bookmarks modal
- `c` connect selected bookmark (falls back to quick-connect), or disconnect when connected
- in Bookmarks modal: `e` edit in quick connect, `d` delete, `D` set default

Quick Connect modal:
- `Ctrl+S` save bookmark
- theme lookup order: `./dd_ftp_theme.yml` then `~/.config/ldnddev/dd_ftp_theme.yml`
- header shows active theme source badge: `theme:local`, `theme:global`, or `theme:default`
- all panes/modals render scrollbars; queue/help panes support `j/k` scrolling when focused/open
- queue pane expands when focused (`3`) to show more active/pending/failed rows
- theme key `scroll_bars` controls scrollbar color (default `#2a2d31`)
- first field is **Name/Label** (used in bookmark list and title bar)
- includes **SSH Key** path field for SFTP key auth (example: `~/.ssh/id_rsa`)
- `u` queue upload (selected local file -> remote cwd)
- `d` queue download (selected remote file -> local cwd)
- `x` show worker status hint
- `X` clear pending queue
- `R` retry last failed transfer
- `C` cancel active transfer

## Local validation checklist
Run these locally to verify cleanup before Phase 2:

```bash
cd ~/projects/dd_ftp
cargo check -p dd_ftp_cli
cargo run -p dd_ftp_cli
```

Quick smoke test:
1. open quick connect with `o`, fill fields, `Ctrl+S` to save bookmark
2. open bookmark modal with `m`
3. test `e` (edit/load), `d` (delete), `D` (set default)
4. `c` connect selected bookmark
5. queue upload with `u`
6. confirm auto worker starts and queue updates
7. press `C` during transfer to test cancellation
8. press `R` to retry last failed
9. press `X` to clear pending

## Keyring notes (Linux)
- `archlinux-keyring` is unrelated (package-signing keys for pacman).
- Password persistence uses the desktop keyring backend selected by the `keyring` crate.
- Workspace enables `keyring` with Linux persistent backend feature (`linux-native-sync-persistent`).
- Use `Ctrl+K` in-app to verify keyring backend availability.

## Protocol status
- SFTP: ✅ production path
- FTP: ✅ connect/list/upload/download in unified crate
- FTPS: ✅ explicit TLS connect/list/upload/download in unified crate

## Architecture
See [ARCHITECTURE.md](./ARCHITECTURE.md).
