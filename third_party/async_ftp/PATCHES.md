# Patches on top of async_ftp 6.0.0

This directory is [async_ftp 6.0.0](https://github.com/dani-garcia/rust_async_ftp)
(MIT OR Apache-2.0), with two local changes in `src/ftp.rs`:

1. **Data connections use the control connection's address.** `PASV` tells the
   client where to open the data socket. A server can put any address in that
   reply, including a loopback or LAN address the client can reach and the
   server cannot. The port from `PASV` is kept. The IP is always the peer of
   the control connection (the same idea as curl `--ftp-skip-pasv-ip`).
2. **A command must be one line.** `USER`, `PASS`, `CWD`, `RETR`, `STOR`, and
   `DELE` are sent as text. A CR, LF, or NUL inside an argument would start a
   second command. `write_str` rejects that. `PASS` is redacted if `debug_print`
   is enabled.
3. `MDTM` parsing uses `with_ymd_and_hms` so current `chrono` does not warn.

Do not enable the `debug_print` feature in production builds.
