# GUI parity

The terminal in [`README.md`](README.md) is the feature contract. `dd_ftp-gui` is a GPUI shell (Stack S, `gpui-pre` 0.3.12, profile A) that follows [`dd-gui-template-master.md`](../dd-gui-template-master.md). This file is the build order. Finish and test one phase before starting the next. Do not merge a phase that changes what `dd_ftp` does.

## Rules

- The window crate does not depend on `dd_ftp_cli` or `dd_ftp_ui`. Both frontends call `dd_ftp_app`, `dd_ftp_core`, `dd_ftp_storage`, and, when a phase needs the network, `dd_ftp_protocols`, `dd_ftp_ftp`, and `dd_ftp_transfer`.
- A phase may move a pure function into a shared crate only when the terminal still calls it and the existing terminal tests pass unchanged.
- Do not edit `crates/dd_ftp_cli` or `crates/dd_ftp_ui` to make the window compile.
- `cargo build` and `./install.sh --tui` stay on the default members and do not compile GPUI.
- Headless tests use `visible_text` / `lines`. A phase that changes layout is also exercised in a real window before it is called done.
- Passwords never appear in `visible_text`, toasts, or `sites.toml`.

## What the window does today

Shell from 1.8.4. Local browser and session from 1.9.0. Bookmarks through the Help key sheet shipped in 1.10.0:

- 1280×800 window, app id `dev.ldnddev.dd_ftp`, title `dd_ftp`.
- Settings: Themes, Window, Session, Help. F1 and F2 open Themes and the second press reverts the palette. F3 opens Session. Esc steps back from the color editor, then closes.
- Theme load order matches the terminal, including Omarchy follow and local/global save.
- Session fields write `dd_ftp.toml`. Window density, Vim keys, and reduced motion write `gui.json` only.
- Local pane (phase 1): `.` and `..`, sort, filter, hide-dotfiles, marks, page keys, refresh, parent, and a path bar.
- Remote pane (phase 2): list, enter, parent, and refresh on a worker thread. `SftpSession` or `UnifiedFtpSession` lists `initial_path`. A refused SFTP host key leaves the pane disconnected. An accepted key is appended to `known_hosts`. Plain FTP status says unencrypted.
- Quick connect fields match the terminal: name, host, port, username, password, private key, protocol, path. `DD_FTP_*` fills them and removes `DD_FTP_PASS`.
- Bookmarks (phase 3): load and save `sites.toml` with no password. The live vault is `SecretStore`. Enter and `e` fill the form with a masked password. `c` on a bookmark connects. `d` confirms delete. Shift+D sets the default. `B` saves the form. Auto-connect uses the Session flag and `default_site`. Ctrl+P opens the key picker. Ctrl+K reports keyring health.
- Queue (phase 4): `u`, `d`, and Enter on a file transfer the marked set or the focused row. `M` moves, then deletes the source after success. Directories are walked with the terminal symlink rules. The queue pane shows pending, active, and failed jobs. Retry, cancel, clear pending, and Ctrl+C match the terminal. Workers default to 2 and clamp to 1–8. The overwrite prompt matches the terminal, including the newer-file keys.
- Remote files (phase 5): `n` creates a file or folder, `e` renames, Delete confirms delete, and `p` chmods on SFTP. `E` edits through a private cache and uploads when the bytes change. A name that leaves the pane root is rejected.
- Compare and mouse (phase 6): Shift+C shows `[L]`, `[R]`, `[=]`, and `[≠]`. Click, shift-click, double-click, path segments, the wheel, and the scrollbar match the terminal. Help lists every `KEYMAP` row as `action · gui · tui`. Toasts cover save, bookmark changes, transfer end, disconnect, and rejected actions.
- Ctrl+Q confirms while a transfer is active, then cancels.

## Phases

Each phase lists the README sections it must match and the tests that close it. Regression for every phase:

```bash
cargo test -p dd_ftp_app -p dd_ftp_ui -p dd_ftp_cli -p dd_ftp_core -p dd_ftp_storage -p dd_ftp_transfer
cargo test -p dd_ftp_gui --bin dd_ftp-gui
```

