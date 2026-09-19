//! Capture named TUI frames for `docs/index.html`.
//!
//! Run via `./docs/capture.sh` (preferred) or:
//!
//! ```bash
//! cargo run -p dd_ftp_cli --example tutorial_shots --release
//! ```
//!
//! This writes one self-contained HTML file per shot under
//! `docs/images/_frames/`. `docs/capture.sh` then screenshots those files
//! into `docs/images/*.png`.
//!
//! To add a shot: append a `capture(...)` call in `main`, document it in
//! `docs/README.md`, and reference the PNG from `docs/index.html`.

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use dd_ftp_app::{
    reduce, Action, AppState, ChoicePromptKind, FocusPane, OverwritePolicy, OverwritePrompt,
    PendingFile, SelectPolicy,
};
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, Protocol, TransferDirection, TransferJob};
use dd_ftp_ui::layout::LayoutMap;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const COLS: u16 = 124;
const ROWS: u16 = 34;
const HEADER_QUOTE: &str = "Moving bytes so you don't have to.";

fn main() -> Result<()> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("resolve repo root")?;
    let frames_dir = repo.join("docs/images/_frames");
    fs::create_dir_all(&frames_dir).context("create frames dir")?;
    for old in fs::read_dir(&frames_dir)? {
        let old = old?;
        let _ = fs::remove_file(old.path());
    }

    std::env::set_current_dir(&repo).context("chdir repo")?;

    // Isolate HOME so theme lookup never walks the real user tree.
    let home = PathBuf::from("/tmp/dd-ftp-home");
    if home.exists() {
        fs::remove_dir_all(&home)?;
    }
    fs::create_dir_all(home.join(".config/ldnddev"))?;
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::remove_var("VISUAL");
    std::env::remove_var("EDITOR");
    let _ = dd_ftp_ui::reload_theme();

    let mut app = demo_app();
    let mut shots = Vec::new();

    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "01-main",
        "Main window: local pane, disconnected remote, empty queue",
    )?;

    reduce(&mut app, Action::ToggleHelp);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "02-help",
        "F1 help overlay (keys + mouse)",
    )?;
    reduce(&mut app, Action::ToggleHelp);

    reduce(&mut app, Action::ToggleQuickConnect);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "03-quick-connect",
        "Quick Connect form (o)",
    )?;
    reduce(&mut app, Action::ToggleQuickConnect);

    reduce(&mut app, Action::ToggleBookmarks);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "04-bookmarks",
        "Bookmarks modal (m)",
    )?;
    reduce(&mut app, Action::ToggleBookmarks);

    connect_demo(&mut app);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "05-connected",
        "Connected dual-pane listing",
    )?;

    queue_demo(&mut app);
    reduce(&mut app, Action::SetFocus(FocusPane::Queue));
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "06-queue",
        "Transfer queue with active and pending jobs",
    )?;
    reduce(&mut app, Action::SetFocus(FocusPane::Local));

    reduce(&mut app, Action::ToggleFilter);
    for ch in "index".chars() {
        reduce(&mut app, Action::FilterInput(ch));
    }
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "07-filter",
        "Inline filter matching index",
    )?;
    reduce(&mut app, Action::ToggleFilter);

    reduce(&mut app, Action::ToggleCompare);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "08-compare",
        "Directory compare badges [L] [R] [=] [≠]",
    )?;
    reduce(&mut app, Action::ToggleCompare);
    idle_workers(&mut app);

    let loaded = dd_ftp_ui::cached_theme();
    app.theme_editor = Some(ldnddev_theme::ThemeEditor::new(
        dd_ftp_ui::palette_from_theme(&loaded.theme, loaded.header_quotes.clone()),
        dd_ftp_ui::extra_theme_fields(),
    ));
    reduce(&mut app, Action::ToggleThemeDebug);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "09-theme",
        "F2 live theme editor",
    )?;
    app.theme_editor = None;
    reduce(&mut app, Action::ToggleThemeDebug);

    reduce(&mut app, Action::ToggleSettings);
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "10-settings",
        "F3 settings (editor command)",
    )?;
    reduce(&mut app, Action::ToggleSettings);

    reduce(&mut app, Action::ShowCreatePrompt);
    for ch in "notes.txt".chars() {
        reduce(&mut app, Action::PromptInput(ch));
    }
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "11-create",
        "Create file prompt (Ctrl+n / n)",
    )?;
    reduce(&mut app, Action::CancelPrompt);

    app.overwrite = Some(OverwritePrompt {
        current: pending(
            "/home/demo/www/index.html",
            "/var/www/html/index.html",
            TransferDirection::Upload,
            true,
        ),
        remaining: vec![
            pending(
                "/home/demo/www/logo.svg",
                "/var/www/html/logo.svg",
                TransferDirection::Upload,
                false,
            ),
            pending(
                "/home/demo/www/robots.txt",
                "/var/www/html/robots.txt",
                TransferDirection::Upload,
                false,
            ),
        ],
        apply_all: OverwritePolicy::Ask,
    });
    reduce(
        &mut app,
        Action::ShowChoicePrompt(ChoicePromptKind::Overwrite),
    );
    capture(
        &mut app,
        &frames_dir,
        &mut shots,
        "12-overwrite",
        "Overwrite prompt when the destination already exists",
    )?;

    write_manifest(&frames_dir, &shots)?;
    println!(
        "Wrote {} HTML frames to {}",
        shots.len(),
        frames_dir.display()
    );
    for shot in &shots {
        println!("  {}.html  — {}", shot.id, shot.caption);
    }
    Ok(())
}

