use std::{io, path::Path, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dd_ftp_app::{reduce, Action, AppState, FocusPane};
use dd_ftp_core::{ConnectionInfo, FileEntry, Protocol, RemoteSession, TransferDirection, TransferJob};
use dd_ftp_protocols::SftpSession;
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
    loop {
        terminal.draw(|f| dd_ftp_ui::render(f, app))?;

        // Auto-process one queued transfer per tick when connected.
        if app.connected && !app.queue.pending.is_empty() && app.queue.active.is_empty() {
            process_next_transfer(app, session).await;
        }

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::F(1) {
                    reduce(app, Action::ToggleHelp);
                    continue;
                }

                if app.show_help {
                    // While help modal is open, allow Esc to close it.
                    if key.code == KeyCode::Esc {
                        reduce(app, Action::ToggleHelp);
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
                    KeyCode::Char('c') => {
                        let info = connection_info_from_env();

                        app.remote_cwd = info.initial_path.clone();
                        reduce(app, Action::Connect(info.clone()));

                        match session.connect(info).await {
                            Ok(_) => {
                                reduce(app, Action::SetConnected(true));

                                match session.list_dir(&app.remote_cwd).await {
                                    Ok(entries) => {
                                        reduce(app, Action::SetRemoteEntries(entries));
                                        reduce(app, Action::SetStatus(format!(
                                            "Connected via SFTP. Remote cwd: {}",
                                            app.remote_cwd
                                        )));
                                    }
                                    Err(err) => {
                                        reduce(app, Action::SetStatus(format!(
                                            "Connected but list_dir failed: {err}"
                                        )));
                                    }
                                }
                            }
                            Err(err) => {
                                reduce(app, Action::SetConnected(false));
                                reduce(app, Action::SetStatus(format!("Connect failed: {err}")));
                            }
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
                        // Manual trigger remains available.
                        process_next_transfer(app, session).await;
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

async fn process_next_transfer(app: &mut AppState, session: &mut SftpSession) {
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }

    if let Some(mut job) = app.queue.start_next() {
        let result = match job.direction {
            TransferDirection::Upload => session.upload(&job).await,
            TransferDirection::Download => session.download(&job).await,
        };

        match result {
            Ok(_) => {
                let name = match job.direction {
                    TransferDirection::Upload => "upload",
                    TransferDirection::Download => "download",
                };
                job.last_error = None;
                reduce(app, Action::MarkTransferCompleted(job));
                reduce(app, Action::SetStatus(format!("{name} complete")));

                // refresh views after transfer
                app.local_entries = local_list(&app.local_cwd);
                if let Ok(entries) = session.list_dir(&app.remote_cwd).await {
                    reduce(app, Action::SetRemoteEntries(entries));
                }
            }
            Err(err) => {
                job.last_error = Some(err.to_string());
                reduce(app, Action::MarkTransferFailed(job));
                reduce(app, Action::SetStatus(format!("Transfer failed: {err}")));
            }
        }
    } else {
        reduce(app, Action::SetStatus("Queue is empty".to_string()));
    }
}

fn connection_info_from_env() -> ConnectionInfo {
    let host = std::env::var("DD_FTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("DD_FTP_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(22);
    let username = std::env::var("DD_FTP_USER").unwrap_or_else(|_| "user".to_string());
    let password = std::env::var("DD_FTP_PASS").ok();
    let private_key = std::env::var("DD_FTP_KEY").ok();
    let initial_path = std::env::var("DD_FTP_PATH").unwrap_or_else(|_| "/".to_string());

    ConnectionInfo {
        name: "Env SFTP".to_string(),
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
