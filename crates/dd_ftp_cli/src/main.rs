use std::{
    io,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use tokio::sync::mpsc;

use anyhow::{bail, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dd_ftp_app::{
    reduce, Action, AppState, ChoicePromptKind, FocusPane, HostKeyView, PromptKind,
    QuickConnectField, SelectPolicy, TextPromptKind,
};
use dd_ftp_core::{
    ConnectionInfo, FileEntry, Protocol, RemoteSession, TransferDirection, TransferJob,
};
use dd_ftp_ftp::{FtpVariant, UnifiedFtpSession};
use dd_ftp_protocols::SftpSession;
use dd_ftp_storage::{SecretStore, SiteManager};
use dd_ftp_ui::{hit_test, ControlId, FieldId, Pane, Region, ScrollRegion};
use ratatui::{backend::CrosstermBackend, Terminal};
use uuid::Uuid;

const SCROLL_STEP: usize = 3;

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
    // Theme may override header taglines via `header_quotes`.
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

    let mut session = SftpSession::default();

    let res = run(&mut terminal, &mut app, &mut session).await;

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
    session: &mut SftpSession,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WorkerMessage>();
    let (io_tx, mut io_rx) = mpsc::unbounded_channel::<IoMessage>();
    let mut io = IoState {
        generation: 0,
        in_flight: None,
        io_tx,
        ftp_park: Arc::new(Mutex::new(None)),
        list_select: SelectPolicy::PreserveName,
        list_ok_status: None,
        list_err_prefix: "Remote list failed".to_string(),
        fs_remote: false,
        fs_ok_status: String::new(),
    };
    let mut cancel_flags: Vec<Arc<AtomicBool>> = Vec::new();
    let mut last_click: Option<(u16, u16, Instant)> = None;
    let mut drag: Option<dd_ftp_ui::ScrollRegion> = None;
    let mut drag_field: Option<dd_ftp_ui::FieldId> = None;

    loop {
        app.expire_toast();
        let mut app_layout = dd_ftp_ui::LayoutMap::default();
        terminal.draw(|f| dd_ftp_ui::render(f, app, &mut app_layout))?;

        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMessage::Progress {
                    job_id,
                    transferred_bytes,
                    size_bytes,
                } => {
                    reduce(
                        app,
                        Action::UpdateTransferProgress {
                            job_id,
                            transferred_bytes,
                            size_bytes,
                        },
                    );
                }
                WorkerMessage::Done(result) => {
                    app.worker_active_count = app.worker_active_count.saturating_sub(1);
                    app.worker_running = app.worker_active_count > 0;
                    cancel_flags.retain(|f| !Arc::ptr_eq(f, &result.cancel_flag));
                    handle_worker_result(app, session, &mut io, result);
                }
            }
        }

        while let Ok(msg) = io_rx.try_recv() {
            handle_io_message(app, session, &mut io, msg);
        }

        // Start background workers for queued transfers up to max concurrency.
        while app.connected
            && app.worker_active_count < app.worker_max_concurrency
            && !app.queue.pending.is_empty()
        {
            let Some(job) = app.queue.start_next() else {
                break;
            };

            app.worker_active_count += 1;
            app.worker_running = true;
            app.worker_cancel_requested = false;
            reduce(
                app,
                Action::SetStatus(format!(
                    "Processing {:?}: {}",
                    job.direction, job.remote_path
                )),
            );

            let mut info = app
                .active_connection
                .clone()
                .unwrap_or_else(connection_info_from_env);

            if info.password.is_none() {
                if let Ok(Some(secret)) =
                    SecretStore::load_password(&info.name, &info.username, &info.host, info.port)
                {
                    info.password = Some(secret);
                }
            }

            let tx_clone = tx.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            cancel_flags.push(cancel.clone());

            tokio::spawn(async move {
                let mut worker_session = SftpSession::default();
                let protocol = info.protocol.clone();

                let outcome = match protocol {
                    Protocol::Sftp => {
                        let connect_result = worker_session.connect(info.clone()).await;
                        match connect_result {
                            Ok(_) => {
                                let progress_tx = {
                                    let tx_progress = tx_clone.clone();
                                    move |job_id: Uuid, transferred, size| {
                                        let _ = tx_progress.send(WorkerMessage::Progress {
                                            job_id,
                                            transferred_bytes: transferred,
                                            size_bytes: size,
                                        });
                                    }
                                };

                                match job.direction {
                                    TransferDirection::Upload => {
                                        worker_session
                                            .upload_with_progress(&job, cancel.clone(), progress_tx)
                                            .await
                                    }
                                    TransferDirection::Download => {
                                        worker_session
                                            .download_with_progress(
                                                &job,
                                                cancel.clone(),
                                                progress_tx,
                                            )
                                            .await
                                    }
                                }
                            }
                            Err(err) => Err(err),
                        }
                    }
                    Protocol::Ftp | Protocol::Ftps => {
                        let mut unified = UnifiedFtpSession::new();
                        let variant = match protocol {
                            Protocol::Ftp => FtpVariant::Ftp,
                            Protocol::Ftps => FtpVariant::Ftps,
                            Protocol::Sftp => {
                                let _ = tx_clone.send(WorkerMessage::Done(WorkerResult {
                                    job,
                                    outcome: Err(anyhow::anyhow!("Unexpected SFTP in FTP worker")),
                                    was_cancelled: false,
                                    cancel_flag: cancel,
                                }));
                                return;
                            }
                        };

                        match unified.connect(variant, info.clone()).await {
                            Ok(_) => {
                                let result = match job.direction {
                                    TransferDirection::Upload => {
                                        unified.upload(variant, &job).await
                                    }
                                    TransferDirection::Download => {
                                        unified.download(variant, &job).await
                                    }
                                };
                                unified.disconnect().await.ok();
                                result
                            }
                            Err(err) => Err(err),
                        }
                    }
                };

                let _ = tx_clone.send(WorkerMessage::Done(WorkerResult {
                    job,
                    outcome,
                    was_cancelled: cancel.load(Ordering::Relaxed),
                    cancel_flag: cancel,
                }));
            });
        }

        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        if app.show_help
                            || app.show_filter
                            || app.show_prompt
                            || app.show_quick_connect
                        {
                            continue;
                        }
                        app.worker_cancel_requested = true;
                        for flag in &cancel_flags {
                            flag.store(true, Ordering::Relaxed);
                        }
                        reduce(
                            app,
                            Action::SetStatus("Cancel requested for active transfers".to_string()),
                        );
                        continue;
                    }

                    if !app.any_modal_open()
                        && key.code == KeyCode::Char('k')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        run_keyring_health_check(app);
                        continue;
                    }

                    if key.code == KeyCode::F(1) && (!app.any_modal_open() || app.show_help) {
                        reduce(app, Action::ToggleHelp);
                        continue;
                    }

                    if key.code == KeyCode::F(2) && (!app.any_modal_open() || app.show_theme_debug)
                    {
                        if !app.show_theme_debug {
                            let _ = dd_ftp_ui::reload_theme();
                        }
                        reduce(app, Action::ToggleThemeDebug);
                        continue;
                    }

                    if !app.any_modal_open() && key.code == KeyCode::Char('/') {
                        reduce(app, Action::ToggleFilter);
                        continue;
                    }

                    if app.show_help {
                        match key.code {
                            KeyCode::Esc => reduce(app, Action::ToggleHelp),
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.help_scroll = app.help_scroll.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.help_scroll = app.help_scroll.saturating_add(1);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if app.show_filter {
                        match key.code {
                            KeyCode::Esc => reduce(app, Action::ToggleFilter),
                            KeyCode::Backspace => reduce(app, Action::FilterBackspace),
                            KeyCode::Char(ch) => reduce(app, Action::FilterInput(ch)),
                            _ => {}
                        }
                        continue;
                    }

                    if !app.any_modal_open() && key.code == KeyCode::Char('C') {
                        reduce(app, Action::ToggleCompare);
                        continue;
                    }

                    if app.is_choice_prompt() {
                        let Some(PromptKind::Choice(kind)) = app.prompt_kind else {
                            continue;
                        };
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                reject_host_key(&mut io);
                                reduce(app, Action::CancelPrompt);
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y') => match kind {
                                ChoicePromptKind::ConfirmQuit => return Ok(()),
                                ChoicePromptKind::ConfirmDelete => {
                                    let target = app.prompt_target.clone();
                                    reduce(app, Action::ConfirmPrompt);
                                    if let Some(t) = target {
                                        delete_item(app, &mut io, &t);
                                    }
                                }
                                ChoicePromptKind::ConfirmBookmarkDelete => {
                                    let name = app.prompt_target.clone();
                                    reduce(app, Action::ConfirmPrompt);
                                    if let Some(name) = name {
                                        delete_bookmark_named(app, &name);
                                    }
                                }
                                ChoicePromptKind::HostKey => {
                                    accept_host_key(&mut io);
                                    reduce(app, Action::ConfirmPrompt);
                                }
                            },
                            KeyCode::Char('q') => match kind {
                                ChoicePromptKind::ConfirmQuit => return Ok(()),
                                ChoicePromptKind::HostKey => {
                                    reject_host_key(&mut io);
                                    reduce(app, Action::CancelPrompt);
                                    if request_quit(app) {
                                        return Ok(());
                                    }
                                }
                                _ => {
                                    reduce(app, Action::CancelPrompt);
                                    if request_quit(app) {
                                        return Ok(());
                                    }
                                }
                            },
                            _ => {}
                        }
                        continue;
                    }

                    if app.is_text_prompt() {
                        match key.code {
                            KeyCode::Esc => reduce(app, Action::CancelPrompt),
                            KeyCode::Tab => {
                                app.prompt_kind = match app.prompt_kind {
                                    Some(PromptKind::Text(TextPromptKind::CreateFile)) => {
                                        Some(PromptKind::Text(TextPromptKind::CreateFolder))
                                    }
                                    Some(PromptKind::Text(TextPromptKind::CreateFolder)) => {
                                        Some(PromptKind::Text(TextPromptKind::CreateFile))
                                    }
                                    other => other,
                                };
                            }
                            KeyCode::Enter => {
                                if let Some(PromptKind::Text(kind)) = app.prompt_kind {
                                    match kind {
                                        TextPromptKind::CreateFile => {
                                            let name = app.prompt_value.value.clone();
                                            reduce(app, Action::ConfirmPrompt);
                                            create_file(app, &mut io, &name);
                                        }
                                        TextPromptKind::CreateFolder => {
                                            let name = app.prompt_value.value.clone();
                                            reduce(app, Action::ConfirmPrompt);
                                            create_folder(app, &mut io, &name);
                                        }
                                        TextPromptKind::Rename => {
                                            let new_name = app.prompt_value.value.clone();
                                            let target = app.prompt_target.clone();
                                            reduce(app, Action::ConfirmPrompt);
                                            if let Some(t) = target {
                                                rename_item(app, &mut io, &t, &new_name);
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Left => {
                                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                app.prompt_value.move_cursor(-1, shift);
                            }
                            KeyCode::Right => {
                                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                app.prompt_value.move_cursor(1, shift);
                            }
                            KeyCode::Home => {
                                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                app.prompt_value.move_home(shift);
                            }
                            KeyCode::End => {
                                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                app.prompt_value.move_end(shift);
                            }
                            KeyCode::Delete => {
                                app.prompt_value.delete();
                            }
                            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.prompt_value.delete_word_left();
                            }
                            KeyCode::Backspace => reduce(app, Action::PromptBackspace),
                            KeyCode::Char(ch) => reduce(app, Action::PromptInput(ch)),
                            _ => {}
                        }
                        continue;
                    }

                    if !app.any_modal_open()
                        && key.code == KeyCode::Char('n')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        reduce(app, Action::ShowCreatePrompt);
                        continue;
                    }

                    if !app.any_modal_open()
                        && key.code == KeyCode::Char('e')
                        && key
                            .modifiers
                            .contains(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    {
                        reduce(app, Action::ShowRenamePrompt);
                        if let Some(entry) = get_selected_entry(app) {
                            app.prompt_target = Some(entry.path.clone());
                            app.prompt_value = dd_ftp_app::TextField::from_str(&entry.name);
                        }
                        continue;
                    }

                    if !app.any_modal_open()
                        && key.code == KeyCode::Delete
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        open_delete_prompt(app);
                        continue;
                    }

                    if app.show_quick_connect {
                        match key.code {
                            KeyCode::Esc => reduce(app, Action::ToggleQuickConnect),
                            KeyCode::Tab => reduce(app, Action::QuickConnectNextField),
                            KeyCode::BackTab => reduce(app, Action::QuickConnectPrevField),
                            KeyCode::Left => {
                                if app.quick_connect_field == QuickConnectField::Protocol {
                                    reduce(app, Action::QuickConnectSetProtocolPrev);
                                } else {
                                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                    reduce(app, Action::QuickConnectMoveCursor { dir: -1, shift });
                                }
                            }
                            KeyCode::Right => {
                                if app.quick_connect_field == QuickConnectField::Protocol {
                                    reduce(app, Action::QuickConnectSetProtocolNext);
                                } else {
                                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                    reduce(app, Action::QuickConnectMoveCursor { dir: 1, shift });
                                }
                            }
                            KeyCode::Home => {
                                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                app.qc_field.move_home(shift);
                            }
                            KeyCode::End => {
                                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                                app.qc_field.move_end(shift);
                            }
                            KeyCode::Delete => {
                                app.qc_field.delete();
                                app.qc_flush();
                            }
                            KeyCode::Backspace => reduce(app, Action::QuickConnectBackspace),
                            KeyCode::Enter => {
                                let mut info = app.quick_connect.clone();
                                if info.password.is_none() {
                                    if let Ok(Some(secret)) = SecretStore::load_password(
                                        &info.name,
                                        &info.username,
                                        &info.host,
                                        info.port,
                                    ) {
                                        info.password = Some(secret);
                                    }
                                }
                                connect_off_thread(app, &mut io, info);
                                reduce(app, Action::ToggleQuickConnect);
                            }
                            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.qc_field.delete_word_left();
                                app.qc_flush();
                            }
                            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                save_quick_connect_bookmark(app);
                            }
                            KeyCode::Char(ch) => {
                                reduce(app, Action::QuickConnectInput(ch));
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if app.show_bookmarks {
                        match key.code {
                            KeyCode::Esc => reduce(app, Action::ToggleBookmarks),
                            KeyCode::Char('j') | KeyCode::Down => {
                                reduce(app, Action::SelectNextBookmark)
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                reduce(app, Action::SelectPrevBookmark)
                            }
                            KeyCode::Enter => {
                                if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned()
                                {
                                    let bm =
                                        hydrate_password_from_keyring(app, bm, "bookmark-load");
                                    reduce(app, Action::QuickConnectSetFromBookmark(bm));
                                    reduce(app, Action::ToggleBookmarks);
                                    reduce(app, Action::ToggleQuickConnect);
                                }
                            }
                            KeyCode::Char('c') => {
                                if let Some(mut bm) =
                                    app.bookmarks.get(app.selected_bookmark).cloned()
                                {
                                    if app.connected {
                                        disconnect_session(app, session, &mut io).await;
                                    }
                                    if bm.password.is_none() {
                                        if let Ok(Some(secret)) = SecretStore::load_password(
                                            &bm.name,
                                            &bm.username,
                                            &bm.host,
                                            bm.port,
                                        ) {
                                            bm.password = Some(secret);
                                        }
                                    }
                                    connect_off_thread(app, &mut io, bm);
                                    reduce(app, Action::ToggleBookmarks);
                                }
                            }
                            KeyCode::Char('d') => {
                                if app.bookmarks.is_empty() {
                                    reduce(
                                        app,
                                        Action::SetStatus("No bookmarks to delete".to_string()),
                                    );
                                } else if let Some(bm) = app.bookmarks.get(app.selected_bookmark) {
                                    let name = bm.name.clone();
                                    reduce(
                                        app,
                                        Action::ShowChoicePrompt(
                                            ChoicePromptKind::ConfirmBookmarkDelete,
                                        ),
                                    );
                                    app.prompt_target = Some(name);
                                }
                            }
                            KeyCode::Char('e') => {
                                if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned()
                                {
                                    let bm =
                                        hydrate_password_from_keyring(app, bm, "bookmark-edit");
                                    reduce(app, Action::QuickConnectSetFromBookmark(bm));
                                    reduce(app, Action::ToggleBookmarks);
                                    reduce(app, Action::ToggleQuickConnect);
                                }
                            }
                            KeyCode::Char('D') => {
                                set_default_bookmark(app);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') => {
                            if request_quit(app) {
                                return Ok(());
                            }
                        }
                        KeyCode::Esc => {
                            if app.show_compare {
                                reduce(app, Action::ToggleCompare);
                            }
                        }
                        KeyCode::Enter => match app.focus {
                            FocusPane::Queue => {}
                            FocusPane::Local | FocusPane::Remote => {
                                if let Some(entry) = get_selected_entry(app) {
                                    if entry.kind == dd_ftp_core::EntryKind::Directory {
                                        navigate_into_directory(app, &mut io);
                                    } else if app.focus == FocusPane::Local {
                                        queue_upload_selected(app);
                                    } else {
                                        queue_download_selected(app);
                                    }
                                }
                            }
                        },
                        KeyCode::Char('n') => {
                            reduce(app, Action::ShowCreatePrompt);
                        }
                        KeyCode::Char('e') => {
                            reduce(app, Action::ShowRenamePrompt);
                            if let Some(entry) = get_selected_entry(app) {
                                app.prompt_target = Some(entry.path.clone());
                                app.prompt_value = dd_ftp_app::TextField::from_str(&entry.name);
                            }
                        }
                        KeyCode::Delete => {
                            open_delete_prompt(app);
                        }
                        KeyCode::Tab => reduce(app, Action::FocusNextPane),
                        KeyCode::Char('1') => {
                            app.set_focus(FocusPane::Local);
                            reduce(app, Action::SetStatus("Focus: Local".to_string()));
                        }
                        KeyCode::Char('2') => {
                            app.set_focus(FocusPane::Remote);
                            reduce(app, Action::SetStatus("Focus: Remote".to_string()));
                        }
                        KeyCode::Char('3') => {
                            app.set_focus(FocusPane::Queue);
                            reduce(app, Action::SetStatus("Focus: Queue".to_string()));
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.focus == FocusPane::Queue {
                                app.queue_scroll = app.queue_scroll.saturating_sub(1);
                            } else {
                                reduce(app, Action::SelectUp)
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.focus == FocusPane::Queue {
                                app.queue_scroll = app.queue_scroll.saturating_add(1);
                            } else {
                                reduce(app, Action::SelectDown)
                            }
                        }
                        KeyCode::Char('l') => {
                            navigate_into_directory(app, &mut io);
                        }
                        KeyCode::Char('h') => {
                            navigate_parent_directory(app, &mut io);
                        }
                        KeyCode::Char('r') => {
                            reduce(
                                app,
                                Action::SetLocalEntries {
                                    entries: local_list(&app.local_cwd),
                                    select: SelectPolicy::PreserveName,
                                },
                            );

                            if app.connected {
                                io.list_ok_status =
                                    Some("Refreshed local + remote listing".to_string());
                                io.list_err_prefix = "Remote refresh failed".to_string();
                                list_remote(
                                    app,
                                    &mut io,
                                    app.remote_cwd.clone(),
                                    SelectPolicy::PreserveName,
                                );
                            } else {
                                reduce(
                                    app,
                                    Action::SetStatus("Refreshed local listing".to_string()),
                                );
                            }
                        }
                        KeyCode::Char('b') => {
                            reduce(app, Action::SelectNextBookmark);
                        }
                        KeyCode::Char('B') => {
                            save_quick_connect_bookmark(app);
                        }
                        KeyCode::Char('o') => {
                            reduce(app, Action::ToggleQuickConnect);
                        }
                        KeyCode::Char('m') => {
                            reduce(app, Action::ToggleBookmarks);
                        }
                        KeyCode::Char('c') => {
                            if app.connected {
                                disconnect_session(app, session, &mut io).await;
                            } else {
                                let info = selected_or_quick_connect(app);
                                connect_off_thread(app, &mut io, info);
                            }
                        }
                        KeyCode::Char('u') => {
                            queue_upload_selected(app);
                        }
                        KeyCode::Char('d') => {
                            queue_download_selected(app);
                        }
                        KeyCode::Char('X') => {
                            reduce(app, Action::ClearPendingTransfers);
                        }
                        KeyCode::Char('R') => {
                            reduce(app, Action::RetryLastFailed);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    let (mx, my) = (mouse.column, mouse.row);
                    match mouse.kind {
                        MouseEventKind::Moved => {
                            app.mouse_pos = Some((mx, my));
                        }
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                            let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                            match hit_test(&app_layout, mx, my) {
                                Some(Region::List(Pane::Local))
                                | Some(Region::Scrollbar(ScrollRegion::ListLocal))
                                    if !app.any_modal_open() =>
                                {
                                    app.set_focus(FocusPane::Local);
                                    for _ in 0..SCROLL_STEP {
                                        reduce(
                                            app,
                                            if up {
                                                Action::SelectUp
                                            } else {
                                                Action::SelectDown
                                            },
                                        );
                                    }
                                }
                                Some(Region::List(Pane::Remote))
                                | Some(Region::Scrollbar(ScrollRegion::ListRemote))
                                    if !app.any_modal_open() =>
                                {
                                    app.set_focus(FocusPane::Remote);
                                    for _ in 0..SCROLL_STEP {
                                        reduce(
                                            app,
                                            if up {
                                                Action::SelectUp
                                            } else {
                                                Action::SelectDown
                                            },
                                        );
                                    }
                                }
                                Some(Region::Scrollbar(ScrollRegion::Queue)) => {
                                    app.queue_scroll = if up {
                                        app.queue_scroll.saturating_sub(SCROLL_STEP)
                                    } else {
                                        app.queue_scroll.saturating_add(SCROLL_STEP)
                                    };
                                }
                                Some(Region::Scrollbar(ScrollRegion::Help)) => {
                                    app.help_scroll = if up {
                                        app.help_scroll.saturating_sub(SCROLL_STEP)
                                    } else {
                                        app.help_scroll.saturating_add(SCROLL_STEP)
                                    };
                                }
                                _ => {}
                            }
                        }
                        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                            let now = Instant::now();
                            let is_double = last_click
                                .map(|(lx, ly, t)| {
                                    lx == mx
                                        && ly == my
                                        && now.duration_since(t) < Duration::from_millis(300)
                                })
                                .unwrap_or(false);
                            last_click = Some((mx, my, now));
                            match hit_test(&app_layout, mx, my) {
                                Some(Region::List(pane)) if !app.any_modal_open() => {
                                    app.set_focus(match pane {
                                        Pane::Local => FocusPane::Local,
                                        Pane::Remote => FocusPane::Remote,
                                    });
                                    let (list_rect, offset, len) = match pane {
                                        Pane::Local => (
                                            app_layout.local_list,
                                            app_layout.local_list_offset,
                                            app.visible_local().len(),
                                        ),
                                        Pane::Remote => (
                                            app_layout.remote_list,
                                            app_layout.remote_list_offset,
                                            app.visible_remote().len(),
                                        ),
                                    };
                                    let content_top = list_rect.y + 1; // top border
                                    if my >= content_top {
                                        let row = (my - content_top) as usize;
                                        let idx = offset + row;
                                        if idx < len {
                                            match pane {
                                                Pane::Local => app.selected_local = idx,
                                                Pane::Remote => app.selected_remote = idx,
                                            }
                                            if is_double {
                                                last_click = None;
                                                let is_dir = match pane {
                                                    Pane::Local => app
                                                        .selected_local_entry()
                                                        .is_some_and(|e| {
                                                            e.kind
                                                                == dd_ftp_core::EntryKind::Directory
                                                        }),
                                                    Pane::Remote => app
                                                        .selected_remote_entry()
                                                        .is_some_and(|e| {
                                                            e.kind
                                                                == dd_ftp_core::EntryKind::Directory
                                                        }),
                                                };
                                                if is_dir {
                                                    navigate_into_directory(app, &mut io);
                                                } else {
                                                    match pane {
                                                        Pane::Local => queue_upload_selected(app),
                                                        Pane::Remote => {
                                                            queue_download_selected(app)
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Some(Region::Control(_)) if app.show_prompt => {}
                                Some(Region::Control(ControlId::QcProtocol)) => {
                                    reduce(app, Action::QuickConnectSetProtocolNext);
                                }
                                Some(Region::Control(ControlId::BookmarkRow(i))) => {
                                    app.selected_bookmark =
                                        i.min(app.bookmarks.len().saturating_sub(1));
                                    if is_double {
                                        last_click = None;
                                        if let Some(bm) =
                                            app.bookmarks.get(app.selected_bookmark).cloned()
                                        {
                                            let bm = hydrate_password_from_keyring(
                                                app,
                                                bm,
                                                "bookmark-load",
                                            );
                                            reduce(app, Action::QuickConnectSetFromBookmark(bm));
                                            reduce(app, Action::ToggleBookmarks);
                                            reduce(app, Action::ToggleQuickConnect);
                                        }
                                    }
                                }
                                Some(Region::Scrollbar(sr)) => {
                                    let allow = match sr {
                                        ScrollRegion::Help => true,
                                        _ => !app.any_modal_open(),
                                    };
                                    if allow {
                                        drag = Some(sr);
                                        apply_scrollbar_drag(app, &app_layout, sr, my);
                                    }
                                }
                                Some(Region::Field(fid)) => {
                                    if let Some(fr) =
                                        app_layout.fields.iter().find(|f| f.id == fid).copied()
                                    {
                                        match fid {
                                            FieldId::Prompt if app.is_text_prompt() => {
                                                let len = app.prompt_value.len();
                                                let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                                app.prompt_value.begin_drag(idx);
                                                drag_field = Some(fid);
                                            }
                                            FieldId::Prompt => {}
                                            _ => {
                                                if let Some(qf) = qc_field_for(fid) {
                                                    app.quick_connect_field = qf;
                                                    reduce(app, Action::QuickConnectSyncField);
                                                    let len = app.qc_field.len();
                                                    let idx =
                                                        dd_ftp_ui::char_index_at(&fr, mx, len);
                                                    reduce(
                                                        app,
                                                        Action::QuickConnectBeginSelect(idx),
                                                    );
                                                    drag_field = Some(fid);
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                            if let Some(sr) = drag {
                                apply_scrollbar_drag(app, &app_layout, sr, my);
                            }
                            if let Some(fid) = drag_field {
                                if let Some(fr) =
                                    app_layout.fields.iter().find(|f| f.id == fid).copied()
                                {
                                    match fid {
                                        FieldId::Prompt if app.is_text_prompt() => {
                                            let len = app.prompt_value.len();
                                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                            app.prompt_value.extend_drag(idx);
                                        }
                                        FieldId::Prompt => {}
                                        _ if app.show_quick_connect => {
                                            let len = app.qc_field.len();
                                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                            reduce(app, Action::QuickConnectExtendSelect(idx));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        MouseEventKind::Up(_) => {
                            drag = None;
                            drag_field = None;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn qc_field_for(fid: dd_ftp_ui::FieldId) -> Option<QuickConnectField> {
    use dd_ftp_ui::FieldId::*;
    Some(match fid {
        QcName => QuickConnectField::Name,
        QcHost => QuickConnectField::Host,
        QcPort => QuickConnectField::Port,
        QcUsername => QuickConnectField::Username,
        QcPassword => QuickConnectField::Password,
        QcPrivateKey => QuickConnectField::PrivateKey,
        QcPath => QuickConnectField::Path,
        Prompt => return None,
    })
}

fn apply_scrollbar_drag(
    app: &mut AppState,
    layout: &dd_ftp_ui::LayoutMap,
    sr: dd_ftp_ui::ScrollRegion,
    my: u16,
) {
    let track = match sr {
        ScrollRegion::ListLocal => layout.local_scrollbar,
        ScrollRegion::ListRemote => layout.remote_scrollbar,
        ScrollRegion::Queue => layout.queue_scrollbar,
        ScrollRegion::Help => layout.help_scrollbar.unwrap_or_default(),
    };
    if track.height == 0 {
        return;
    }
    let rel = my
        .saturating_sub(track.y)
        .min(track.height.saturating_sub(1));
    let denom = track.height.saturating_sub(1).max(1) as f32;
    let frac = rel as f32 / denom;
    match sr {
        ScrollRegion::ListLocal => {
            let n = app.visible_local().len().saturating_sub(1);
            app.selected_local = (frac * n as f32).round() as usize;
        }
        ScrollRegion::ListRemote => {
            let n = app.visible_remote().len().saturating_sub(1);
            app.selected_remote = (frac * n as f32).round() as usize;
        }
        ScrollRegion::Queue => {
            let n = app.queue.pending.len()
                + app.queue.active.len()
                + app.queue.completed.len()
                + app.queue.failed.len()
                + app.queue.cancelled.len();
            app.queue_scroll = (frac * n as f32).round() as usize;
        }
        ScrollRegion::Help => {
            app.help_scroll = (frac * track.height as f32).round() as usize;
        }
    }
}

fn navigate_into_directory(app: &mut AppState, io: &mut IoState) {
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            if let Some(entry) = app.selected_local_entry().cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.local_cwd = entry.path;
                    reduce(
                        app,
                        Action::SetLocalEntries {
                            entries: local_list(&app.local_cwd),
                            select: SelectPolicy::Reset,
                        },
                    );
                    reduce(
                        app,
                        Action::SetStatus(format!("Local cwd: {}", app.local_cwd)),
                    );
                }
            }
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            if io_busy(io) {
                return;
            }

            if let Some(entry) = app.selected_remote_entry().cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.remote_cwd = join_remote_path(&app.remote_cwd, &entry.name);
                    io.list_ok_status = Some(format!("Remote cwd: {}", app.remote_cwd));
                    io.list_err_prefix = "Remote enter failed".to_string();
                    list_remote(app, io, app.remote_cwd.clone(), SelectPolicy::Reset);
                } else {
                    reduce(
                        app,
                        Action::SetStatus(format!("'{}' is not a directory", entry.name)),
                    );
                }
            }
        }
        dd_ftp_app::FocusPane::Queue => {}
    }
}

fn navigate_parent_directory(app: &mut AppState, io: &mut IoState) {
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let parent = Path::new(&app.local_cwd)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| app.local_cwd.clone());

            app.local_cwd = parent;
            reduce(
                app,
                Action::SetLocalEntries {
                    entries: local_list(&app.local_cwd),
                    select: SelectPolicy::Reset,
                },
            );
            reduce(
                app,
                Action::SetStatus(format!("Local cwd: {}", app.local_cwd)),
            );
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            if io_busy(io) {
                return;
            }

            app.remote_cwd = parent_remote_path(&app.remote_cwd);
            io.list_ok_status = Some(format!("Remote cwd: {}", app.remote_cwd));
            io.list_err_prefix = "Remote parent failed".to_string();
            list_remote(app, io, app.remote_cwd.clone(), SelectPolicy::Reset);
        }
        dd_ftp_app::FocusPane::Queue => {}
    }
}

fn list_remote(app: &mut AppState, io: &mut IoState, path: String, select: SelectPolicy) {
    if io_busy(io) {
        return;
    }
    io.list_select = select;
    let gen = io.generation;
    io.in_flight = Some(InFlight::List {
        generation: gen,
        path: path.clone(),
    });
    reduce(app, Action::SetStatus(format!("Listing {path}...")));

    let variant_opt = app.active_connection.as_ref().map(|c| c.protocol.clone());
    let ftp = app.ftp_session.take();
    let sftp_info = app.active_connection.clone();
    let io_tx = io.io_tx.clone();
    let park = io.ftp_park.clone();

    tokio::spawn(async move {
        let mut ftp = ftp;
        let path_msg = path.clone();
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            match (&mut ftp, variant_opt) {
                (Some(f), Some(Protocol::Ftp)) => f.list_dir(FtpVariant::Ftp, &path).await,
                (Some(f), Some(Protocol::Ftps)) => f.list_dir(FtpVariant::Ftps, &path).await,
                (Some(_), _) => Err(anyhow::anyhow!("Unknown FTP variant")),
                (None, _) => {
                    let info = sftp_info.ok_or_else(|| anyhow::anyhow!("not connected"))?;
                    SftpSession::with_info(info).list_dir(&path).await
                }
            }
        })
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("list timed out")));

        if let Some(f) = ftp {
            if let Ok(mut g) = park.lock() {
                *g = Some(f);
            }
        }
        let _ = io_tx.send(IoMessage::ListDone {
            generation: gen,
            path: path_msg,
            result,
        });
    });
}

fn relist_remote(app: &mut AppState, io: &mut IoState) {
    io.list_ok_status = io.fs_ok_status.clone().into();
    io.list_err_prefix = "Remote list failed".to_string();
    list_remote(app, io, app.remote_cwd.clone(), SelectPolicy::PreserveName);
}

#[derive(Debug)]
struct WorkerResult {
    job: TransferJob,
    outcome: anyhow::Result<()>,
    was_cancelled: bool,
    cancel_flag: Arc<AtomicBool>,
}

#[derive(Debug)]
enum WorkerMessage {
    Progress {
        job_id: Uuid,
        transferred_bytes: u64,
        size_bytes: Option<u64>,
    },
    Done(WorkerResult),
}

#[allow(dead_code)]
struct PendingFile {
    local_path: String,
    remote_path: String,
    direction: TransferDirection,
    size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsKind {
    CreateFile,
    CreateFolder,
    Rename,
    Delete,
}

enum InFlight {
    Connect {
        generation: u64,
    },
    List {
        generation: u64,
        path: String,
    },
    #[allow(dead_code)]
    Scan {
        generation: u64,
    },
    HostKey {
        generation: u64,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    Fs {
        generation: u64,
        kind: FsKind,
    },
}

#[allow(clippy::large_enum_variant)]
enum ConnectOk {
    Sftp {
        info: ConnectionInfo,
        session: SftpSession,
        entries: Vec<FileEntry>,
    },
    Ftp {
        info: ConnectionInfo,
        session: UnifiedFtpSession,
        entries: Vec<FileEntry>,
    },
}

#[allow(clippy::large_enum_variant)]
enum IoMessage {
    ConnectDone {
        generation: u64,
        result: Result<ConnectOk, anyhow::Error>,
    },
    ListDone {
        generation: u64,
        path: String,
        result: Result<Vec<FileEntry>, anyhow::Error>,
    },
    FsDone {
        generation: u64,
        kind: FsKind,
        result: Result<(), anyhow::Error>,
    },
    HostKeyChallenge {
        generation: u64,
        host: String,
        port: u16,
        fingerprint: String,
        changed: bool,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    #[allow(dead_code)]
    ScanItem { generation: u64, file: PendingFile },
    #[allow(dead_code)]
    ScanDone { generation: u64 },
    #[allow(dead_code)]
    ScanError { generation: u64, error: String },
}

struct IoState {
    generation: u64,
    in_flight: Option<InFlight>,
    io_tx: mpsc::UnboundedSender<IoMessage>,
    ftp_park: Arc<Mutex<Option<UnifiedFtpSession>>>,
    list_select: SelectPolicy,
    list_ok_status: Option<String>,
    list_err_prefix: String,
    fs_remote: bool,
    fs_ok_status: String,
}

fn io_busy(io: &IoState) -> bool {
    io.in_flight.is_some()
}

fn take_parked_ftp(park: &Arc<Mutex<Option<UnifiedFtpSession>>>) -> Option<UnifiedFtpSession> {
    park.lock().ok().and_then(|mut g| g.take())
}

fn accept_host_key(io: &mut IoState) {
    if let Some(InFlight::HostKey { generation, reply }) = io.in_flight.take() {
        let _ = reply.send(true);
        io.in_flight = Some(InFlight::Connect { generation });
    }
}

fn reject_host_key(io: &mut IoState) {
    if let Some(InFlight::HostKey { generation, reply }) = io.in_flight.take() {
        let _ = reply.send(false);
        io.in_flight = Some(InFlight::Connect { generation });
    }
}

fn handle_io_message(
    app: &mut AppState,
    session: &mut SftpSession,
    io: &mut IoState,
    msg: IoMessage,
) {
    match msg {
        IoMessage::ConnectDone { generation, result } => {
            let matches_inflight = matches!(
                io.in_flight,
                Some(InFlight::Connect { generation: g } | InFlight::HostKey { generation: g, .. })
                    if g == generation
            );
            if generation != io.generation || !matches_inflight {
                return;
            }
            io.in_flight = None;
            match result {
                Ok(ConnectOk::Sftp {
                    info,
                    session: sftp,
                    entries,
                }) => {
                    *session = sftp;
                    app.ftp_session = None;
                    reduce(app, Action::SetConnected(true));
                    reduce(
                        app,
                        Action::SetRemoteEntries {
                            entries,
                            select: SelectPolicy::Reset,
                        },
                    );
                    app.active_connection = Some(info.clone());
                    reduce(
                        app,
                        Action::SetStatus(format!(
                            "Connected via {:?} to {} as {} (cwd: {})",
                            info.protocol, info.host, info.username, app.remote_cwd
                        )),
                    );
                }
                Ok(ConnectOk::Ftp {
                    info,
                    session: ftp,
                    entries,
                }) => {
                    *session = SftpSession::default();
                    app.ftp_session = Some(ftp);
                    reduce(app, Action::SetConnected(true));
                    reduce(
                        app,
                        Action::SetRemoteEntries {
                            entries,
                            select: SelectPolicy::Reset,
                        },
                    );
                    app.active_connection = Some(info.clone());
                    reduce(
                        app,
                        Action::SetStatus(format!(
                            "Connected via {:?} to {} as {} (cwd: {})",
                            info.protocol, info.host, info.username, app.remote_cwd
                        )),
                    );
                }
                Err(err) => {
                    reduce(app, Action::SetConnected(false));
                    reduce(app, Action::ShowError(err.to_string()));
                }
            }
        }
        IoMessage::ListDone {
            generation,
            path,
            result,
        } => {
            let parked = take_parked_ftp(&io.ftp_park);
            if generation != io.generation {
                return;
            }
            if let Some(ftp) = parked {
                app.ftp_session = Some(ftp);
            }
            let matches_inflight = matches!(
                &io.in_flight,
                Some(InFlight::List { generation: g, path: p })
                    if *g == generation && *p == path
            );
            if !matches_inflight {
                return;
            }
            io.in_flight = None;
            match result {
                Ok(entries) => {
                    let select = io.list_select;
                    reduce(app, Action::SetRemoteEntries { entries, select });
                    if let Some(status) = io.list_ok_status.take() {
                        reduce(app, Action::SetStatus(status));
                    }
                }
                Err(err) => {
                    reduce(
                        app,
                        Action::ShowError(format!("{}: {err}", io.list_err_prefix)),
                    );
                }
            }
        }
        IoMessage::FsDone {
            generation,
            kind,
            result,
        } => {
            let parked = take_parked_ftp(&io.ftp_park);
            if generation != io.generation {
                return;
            }
            if let Some(ftp) = parked {
                app.ftp_session = Some(ftp);
            }
            let matches_inflight = matches!(
                io.in_flight,
                Some(InFlight::Fs { generation: g, kind: k }) if g == generation && k == kind
            );
            if !matches_inflight {
                return;
            }
            io.in_flight = None;
            match result {
                Ok(()) => {
                    let status = io.fs_ok_status.clone();
                    reduce(app, Action::SetStatus(status));
                    if io.fs_remote {
                        if app.connected {
                            relist_remote(app, io);
                        }
                    } else {
                        reduce(
                            app,
                            Action::SetLocalEntries {
                                entries: local_list(&app.local_cwd),
                                select: SelectPolicy::PreserveName,
                            },
                        );
                    }
                }
                Err(err) => {
                    reduce(app, Action::ShowError(format!("{err}")));
                }
            }
        }
        IoMessage::HostKeyChallenge {
            generation,
            host,
            port,
            fingerprint,
            changed,
            reply,
        } => {
            if generation != io.generation
                || matches!(io.in_flight, Some(InFlight::HostKey { .. }))
                || !matches!(
                    io.in_flight,
                    Some(InFlight::Connect { generation: g }) if g == generation
                )
            {
                let _ = reply.send(false);
                return;
            }
            io.in_flight = Some(InFlight::HostKey { generation, reply });
            app.host_key = Some(HostKeyView {
                host,
                port,
                fingerprint,
                changed,
            });
            reduce(app, Action::ShowChoicePrompt(ChoicePromptKind::HostKey));
        }
        IoMessage::ScanItem { .. } | IoMessage::ScanDone { .. } | IoMessage::ScanError { .. } => {}
    }
}

fn handle_worker_result(
    app: &mut AppState,
    _session: &mut SftpSession,
    io: &mut IoState,
    mut msg: WorkerResult,
) {
    if app.worker_active_count == 0 {
        app.worker_cancel_requested = false;
    }

    if app.worker_cancel_requested || msg.was_cancelled {
        if app.worker_active_count == 0 {
            app.worker_cancel_requested = false;
        }
        msg.job.last_error = Some("Cancelled by user".to_string());
        reduce(app, Action::MarkTransferCancelled(msg.job));
        return;
    }

    match msg.outcome {
        Ok(_) => {
            let name = match msg.job.direction {
                TransferDirection::Upload => "upload",
                TransferDirection::Download => "download",
            };
            msg.job.last_error = None;
            reduce(app, Action::MarkTransferCompleted(msg.job));
            reduce(app, Action::SetStatus(format!("{name} complete")));

            reduce(
                app,
                Action::SetLocalEntries {
                    entries: local_list(&app.local_cwd),
                    select: SelectPolicy::PreserveName,
                },
            );
            if app.connected {
                io.list_ok_status = None;
                io.list_err_prefix = "Remote list failed".to_string();
                list_remote(app, io, app.remote_cwd.clone(), SelectPolicy::PreserveName);
            }
        }
        Err(err) => {
            msg.job.last_error = Some(err.to_string());
            reduce(app, Action::MarkTransferFailed(msg.job));
            reduce(app, Action::ShowError(format!("Transfer failed: {err}")));
        }
    }
}

async fn disconnect_session(app: &mut AppState, session: &mut SftpSession, io: &mut IoState) {
    if let Some(InFlight::HostKey { reply, .. }) = io.in_flight.take() {
        let _ = reply.send(false);
    }
    io.in_flight = None;
    io.generation = io.generation.wrapping_add(1);
    let _ = take_parked_ftp(&io.ftp_park);
    if let Some(mut ftp) = app.ftp_session.take() {
        let _ = ftp.disconnect().await;
    }
    match session.disconnect().await {
        Ok(_) => {
            reduce(app, Action::Disconnect);
            app.remote_entries.clear();
            app.queue.active.clear();
            app.worker_running = false;
            app.worker_active_count = 0;
            app.worker_cancel_requested = false;
            app.active_connection = None;
            reduce(app, Action::SetStatus("Disconnected".to_string()));
        }
        Err(err) => {
            reduce(app, Action::SetStatus(format!("Disconnect failed: {err}")));
        }
    }
}

fn connect_off_thread(app: &mut AppState, io: &mut IoState, info: ConnectionInfo) {
    if io_busy(io) {
        return;
    }
    if info.name.trim().is_empty() {
        reduce(
            app,
            Action::SetStatus("Connect failed: label/name is required".to_string()),
        );
        return;
    }
    if info.host.trim().is_empty() {
        reduce(
            app,
            Action::SetStatus("Connect failed: host is required".to_string()),
        );
        return;
    }
    if info.username.trim().is_empty() {
        reduce(
            app,
            Action::SetStatus("Connect failed: username is required".to_string()),
        );
        return;
    }
    if info.port == 0 {
        reduce(
            app,
            Action::SetStatus("Connect failed: port must be > 0".to_string()),
        );
        return;
    }

    io.generation = io.generation.wrapping_add(1);
    let gen = io.generation;
    io.in_flight = Some(InFlight::Connect { generation: gen });

    app.remote_cwd = if info.initial_path.trim().is_empty() {
        "/".to_string()
    } else {
        info.initial_path.clone()
    };
    reduce(app, Action::Connect(info.clone()));

    let list_path = app.remote_cwd.clone();
    let io_tx = io.io_tx.clone();
    tokio::spawn(async move {
        let label = format!(
            "{}@{}:{} via {:?}",
            info.username, info.host, info.port, info.protocol
        );
        let result = connect_task(info, list_path, gen, io_tx.clone())
            .await
            .map_err(|err| anyhow::anyhow!("Connect failed for {label} -> {err}"));
        let _ = io_tx.send(IoMessage::ConnectDone {
            generation: gen,
            result,
        });
    });
}

async fn connect_task(
    info: ConnectionInfo,
    list_path: String,
    generation: u64,
    io_tx: mpsc::UnboundedSender<IoMessage>,
) -> Result<ConnectOk> {
    match info.protocol {
        Protocol::Sftp => {
            let mut session = SftpSession::default();
            let io_tx_h = io_tx.clone();
            session
                .connect_with_host_key_handler(info.clone(), move |offer| {
                    let (reply, rx) = tokio::sync::oneshot::channel();
                    let _ = io_tx_h.send(IoMessage::HostKeyChallenge {
                        generation,
                        host: offer.host,
                        port: offer.port,
                        fingerprint: offer.fingerprint,
                        changed: offer.changed,
                        reply,
                    });
                    tokio::runtime::Handle::current()
                        .block_on(rx)
                        .unwrap_or(false)
                })
                .await?;
            let entries = session.list_dir(&list_path).await?;
            Ok(ConnectOk::Sftp {
                info,
                session,
                entries,
            })
        }
        Protocol::Ftp | Protocol::Ftps => {
            let variant = match info.protocol {
                Protocol::Ftp => FtpVariant::Ftp,
                Protocol::Ftps => FtpVariant::Ftps,
                Protocol::Sftp => unreachable!("connect_task Sftp in FTP branch"),
            };
            let mut unified = UnifiedFtpSession::new();
            unified.connect(variant, info.clone()).await?;
            let entries = unified.list_dir(variant, &list_path).await?;
            Ok(ConnectOk::Ftp {
                info,
                session: unified,
                entries,
            })
        }
    }
}

fn selected_or_quick_connect(app: &mut AppState) -> ConnectionInfo {
    quick_connect_info(app)
}

fn quick_connect_info(app: &mut AppState) -> ConnectionInfo {
    hydrate_password_from_keyring(app, app.quick_connect.clone(), "quick-connect")
}

fn request_quit(app: &mut AppState) -> bool {
    if app.worker_running || !app.queue.active.is_empty() {
        reduce(app, Action::ShowChoicePrompt(ChoicePromptKind::ConfirmQuit));
        false
    } else {
        true
    }
}

fn queue_upload_selected(app: &mut AppState) {
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }

    let selected = app.selected_local_entry().cloned();
    if let Some(local) = selected {
        if local.kind == dd_ftp_core::EntryKind::Directory {
            reduce(
                app,
                Action::SetStatus("Select a local file to upload".to_string()),
            );
            return;
        }

        let remote_target = match safe_remote_child(&app.remote_cwd, &local.name) {
            Ok(p) => p,
            Err(_) => {
                reduce(app, Action::ShowError("path escapes directory".to_string()));
                return;
            }
        };
        let job = TransferJob::new(local.path, remote_target, TransferDirection::Upload);
        reduce(app, Action::QueueTransfer(job));
    }
}

fn queue_download_selected(app: &mut AppState) {
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }

    let selected = app.selected_remote_entry().cloned();
    if let Some(remote) = selected {
        if remote.kind == dd_ftp_core::EntryKind::Directory {
            reduce(
                app,
                Action::SetStatus("Select a remote file to download".to_string()),
            );
            return;
        }

        let local_target = match safe_local_child(Path::new(&app.local_cwd), &remote.name) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => {
                reduce(app, Action::ShowError("path escapes directory".to_string()));
                return;
            }
        };
        let remote_path = match safe_remote_child(&app.remote_cwd, &remote.name) {
            Ok(p) => p,
            Err(_) => {
                reduce(app, Action::ShowError("path escapes directory".to_string()));
                return;
            }
        };
        let job = TransferJob::new(local_target, remote_path, TransferDirection::Download);
        reduce(app, Action::QueueTransfer(job));
    }
}

fn hydrate_password_from_keyring(
    app: &mut AppState,
    mut info: ConnectionInfo,
    context: &str,
) -> ConnectionInfo {
    if info.password.is_none() {
        match SecretStore::load_password(&info.name, &info.username, &info.host, info.port) {
            Ok(Some(secret)) => {
                info.password = Some(secret);
            }
            Ok(None) => {
                reduce(
                    app,
                    Action::SetStatus(format!(
                        "No keyring password found ({context}) for {}@{}:{}",
                        info.username, info.host, info.port
                    )),
                );
            }
            Err(err) => {
                let msg = format!(
                    "Keyring load failed ({context}) for {}@{}:{}: {err}",
                    info.username, info.host, info.port
                );
                reduce(app, Action::SetStatus(msg.clone()));
                reduce(app, Action::ShowError(msg));
            }
        }
    }
    info
}

fn run_keyring_health_check(app: &mut AppState) {
    match SecretStore::check_backend_available() {
        Ok(_) => {
            reduce(
                app,
                Action::SetStatus(
                    "Keyring backend detected: password persistence enabled".to_string(),
                ),
            );
        }
        Err(err) => {
            let msg = format!(
                "Keyring backend unavailable. Password persistence disabled. Details: {err}"
            );
            reduce(app, Action::SetStatus(msg.clone()));
            reduce(app, Action::ShowError(msg));
        }
    }
}

fn save_quick_connect_bookmark(app: &mut AppState) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    let info = app.quick_connect.clone();

    if info.name.trim().is_empty()
        || info.host.trim().is_empty()
        || info.username.trim().is_empty()
        || info.port == 0
    {
        reduce(
            app,
            Action::SetStatus("Cannot save bookmark: host/user/port required".to_string()),
        );
        return;
    }

    let secret_status = if let Some(password) = info.password.as_deref() {
        if let Err(err) =
            SecretStore::save_password(&info.name, &info.username, &info.host, info.port, password)
        {
            let msg = format!("Save secret failed: {err}");
            reduce(app, Action::SetStatus(msg.clone()));
            reduce(app, Action::ShowError(msg));
            return;
        }

        let key = SecretStore::primary_key_for(&info.name, &info.username, &info.host, info.port);
        match SecretStore::load_password(&info.name, &info.username, &info.host, info.port) {
            Ok(Some(_)) => format!("Password saved to keyring (verified key: {key})"),
            Ok(None) => {
                let msg = format!(
                    "Password save reported success, but verification lookup returned no entry (key: {key})"
                );
                reduce(app, Action::ShowError(msg.clone()));
                msg
            }
            Err(err) => {
                let msg = format!("Password save verification failed for key {key}: {err}");
                reduce(app, Action::ShowError(msg.clone()));
                msg
            }
        }
    } else {
        "No password provided (bookmark saved without keyring secret)".to_string()
    };

    let existing_idx = cfg
        .sites
        .iter()
        .position(|s| s.host == info.host && s.username == info.username && s.port == info.port);

    if let Some(idx) = existing_idx {
        cfg.sites[idx] = info;
        if cfg.default_site.is_none() {
            cfg.default_site = Some(idx);
        }

        match SiteManager::save_to_default_path(&cfg) {
            Ok(_) => {
                app.selected_bookmark = idx;
                reduce(app, Action::SetBookmarks(cfg.sites));
                reduce(
                    app,
                    Action::SetStatus(format!("Updated bookmark | {}", secret_status)),
                );
            }
            Err(err) => {
                reduce(
                    app,
                    Action::SetStatus(format!("Save bookmark failed: {err}")),
                );
            }
        }
    } else {
        cfg.sites.push(info);
        let idx = cfg.sites.len().saturating_sub(1);
        if cfg.default_site.is_none() {
            cfg.default_site = Some(0);
        }

        match SiteManager::save_to_default_path(&cfg) {
            Ok(_) => {
                app.selected_bookmark = idx;
                reduce(app, Action::SetBookmarks(cfg.sites));
                reduce(
                    app,
                    Action::SetStatus(format!("Saved bookmark | {}", secret_status)),
                );
            }
            Err(err) => {
                reduce(
                    app,
                    Action::SetStatus(format!("Save bookmark failed: {err}")),
                );
            }
        }
    }
}

fn delete_bookmark_named(app: &mut AppState, name: &str) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    if cfg.sites.is_empty() {
        reduce(app, Action::SetStatus("No bookmarks to delete".to_string()));
        return;
    }

    let Some(idx) = cfg.sites.iter().position(|s| s.name == name) else {
        reduce(
            app,
            Action::SetStatus(format!("Bookmark not found: {name}")),
        );
        return;
    };

    let removed = cfg.sites.remove(idx);
    let _ = SecretStore::delete_password(
        &removed.name,
        &removed.username,
        &removed.host,
        removed.port,
    );

    if let Some(default_idx) = cfg.default_site {
        cfg.default_site = if cfg.sites.is_empty() {
            None
        } else if default_idx == idx {
            Some(0)
        } else if default_idx > idx {
            Some(default_idx - 1)
        } else {
            Some(default_idx)
        };
    }

    match SiteManager::save_to_default_path(&cfg) {
        Ok(_) => {
            reduce(app, Action::SetBookmarks(cfg.sites));
            reduce(
                app,
                Action::SetStatus(format!("Deleted bookmark: {}", removed.name)),
            );
        }
        Err(err) => {
            reduce(
                app,
                Action::SetStatus(format!("Delete bookmark failed: {err}")),
            );
        }
    }
}

fn set_default_bookmark(app: &mut AppState) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    if cfg.sites.is_empty() {
        reduce(
            app,
            Action::SetStatus("No bookmarks to set as default".to_string()),
        );
        return;
    }

    if app.selected_bookmark >= cfg.sites.len() {
        reduce(
            app,
            Action::SetStatus("Invalid bookmark selection".to_string()),
        );
        return;
    }

    let selected = app.selected_bookmark;
    if selected != 0 {
        cfg.sites.swap(0, selected);
    }
    cfg.default_site = Some(0);

    match SiteManager::save_to_default_path(&cfg) {
        Ok(_) => {
            reduce(app, Action::SetBookmarks(cfg.sites));
            reduce(
                app,
                Action::SetStatus("Default bookmark updated".to_string()),
            );
        }
        Err(err) => {
            reduce(app, Action::SetStatus(format!("Set default failed: {err}")));
        }
    }
}

fn connection_info_from_env() -> ConnectionInfo {
    let host = std::env::var("DD_FTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let name = std::env::var("DD_FTP_NAME").unwrap_or_else(|_| host.clone());
    let port = std::env::var("DD_FTP_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(22);
    let username = std::env::var("DD_FTP_USER").unwrap_or_else(|_| "user".to_string());
    let password = std::env::var("DD_FTP_PASS").ok();
    let private_key = std::env::var("DD_FTP_KEY").ok();
    let initial_path = std::env::var("DD_FTP_PATH").unwrap_or_else(|_| "/".to_string());

    ConnectionInfo {
        name,
        host,
        port,
        protocol: Protocol::Sftp,
        username,
        password,
        private_key,
        initial_path,
    }
}

fn is_safe_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    if Path::new(name).is_absolute() {
        return false;
    }
    if name.contains('/') || name.contains('\\') {
        return false;
    }
    true
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Join `name` onto `cwd` for a *local* create/rename/download destination.
/// Rejects empty, `.`, `..`, absolute names, and any name containing a
/// path separator. After join, the normalized result must stay under `cwd`
/// (also reject `name` values like `foo/../../etc`).
pub fn safe_local_child(cwd: &Path, name: &str) -> anyhow::Result<PathBuf> {
    if !is_safe_component(name) {
        bail!("path escapes directory");
    }
    let joined = cwd.join(name);
    let cwd_n = normalize_lexical(cwd);
    let joined_n = normalize_lexical(&joined);
    if !joined_n.starts_with(&cwd_n) {
        bail!("path escapes directory");
    }
    Ok(joined)
}

/// Join `name` onto remote `cwd`. Rejects empty, `.`, `..`, leading `/`,
/// and any name containing `/`. Recursive builders call this once per
/// path component.
pub fn safe_remote_child(cwd: &str, name: &str) -> anyhow::Result<String> {
    if !is_safe_component(name) {
        bail!("path escapes directory");
    }
    let joined = join_remote_path(cwd, name);
    let cwd_n = if cwd.is_empty() { "/" } else { cwd };
    let cwd_prefix = cwd_n.trim_end_matches('/');
    if cwd_prefix.is_empty() || cwd_prefix == "/" {
        if !joined.starts_with('/') {
            bail!("path escapes directory");
        }
        return Ok(joined);
    }
    if joined == cwd_prefix || joined.starts_with(&format!("{cwd_prefix}/")) {
        Ok(joined)
    } else {
        bail!("path escapes directory");
    }
}

fn join_remote_path(base: &str, child: &str) -> String {
    if child.starts_with('/') {
        return child.to_string();
    }
    let base = if base.is_empty() { "/" } else { base };
    if base == "/" {
        format!("/{}", child.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            child.trim_start_matches('/')
        )
    }
}

fn parent_remote_path(path: &str) -> String {
    let p = if path.is_empty() { "/" } else { path };
    if p == "/" {
        return "/".to_string();
    }
    let trimmed = p.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
    }
}

fn local_list(path: &str) -> Vec<FileEntry> {
    let mut out = Vec::new();

    let current_path = if path.is_empty() { "." } else { path };
    let parent_path = Path::new(current_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| current_path.to_string());

    out.push(FileEntry {
        name: ".".to_string(),
        path: current_path.to_string(),
        kind: dd_ftp_core::EntryKind::Directory,
        size: 0,
        modified: None,
        permissions: None,
    });

    out.push(FileEntry {
        name: "..".to_string(),
        path: parent_path,
        kind: dd_ftp_core::EntryKind::Directory,
        size: 0,
        modified: None,
        permissions: None,
    });

    if let Ok(entries) = std::fs::read_dir(current_path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let kind = if meta.is_dir() {
                    dd_ftp_core::EntryKind::Directory
                } else {
                    dd_ftp_core::EntryKind::File
                };

                let modified = meta.modified().ok().map(Into::into);
                #[cfg(unix)]
                let permissions = {
                    use std::os::unix::fs::PermissionsExt;
                    Some(format!("{:o}", meta.permissions().mode() & 0o7777))
                };
                #[cfg(not(unix))]
                let permissions = None;

                out.push(FileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().to_string_lossy().to_string(),
                    kind,
                    size: meta.len(),
                    modified,
                    permissions,
                });
            }
        }
    }

    let (special, mut regular): (Vec<_>, Vec<_>) = out
        .into_iter()
        .partition(|e| e.name == "." || e.name == "..");

    regular.sort_by_key(|a| a.name.to_lowercase());

    let mut result = special;
    result.extend(regular);
    result
}

fn get_selected_entry(app: &AppState) -> Option<dd_ftp_core::FileEntry> {
    match app.focus {
        dd_ftp_app::FocusPane::Local => app.selected_local_entry().cloned(),
        dd_ftp_app::FocusPane::Remote => app.selected_remote_entry().cloned(),
        _ => None,
    }
}

fn open_delete_prompt(app: &mut AppState) {
    if let Some(entry) = get_selected_entry(app) {
        reduce(app, Action::ShowDeletePrompt);
        app.prompt_target = Some(entry.path.clone());
    } else {
        reduce(
            app,
            Action::SetStatus("Nothing selected to delete".to_string()),
        );
    }
}

fn begin_fs(app: &mut AppState, io: &mut IoState, kind: FsKind, remote: bool, ok_status: String) {
    io.fs_remote = remote;
    io.fs_ok_status = ok_status;
    io.in_flight = Some(InFlight::Fs {
        generation: io.generation,
        kind,
    });
    let status = match kind {
        FsKind::CreateFile | FsKind::CreateFolder => "Creating…",
        FsKind::Rename => "Renaming…",
        FsKind::Delete => "Deleting…",
    };
    reduce(app, Action::SetStatus(status.to_string()));
}

fn spawn_local_fs(io: &IoState, kind: FsKind, work: impl FnOnce() -> Result<()> + Send + 'static) {
    let gen = io.generation;
    let io_tx = io.io_tx.clone();
    tokio::task::spawn_blocking(move || {
        let result = work();
        let _ = io_tx.send(IoMessage::FsDone {
            generation: gen,
            kind,
            result,
        });
    });
}

fn spawn_remote_fs<F, Fut>(app: &mut AppState, io: &IoState, kind: FsKind, work: F)
where
    F: FnOnce(Option<UnifiedFtpSession>, Option<Protocol>, Option<ConnectionInfo>) -> Fut
        + Send
        + 'static,
    Fut: std::future::Future<Output = (Option<UnifiedFtpSession>, Result<()>)> + Send,
{
    let gen = io.generation;
    let io_tx = io.io_tx.clone();
    let park = io.ftp_park.clone();
    let ftp = app.ftp_session.take();
    let variant_opt = app.active_connection.as_ref().map(|c| c.protocol.clone());
    let sftp_info = app.active_connection.clone();
    tokio::spawn(async move {
        let (ftp, result) = work(ftp, variant_opt, sftp_info).await;
        if let Some(f) = ftp {
            if let Ok(mut g) = park.lock() {
                *g = Some(f);
            }
        }
        let _ = io_tx.send(IoMessage::FsDone {
            generation: gen,
            kind,
            result,
        });
    });
}

fn create_file(app: &mut AppState, io: &mut IoState, name: &str) {
    if io_busy(io) {
        return;
    }
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let path = match safe_local_child(Path::new(&app.local_cwd), name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            begin_fs(
                app,
                io,
                FsKind::CreateFile,
                false,
                format!("Created file: {name}"),
            );
            spawn_local_fs(io, FsKind::CreateFile, move || {
                std::fs::File::create(&path).map(|_| ()).map_err(Into::into)
            });
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            let path = match safe_remote_child(&app.remote_cwd, name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            let temp_file = std::env::temp_dir().join(format!("dd_ftp_empty_{name}"));
            begin_fs(
                app,
                io,
                FsKind::CreateFile,
                true,
                format!("Created file: {name}"),
            );
            spawn_remote_fs(
                app,
                io,
                FsKind::CreateFile,
                move |mut ftp, variant, info| async move {
                    let result = async {
                        std::fs::File::create(&temp_file)?;
                        let job = TransferJob::new(
                            temp_file.to_string_lossy().to_string(),
                            path,
                            TransferDirection::Upload,
                        );
                        let r = match (&mut ftp, variant) {
                            (Some(f), Some(Protocol::Ftp)) => f.upload(FtpVariant::Ftp, &job).await,
                            (Some(f), Some(Protocol::Ftps)) => {
                                f.upload(FtpVariant::Ftps, &job).await
                            }
                            (Some(_), _) => Err(anyhow::anyhow!("Unknown FTP variant")),
                            (None, _) => {
                                let info = info.ok_or_else(|| anyhow::anyhow!("not connected"))?;
                                SftpSession::with_info(info).upload(&job).await
                            }
                        };
                        let _ = std::fs::remove_file(&temp_file);
                        r
                    }
                    .await;
                    (ftp, result)
                },
            );
        }
        _ => {}
    }
}

fn create_folder(app: &mut AppState, io: &mut IoState, name: &str) {
    if io_busy(io) {
        return;
    }
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let path = match safe_local_child(Path::new(&app.local_cwd), name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            begin_fs(
                app,
                io,
                FsKind::CreateFolder,
                false,
                format!("Created folder: {name}"),
            );
            spawn_local_fs(io, FsKind::CreateFolder, move || {
                std::fs::create_dir(&path).map_err(Into::into)
            });
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            let path = match safe_remote_child(&app.remote_cwd, name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            begin_fs(
                app,
                io,
                FsKind::CreateFolder,
                true,
                format!("Created folder: {name}"),
            );
            spawn_remote_fs(
                app,
                io,
                FsKind::CreateFolder,
                move |mut ftp, variant, info| async move {
                    let result = match (&mut ftp, variant) {
                        (Some(f), Some(Protocol::Ftp)) => {
                            f.create_dir(FtpVariant::Ftp, &path).await
                        }
                        (Some(f), Some(Protocol::Ftps)) => {
                            f.create_dir(FtpVariant::Ftps, &path).await
                        }
                        (Some(_), _) => Err(anyhow::anyhow!("Unknown FTP variant")),
                        (None, _) => match info {
                            Some(info) => SftpSession::with_info(info).create_dir(&path).await,
                            None => Err(anyhow::anyhow!("not connected")),
                        },
                    };
                    (ftp, result)
                },
            );
        }
        _ => {}
    }
}

fn rename_item(app: &mut AppState, io: &mut IoState, _target: &str, new_name: &str) {
    if io_busy(io) {
        return;
    }
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let old_name = match app.selected_local_entry() {
                Some(e) => e.name.clone(),
                None => {
                    reduce(app, Action::SetStatus("No item selected".to_string()));
                    return;
                }
            };
            let from = match safe_local_child(Path::new(&app.local_cwd), &old_name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            let new_path = match safe_local_child(Path::new(&app.local_cwd), new_name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            begin_fs(
                app,
                io,
                FsKind::Rename,
                false,
                format!("Renamed to: {new_name}"),
            );
            spawn_local_fs(io, FsKind::Rename, move || {
                std::fs::rename(&from, &new_path).map_err(Into::into)
            });
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            let old_name = match app.selected_remote_entry() {
                Some(e) => e.name.clone(),
                None => {
                    reduce(app, Action::SetStatus("No item selected".to_string()));
                    return;
                }
            };
            let from = match safe_remote_child(&app.remote_cwd, &old_name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            let to = match safe_remote_child(&app.remote_cwd, new_name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            begin_fs(
                app,
                io,
                FsKind::Rename,
                true,
                format!("Renamed to: {new_name}"),
            );
            spawn_remote_fs(
                app,
                io,
                FsKind::Rename,
                move |mut ftp, variant, info| async move {
                    let result = match (&mut ftp, variant) {
                        (Some(f), Some(Protocol::Ftp)) => {
                            f.rename(FtpVariant::Ftp, &from, &to).await
                        }
                        (Some(f), Some(Protocol::Ftps)) => {
                            f.rename(FtpVariant::Ftps, &from, &to).await
                        }
                        (Some(_), _) => Err(anyhow::anyhow!("Unknown FTP variant")),
                        (None, _) => match info {
                            Some(info) => SftpSession::with_info(info).rename(&from, &to).await,
                            None => Err(anyhow::anyhow!("not connected")),
                        },
                    };
                    (ftp, result)
                },
            );
        }
        _ => {}
    }
}

fn delete_item(app: &mut AppState, io: &mut IoState, target: &str) {
    if io_busy(io) {
        return;
    }
    let is_dir = match app.focus {
        dd_ftp_app::FocusPane::Local => app
            .selected_local_entry()
            .map(|e| e.kind == dd_ftp_core::EntryKind::Directory)
            .unwrap_or(false),
        dd_ftp_app::FocusPane::Remote => app
            .selected_remote_entry()
            .map(|e| e.kind == dd_ftp_core::EntryKind::Directory)
            .unwrap_or(false),
        _ => false,
    };

    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let name = app
                .selected_local_entry()
                .map(|e| e.name.clone())
                .unwrap_or_else(|| {
                    Path::new(target)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
            let target_path = match safe_local_child(Path::new(&app.local_cwd), &name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            let target_str = target_path.to_string_lossy().to_string();
            begin_fs(
                app,
                io,
                FsKind::Delete,
                false,
                format!("Deleted: {target_str}"),
            );
            spawn_local_fs(io, FsKind::Delete, move || {
                if is_dir {
                    std::fs::remove_dir(&target_path).map_err(Into::into)
                } else {
                    std::fs::remove_file(&target_path).map_err(Into::into)
                }
            });
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            let name = match app.selected_remote_entry() {
                Some(e) => e.name.clone(),
                None => {
                    reduce(app, Action::SetStatus("No item selected".to_string()));
                    return;
                }
            };
            let path = match safe_remote_child(&app.remote_cwd, &name) {
                Ok(p) => p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return;
                }
            };
            begin_fs(app, io, FsKind::Delete, true, format!("Deleted: {path}"));
            spawn_remote_fs(
                app,
                io,
                FsKind::Delete,
                move |mut ftp, variant, info| async move {
                    let result = match (&mut ftp, variant) {
                        (Some(f), Some(Protocol::Ftp)) => {
                            if is_dir {
                                f.remove_dir(FtpVariant::Ftp, &path).await
                            } else {
                                f.remove_file(FtpVariant::Ftp, &path).await
                            }
                        }
                        (Some(f), Some(Protocol::Ftps)) => {
                            if is_dir {
                                f.remove_dir(FtpVariant::Ftps, &path).await
                            } else {
                                f.remove_file(FtpVariant::Ftps, &path).await
                            }
                        }
                        (Some(_), _) => Err(anyhow::anyhow!("Unknown FTP variant")),
                        (None, _) => match info {
                            Some(info) => {
                                let s = SftpSession::with_info(info);
                                if is_dir {
                                    s.remove_dir(&path).await
                                } else {
                                    s.remove_file(&path).await
                                }
                            }
                            None => Err(anyhow::anyhow!("not connected")),
                        },
                    };
                    (ftp, result)
                },
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod connect_info_tests {
    use super::*;

    #[test]
    fn selected_or_quick_connect_uses_form_not_bookmark() {
        let mut app = AppState {
            bookmarks: vec![ConnectionInfo {
                name: "saved".into(),
                host: "bookmark.example".into(),
                port: 22,
                protocol: Protocol::Sftp,
                username: "bmuser".into(),
                password: Some("bmpass".into()),
                private_key: None,
                initial_path: "/".into(),
            }],
            selected_bookmark: 0,
            quick_connect: ConnectionInfo {
                host: "form.example".into(),
                username: "formuser".into(),
                password: Some("formpass".into()),
                ..ConnectionInfo::default()
            },
            ..Default::default()
        };

        let info = selected_or_quick_connect(&mut app);
        assert_eq!(info.host, "form.example");
        assert_eq!(info.username, "formuser");

        let info = quick_connect_info(&mut app);
        assert_eq!(info.host, "form.example");
        assert_eq!(info.username, "formuser");
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn join_remote_path_joins_base_and_name() {
        assert_eq!(join_remote_path("/pub", "file.bin"), "/pub/file.bin");
        assert_eq!(join_remote_path("/", "file.bin"), "/file.bin");
    }

    #[test]
    fn safe_local_child_table() {
        let cwd = Path::new("/tmp/pane");
        struct Case {
            name: &'static str,
            ok: bool,
        }
        let cases = [
            Case {
                name: "foo",
                ok: true,
            },
            Case {
                name: ".",
                ok: false,
            },
            Case {
                name: "..",
                ok: false,
            },
            Case {
                name: "/etc/passwd",
                ok: false,
            },
            Case {
                name: "foo/bar",
                ok: false,
            },
            Case {
                name: "foo/../../etc",
                ok: false,
            },
        ];
        for case in cases {
            let got = safe_local_child(cwd, case.name);
            assert_eq!(got.is_ok(), case.ok, "local name {:?}", case.name);
            if case.ok {
                assert_eq!(got.unwrap(), cwd.join("foo"));
            }
        }
    }

    #[test]
    fn safe_remote_child_table() {
        struct Case {
            name: &'static str,
            ok: bool,
        }
        let cases = [
            Case {
                name: "foo",
                ok: true,
            },
            Case {
                name: ".",
                ok: false,
            },
            Case {
                name: "..",
                ok: false,
            },
            Case {
                name: "/etc/passwd",
                ok: false,
            },
            Case {
                name: "foo/bar",
                ok: false,
            },
            Case {
                name: "foo/../../etc",
                ok: false,
            },
            Case {
                name: "/leading",
                ok: false,
            },
        ];
        for case in cases {
            let got = safe_remote_child("/pub", case.name);
            assert_eq!(got.is_ok(), case.ok, "remote name {:?}", case.name);
            if case.ok {
                assert_eq!(got.unwrap(), "/pub/foo");
            }
        }
    }

    #[test]
    fn download_jobs_must_not_contain_a_list_line_path() {
        let mut app = AppState {
            connected: true,
            local_cwd: "/tmp".into(),
            remote_cwd: "/pub".into(),
            remote_entries: vec![FileEntry {
                name: "file.bin".into(),
                path: "-rw-r--r-- 1 user group 123 Jan 01 file.bin".into(),
                kind: dd_ftp_core::EntryKind::File,
                size: 123,
                modified: None,
                permissions: None,
            }],
            selected_remote: 0,
            ..Default::default()
        };

        queue_download_selected(&mut app);

        let job = app.queue.pending.first().expect("download job queued");
        assert_eq!(job.remote_path, "/pub/file.bin");
        assert!(
            !job.remote_path.contains("-rw-"),
            "download remote_path must not be a LIST line, got {}",
            job.remote_path
        );
    }

    #[test]
    fn local_list_fills_size_and_modified() {
        let dir = std::env::temp_dir().join(format!(
            "dd_ftp_local_list_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("hello.txt");
        std::fs::write(&file, b"hello world").expect("write file");

        let entries = local_list(dir.to_str().expect("utf8 path"));
        let hello = entries
            .iter()
            .find(|e| e.name == "hello.txt")
            .expect("listed file");
        assert!(hello.size > 0);
        assert!(hello.modified.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