struct Shot {
    id: String,
    caption: String,
}

fn capture(
    app: &mut AppState,
    frames_dir: &Path,
    shots: &mut Vec<Shot>,
    id: &str,
    caption: &str,
) -> Result<()> {
    let backend = TestBackend::new(COLS, ROWS);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| {
        let mut map = LayoutMap::default();
        dd_ftp_ui::render(f, app, &mut map);
    })?;
    let html = render_html(terminal.backend().buffer(), id, caption);
    fs::write(frames_dir.join(format!("{id}.html")), html)?;
    shots.push(Shot {
        id: id.to_string(),
        caption: caption.to_string(),
    });
    Ok(())
}

fn demo_app() -> AppState {
    let mut app = AppState {
        local_cwd: "/home/demo/www".into(),
        remote_cwd: "/".into(),
        header_copy: HEADER_QUOTE.into(),
        status: "Ready".into(),
        editor: "hx".into(),
        quick_connect: site(
            "Production",
            "demo.example.com",
            22,
            Protocol::Sftp,
            "deploy",
            Some("secret12"),
            Some("/home/demo/.ssh/id_ed25519"),
            "/var/www/html",
        ),
        bookmarks: vec![
            site(
                "Production",
                "demo.example.com",
                22,
                Protocol::Sftp,
                "deploy",
                None,
                Some("/home/demo/.ssh/id_ed25519"),
                "/var/www/html",
            ),
            site(
                "Staging",
                "files.example.com",
                21,
                Protocol::Ftp,
                "ftpuser",
                None,
                None,
                "/public",
            ),
            site(
                "Backup",
                "backup.example.com",
                990,
                Protocol::Ftps,
                "backup",
                None,
                None,
                "/",
            ),
        ],
        ..Default::default()
    };
    reduce(
        &mut app,
        Action::SetLocalEntries {
            entries: local_listing(),
            select: SelectPolicy::Reset,
        },
    );
    select_local(&mut app, "index.html");
    app
}

fn connect_demo(app: &mut AppState) {
    let info = app.bookmarks[0].clone();
    reduce(app, Action::SetConnected(true));
    app.active_connection = Some(info);
    app.remote_cwd = "/var/www/html".into();
    reduce(
        app,
        Action::SetRemoteEntries {
            entries: remote_listing(),
            select: SelectPolicy::Reset,
        },
    );
    select_local(app, "index.html");
    select_remote(app, "index.html");
    reduce(app, Action::SetStatus("Connected".into()));
    app.toast = Some(dd_ftp_app::Toast::info(
        "Connected to Production (demo.example.com)".into(),
    ));
}

fn queue_demo(app: &mut AppState) {
    app.toast = None;
    let jobs = [
        (
            "/home/demo/www/index.html",
            "/var/www/html/index.html",
            TransferDirection::Upload,
            48_000u64,
            31_200u64,
        ),
        (
            "/home/demo/www/assets/hero.webp",
            "/var/www/html/assets/hero.webp",
            TransferDirection::Upload,
            2_400_000,
            0,
        ),
        (
            "/home/demo/www/logo.svg",
            "/var/www/html/logo.svg",
            TransferDirection::Download,
            12_400,
            0,
        ),
    ];
    for (local, remote, dir, size, _done) in jobs {
        let mut job = TransferJob::new(local, remote, dir);
        job.size_bytes = Some(size);
        reduce(app, Action::QueueTransfer(job));
    }
    reduce(app, Action::StartNextTransfer);
    if let Some(id) = app.queue.active.first().map(|j| j.id) {
        reduce(
            app,
            Action::UpdateTransferProgress {
                job_id: id,
                transferred_bytes: 0,
                size_bytes: Some(48_000),
            },
        );
        std::thread::sleep(Duration::from_millis(80));
        reduce(
            app,
            Action::UpdateTransferProgress {
                job_id: id,
                transferred_bytes: 31_200,
                size_bytes: Some(48_000),
            },
        );
    }
    reduce(
        app,
        Action::SetWorkerView {
            active_count: 1,
            running: true,
            cancel_requested: false,
        },
    );
    reduce(app, Action::SetBusy(true));
    reduce(app, Action::SetStatus("Uploading index.html".into()));
}

