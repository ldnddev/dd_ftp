mod bookmarks;
mod edit;
mod events;
mod fs_ops;
mod mouse;
mod paths;
mod session;
mod workers;

use std::{future::poll_fn, io, path::Path, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dd_ftp_app::{reduce, Action, AppState, OverwritePolicy};
use dd_ftp_storage::{AppConfig, SiteManager};
use futures_core::Stream;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;
use tokio::time::{interval_at, Instant as TokioInstant, MissedTickBehavior};

use crate::bookmarks::{hydrate_password_from_keyring, run_keyring_health_check};
use crate::events::{accept_key_event, handle_key, LoopControl};
use crate::mouse::handle_mouse;
use crate::paths::local_list;
use crate::session::{connection_info_from_env, handle_io_message, Runtime};
use crate::workers::{
    accept_worker_msg, handle_worker_result, return_pooled_session, spawn_pending_workers,
    WorkerMessage,
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

    match AppConfig::load_or_default() {
        Ok(cfg) => {
            reduce(
                &mut app,
                Action::ApplySettings {
                    editor: cfg.editor_or_empty(),
                    worker_max_concurrency: cfg.worker_max_or_default(),
                    hide_dotfiles: cfg.hide_dotfiles,
                    overwrite: OverwritePolicy::from_config(cfg.overwrite_policy_or_default()),
                    auto_connect: cfg.auto_connect_default,
                },
            );
            if let Some(path) = cfg.last_local_cwd.as_deref() {
                if Path::new(path).is_dir() {
                    app.local_cwd = path.to_string();
                    app.local_entries = local_list(path);
                }
            }
        }
        Err(err) => {
            app.toast = Some(dd_ftp_app::Toast::warning(format!(
                "Could not load settings: {err}"
            )));
        }
    }

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
    runtime.default_overwrite_policy = app.default_overwrite_policy;
    runtime.overwrite_policy = app.default_overwrite_policy;
    if app.auto_connect_default && !app.quick_connect.host.trim().is_empty() {
        crate::session::connect_off_thread(app, &mut runtime, app.quick_connect.clone());
    }
    let mut last_click: Option<(u16, u16, std::time::Instant)> = None;
    let mut drag: Option<dd_ftp_ui::ScrollRegion> = None;
    let mut drag_field: Option<dd_ftp_ui::FieldId> = None;
    let mut events = EventStream::new();
    let mut tick = interval_at(
        TokioInstant::now() + Duration::from_millis(150),
        Duration::from_millis(150),
    );
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut app_layout = dd_ftp_ui::LayoutMap::default();
    let mut needs_draw = true;

    loop {
        spawn_pending_workers(app, &mut runtime, &tx);
        if app.expire_toast() {
            needs_draw = true;
        }
        if runtime.sync_worker_view(app) {
            needs_draw = true;
        }
        if runtime.sync_busy(app) {
            needs_draw = true;
        }

        if needs_draw {
            terminal.draw(|f| dd_ftp_ui::render(f, app, &mut app_layout))?;
            needs_draw = false;
        }

        if runtime.editor_ready {
            runtime.editor_ready = false;
            drop(events);
            run_blocking_editor(terminal, app, &mut runtime).await?;
            events = EventStream::new();
            needs_draw = true;
            continue;
        }

        let need_tick = app.busy || app.toast.is_some();

        tokio::select! {
            biased;
            maybe_ev = poll_fn(|cx| std::pin::Pin::new(&mut events).poll_next(cx)) => {
                match maybe_ev {
                    Some(Ok(ev)) => {
                        let (ctl, draw) = handle_terminal_event(
                            app,
                            &mut runtime,
                            &app_layout,
                            ev,
                            &mut last_click,
                            &mut drag,
                            &mut drag_field,
                        )?;
                        if ctl == LoopControl::Quit {
                            return Ok(());
                        }
                        needs_draw |= draw;
                    }
                    Some(Err(err)) => return Err(err.into()),
                    None => {}
                }
                while event::poll(Duration::from_millis(0))? {
                    let (ctl, draw) = handle_terminal_event(
                        app,
                        &mut runtime,
                        &app_layout,
                        event::read()?,
                        &mut last_click,
                        &mut drag,
                        &mut drag_field,
                    )?;
                    if ctl == LoopControl::Quit {
                        return Ok(());
                    }
                    needs_draw |= draw;
                }
            }
            Some(msg) = rx.recv() => {
                let mut changed = apply_worker_message(app, &mut runtime, msg);
                while let Ok(more) = rx.try_recv() {
                    changed |= apply_worker_message(app, &mut runtime, more);
                }
                if changed {
                    spawn_pending_workers(app, &mut runtime, &tx);
                    needs_draw = true;
                }
            }
            Some(msg) = io_rx.recv() => {
                handle_io_message(app, &mut runtime, msg);
                while let Ok(more) = io_rx.try_recv() {
                    handle_io_message(app, &mut runtime, more);
                }
                needs_draw = true;
            }
            _ = tick.tick(), if need_tick => {
                if app.expire_toast() {
                    needs_draw = true;
                }
                if runtime.sync_busy(app) {
                    needs_draw = true;
                }
                if app.busy {
                    needs_draw = true;
                }
            }
        }
    }
}

fn apply_worker_message(app: &mut AppState, runtime: &mut Runtime, msg: WorkerMessage) -> bool {
    match msg {
        WorkerMessage::Progress {
            generation,
            job_id,
            transferred_bytes,
            size_bytes,
        } => {
            if !accept_worker_msg(runtime.generation, generation) {
                return false;
            }
            reduce(
                app,
                Action::UpdateTransferProgress {
                    job_id,
                    transferred_bytes,
                    size_bytes,
                },
            );
            true
        }
        WorkerMessage::Done {
            generation,
            result,
            session,
        } => {
            if !accept_worker_msg(runtime.generation, generation) {
                return false;
            }
            runtime.worker_active_count = runtime.worker_active_count.saturating_sub(1);
            runtime
                .cancel_flags
                .retain(|f| !std::sync::Arc::ptr_eq(f, &result.cancel_flag));
            runtime.job_cancels.remove(&result.job.id);
            if let Some(session) = session {
                if let Some(info) = app.active_connection.as_ref() {
                    return_pooled_session(runtime, *session, info, app.worker_max_concurrency);
                }
            }
            handle_worker_result(app, runtime, result);
            true
        }
    }
}

fn handle_terminal_event(
    app: &mut AppState,
    runtime: &mut Runtime,
    app_layout: &dd_ftp_ui::LayoutMap,
    ev: Event,
    last_click: &mut Option<(u16, u16, std::time::Instant)>,
    drag: &mut Option<dd_ftp_ui::ScrollRegion>,
    drag_field: &mut Option<dd_ftp_ui::FieldId>,
) -> Result<(LoopControl, bool)> {
    match ev {
        Event::Key(key) => {
            if !accept_key_event(key, app) {
                return Ok((LoopControl::Continue, false));
            }
            let control = handle_key(app, runtime, key, app_layout)?;
            Ok((control, true))
        }
        Event::Mouse(mouse) => {
            let draw = handle_mouse(
                app, runtime, app_layout, mouse, last_click, drag, drag_field,
            );
            Ok((LoopControl::Continue, draw))
        }
        Event::Resize(_, _) => Ok((LoopControl::Continue, true)),
        _ => Ok((LoopControl::Continue, false)),
    }
}

fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read();
    }
    Ok(())
}

