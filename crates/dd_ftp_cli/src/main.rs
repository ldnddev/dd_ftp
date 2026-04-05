use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    path::Path,
    time::Duration,
};

use tokio::sync::mpsc;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dd_ftp_app::{reduce, Action, AppState, FocusPane};
use dd_ftp_core::{ConnectionInfo, FileEntry, Protocol, RemoteSession, TransferDirection, TransferJob};
use uuid::Uuid;
use dd_ftp_ftp::{FtpVariant, UnifiedFtpSession};
use dd_ftp_protocols::SftpSession;
use dd_ftp_storage::{SecretStore, SiteManager};
use ratatui::{backend::CrosstermBackend, Terminal};

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState::default();
    app.local_entries = local_list(".");

    // Seed quick-connect from env for first run, then load bookmarks.
    app.quick_connect = connection_info_from_env();

    run_keyring_health_check(&mut app);

    if let Ok(cfg) = SiteManager::load_or_default() {
        if !cfg.sites.is_empty() {
            reduce(&mut app, Action::SetBookmarks(cfg.sites.clone()));
            let selected_idx = cfg.default_site.unwrap_or(0).min(cfg.sites.len().saturating_sub(1));
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    session: &mut SftpSession,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WorkerMessage>();
    let mut cancel_flags: Vec<Arc<AtomicBool>> = Vec::new();

    loop {
        terminal.draw(|f| dd_ftp_ui::render(f, app))?;

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
                    handle_worker_result(app, session, result).await;
                }
            }
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
            reduce(app, Action::SetStatus(format!("Processing {:?}: {}", job.direction, job.remote_path)));

            let mut info = app
                .active_connection
                .clone()
                .unwrap_or_else(connection_info_from_env);

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
                                    TransferDirection::Upload => worker_session
                                        .upload_with_progress(&job, cancel.clone(), progress_tx)
                                        .await,
                                    TransferDirection::Download => worker_session
                                        .download_with_progress(&job, cancel.clone(), progress_tx)
                                        .await,
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
                            Protocol::Sftp => unreachable!(),
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
            if let Event::Key(key) = event::read()? {
                if app.error_modal.is_some() {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => reduce(app, Action::ClearError),
                        _ => {}
                    }
                    continue;
                }

                if key.code == KeyCode::Char('k')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    run_keyring_health_check(app);
                    continue;
                }

                if key.code == KeyCode::F(1) {
                    reduce(app, Action::ToggleHelp);
                    continue;
                }

                if app.show_help {
                    if key.code == KeyCode::Esc {
                        reduce(app, Action::ToggleHelp);
                    }
                    continue;
                }

                if app.show_quick_connect {
                    match key.code {
                        KeyCode::Esc => reduce(app, Action::ToggleQuickConnect),
                        KeyCode::Tab => reduce(app, Action::QuickConnectNextField),
                        KeyCode::BackTab => reduce(app, Action::QuickConnectPrevField),
                        KeyCode::Left => reduce(app, Action::QuickConnectSetProtocolPrev),
                        KeyCode::Right => reduce(app, Action::QuickConnectSetProtocolNext),
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
                            connect_with_info(app, session, info).await;
                            reduce(app, Action::ToggleQuickConnect);
                        }
                        KeyCode::Char('s')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
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
                        KeyCode::Char('j') | KeyCode::Down => reduce(app, Action::SelectNextBookmark),
                        KeyCode::Char('k') | KeyCode::Up => reduce(app, Action::SelectPrevBookmark),
                        KeyCode::Enter => {
                            if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                                let bm = hydrate_password_from_keyring(app, bm, "bookmark-load");
                                reduce(app, Action::QuickConnectSetFromBookmark(bm));
                                reduce(app, Action::ToggleBookmarks);
                                reduce(app, Action::ToggleQuickConnect);
                            }
                        }
                        KeyCode::Char('c') => {
                            if let Some(mut bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
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
                                connect_with_info(app, session, bm).await;
                                reduce(app, Action::ToggleBookmarks);
                            }
                        }
                        KeyCode::Char('d') => {
                            delete_selected_bookmark(app);
                        }
                        KeyCode::Char('e') => {
                            if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                                let bm = hydrate_password_from_keyring(app, bm, "bookmark-edit");
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
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Tab => reduce(app, Action::FocusNextPane),
                    KeyCode::Char('1') => {
                        app.focus = FocusPane::Local;
                        reduce(app, Action::SetStatus("Focus: Local".to_string()));
                    }
                    KeyCode::Char('2') => {
                        app.focus = FocusPane::Remote;
                        reduce(app, Action::SetStatus("Focus: Remote".to_string()));
                    }
                    KeyCode::Char('3') => {
                        app.focus = FocusPane::Queue;
                        reduce(app, Action::SetStatus("Focus: Queue".to_string()));
                    }
                    KeyCode::Up | KeyCode::Char('k') => reduce(app, Action::SelectUp),
                    KeyCode::Down | KeyCode::Char('j') => reduce(app, Action::SelectDown),
                    KeyCode::Char('l') => {
                        navigate_into_directory(app, session).await;
                    }
                    KeyCode::Char('h') => {
                        navigate_parent_directory(app, session).await;
                    },
                    KeyCode::Char('r') => {
                        app.local_entries = local_list(&app.local_cwd);

                        if app.connected {
                            match session.list_dir(&app.remote_cwd).await {
                                Ok(entries) => {
                                    reduce(app, Action::SetRemoteEntries(entries));
                                    reduce(app, Action::SetStatus("Refreshed local + remote listing".to_string()));
                                }
                                Err(err) => {
                                    reduce(app, Action::SetStatus(format!("Remote refresh failed: {err}")));
                                }
                            }
                        } else {
                            reduce(app, Action::SetStatus("Refreshed local listing".to_string()));
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
                            disconnect_session(app, session).await;
                        } else {
                            let mut info = selected_or_quick_connect(app);
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
                            connect_with_info(app, session, info).await;
                        }
                    }
                    KeyCode::Char('u') => {
                        if !app.connected {
                            reduce(app, Action::SetStatus("Not connected".to_string()));
                            continue;
                        }

                        let selected = app.local_entries.get(app.selected_local).cloned();
                        if let Some(local) = selected {
                            if local.kind == dd_ftp_core::EntryKind::Directory {
                                reduce(app, Action::SetStatus("Select a local file to upload".to_string()));
                                continue;
                            }

                            let remote_target = format!("{}/{}", app.remote_cwd.trim_end_matches('/'), local.name);
                            let job = TransferJob::new(local.path, remote_target, TransferDirection::Upload);
                            reduce(app, Action::QueueTransfer(job));
                        }
                    }
                    KeyCode::Char('d') => {
                        if !app.connected {
                            reduce(app, Action::SetStatus("Not connected".to_string()));
                            continue;
                        }

                        let selected = app.remote_entries.get(app.selected_remote).cloned();
                        if let Some(remote) = selected {
                            if remote.kind == dd_ftp_core::EntryKind::Directory {
                                reduce(app, Action::SetStatus("Select a remote file to download".to_string()));
                                continue;
                            }

                            let local_target = format!("{}/{}", app.local_cwd.trim_end_matches('/'), remote.name);
                            let job = TransferJob::new(local_target, remote.path, TransferDirection::Download);
                            reduce(app, Action::QueueTransfer(job));
                        }
                    }
                    KeyCode::Char('x') => {
                        reduce(app, Action::SetStatus("Worker auto-processes queue".to_string()));
                    }
                    KeyCode::Char('X') => {
                        reduce(app, Action::ClearPendingTransfers);
                    }
                    KeyCode::Char('R') => {
                        reduce(app, Action::RetryLastFailed);
                    }
                    KeyCode::Char('C') => {
                        if app.worker_running {
                            app.worker_cancel_requested = true;
                            for flag in &cancel_flags {
                                flag.store(true, Ordering::Relaxed);
                            }
                            reduce(app, Action::SetStatus("Cancel requested for active transfers".to_string()));
                        } else {
                            reduce(app, Action::SetStatus("No active transfer to cancel".to_string()));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn navigate_into_directory(app: &mut AppState, session: &mut SftpSession) {
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            if let Some(entry) = app.local_entries.get(app.selected_local).cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.local_cwd = entry.path;
                    app.local_entries = local_list(&app.local_cwd);
                    reduce(app, Action::SetStatus(format!("Local cwd: {}", app.local_cwd)));
                }
            }
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }

            if let Some(entry) = app.remote_entries.get(app.selected_remote).cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.remote_cwd = entry.path;
                    match session.list_dir(&app.remote_cwd).await {
                        Ok(entries) => {
                            reduce(app, Action::SetRemoteEntries(entries));
                            reduce(app, Action::SetStatus(format!("Remote cwd: {}", app.remote_cwd)));
                        }
                        Err(err) => {
                            reduce(app, Action::SetStatus(format!("Remote enter failed: {err}")));
                        }
                    }
                }
            }
        }
        dd_ftp_app::FocusPane::Queue => {}
    }
}

