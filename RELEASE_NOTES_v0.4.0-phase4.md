# dd_ftp v0.4.0-phase4 — Release Notes

## Summary
Phase 4 completes the unified FTP-family backend and closes the FTPS cleanup by delivering explicit TLS operations in production flow.

## Highlights

### Unified FTP/FTPS backend
- `dd_ftp_ftp` is now the single FTP-family runtime implementation path.
- FTP and FTPS both support:
  - connect
  - list
  - upload
  - download

### FTPS explicit TLS completed
- FTPS now uses explicit TLS upgrade (`AUTH TLS`) before login.
- Implemented with `async_ftp` secure flow and compatible TLS dependency stack.

### Dependency compatibility hardening
- `async_ftp` enabled with `secure` feature
- `tokio-rustls` aligned to `0.23` for `async_ftp` secure API compatibility
- `webpki-roots` aligned to `0.22` for trust-anchor compatibility

## What changed from Phase 3
- FTPS moved from "wired but not enabled" to fully implemented data-plane operations.
- Legacy FTP/FTPS exports in `dd_ftp_protocols` were removed from active surface (SFTP export retained).

## Validation checklist
```bash
cd ~/projects/dd_ftp
cargo check -p dd_ftp_ftp
cargo check -p dd_ftp_cli
cargo run -p dd_ftp_cli
```

Smoke test:
1. Create/load FTP bookmark and verify connect/list/upload/download
2. Create/load FTPS bookmark and verify connect/list/upload/download
3. Verify SFTP path still works for connect/list/upload/download
4. Validate queue controls (`C`, `R`, `X`) still function during worker operations

## Remaining follow-ups
- Multi-worker parallel queue processing
- Queue panel visual polish for long paths/progress rows
- Optional inline bookmark edit/delete forms