fn site(
    name: &str,
    host: &str,
    port: u16,
    protocol: Protocol,
    username: &str,
    password: Option<&str>,
    private_key: Option<&str>,
    initial_path: &str,
) -> ConnectionInfo {
    ConnectionInfo {
        name: name.into(),
        host: host.into(),
        port,
        protocol,
        username: username.into(),
        password: password.map(str::to_string),
        private_key: private_key.map(str::to_string),
        initial_path: initial_path.into(),
    }
}

fn select_local(app: &mut AppState, name: &str) {
    let index = app.visible_local().iter().position(|e| e.name == name);
    if let Some(index) = index {
        reduce(
            app,
            Action::SelectIndex {
                pane: FocusPane::Local,
                index,
            },
        );
    }
}

fn select_remote(app: &mut AppState, name: &str) {
    let index = app.visible_remote().iter().position(|e| e.name == name);
    if let Some(index) = index {
        reduce(
            app,
            Action::SelectIndex {
                pane: FocusPane::Remote,
                index,
            },
        );
    }
}

fn idle_workers(app: &mut AppState) {
    app.queue = Default::default();
    reduce(
        app,
        Action::SetWorkerView {
            active_count: 0,
            running: false,
            cancel_requested: false,
        },
    );
    reduce(app, Action::SetBusy(false));
    reduce(app, Action::SetStatus("Connected".into()));
    app.toast = None;
}

fn pending(
    local: &str,
    remote: &str,
    direction: TransferDirection,
    dest_newer: bool,
) -> PendingFile {
    let mut file = PendingFile::new(local, remote, direction);
    file.size_bytes = Some(48_000);
    file.source_modified = Some(dt(2026, 8, 2, 9, 15));
    file.dest_modified = Some(if dest_newer {
        dt(2026, 9, 12, 11, 40)
    } else {
        dt(2026, 7, 1, 8, 0)
    });
    file
}

fn local_listing() -> Vec<FileEntry> {
    vec![
        dir(".", "/home/demo/www"),
        dir("..", "/home/demo"),
        dir("assets", "/home/demo/www/assets"),
        dir("css", "/home/demo/www/css"),
        file(
            "index.html",
            "/home/demo/www/index.html",
            48_210,
            "-rw-r--r--",
        ),
        file("logo.svg", "/home/demo/www/logo.svg", 12_400, "-rw-r--r--"),
        file("robots.txt", "/home/demo/www/robots.txt", 84, "-rw-r--r--"),
        file("README.md", "/home/demo/www/README.md", 1_204, "-rw-r--r--"),
        file("deploy.sh", "/home/demo/www/deploy.sh", 612, "-rwxr-xr-x"),
        file(".gitignore", "/home/demo/www/.gitignore", 96, "-rw-r--r--"),
    ]
}

fn remote_listing() -> Vec<FileEntry> {
    vec![
        dir(".", "/var/www/html"),
        dir("..", "/var/www"),
        dir("assets", "/var/www/html/assets"),
        dir("css", "/var/www/html/css"),
        file(
            "index.html",
            "/var/www/html/index.html",
            51_002,
            "-rw-r--r--",
        ),
        file("logo.svg", "/var/www/html/logo.svg", 12_400, "-rw-r--r--"),
        file(
            "about.html",
            "/var/www/html/about.html",
            3_330,
            "-rw-r--r--",
        ),
        file(
            "favicon.ico",
            "/var/www/html/favicon.ico",
            15_086,
            "-rw-r--r--",
        ),
    ]
}

fn dir(name: &str, path: &str) -> FileEntry {
    FileEntry {
        name: name.into(),
        path: path.into(),
        kind: EntryKind::Directory,
        size: 4096,
        modified: Some(dt(2026, 9, 10, 16, 2)),
        permissions: Some("drwxr-xr-x".into()),
    }
}

fn file(name: &str, path: &str, size: u64, perms: &str) -> FileEntry {
    FileEntry {
        name: name.into(),
        path: path.into(),
        kind: EntryKind::File,
        size,
        modified: Some(dt(2026, 9, 12, 14, 30)),
        permissions: Some(perms.into()),
    }
}

fn dt(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, hh, mm, 0)
        .single()
        .expect("valid date")
}