async fn navigate_parent_directory(app: &mut AppState, session: &mut SftpSession) {
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let parent = Path::new(&app.local_cwd)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| app.local_cwd.clone());

            app.local_cwd = parent;
            app.local_entries = local_list(&app.local_cwd);
            reduce(app, Action::SetStatus(format!("Local cwd: {}", app.local_cwd)));
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }

            let parent = Path::new(&app.remote_cwd)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "/".to_string());

            app.remote_cwd = parent;
            match session.list_dir(&app.remote_cwd).await {
                Ok(entries) => {
                    reduce(app, Action::SetRemoteEntries(entries));
                    reduce(app, Action::SetStatus(format!("Remote cwd: {}", app.remote_cwd)));
                }
                Err(err) => {
                    reduce(app, Action::SetStatus(format!("Remote parent failed: {err}")));
                }
            }
        }
        dd_ftp_app::FocusPane::Queue => {}
    }
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

async fn handle_worker_result(app: &mut AppState, session: &mut SftpSession, mut msg: WorkerResult) {
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

            app.local_entries = local_list(&app.local_cwd);
            if app.connected {
                if let Ok(entries) = session.list_dir(&app.remote_cwd).await {
                    reduce(app, Action::SetRemoteEntries(entries));
                }
            }
        }
        Err(err) => {
            msg.job.last_error = Some(err.to_string());
            reduce(app, Action::MarkTransferFailed(msg.job));
            reduce(app, Action::SetStatus(format!("Transfer failed: {err}")));
        }
    }
}

