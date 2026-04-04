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

### Current Phase 1 progress
- ✅ Added runner binary crate (`dd_ftp_cli`) and wired tokio + ratatui event loop
- ✅ Connected keyboard actions to reducer (`Tab`, `j/k`, `h/l`, `r`, `c`, `q`)
- ✅ Added selectable local/remote panes with highlighted cursor row
- ✅ Added local filesystem listing refresh
- ✅ Implemented real SFTP connect + remote `list_dir` using `ssh2`
- ✅ Wired `c` to connect and immediately populate remote pane
- ✅ Wired `r` to refresh both local + remote (when connected)
- ✅ Added transfer queue lifecycle (pending -> active -> completed/failed)
- ✅ Added `u` (queue upload), `d` (queue download)
- ✅ Implemented auto queue processing loop (one transfer per tick)
- ✅ Kept `x` manual process trigger for debugging/forced run
- ✅ Implemented SFTP file upload/download for single file jobs
- ⏳ Persist site manager config to disk
- ⏳ Add async worker loop for continuous parallel transfer processing

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
- `q` quit
- `1` focus Local pane
- `2` focus Remote pane
- `3` focus Queue pane
- `Tab` cycle pane focus
- `j/k` move selection in focused pane (↓/↑ also supported)
- `l` enter selected directory in focused pane
- `h` move to parent directory in focused pane
- `r` refresh local listing (and remote when connected)
- `c` connect via SFTP using env vars
- `u` queue upload (selected local file -> remote cwd)
- `d` queue download (selected remote file -> local cwd)
- `x` force process one queued transfer now (optional)

## Architecture
See [ARCHITECTURE.md](./ARCHITECTURE.md).
