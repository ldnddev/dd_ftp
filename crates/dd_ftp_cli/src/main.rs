use std::{io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dd_ftp_app::{reduce, Action, AppState};
use dd_ftp_core::{ConnectionInfo, FileEntry, Protocol, RemoteSession};
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

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Tab => reduce(app, Action::FocusNextPane),
                    KeyCode::Up => reduce(app, Action::SelectUp),
                    KeyCode::Down => reduce(app, Action::SelectDown),
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
                    _ => {}
                }
            }
        }
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