async fn disconnect_session(app: &mut AppState, session: &mut SftpSession) {
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

async fn connect_with_info(app: &mut AppState, session: &mut SftpSession, info: ConnectionInfo) {
    if info.name.trim().is_empty() {
        reduce(app, Action::SetStatus("Connect failed: label/name is required".to_string()));
        return;
    }
    if info.host.trim().is_empty() {
        reduce(app, Action::SetStatus("Connect failed: host is required".to_string()));
        return;
    }
    if info.username.trim().is_empty() {
        reduce(app, Action::SetStatus("Connect failed: username is required".to_string()));
        return;
    }
    if info.port == 0 {
        reduce(app, Action::SetStatus("Connect failed: port must be > 0".to_string()));
        return;
    }

    app.remote_cwd = if info.initial_path.trim().is_empty() {
        "/".to_string()
    } else {
        info.initial_path.clone()
    };
    reduce(app, Action::Connect(info.clone()));

    let list_target = app.remote_cwd.clone();
    let result = connect_and_list_by_protocol(session, &info, &list_target).await;

    match result {
        Ok(entries) => {
            reduce(app, Action::SetConnected(true));
            reduce(app, Action::SetRemoteEntries(entries));
            app.active_connection = Some(info.clone());
            reduce(app, Action::SetStatus(format!(
                "Connected via {:?} to {} as {} (cwd: {})",
                info.protocol,
                info.host,
                info.username,
                app.remote_cwd
            )));
        }
        Err(err) => {
            reduce(app, Action::SetConnected(false));
            reduce(app, Action::SetStatus(format!(
                "Connect failed for {}@{}:{} via {:?} -> {err}",
                info.username,
                info.host,
                info.port,
                info.protocol
            )));
        }
    }
}

async fn connect_and_list_by_protocol(
    sftp_session: &mut SftpSession,
    info: &ConnectionInfo,
    path: &str,
) -> Result<Vec<FileEntry>> {
    match info.protocol {
        Protocol::Sftp => {
            sftp_session.connect(info.clone()).await?;
            sftp_session.list_dir(path).await
        }
        Protocol::Ftp | Protocol::Ftps => {
            let mut unified = UnifiedFtpSession::new();
            let variant = match info.protocol {
                Protocol::Ftp => FtpVariant::Ftp,
                Protocol::Ftps => FtpVariant::Ftps,
                Protocol::Sftp => unreachable!(),
            };
            unified.connect(variant, info.clone()).await?;
            let entries = unified.list_dir(variant, path).await?;
            unified.disconnect().await.ok();
            Ok(entries)
        }
    }
}

fn selected_or_quick_connect(app: &mut AppState) -> ConnectionInfo {
    if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
        hydrate_password_from_keyring(app, bm, "selected-bookmark")
    } else {
        app.quick_connect.clone()
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
                Action::SetStatus("Keyring backend detected: password persistence enabled".to_string()),
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
        if let Err(err) = SecretStore::save_password(
            &info.name,
            &info.username,
            &info.host,
            info.port,
            password,
        ) {
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
                reduce(app, Action::SetStatus(format!("Save bookmark failed: {err}")));
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
                reduce(app, Action::SetStatus(format!("Save bookmark failed: {err}")));
            }
        }
    }
}