The first command must stay green with no diff under `dd_ftp_cli` or `dd_ftp_ui`.

### Phase 1 — Local browser

**Done.** Headless coverage is `local_browser_filters_sorts_marks_and_walks_the_path` plus the listing unit tests. A live window was not driven.

Match the local half of Navigation, Files (marks, sort, filter, hide-dotfiles), and the local path bar. No sockets.

- Parent, refresh, page keys, Home/End.
- `/` opens the filter. Esc clears it. Enter keeps it.
- `s` / `S` cycle name, size, and date, and the direction. `.` and `..` stay first. Name sort keeps directories above files. Size and date sort by that field.
- `.` hides dotfiles. Space, Shift+move, and Ctrl+A mark. Esc clears marks when no sheet is open.
- The dump shows the mark, the sort key, and the filter pattern.
- Connect was a toast in this phase. Phase 2 replaces it with a session.

Done when a temp directory test covers filter, sort, marks, parent, and hide-dotfiles, and the settings tests still pass.

### Phase 2 — Session

**Done.** Headless coverage is `session_connects_lists_changes_directory_and_disconnects`, `refused_host_key_leaves_the_pane_disconnected`, `plain_ftp_status_says_unencrypted_and_ftps_does_not`, and `launch_env_fills_the_form_and_drops_the_password`. A live window was not driven. The live path calls `SftpSession::connect_with_host_key_handler` (append on accept) and `UnifiedFtpSession` for FTP and FTPS. Tests use a fake tree behind those commands.

Match Protocols and the connect half of Connection and bookmarks. Still no transfers.

- Quick connect fields match the terminal: name, host, port, username, password, private key, protocol, path.
- `c` connects with `SftpSession` or `UnifiedFtpSession` on a worker thread and lists `initial_path`. Disconnect closes it.
- SFTP host-key prompt (append to `known_hosts` on accept). Plain FTP status says unencrypted. FTPS is the explicit TLS upgrade.
- Remote pane: list, enter, parent, refresh. Stale list results are ignored.
- Busy header uses the same four-frame mark and a named reason.
- `DD_FTP_*` fills the form the way `connection_info_from_env` does, including removing `DD_FTP_PASS`.
- Tests use a local server or a fake session behind the same commands. The password never shows up in the dump.

Done when a headless test connects, lists, changes directory, and disconnects, and a refused host key leaves the pane disconnected.

### Phase 3 — Bookmarks and secrets

**Done.** Headless coverage is `bookmark_round_trip_keeps_the_password_out_of_sites_toml`, `bookmark_sets_the_default_connects_and_edits`, `key_picker_selects_a_private_key_and_ctrl_k_reports_the_vault`, and `auto_connect_uses_the_default_site`, plus the vault, bookmark, and key-picker unit tests. A live window was not driven. The live path calls `SecretStore` and `SiteManager::save_path` (production uses `SiteManager::default_config_path`). Tests use a memory vault and a temp `sites.toml`.

Match the rest of Connection and bookmarks.

- Load and save `sites.toml` with no password field. Keyring save, load, and delete use `SecretStore`.
- Bookmark list: connect, edit, delete with confirm, set default.
- `B` saves the form. Auto-connect uses the Session flag and `default_site`.
- SSH key picker. Ctrl+K reports keyring health.
- A test writes a temp `sites.toml`, round-trips a site, and asserts the password string is absent.

### Phase 4 — Queue and transfers

**Done.** Headless coverage is `upload_and_download_round_trip_keeps_the_bytes`, `overwrite_skip_leaves_the_remote_bytes`, `ctrl_c_cancels_the_active_transfer_and_one_worker_stays_capped`, and `move_uploads_then_deletes_the_local_file`, plus `spawn_gate_matches_the_worker_cap`, `progress_emits_the_first_sample_then_waits`, `conflict_policy_table`, `identity_ignores_password_and_label`, and `folder_upload_skips_a_symlink_that_leaves_the_tree`. A live window was not driven. Tests use a fake tree and a memory vault.

Match Transfers.

