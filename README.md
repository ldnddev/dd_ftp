# dd_ftp

Terminal-based FTP/SFTP/FTPS client built with Rust + ratatui.

## Install

### Quick install (recommended)
```bash
./install.sh
```

This will:
- build `dd_ftp_cli` in release mode
- install binary as `dd_ftp` to `~/.local/bin` (default)

Optional overrides:
```bash
INSTALL_DIR=/usr/local/bin ./install.sh
BIN_NAME=dd_ftp_cli ./install.sh
```

### Manual build/run
```bash
cargo run -p dd_ftp_cli
```

## Status

Current implementation includes:
- SFTP + FTP + FTPS connect/list/upload/download
- Dual-pane browser (local/remote) + queue panel
- Parallel transfer workers + cancellation/retry/clear
- Quick Connect + Bookmarks + keyring-backed credential storage
- Theme system (`dd_ftp_theme.yml`) + F2 theme debug
- File-type coloring and directory prefixing
- Directory compare overlay
- File operations (local): create file/folder, rename, delete

## Environment variables (optional)

```bash
export DD_FTP_HOST=your.server.com
export DD_FTP_PORT=22
export DD_FTP_USER=your_username
export DD_FTP_PASS='your_password'   # or DD_FTP_KEY
export DD_FTP_KEY="$HOME/.ssh/id_rsa"
export DD_FTP_PATH=/
```

## Controls

### Global
- `F1` help
- `F2` theme debug
- `q` quit

### Navigation
- `1` Local pane
- `2` Remote pane
- `3` Queue pane
- `Tab` cycle focus
- `j/k` move selection
- `l` enter directory
- `h` parent directory
- `r` refresh

### Connection / bookmarks
- `o` quick connect
- `m` bookmarks modal
- `b` cycle bookmarks
- `c` connect/disconnect
- `B` save current quick-connect as bookmark
- `Ctrl+K` keyring health check

### Transfers
- `u` queue upload
- `d` queue download
- `R` retry last failed
- `X` clear pending queue
- `C` cancel active transfer

### Filters / compare
- `/` toggle filter
- `C` toggle directory compare

### File operations
- `Ctrl+n` create file
- `Ctrl+Shift+n` create folder
- `Ctrl+Alt+e` rename selected item
- `Ctrl+Delete` delete selected item

## Theme

Theme lookup order:
1. `./dd_ftp_theme.yml`
2. `~/.config/ldnddev/dd_ftp_theme.yml`

Notable tokens:
- `scrollbar`
- `scrollbar_hover`
- `cursor`
- `folders`
- `files`
- `links`

## Protocol status
- SFTP: ✅
- FTP: ✅
- FTPS: ✅

## Architecture
See [ARCHITECTURE.md](./ARCHITECTURE.md).

## License
MIT
