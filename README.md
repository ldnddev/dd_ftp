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
- File operations (local and remote): create file/folder, rename, delete

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
- `F1` toggle help (opening closes theme debug)
- `Esc` close current modal; when compare is on and no modal is open, close compare
- `F2` toggle theme debug (opening closes help)
- `q` quit (confirms if transfers are active)
- `Ctrl+C` cancel in-flight transfers (ignored while help, filter, prompt, or quick-connect is open)
- `C` toggle directory compare

### Navigation
- `1` Local pane
- `2` Remote pane
- `3` Queue pane
- `Tab` cycle focus
- `j/k` move selection
- `l` enter directory
- `h` parent directory
- `r` refresh
- `Enter` enter directory, or queue upload (local file) / download (remote file). Queue pane: no-op.

### Connection / bookmarks
- `o` quick connect
- `m` bookmarks modal
- `b` cycle bookmarks
- `c` connect using the quick-connect form / disconnect if already connected
- `c` (bookmarks) connect the highlighted bookmark (disconnects first if already connected)
- `d` (bookmarks) delete bookmark with confirm
- `e` (bookmarks) edit bookmark
- `D` (bookmarks) set default bookmark
- `B` save current quick-connect as bookmark
- `Ctrl+K` keyring health check

### Transfers
- `u` queue upload (directories recurse)
- `d` queue download (directories recurse)
- `R` retry last failed
- `X` clear pending queue
- `Ctrl+C` cancel in-flight transfers
- `Enter` (on a file) queue upload/download
- Overwrite prompt (default skip): `Enter`/`s` skip, `o` overwrite, `a` overwrite-all, `n` skip-all, `r` rename, `Esc` abort

### Filters / compare
- `/` toggle filter (`Esc` closes and clears the pattern)
- `C` toggle directory compare
- `Esc` close compare when no modal is open

### File operations
- `n` / `Ctrl+n` open new-item prompt (`Tab` toggles between file and folder)
- `e` / `Ctrl+Alt+e` rename selected item
- `Delete` / `Ctrl+Delete` delete selected item with confirm

### Selection / sort
- `Space` toggle multi-select mark on the focused row
- `p` SFTP chmod prompt (remote pane)
- `s` cycle sort key: name → size → date → name
- `S` toggle sort direction
- `.` toggle hide-dotfiles

### Mouse
- Scroll wheel over a list, queue, or help region scrolls it (also works over the scrollbar rail)
- Single-click a file row: focus that pane and select the row
- Double-click a directory row: enter it
- Double-click a file row: queue transfer (same as Enter)
- Click-drag a list / queue / help scrollbar thumb to scroll
- Bookmarks modal: single-click selects a bookmark; double-click loads it into quick connect
- Quick connect: click the Protocol field to cycle SFTP → FTP → FTPS
- Input fields (quick connect + text prompts): click to position the cursor; click-drag to select a range; typing or `Backspace` replaces the selection. Choice prompts have no text field.

### Keyboard field editing (quick-connect fields and prompts)
- `Left` / `Right` move the cursor one character
- `Shift+Left` / `Shift+Right` extend the selection
- `Home` / `End` jump to start / end of the field (`Shift` to extend)
- `Delete` removes the character at the cursor (or the active selection)
- `Ctrl+W` deletes the previous word
- When the Protocol field is focused, `Left` / `Right` cycle the protocol

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

## License
MIT
