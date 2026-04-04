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
- ✅ Connected keyboard actions to reducer (`Tab`, `↑/↓`, `r`, `c`, `q`)
- ✅ Added selectable local/remote panes with highlighted cursor row
- ✅ Added local filesystem listing refresh
- ✅ Implemented real SFTP connect + remote `list_dir` using `ssh2`
- ✅ Wired `c` to connect and immediately populate remote pane
- ✅ Wired `r` to refresh both local + remote (when connected)
- ⏳ Wire upload/download actions into transfer queue + side effects
- ⏳ Persist site manager config to disk

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
- `q` quit
- `Tab` switch pane focus
- `↑/↓` move selection in focused pane
- `r` refresh local listing (and remote when connected)
- `c` connect via SFTP using env vars

## Architecture
See [ARCHITECTURE.md](./ARCHITECTURE.md).
