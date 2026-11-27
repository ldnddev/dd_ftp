//! Listing session for the window. Live connects use `SftpSession` or
//! `UnifiedFtpSession` on a worker thread. Tests use [`Backend::Fake`] and the
//! same commands.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, Protocol, RemoteSession};
use dd_ftp_ftp::UnifiedFtpSession;
use dd_ftp_protocols::{HostKeyOffer, SftpSession};

#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum FakeHostKey {
    Trusted,
    Ask { fingerprint: String, changed: bool },
}

#[derive(Clone, Debug)]
pub struct FakeTree {
    pub dirs: HashMap<String, Vec<FileEntry>>,
    /// File bytes keyed by the full remote path. Listings stay in `dirs`.
    pub files: HashMap<String, Vec<u8>>,
    pub host_key: FakeHostKey,
    /// Appended the second time a path is listed. Connect is the first list.
    pub extra_on_refresh: Option<FileEntry>,
    /// Transfer workers wait here until a test clears the flag or cancels.
    pub hold_transfers: bool,
}

impl FakeTree {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn empty() -> Self {
        let mut dirs = HashMap::new();
        dirs.insert("/".to_string(), Vec::new());
        Self {
            dirs,
            files: HashMap::new(),
            host_key: FakeHostKey::Trusted,
            extra_on_refresh: None,
            hold_transfers: false,
        }
    }

    pub fn dir_entry(&self, parent: &str, name: &str) -> Option<&FileEntry> {
        self.dirs
            .get(parent)
            .and_then(|rows| rows.iter().find(|entry| entry.name == name))
    }

