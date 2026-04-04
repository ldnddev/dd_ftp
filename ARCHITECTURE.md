# dd_ftp — Production Architecture

## Overview
A terminal-based FTP client built with Rust + ratatui, inspired by FileZilla but redesigned for TUI workflows.

## Workspace

```text
dd_ftp/
├── Cargo.toml
├── crates/
│   ├── dd_ftp_app
│   ├── dd_ftp_ui
│   ├── dd_ftp_core
│   ├── dd_ftp_protocols
│   ├── dd_ftp_transfer
│   ├── dd_ftp_storage
│   └── dd_ftp_utils
```

## Core Principles

- Protocol abstraction via traits
- Event-driven architecture (Redux-style reducer)
- Async-first (tokio)
- Queue-based transfers
- Separation of concerns (UI vs domain vs IO)

## Build Phases

### Phase 1 (MVP)
- SFTP
- Dual pane
- Upload/download

### Phase 2
- Queue
- Resume
- Site manager

### Phase 3
- FTP/FTPS

### Phase 4
- Parallel transfers
- Bookmarks

### Phase 5 (Deferred)
- Filters
- Directory comparison
- Sync browsing
- Proxy
- Logging expansion
- Remote search

## Next Steps

- Scaffold workspace
- Implement SFTP session
- Build minimal UI loop
- Add queue system
