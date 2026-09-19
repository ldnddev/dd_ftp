# Docs

The illustrated tutorial is a GitHub Pages site:

**https://ldnddev.github.io/dd_ftp/**

GitHub does not execute HTML in the file browser, so do not send users to `docs/index.html` on github.com. The Pages workflow publishes `index.html` plus the PNGs to that URL.

`.github/workflows/pages.yml` deploys on pushes to `master` that touch `docs/index.html` or `docs/images/`. It copies only the tutorial HTML and PNGs (not `capture.sh` or this file). `docs/.nojekyll` keeps GitHub from running Jekyll over the static page.

Local preview:

```bash
# from the repo root
xdg-open docs/index.html
```

## Updating tutorial screenshots

PNGs under `docs/images/` are generated from the live TUI. Do not edit them in an image editor — change the capture scene and regenerate.

```bash
./docs/capture.sh
```

### What the script does

1. `cargo run -p dd_ftp_cli --example tutorial_shots --release`  
   Builds a fixture `AppState` (demo listings, bookmarks, jobs) with `$HOME` pointed at `/tmp/dd-ftp-home` so private paths never appear. Draws each scene to a ratatui `TestBackend` and writes HTML frames to `docs/images/_frames/` (gitignored).
2. `node docs/rasterize.mjs`  
   Opens each frame in Chromium via Playwright and screenshots the `.shot` card to `docs/images/<id>.png` at 2×.
3. Greps `docs/index.html` for `images/*.png` and exits non-zero if a referenced file is missing.

Commit the new PNGs with the tutorial copy change.

### Requirements

- Rust toolchain (same as the crate)
- Node, with the `playwright` package resolvable
- Chromium at `/usr/bin/chromium` (or set `TUTORIAL_CHROMIUM`)

On this machine Playwright is typically:

```text
~/.local/share/mise/installs/npm-playwright/latest/node_modules
```

Override with `PLAYWRIGHT_NODE_MODULES` if yours lives elsewhere. `docs/capture.sh` also tries `require.resolve("playwright")`.

### Shot catalog

| File | Scene in `crates/dd_ftp_cli/examples/tutorial_shots.rs` |
|------|---------------------------------------------------------|
| `01-main.png` | Disconnected dual-pane, empty remote, idle queue |
| `02-help.png` | `F1` help overlay |
| `03-quick-connect.png` | Quick Connect form (`o`) |
| `04-bookmarks.png` | Bookmarks modal (`m`) |
| `05-connected.png` | Connected local + remote listings |
| `06-queue.png` | Active upload plus pending jobs, queue focused |
| `07-filter.png` | `/` then `index` |
| `08-compare.png` | `C` directory compare badges |
| `09-theme.png` | `F2` live theme editor |
| `10-settings.png` | `F3` settings |
| `11-create.png` | Create file prompt with `notes.txt` |
| `12-overwrite.png` | Overwrite prompt, dest newer |

The header quote is pinned to `Moving bytes so you don't have to.` so screenshots do not flicker between taglines.

### Add or change a shot

1. Edit `crates/dd_ftp_cli/examples/tutorial_shots.rs`: after the app is in the state you want, call `capture(&mut app, &frames_dir, &mut shots, "13-name", "caption")`.
2. Add a row to the table above.
3. Use the PNG from `docs/index.html` (`<img src="images/13-name.png" alt="...">`). Give `alt` a description of what the reader should learn from the frame, not “screenshot”.
4. Run `./docs/capture.sh`.
5. Open `docs/index.html` and confirm the new figure.

To change the fixture (listings, bookmarks, jobs), edit `demo_app` / `local_listing` / `remote_listing` in the example. Keep paths under `/home/demo` and `/var/www` so columns stay readable.

### Do not

- Point `$HOME` at a real user directory while capturing.
- Check in `docs/images/_frames/` — those HTML files are intermediates.
- Hand-patch a PNG to hide a UI bug; fix the TUI or the scene instead.