fn write_manifest(frames_dir: &Path, shots: &[Shot]) -> Result<()> {
    let mut out = String::from("{\n  \"cols\": ");
    out.push_str(&COLS.to_string());
    out.push_str(",\n  \"rows\": ");
    out.push_str(&ROWS.to_string());
    out.push_str(",\n  \"shots\": [\n");
    for (i, shot) in shots.iter().enumerate() {
        out.push_str("    { \"id\": \"");
        out.push_str(&shot.id);
        out.push_str("\", \"caption\": \"");
        out.push_str(&json_escape(&shot.caption));
        out.push_str("\" }");
        if i + 1 != shots.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    fs::write(frames_dir.join("manifest.json"), out)?;
    Ok(())
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_html(buffer: &ratatui::buffer::Buffer, id: &str, caption: &str) -> String {
    let body = render_pre(buffer);
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{id}</title>
<style>
  html, body {{
    margin: 0;
    background: #0b0d10;
    color: #f5f6f7;
  }}
  .shot {{
    display: inline-block;
    background: #0f1114;
    border: 1px solid #2a2d31;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 22px 60px rgba(0, 0, 0, 0.45);
  }}
  .titlebar {{
    height: 34px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    background: #1c1e21;
    color: #9ea3aa;
    font: 12px "JetBrainsMono Nerd Font", "JetBrains Mono", ui-monospace, monospace;
  }}
  .dots {{ display: flex; gap: 6px; }}
  .dot {{ width: 10px; height: 10px; border-radius: 50%; }}
  .term {{
    margin: 0;
    padding: 8px 10px 10px;
    font: 15px/20px "JetBrainsMono Nerd Font", "JetBrains Mono", ui-monospace, monospace;
    white-space: pre;
    background: #0f1114;
  }}
</style>
</head>
<body>
  <div class="shot" data-id="{id}" data-caption="{caption_esc}">
    <div class="titlebar">
      <span class="dots">
        <span class="dot" style="background:#e57373"></span>
        <span class="dot" style="background:#f5c469"></span>
        <span class="dot" style="background:#82e0aa"></span>
      </span>
      <span>dd_ftp — {caption_esc}</span>
    </div>
    <pre class="term">{body}</pre>
  </div>
</body>
</html>
"##,
        caption_esc = html_escape(caption),
    )
}

fn render_pre(buffer: &ratatui::buffer::Buffer) -> String {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let mut out = String::new();
    for y in 0..height {
        let mut x = 0u16;
        while x < width {
            let cell = &buffer[(x, y)];
            let fg = css_color(cell.fg, "#f5f6f7");
            let bg = css_color(cell.bg, "#0f1114");
            let bold = cell.modifier.contains(Modifier::BOLD);
            let dim = cell.modifier.contains(Modifier::DIM);
            let mut run = cell.symbol().to_string();
            let mut run_w = 1u16;
            while x + run_w < width {
                let next = &buffer[(x + run_w, y)];
                if css_color(next.fg, "#f5f6f7") != fg
                    || css_color(next.bg, "#0f1114") != bg
                    || next.modifier.contains(Modifier::BOLD) != bold
                    || next.modifier.contains(Modifier::DIM) != dim
                {
                    break;
                }
                run.push_str(next.symbol());
                run_w += 1;
            }
            let mut style = format!("color:{fg};background:{bg}");
            if bold {
                style.push_str(";font-weight:700");
            }
            if dim {
                style.push_str(";opacity:0.72");
            }
            out.push_str("<span style=\"");
            out.push_str(&style);
            out.push_str("\">");
            out.push_str(&html_escape(&run));
            out.push_str("</span>");
            x += run_w;
        }
        if y + 1 != height {
            out.push('\n');
        }
    }
    out
}

fn css_color(color: Color, fallback: &str) -> String {
    match color {
        Color::Reset => fallback.to_string(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#000000".into(),
        Color::Red => "#e57373".into(),
        Color::Green => "#82e0aa".into(),
        Color::Yellow => "#f5c469".into(),
        Color::Blue => "#64b4f5".into(),
        Color::Magenta => "#ffa087".into(),
        Color::Cyan => "#5dade2".into(),
        Color::Gray => "#9ea3aa".into(),
        Color::DarkGray => "#2a2d31".into(),
        Color::LightRed => "#ef9a9a".into(),
        Color::LightGreen => "#a9dfbf".into(),
        Color::LightYellow => "#f8d486".into(),
        Color::LightBlue => "#90caf9".into(),
        Color::LightMagenta => "#ffc1b3".into(),
        Color::LightCyan => "#85c1e9".into(),
        Color::White => "#f5f6f7".into(),
        Color::Indexed(i) => format!("#{i:02x}{i:02x}{i:02x}"),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
