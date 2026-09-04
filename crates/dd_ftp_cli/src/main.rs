mod bookmarks;
mod events;
mod fs_ops;
mod mouse;
mod paths;
mod session;
mod workers;

use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dd_ftp_app::{reduce, Action, AppState};
use dd_ftp_storage::SiteManager;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use crate::bookmarks::{hydrate_password_from_keyring, run_keyring_health_check};
use crate::events::{handle_key, LoopControl};
use crate::mouse::handle_mouse;
use crate::paths::local_list;
use crate::session::{connection_info_from_env, handle_io_message, Runtime};
use crate::workers::{
    accept_worker_msg, handle_worker_result, spawn_pending_workers, WorkerMessage,
};

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState {
        local_entries: local_list("."),
        quick_connect: connection_info_from_env(),
        ..Default::default()
    };
    // Seed the active-field editor from the env-populated connection.
    app.qc_hydrate();

    run_keyring_health_check(&mut app);

    let theme_loaded = dd_ftp_ui::reload_theme();
    reduce(
        &mut app,
        Action::SetStatus(format!("Theme loaded: {}", theme_loaded.source.label())),
    );
    // Theme may override header taglines via `header_quotes`. Init-only write.
    if !theme_loaded.header_quotes.is_empty() {
        app.header_copy = dd_ftp_app::random_header_copy_from(&theme_loaded.header_quotes);
    }
    if let Some(w) = theme_loaded.warning {
        app.toast = Some(dd_ftp_app::Toast::warning(w));
    }

    if let Ok(cfg) = SiteManager::load_or_default() {
        if !cfg.sites.is_empty() {
            reduce(&mut app, Action::SetBookmarks(cfg.sites.clone()));
            let selected_idx = cfg
                .default_site
                .unwrap_or(0)
                .min(cfg.sites.len().saturating_sub(1));
            if let Some(selected) = cfg.sites.get(selected_idx) {
                let selected = hydrate_password_from_keyring(&mut app, selected.clone(), "startup");
                reduce(&mut app, Action::QuickConnectSetFromBookmark(selected));
                app.selected_bookmark = selected_idx;
            }
        }
    }

    let res = run(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WorkerMessage>();
    let (io_tx, mut io_rx) = mpsc::unbounded_channel();
    let mut runtime = Runtime::new(io_tx);
    let mut last_click: Option<(u16, u16, std::time::Instant)> = None;
    let mut drag: Option<dd_ftp_ui::ScrollRegion> = None;
    let mut drag_field: Option<dd_ftp_ui::FieldId> = None;

    loop {
        app.expire_toast();
        let mut app_layout = dd_ftp_ui::LayoutMap::default();
        terminal.draw(|f| dd_ftp_ui::render(f, app, &mut app_layout))?;

        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMessage::Progress {
                    generation,
                    job_id,
                    transferred_bytes,
                    size_bytes,
                } => {
                    if !accept_worker_msg(runtime.generation, generation) {
                        continue;
                    }
                    reduce(
                        app,
                        Action::UpdateTransferProgress {
                            job_id,
                            transferred_bytes,
                            size_bytes,
                        },
                    );
                }
                WorkerMessage::Done { generation, result } => {
                    if !accept_worker_msg(runtime.generation, generation) {
                        continue;
                    }
                    runtime.worker_active_count = runtime.worker_active_count.saturating_sub(1);
                    runtime
                        .cancel_flags
                        .retain(|f| !std::sync::Arc::ptr_eq(f, &result.cancel_flag));
                    handle_worker_result(app, &mut runtime, result);
                }
            }
        }

        while let Ok(msg) = io_rx.try_recv() {
            handle_io_message(app, &mut runtime, msg);
        }

        spawn_pending_workers(app, &mut runtime, &tx);

        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                Event::Key(key) => match handle_key(app, &mut runtime, key).await? {
                    LoopControl::Quit => return Ok(()),
                    LoopControl::Continue => {}
                },
                Event::Mouse(mouse) => {
                    handle_mouse(
                        app,
                        &mut runtime,
                        &app_layout,
                        mouse,
                        &mut last_click,
                        &mut drag,
                        &mut drag_field,
                    );
                }
                _ => {}
            }
        }

        runtime.sync_worker_view(app);
    }
}