fn delete_selected_bookmark(app: &mut AppState) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    if cfg.sites.is_empty() {
        reduce(app, Action::SetStatus("No bookmarks to delete".to_string()));
        return;
    }

    if app.selected_bookmark >= cfg.sites.len() {
        reduce(app, Action::SetStatus("Invalid bookmark selection".to_string()));
        return;
    }

    let removed = cfg.sites.remove(app.selected_bookmark);
    let _ = SecretStore::delete_password(&removed.name, &removed.username, &removed.host, removed.port);

    if let Some(default_idx) = cfg.default_site {
        cfg.default_site = if cfg.sites.is_empty() {
            None
        } else if default_idx == app.selected_bookmark {
            Some(0)
        } else if default_idx > app.selected_bookmark {
            Some(default_idx - 1)
        } else {
            Some(default_idx)
        };
    }

    match SiteManager::save_to_default_path(&cfg) {
        Ok(_) => {
            reduce(app, Action::SetBookmarks(cfg.sites));
            reduce(app, Action::SetStatus(format!("Deleted bookmark: {}", removed.name)));
        }
        Err(err) => {
            reduce(app, Action::SetStatus(format!("Delete bookmark failed: {err}")));
        }
    }
}

fn set_default_bookmark(app: &mut AppState) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    if cfg.sites.is_empty() {
        reduce(app, Action::SetStatus("No bookmarks to set as default".to_string()));
        return;
    }

    if app.selected_bookmark >= cfg.sites.len() {
        reduce(app, Action::SetStatus("Invalid bookmark selection".to_string()));
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
            reduce(app, Action::SetStatus("Default bookmark updated".to_string()));
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

fn local_list(path: &str) -> Vec<FileEntry> {
    let mut out = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let kind = if meta.is_dir() {
                    dd_ftp_core::EntryKind::Directory
                } else {
                    dd_ftp_core::EntryKind::File
                };

                out.push(FileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().to_string_lossy().to_string(),
                    kind,
                    size: meta.len(),
                    modified: None,
                    permissions: None,
                });
            }
        }
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}