- Upload, download, and move (transfer, then delete the source) for the marked set or the focused row.
- Recursive directories. The symlink rules in the README.
- Queue pane: pending, active, failed. Retry one, retry all, cancel one, clear pending, Ctrl+C.
- Worker cap from Session (1–8, default 2), session reuse, 100ms progress, speed and ETA.
- Overwrite prompt, including the newer-file keys.
- Quit confirms while a job is active, then cancels.
- Tests drive a local server or an in-process pipe and assert byte equality, cancel, and skip.

### Phase 5 — Remote file operations

**Done.** Headless coverage is `remote_create_rename_chmod_and_delete_round_trip`, `path_escape_rejects_a_remote_name`, `ftp_chmod_is_refused`, `remote_edit_uploads_when_the_file_changes`, `remote_edit_skips_upload_when_unchanged`, `remote_edit_asks_before_a_binary_file`, `remote_edit_asks_before_a_large_file`, and `remote_edit_asks_when_the_remote_copy_is_newer`, plus `editor_order_is_visual_then_editor_then_session_then_vi`, `cache_path_drops_dotdot_and_separators`, `binary_sniff_sees_a_nul_and_delete_summary_quotes_one_name`, and `edit_cache_file_is_owner_readable_only`. A live window was not driven. Tests use a fake tree, a memory vault, and a temp edit cache.

Match Files, except compare (phase 6).

- Create file, create folder, rename, delete with confirm, on the remote pane through `RemoteSession`.
- `p` chmod on SFTP only. Octal parse stays `parse_octal_mode`.
- `E` remote edit: private cache, `$VISUAL` / `$EDITOR` / Session editor / `vi`, upload on change, prompts for large, binary, and remote conflict.
- Path-escape rejects a name that leaves the pane root.

### Phase 6 — Compare, mouse, and the key sheet

**Done.** Headless coverage is `compare_badges_escape_and_a_letter_in_a_field`, `pointer_selects_ranges_paths_wheels_and_fields`, `queue_wheel_moves_three_rows`, `help_lists_every_keymap_row`, and `rejected_actions_and_disconnect_toast`, plus the compare unit tests in `dd_ftp_app`. A live window was not driven. Compare and `KEYMAP` live in `dd_ftp_app`. The terminal still calls them through `dd_ftp_ui` re-exports, and terminal help still prints `{keys}  {action}`.

Match directory compare, the mouse rows, and toasts.

- `C` shows `[L]` `[R]` `[=]` `[≠]` from `dd_ftp_ui::compare` moved to a crate the window can use, or from the same function if it already lives in `dd_ftp_app`. Prefer moving it into `dd_ftp_core` or `dd_ftp_app` only with the terminal still calling it.
- Click row, shift-click range, double-click, path segments, wheel, scrollbar.
- Help lists every `KEYMAP` row as `action · gui · tui`. GUI chords may be buttons plus the same letters. A letter does nothing while a text field is focused.
- Toasts for save, bookmark change, transfer end, disconnect, and rejected actions.

### Phase 7 — Install and docs

**Done.** `./install.sh --self-test` passed. `--gui` and `--both` build `dd_ftp-gui` with `cargo build --release -p dd_ftp_gui`. `--tui` builds `dd_ftp_cli` only. `dd_ftp_gui` stays off `default-members`, and `gpui-pre` is not in the `dd_ftp_cli` dependency tree. The README Window section describes the desktop client. Tutorial F1 captions say Settings. `keymap_keys_appear_in_tutorial_controls` passed. A live window was not driven. Tutorial PNGs were not recaptured.

- `./install.sh --gui` and `--both` still install the binary that passes phases 1–6.
- README “Window” section describes the finished client, not the shell.
- Tutorial key captions that still say F1 toggles help are updated to Settings. `KEYMAP` strings must remain in `docs/index.html` between `id="keys"` and `id="uninstall"`.

## Out of scope

- A second document model. Listings, marks, the queue, and prompts stay on `AppState` and `Action`.
- Editing the terminal key bindings to match the window.
- Publishing a new GitHub release as part of a phase. Version bumps stay a separate release step.
