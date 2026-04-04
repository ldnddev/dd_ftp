# dd_ftp v0.3.0-phase3 — Release Notes

## Summary
Phase 3 moves `dd_ftp` from a scaffolded prototype into a practical SFTP workflow tool with:
- persistent bookmark-driven connection management,
- modal-based quick connect UX,
- background transfer worker execution,
- retry/cancel queue controls,
- and transfer progress visibility.

## Highlights

### Connection UX
- Added **Quick Connect modal** (`o`) with full connection field set.
- Added **Bookmarks modal** (`m`) with connect, edit/load, delete, and set-default actions.
- Added **Name/Label-first bookmarks**, so users do not need to identify hosts by IP.
- Added connect/disconnect toggle via `c`, with context-aware header label.

### Transfer Engine
- Implemented real SFTP upload/download using chunked transfer loops.
- Added async worker pipeline for automatic queue processing.
- Added queue progress updates and state transitions.
- Added controls:
  - `R` retry last failed,
  - `C` cancel active transfer,
  - `X` clear pending queue.

### Persistence
- Bookmarks now persist to:
  - `~/.config/dd_ftp/sites.toml`
- Added default bookmark support (`default_site`).

## Operator Controls (Phase 3)
- `F1` help modal (`Esc` closes)
- `o` quick connect
- `m` bookmarks
- `Ctrl+S` save bookmark (inside quick connect)
- `b` cycle bookmarks
- `c` connect/disconnect
- `u` queue upload
- `d` queue download
- `R` retry failed
- `C` cancel active
- `X` clear pending
- `1/2/3` local/remote/queue focus
- `j/k/h/l` navigation

## Validation Checklist
```bash
cd ~/projects/dd_ftp
cargo check -p dd_ftp_cli
cargo run -p dd_ftp_cli
```

Smoke test:
1. Open quick connect (`o`), set fields, `Ctrl+S` to save
2. Open bookmarks (`m`) and confirm label appears
3. Connect selected bookmark (`c`)
4. Queue upload/download (`u` / `d`) and verify auto worker execution
5. Cancel (`C`), retry (`R`), clear pending (`X`)
6. Disconnect with `c` and confirm state reset

## Notes
- Primary production path in this phase is **SFTP**.
- FTP/FTPS transport implementation remains planned for subsequent phases.
- Multi-worker parallel processing is still pending.