    pub fn dir_names(&self, parent: &str) -> Vec<String> {
        self.dirs
            .get(parent)
            .map(|rows| rows.iter().map(|entry| entry.name.clone()).collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn bytes(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    pub fn put_bytes(&mut self, path: &str, bytes: &[u8]) {
        let parent = parent_remote(path);
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.is_empty() || name == "/" {
            return;
        }
        self.ensure_dir_chain(&parent);
        self.files.insert(path.to_string(), bytes.to_vec());
        let rows = self.dirs.entry(parent).or_default();
        if let Some(row) = rows.iter_mut().find(|entry| entry.name == name) {
            row.kind = EntryKind::File;
            row.size = bytes.len() as u64;
            row.path = path.to_string();
            row.modified = Some(Utc::now());
        } else {
            rows.push(FileEntry {
                name: name.to_string(),
                path: path.to_string(),
                kind: EntryKind::File,
                size: bytes.len() as u64,
                modified: Some(Utc::now()),
                permissions: None,
            });
        }
    }

    pub fn remove_bytes(&mut self, path: &str) {
        self.files.remove(path);
        let parent = parent_remote(path);
        let name = path.rsplit('/').next().unwrap_or(path);
        if let Some(rows) = self.dirs.get_mut(&parent) {
            rows.retain(|entry| entry.name != name);
        }
    }

    fn ensure_dir_chain(&mut self, path: &str) {
        if path.is_empty() {
            return;
        }
        self.dirs.entry("/".to_string()).or_default();
        if path == "/" {
            return;
        }
        let mut acc = String::new();
        for part in path.trim_matches('/').split('/') {
            if part.is_empty() {
                continue;
            }
            let parent = if acc.is_empty() {
                "/".to_string()
            } else {
                acc.clone()
            };
            acc = if parent == "/" {
                format!("/{part}")
            } else {
                format!("{parent}/{part}")
            };
            let rows = self.dirs.entry(parent).or_default();
            if !rows.iter().any(|entry| entry.name == part) {
                rows.push(FileEntry {
                    name: part.to_string(),
                    path: acc.clone(),
                    kind: EntryKind::Directory,
                    size: 0,
                    modified: Some(Utc::now()),
                    permissions: None,
                });
            }
            self.dirs.entry(acc.clone()).or_default();
        }
    }
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Backend {
    Live,
    Fake(Arc<Mutex<FakeTree>>),
}

impl Backend {
    #[cfg(test)]
    pub fn fake(tree: FakeTree) -> Self {
        Self::Fake(Arc::new(Mutex::new(tree)))
    }
}

enum LiveSession {
    Sftp(SftpSession),
    Ftp(Box<UnifiedFtpSession>),
}

impl LiveSession {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>> {
        match self {
            Self::Sftp(session) => session.list_dir(path).await,
            Self::Ftp(session) => session.list_dir(path).await,
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        match self {
            Self::Sftp(session) => session.disconnect().await,
            Self::Ftp(session) => session.disconnect().await,
        }
    }
}

struct FakeState {
    tree: Arc<Mutex<FakeTree>>,
    lists: HashMap<String, usize>,
    connected: bool,
}

impl FakeState {
    fn list(&mut self, path: &str) -> Result<Vec<FileEntry>> {
        let (mut rows, extra) = {
            let tree = self.tree.lock().unwrap_or_else(|err| err.into_inner());
            let rows = tree
                .dirs
                .get(path)
                .cloned()
                .with_context(|| format!("failed reading remote path: {path}"))?;
            (rows, tree.extra_on_refresh.clone())
        };
        let count = self.lists.entry(path.to_string()).or_insert(0);
        *count += 1;
        if *count == 2
            && let Some(extra) = extra
        {
            rows.push(extra);
        }
        Ok(rows)
    }
}

enum Cmd {
    Connect {
        generation: u64,
        info: ConnectionInfo,
        path: String,
    },
    List {
        generation: u64,
        path: String,
    },
    Disconnect {
        generation: u64,
    },
    Shutdown,
}

pub enum SessionEvent {
    HostKey {
        generation: u64,
        host: String,
        port: u16,
        fingerprint: String,
        changed: bool,
        reply: Sender<bool>,
    },
    Connected {
        generation: u64,
        path: String,
        entries: Vec<FileEntry>,
    },
    Listed {
        generation: u64,
        path: String,
        entries: Vec<FileEntry>,
    },
    Failed {
        generation: u64,
        message: String,
    },
    Disconnected {
        generation: u64,
    },
}

pub struct SessionClient {
    cmd_tx: Sender<Cmd>,
    evt_rx: Receiver<SessionEvent>,
}

impl SessionClient {
    pub fn spawn(backend: Backend) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dd-ftp-session".into())
            .spawn(move || worker(backend, cmd_rx, evt_tx))
            .expect("spawn session worker");
        Self { cmd_tx, evt_rx }
    }

    pub fn connect(&self, generation: u64, info: ConnectionInfo, path: String) {
        let _ = self.cmd_tx.send(Cmd::Connect {
            generation,
            info,
            path,
        });
    }

    pub fn list(&self, generation: u64, path: String) {
        let _ = self.cmd_tx.send(Cmd::List { generation, path });
    }

    pub fn disconnect(&self, generation: u64) {
        let _ = self.cmd_tx.send(Cmd::Disconnect { generation });
    }

    pub fn try_recv(&self) -> Result<SessionEvent, TryRecvError> {
        self.evt_rx.try_recv()
    }
}

impl Drop for SessionClient {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Shutdown);
    }
}

fn worker(backend: Backend, cmd_rx: Receiver<Cmd>, evt_tx: Sender<SessionEvent>) {
    let runtime = match backend {
        Backend::Live => Some(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("dd-ftp-net")
                .build()
                .expect("session runtime"),
        ),
        Backend::Fake(_) => None,
    };
    let mut live: Option<LiveSession> = None;
    let mut fake = match backend {
        Backend::Fake(tree) => Some(FakeState {
            tree,
            lists: HashMap::new(),
            connected: false,
        }),
        Backend::Live => None,
    };

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Shutdown => {
                drop_live(&runtime, &mut live);
                break;
            }
            Cmd::Disconnect { generation } => {
                drop_live(&runtime, &mut live);
                if let Some(fake) = fake.as_mut() {
                    fake.connected = false;
                }
                let _ = evt_tx.send(SessionEvent::Disconnected { generation });
            }
            Cmd::Connect {
                generation,
                info,
                path,
            } => {
                drop_live(&runtime, &mut live);
                if let Some(fake) = fake.as_mut() {
                    fake.connected = false;
                }
                let secret = info.password.clone();
                let outcome = if let Some(fake) = fake.as_mut() {
                    fake_connect(fake, &info, &path, generation, &evt_tx)
                        .map(|entries| (None, entries))
                } else {
                    let runtime = runtime.as_ref().expect("live runtime");
                    runtime
                        .block_on(live_connect(info.clone(), &path, generation, &evt_tx))
                        .map(|(session, entries)| (Some(session), entries))
                };
                match outcome {
                    Ok((session, entries)) => {
                        live = session;
                        if let Some(fake) = fake.as_mut() {
                            fake.connected = true;
                        }
                        if evt_tx
                            .send(SessionEvent::Connected {
                                generation,
                                path,
                                entries,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(err) => {
                        let message = redact(
                            &format!(
                                "Connect failed for {}@{}:{} via {} -> {err}",
                                info.username,
                                info.host,
                                info.port,
                                protocol_name(info.protocol)
                            ),
                            secret.as_deref(),
                        );
                        if evt_tx
                            .send(SessionEvent::Failed {
                                generation,
                                message,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Cmd::List { generation, path } => {
                let result = if let Some(fake) = fake.as_mut() {
                    if fake.connected {
                        fake.list(&path)
                    } else {
                        Err(anyhow!("not connected"))
                    }
                } else if let Some(session) = live.as_mut() {
                    let runtime = runtime.as_ref().expect("live runtime");
                    runtime.block_on(session.list_dir(&path))
                } else {
                    Err(anyhow!("not connected"))
                };
                let event = match result {
                    Ok(entries) => SessionEvent::Listed {
                        generation,
                        path,
                        entries,
                    },
                    Err(err) => SessionEvent::Failed {
                        generation,
                        message: format!("Remote list failed: {err}"),
                    },
                };
                if evt_tx.send(event).is_err() {
                    break;
                }
            }
        }
    }
}

fn drop_live(runtime: &Option<tokio::runtime::Runtime>, live: &mut Option<LiveSession>) {
    let Some(mut session) = live.take() else {
        return;
    };
    if let Some(runtime) = runtime {
        let _ = runtime.block_on(session.disconnect());
    }
}

fn fake_connect(
    fake: &mut FakeState,
    info: &ConnectionInfo,
    path: &str,
    generation: u64,
    evt_tx: &Sender<SessionEvent>,
) -> Result<Vec<FileEntry>> {
    let host_key = fake
        .tree
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .host_key
        .clone();
    if let FakeHostKey::Ask {
        fingerprint,
        changed,
    } = host_key
    {
        let (reply_tx, reply_rx) = mpsc::channel();
        evt_tx
            .send(SessionEvent::HostKey {
                generation,
                host: info.host.clone(),
                port: info.port,
                fingerprint,
                changed,
                reply: reply_tx,
            })
            .map_err(|_| anyhow!("host key prompt closed"))?;
        let accepted = reply_rx.recv().unwrap_or(false);
        if !accepted {
            anyhow::bail!("host key rejected for {}:{}", info.host, info.port);
        }
    }
    fake.list(path)
}

async fn live_connect(
    info: ConnectionInfo,
    path: &str,
    generation: u64,
    evt_tx: &Sender<SessionEvent>,
) -> Result<(LiveSession, Vec<FileEntry>)> {
    match info.protocol {
        Protocol::Sftp => {
            let mut session = SftpSession::default();
            let evt_tx = evt_tx.clone();
            session
                .connect_with_host_key_handler(info, move |offer: HostKeyOffer| {
                    let (reply_tx, reply_rx) = mpsc::channel();
                    if evt_tx
                        .send(SessionEvent::HostKey {
                            generation,
                            host: offer.host,
                            port: offer.port,
                            fingerprint: offer.fingerprint,
                            changed: offer.changed,
                            reply: reply_tx,
                        })
                        .is_err()
                    {
                        return false;
                    }
                    reply_rx.recv().unwrap_or(false)
                })
                .await?;
            let entries = session.list_dir(path).await?;
            Ok((LiveSession::Sftp(session), entries))
        }
        Protocol::Ftp | Protocol::Ftps => {
            let mut session = UnifiedFtpSession::new();
            session.connect(info).await?;
            let entries = session.list_dir(path).await?;
            Ok((LiveSession::Ftp(Box::new(session)), entries))
        }
    }
}

pub fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Sftp => "SFTP",
        Protocol::Ftp => "FTP",
        Protocol::Ftps => "FTPS",
    }
}

pub fn connected_status(protocol: Protocol, host: &str, username: &str, cwd: &str) -> String {
    let via = match protocol {
        Protocol::Ftp => "FTP (unencrypted)",
        Protocol::Ftps => "FTPS",
        Protocol::Sftp => "SFTP",
    };
    format!("Connected via {via} to {host} as {username} (cwd: {cwd})")
}

pub fn normalize_remote(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn parent_remote(path: &str) -> String {
    let path = if path.is_empty() { "/" } else { path };
    if path == "/" {
        return "/".to_string();
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(index) => trimmed[..index].to_string(),
    }
}

fn safe_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.starts_with('/') {
        return false;
    }
    !name.chars().any(|ch| ch.is_control())
}

pub fn join_remote(cwd: &str, name: &str) -> Result<String> {
    if !safe_component(name) {
        anyhow::bail!("path escapes directory");
    }
    let cwd = if cwd.is_empty() { "/" } else { cwd };
    let joined = if cwd == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), name)
    };
    let prefix = cwd.trim_end_matches('/');
    if prefix.is_empty() || prefix == "/" {
        return Ok(joined);
    }
    if joined == prefix || joined.starts_with(&format!("{prefix}/")) {
        Ok(joined)
    } else {
        anyhow::bail!("path escapes directory");
    }
}

pub fn redact(message: &str, secret: Option<&str>) -> String {
    match secret {
        Some(secret) if !secret.is_empty() && message.contains(secret) => {
            message.replace(secret, "<redacted>")
        }
        _ => message.to_string(),
    }
}

/// Same fields as the terminal `connection_info_from_env`, including removal of
/// `DD_FTP_PASS` after it is copied.
pub fn connection_from_env() -> ConnectionInfo {
    connection_from_lookup(
        |key| std::env::var(key).ok(),
        |key| {
            // SAFETY: same point as the terminal startup path. The window calls
            // this before it spawns the session thread, so DD_FTP_PASS is gone
            // before a child editor can inherit it.
            unsafe { std::env::remove_var(key) }
        },
    )
}

#[cfg(test)]
pub(crate) fn set_test_var(key: &str, value: &str) {
    // SAFETY: tests hold ENV_LOCK and restore the previous values before
    // releasing it. Nothing else in this process reads these keys.
    unsafe { std::env::set_var(key, value) }
}

#[cfg(test)]
pub(crate) fn remove_test_var(key: &str) {
    // SAFETY: see `set_test_var`.
    unsafe { std::env::remove_var(key) }
}

pub fn connection_from_lookup(
    mut get: impl FnMut(&str) -> Option<String>,
    mut remove: impl FnMut(&str),
) -> ConnectionInfo {
    let host = get("DD_FTP_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let name = get("DD_FTP_NAME").unwrap_or_else(|| host.clone());
    let port = get("DD_FTP_PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(22);
    let username = get("DD_FTP_USER").unwrap_or_else(|| "user".to_string());
    let password = get("DD_FTP_PASS");
    remove("DD_FTP_PASS");
    let private_key = get("DD_FTP_KEY");
    let initial_path = get("DD_FTP_PATH").unwrap_or_else(|| "/".to_string());
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

#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub fn entry(name: &str, directory: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        path: name.to_string(),
        kind: if directory {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        size: if directory { 0 } else { 1 },
        modified: None,
        permissions: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_lookup_matches_the_terminal_and_drops_the_password_var() {
        let mut removed = Vec::new();
        let info = connection_from_lookup(
            |key| match key {
                "DD_FTP_HOST" => Some("files.example".into()),
                "DD_FTP_NAME" => Some("Desk".into()),
                "DD_FTP_PORT" => Some("2222".into()),
                "DD_FTP_USER" => Some("ada".into()),
                "DD_FTP_PASS" => Some("s3cret-value".into()),
                "DD_FTP_KEY" => Some("/keys/id".into()),
                "DD_FTP_PATH" => Some("/docs".into()),
                _ => None,
            },
            |key| removed.push(key.to_string()),
        );
        assert_eq!(removed, vec!["DD_FTP_PASS".to_string()]);
        assert_eq!(info.name, "Desk");
        assert_eq!(info.host, "files.example");
        assert_eq!(info.port, 2222);
        assert_eq!(info.username, "ada");
        assert_eq!(info.password.as_deref(), Some("s3cret-value"));
        assert_eq!(info.private_key.as_deref(), Some("/keys/id"));
        assert_eq!(info.initial_path, "/docs");
        assert_eq!(info.protocol, Protocol::Sftp);
        assert!(!format!("{info:?}").contains("s3cret-value"));
    }

    #[test]
    fn unset_env_uses_the_terminal_defaults() {
        let info = connection_from_lookup(|_| None, |_| {});
        assert_eq!(info.host, "127.0.0.1");
        assert_eq!(info.name, "127.0.0.1");
        assert_eq!(info.port, 22);
        assert_eq!(info.username, "user");
        assert_eq!(info.password, None);
        assert_eq!(info.initial_path, "/");
        assert_eq!(info.protocol, Protocol::Sftp);
    }

    #[test]
    fn status_names_plain_ftp_as_unencrypted() {
        assert!(connected_status(Protocol::Ftp, "h", "u", "/").contains("FTP (unencrypted)"));
        let ftps = connected_status(Protocol::Ftps, "h", "u", "/");
        assert!(ftps.contains("FTPS"));
        assert!(!ftps.contains("unencrypted"));
        assert!(connected_status(Protocol::Sftp, "h", "u", "/docs").contains("cwd: /docs"));
    }

    #[test]
    fn parent_and_join_stay_inside_the_remote_root() {
        assert_eq!(parent_remote("/"), "/");
        assert_eq!(parent_remote("/docs"), "/");
        assert_eq!(parent_remote("/docs/nested"), "/docs");
        assert_eq!(join_remote("/", "docs").unwrap(), "/docs");
        assert_eq!(join_remote("/docs", "a.txt").unwrap(), "/docs/a.txt");
        assert!(join_remote("/docs", "..").is_err());
        assert!(join_remote("/docs", "a/b").is_err());
        assert!(join_remote("/docs", "a\nb").is_err());
    }

    #[test]
    fn connection_from_env_removes_dd_ftp_pass() {
        let _guard = ENV_LOCK.lock().unwrap();
        let keys = [
            "DD_FTP_HOST",
            "DD_FTP_NAME",
            "DD_FTP_PORT",
            "DD_FTP_USER",
            "DD_FTP_PASS",
            "DD_FTP_KEY",
            "DD_FTP_PATH",
        ];
        let saved: Vec<(String, Option<String>)> = keys
            .iter()
            .map(|key| (key.to_string(), std::env::var(key).ok()))
            .collect();
        set_test_var("DD_FTP_HOST", "env.example");
        set_test_var("DD_FTP_NAME", "Env");
        set_test_var("DD_FTP_PORT", "2200");
        set_test_var("DD_FTP_USER", "ada");
        set_test_var("DD_FTP_PASS", "s3cret-value");
        set_test_var("DD_FTP_KEY", "/keys/id");
        set_test_var("DD_FTP_PATH", "/inbox");
        let info = connection_from_env();
        assert!(std::env::var("DD_FTP_PASS").is_err());
        assert_eq!(info.password.as_deref(), Some("s3cret-value"));
        assert_eq!(info.host, "env.example");
        assert_eq!(info.initial_path, "/inbox");
        assert!(!format!("{info:?}").contains("s3cret-value"));
        for (key, value) in saved {
            match value {
                Some(value) => set_test_var(&key, &value),
                None => remove_test_var(&key),
            }
        }
    }

    #[test]
    fn redact_removes_the_password_and_leaves_other_text() {
        assert_eq!(
            redact("bad s3cret-value here", Some("s3cret-value")),
            "bad <redacted> here"
        );
        assert_eq!(redact("plain", None), "plain");
        assert_eq!(redact("plain", Some("")), "plain");
    }
}