fn resume_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read();
    }
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

async fn run_blocking_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    runtime: &mut crate::session::Runtime,
) -> Result<()> {
    let Some(edit) = runtime.edit_session.clone() else {
        return Ok(());
    };
    suspend_tui(terminal)?;
    let path = edit.local_path.clone();
    let before = edit.fingerprint;
    let configured = app.editor.clone();
    let editor_result =
        tokio::task::spawn_blocking(move || crate::edit::run_editor(&path, &configured)).await;
    resume_tui(terminal)?;
    match editor_result {
        Ok(Ok(status)) if status.success() => {
            let after = crate::edit::fingerprint(&edit.local_path);
            let changed = after != before;
            crate::session::finish_edit_after_editor(app, runtime, changed);
        }
        Ok(Ok(status)) => {
            reduce(
                app,
                Action::ShowError(format!(
                    "Editor exited {}; file kept at {}",
                    status.code().unwrap_or(-1),
                    edit.local_path.display()
                )),
            );
        }
        Ok(Err(err)) => {
            reduce(
                app,
                Action::ShowError(format!(
                    "Could not run editor ({err}); file kept at {}",
                    edit.local_path.display()
                )),
            );
        }
        Err(err) => {
            reduce(app, Action::ShowError(format!("Editor task failed: {err}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod redraw_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn key_release_and_focus_skip_redraw_resize_does_not() {
        let mut app = AppState::default();
        let (mut runtime, _rx) = crate::session::test_runtime();
        let layout = dd_ftp_ui::LayoutMap::default();
        let mut last_click = None;
        let mut drag = None;
        let mut drag_field = None;

        let mut release = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        let (ctl, draw) = handle_terminal_event(
            &mut app,
            &mut runtime,
            &layout,
            Event::Key(release),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        )
        .unwrap();
        assert_eq!(ctl, LoopControl::Continue);
        assert!(!draw);

        let (ctl, draw) = handle_terminal_event(
            &mut app,
            &mut runtime,
            &layout,
            Event::FocusGained,
            &mut last_click,
            &mut drag,
            &mut drag_field,
        )
        .unwrap();
        assert_eq!(ctl, LoopControl::Continue);
        assert!(!draw);

        let (ctl, draw) = handle_terminal_event(
            &mut app,
            &mut runtime,
            &layout,
            Event::Resize(120, 40),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        )
        .unwrap();
        assert_eq!(ctl, LoopControl::Continue);
        assert!(draw);
    }
}
