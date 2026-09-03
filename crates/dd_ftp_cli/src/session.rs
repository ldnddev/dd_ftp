use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::Result;
use dd_ftp_app::{
    reduce, Action, AppState, ChoicePromptKind, HostKeyView, OverwritePolicy, OverwritePrompt,
    PendingFile, PromptKind, SelectPolicy, TextPromptKind, Toast,
};
use dd_ftp_core::{
    ConnectionInfo, FileEntry, Protocol, RemoteSession, TransferDirection, TransferJob,
};
use dd_ftp_ftp::UnifiedFtpSession;
use dd_ftp_protocols::SftpSession;
use tokio::{sync::mpsc, task::JoinHandle};

use crate::paths::{local_list, parent_remote_path, safe_local_child, safe_remote_child};

#[allow(clippy::large_enum_variant)]
pub(crate) enum SessionHandle {
    Sftp(SftpSession),
    Ftp(UnifiedFtpSession),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FsKind {
    CreateFile,
    CreateFolder,
    Rename,
    Delete,
    Chmod,
}

pub(crate) enum InFlight {
    Connect {
        generation: u64,
    },
    List {
        generation: u64,
        path: String,
    },
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
pub(crate) enum ConnectOk {
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
pub(crate) enum IoMessage {
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
    ScanItem {
        generation: u64,
        file: PendingFile,
    },
    ScanDone {
        generation: u64,
    },
    ScanError {
        generation: u64,
        error: String,
    },
}

/// CLI-owned sockets and in-flight IO. No `dyn RemoteSession`.
pub(crate) struct Runtime {
    pub generation: u64,
    pub handle: Option<SessionHandle>,
    pub cancel_flags: Vec<Arc<AtomicBool>>,
    pub worker_handles: Vec<JoinHandle<()>>,
    pub in_flight: Option<InFlight>,
    pub pending_scan: VecDeque<PendingFile>,
    pub io_tx: mpsc::UnboundedSender<IoMessage>,
    pub park: Arc<Mutex<Option<SessionHandle>>>,
    pub list_select: SelectPolicy,
    pub list_ok_status: Option<String>,
    pub list_err_prefix: String,
    pub fs_remote: bool,
    pub fs_ok_status: String,
    pub overwrite_policy: OverwritePolicy,
    pub drain_list: bool,
    pub drain_mkdir: bool,
    pub mkdir_queue: VecDeque<String>,
    pub worker_active_count: usize,
}

impl Runtime {
    pub fn new(io_tx: mpsc::UnboundedSender<IoMessage>) -> Self {
        Self {
            generation: 0,
            handle: None,
            cancel_flags: Vec::new(),
            worker_handles: Vec::new(),
            in_flight: None,
            pending_scan: VecDeque::new(),
            io_tx,
            park: Arc::new(Mutex::new(None)),
            list_select: SelectPolicy::PreserveName,
            list_ok_status: None,
            list_err_prefix: "Remote list failed".to_string(),
            fs_remote: false,
            fs_ok_status: String::new(),
            overwrite_policy: OverwritePolicy::Ask,
            drain_list: false,
            drain_mkdir: false,
            mkdir_queue: VecDeque::new(),
            worker_active_count: 0,
        }
    }

    pub fn sync_worker_view(&self, app: &mut AppState) {
        reduce(
            app,
            Action::SetWorkerView {
                active_count: self.worker_active_count,
                running: self.worker_active_count > 0,
                cancel_requested: app.worker_cancel_requested,
            },
        );
    }

    pub fn request_list(
        &mut self,
        app: &mut AppState,
        path: String,
        select: SelectPolicy,
        drain: bool,
    ) {
        if io_busy(self) {
            return;
        }
        self.list_select = select;
        self.drain_list = drain;
        let gen = self.generation;
        self.in_flight = Some(InFlight::List {
            generation: gen,
            path: path.clone(),
        });
        if !drain {
            reduce(app, Action::SetStatus(format!("Listing {path}...")));
        }

        let handle = self.handle.take();
        let io_tx = self.io_tx.clone();
        let park = self.park.clone();

        tokio::spawn(async move {
            let mut handle = handle;
            let path_msg = path.clone();
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                match &mut handle {
                    Some(SessionHandle::Sftp(s)) => s.list_dir(&path).await,
                    Some(SessionHandle::Ftp(f)) => f.list_dir(&path).await,
                    None => Err(anyhow::anyhow!("not connected")),
                }
            })
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("list timed out")));

            if let Some(h) = handle {
                if let Ok(mut g) = park.lock() {
                    *g = Some(h);
                }
            }
            let _ = io_tx.send(IoMessage::ListDone {
                generation: gen,
                path: path_msg,
                result,
            });
        });
    }

    pub fn request_fs(&mut self, app: &mut AppState, kind: FsKind, path: String) {
        if io_busy(self) {
            return;
        }
        self.drain_mkdir = true;
        self.begin_fs(app, kind, true, format!("Creating folder: {path}"));
        self.spawn_remote_fs(kind, move |handle| async move {
            let mut handle = handle;
            let result = match &mut handle {
                Some(SessionHandle::Sftp(s)) => s.create_dir(&path).await,
                Some(SessionHandle::Ftp(f)) => f.create_dir(&path).await,
                None => Err(anyhow::anyhow!("not connected")),
            };
            (handle, result)
        });
    }

    pub fn begin_fs(&mut self, app: &mut AppState, kind: FsKind, remote: bool, ok_status: String) {
        self.fs_remote = remote;
        self.fs_ok_status = ok_status;
        self.in_flight = Some(InFlight::Fs {
            generation: self.generation,
            kind,
        });
        let status = match kind {
            FsKind::CreateFile | FsKind::CreateFolder => "Creating…",
            FsKind::Rename => "Renaming…",
            FsKind::Delete => "Deleting…",
            FsKind::Chmod => "Setting permissions…",
        };
        reduce(app, Action::SetStatus(status.to_string()));
    }

    pub fn spawn_local_fs(&self, kind: FsKind, work: impl FnOnce() -> Result<()> + Send + 'static) {
        let gen = self.generation;
        let io_tx = self.io_tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = work();
            let _ = io_tx.send(IoMessage::FsDone {
                generation: gen,
                kind,
                result,
            });
        });
    }

    pub fn spawn_remote_fs<F, Fut>(&mut self, kind: FsKind, work: F)
    where
        F: FnOnce(Option<SessionHandle>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = (Option<SessionHandle>, Result<()>)> + Send,
    {
        let gen = self.generation;
        let io_tx = self.io_tx.clone();
        let park = self.park.clone();
        let handle = self.handle.take();
        tokio::spawn(async move {
            let (handle, result) = work(handle).await;
            if let Some(h) = handle {
                if let Ok(mut g) = park.lock() {
                    *g = Some(h);
                }
            }
            let _ = io_tx.send(IoMessage::FsDone {
                generation: gen,
                kind,
                result,
            });
        });
    }
}

pub(crate) fn io_busy(runtime: &Runtime) -> bool {
    runtime.in_flight.is_some()
}

pub(crate) fn drain_busy(runtime: &Runtime) -> bool {
    !runtime.pending_scan.is_empty()
        || matches!(
            runtime.in_flight,
            Some(InFlight::Scan { .. } | InFlight::List { .. })
        )
}

pub(crate) fn clear_scan_state(runtime: &mut Runtime) {
    runtime.pending_scan.clear();
    runtime.overwrite_policy = OverwritePolicy::Ask;
    runtime.mkdir_queue.clear();
    runtime.drain_list = false;
    runtime.drain_mkdir = false;
}

fn bump_generation(runtime: &mut Runtime) {
    runtime.generation = runtime.generation.wrapping_add(1);
    clear_scan_state(runtime);
}

async fn wait_worker_handles(handles: &mut Vec<JoinHandle<()>>) {
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        for handle in handles.drain(..) {
            let _ = handle.await;
        }
    })
    .await;
    handles.clear();
}

pub(crate) async fn bump_generation_drop_workers(app: &mut AppState, runtime: &mut Runtime) {
    bump_generation(runtime);
    for flag in runtime.cancel_flags.iter() {
        flag.store(true, Ordering::Relaxed);
    }
    runtime.worker_active_count = 0;
    reduce(
        app,
        Action::SetWorkerView {
            active_count: 0,
            running: false,
            cancel_requested: true,
        },
    );
    let active = std::mem::take(&mut app.queue.active);
    for job in active {
        reduce(app, Action::MarkTransferCancelled(job));
    }
    reduce(app, Action::ClearPendingTransfers);
    wait_worker_handles(&mut runtime.worker_handles).await;
    runtime.cancel_flags.clear();
}

fn take_parked(park: &Arc<Mutex<Option<SessionHandle>>>) -> Option<SessionHandle> {
    park.lock().ok().and_then(|mut g| g.take())
}

fn drop_ui_sessions(runtime: &mut Runtime) {
    runtime.handle = None;
    let _ = take_parked(&runtime.park);
}

pub(crate) fn accept_host_key(runtime: &mut Runtime) {
    if let Some(InFlight::HostKey { generation, reply }) = runtime.in_flight.take() {
        let _ = reply.send(true);
        runtime.in_flight = Some(InFlight::Connect { generation });
    }
}

pub(crate) fn reject_host_key(runtime: &mut Runtime) {
    if let Some(InFlight::HostKey { generation, reply }) = runtime.in_flight.take() {
        let _ = reply.send(false);
        runtime.in_flight = Some(InFlight::Connect { generation });
    }
}

pub(crate) fn list_remote(
    app: &mut AppState,
    runtime: &mut Runtime,
    path: String,
    select: SelectPolicy,
) {
    runtime.request_list(app, path, select, false);
}

pub(crate) fn relist_remote(app: &mut AppState, runtime: &mut Runtime) {
    runtime.list_ok_status = runtime.fs_ok_status.clone().into();
    runtime.list_err_prefix = "Remote list failed".to_string();
    list_remote(
        app,
        runtime,
        app.remote_cwd.clone(),
        SelectPolicy::PreserveName,
    );
}

pub(crate) fn request_list(
    app: &mut AppState,
    runtime: &mut Runtime,
    path: String,
    select: SelectPolicy,
    drain: bool,
) {
    runtime.request_list(app, path, select, drain);
}

pub(crate) fn request_fs(app: &mut AppState, runtime: &mut Runtime, kind: FsKind, path: String) {
    runtime.request_fs(app, kind, path);
}

pub(crate) fn handle_io_message(app: &mut AppState, runtime: &mut Runtime, msg: IoMessage) {
    match msg {
        IoMessage::ConnectDone { generation, result } => {
            let matches_inflight = matches!(
                runtime.in_flight,
                Some(InFlight::Connect { generation: g } | InFlight::HostKey { generation: g, .. })
                    if g == generation
            );
            if generation != runtime.generation || !matches_inflight {
                return;
            }
            runtime.in_flight = None;
            match result {
                Ok(ConnectOk::Sftp {
                    info,
                    session: sftp,
                    entries,
                }) => {
                    runtime.handle = Some(SessionHandle::Sftp(sftp));
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
                    runtime.handle = Some(SessionHandle::Ftp(ftp));
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
                    drop_ui_sessions(runtime);
                    reduce(app, Action::SetConnected(false));
                    reduce(app, Action::ShowError(err.to_string()));
                }
            }
            maybe_resume_drain(app, runtime);
        }
        IoMessage::ListDone {
            generation,
            path,
            result,
        } => {
            let parked = take_parked(&runtime.park);
            if generation != runtime.generation {
                return;
            }
            if runtime.handle.is_none() {
                runtime.handle = parked;
            }
            let matches_inflight = matches!(
                &runtime.in_flight,
                Some(InFlight::List { generation: g, path: p })
                    if *g == generation && *p == path
            );
            if !matches_inflight {
                return;
            }
            runtime.in_flight = None;
            if runtime.drain_list {
                runtime.drain_list = false;
                handle_drain_list_result(app, runtime, result);
                return;
            }
            if !runtime.pending_scan.is_empty() {
                maybe_resume_drain(app, runtime);
                return;
            }
            match result {
                Ok(entries) => {
                    let select = runtime.list_select;
                    reduce(app, Action::SetRemoteEntries { entries, select });
                    if let Some(status) = runtime.list_ok_status.take() {
                        reduce(app, Action::SetStatus(status));
                    }
                }
                Err(err) => {
                    reduce(
                        app,
                        Action::ShowError(format!("{}: {err}", runtime.list_err_prefix)),
                    );
                }
            }
        }
        IoMessage::FsDone {
            generation,
            kind,
            result,
        } => {
            let parked = take_parked(&runtime.park);
            if generation != runtime.generation {
                return;
            }
            if runtime.handle.is_none() {
                runtime.handle = parked;
            }
            let matches_inflight = matches!(
                runtime.in_flight,
                Some(InFlight::Fs { generation: g, kind: k }) if g == generation && k == kind
            );
            if !matches_inflight {
                return;
            }
            runtime.in_flight = None;
            if runtime.drain_mkdir {
                runtime.drain_mkdir = false;
                handle_drain_mkdir_result(app, runtime, result);
                return;
            }
            match result {
                Ok(()) => {
                    let status = runtime.fs_ok_status.clone();
                    reduce(app, Action::SetStatus(status));
                    if !runtime.pending_scan.is_empty() {
                        maybe_resume_drain(app, runtime);
                    } else if runtime.fs_remote {
                        if app.connected {
                            relist_remote(app, runtime);
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
                    maybe_resume_drain(app, runtime);
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
            if generation != runtime.generation
                || matches!(runtime.in_flight, Some(InFlight::HostKey { .. }))
                || !matches!(
                    runtime.in_flight,
                    Some(InFlight::Connect { generation: g }) if g == generation
                )
            {
                let _ = reply.send(false);
                return;
            }
            runtime.in_flight = Some(InFlight::HostKey { generation, reply });
            app.host_key = Some(HostKeyView {
                host,
                port,
                fingerprint,
                changed,
            });
            reduce(app, Action::ShowChoicePrompt(ChoicePromptKind::HostKey));
        }
        IoMessage::ScanItem { generation, file } => {
            if generation != runtime.generation {
                return;
            }
            let matches_inflight = matches!(
                runtime.in_flight,
                Some(InFlight::Scan { generation: g }) if g == generation
            );
            if !matches_inflight {
                return;
            }
            if app.worker_cancel_requested {
                enqueue_pending_file(app, file);
                return;
            }
            apply_scan_item(&mut runtime.pending_scan, file);
        }
        IoMessage::ScanDone { generation } => {
            if generation != runtime.generation {
                return;
            }
            let matches_inflight = matches!(
                runtime.in_flight,
                Some(InFlight::Scan { generation: g }) if g == generation
            );
            if !matches_inflight {
                return;
            }
            runtime.in_flight = None;
            if app.worker_cancel_requested {
                park_pending_scan(app, runtime);
                return;
            }
            drain_scan_next(app, runtime);
        }
        IoMessage::ScanError { generation, error } => {
            if generation != runtime.generation {
                return;
            }
            let matches_inflight = matches!(
                runtime.in_flight,
                Some(InFlight::Scan { generation: g }) if g == generation
            );
            if !matches_inflight {
                return;
            }
            runtime.in_flight = None;
            clear_scan_state(runtime);
            reduce(app, Action::ShowError(format!("Scan failed: {error}")));
        }
    }
}

pub(crate) async fn disconnect_session(app: &mut AppState, runtime: &mut Runtime) {
    if let Some(InFlight::HostKey { reply, .. }) = runtime.in_flight.take() {
        let _ = reply.send(false);
    }
    runtime.in_flight = None;
    bump_generation_drop_workers(app, runtime).await;
    reduce(app, Action::CancelPrompt);
    let _ = take_parked(&runtime.park);
    let handle = runtime.handle.take();
    let disconnect_ok = match handle {
        Some(SessionHandle::Ftp(mut ftp)) => {
            let _ = ftp.disconnect().await;
            Ok(())
        }
        Some(SessionHandle::Sftp(mut sftp)) => sftp.disconnect().await,
        None => Ok(()),
    };
    match disconnect_ok {
        Ok(_) => {
            reduce(app, Action::Disconnect);
            reduce(
                app,
                Action::SetRemoteEntries {
                    entries: vec![],
                    select: SelectPolicy::Reset,
                },
            );
            app.active_connection = None;
            reduce(app, Action::SetStatus("Disconnected".to_string()));
        }
        Err(err) => {
            reduce(app, Action::SetStatus(format!("Disconnect failed: {err}")));
        }
    }
}

pub(crate) async fn connect_off_thread(
    app: &mut AppState,
    runtime: &mut Runtime,
    info: ConnectionInfo,
) {
    if io_busy(runtime) {
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

    if app.connected || runtime.handle.is_some() {
        disconnect_session(app, runtime).await;
    }
    let leftover_workers = runtime.worker_active_count > 0
        || !app.queue.active.is_empty()
        || !runtime.worker_handles.is_empty();
    if leftover_workers {
        bump_generation_drop_workers(app, runtime).await;
    } else {
        bump_generation(runtime);
    }
    let gen = runtime.generation;
    runtime.in_flight = Some(InFlight::Connect { generation: gen });

    app.remote_cwd = if info.initial_path.trim().is_empty() {
        "/".to_string()
    } else {
        info.initial_path.clone()
    };
    reduce(app, Action::Connect(info.clone()));

    let list_path = app.remote_cwd.clone();
    let io_tx = runtime.io_tx.clone();
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
            let mut unified = UnifiedFtpSession::new();
            unified.connect(info.clone()).await?;
            let entries = unified.list_dir(&list_path).await?;
            Ok(ConnectOk::Ftp {
                info,
                session: unified,
                entries,
            })
        }
    }
}

pub(crate) fn connection_info_from_env() -> ConnectionInfo {
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

fn pane_entries_for_transfer(app: &AppState, pane: dd_ftp_app::FocusPane) -> Vec<FileEntry> {
    let (entries, marks, selected) = match pane {
        dd_ftp_app::FocusPane::Local => (
            &app.local_entries,
            &app.marked_local,
            app.selected_local_entry().cloned(),
        ),
        dd_ftp_app::FocusPane::Remote => (
            &app.remote_entries,
            &app.marked_remote,
            app.selected_remote_entry().cloned(),
        ),
        dd_ftp_app::FocusPane::Queue => return Vec::new(),
    };
    if marks.is_empty() {
        return selected.into_iter().collect();
    }
    entries
        .iter()
        .filter(|e| marks.contains(&e.path) && e.name != "." && e.name != "..")
        .cloned()
        .collect()
}

pub(crate) fn queue_upload_selected(app: &mut AppState, runtime: &mut Runtime) {
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }

    let entries = pane_entries_for_transfer(app, dd_ftp_app::FocusPane::Local);
    enqueue_selected(app, runtime, entries, TransferDirection::Upload);
}

pub(crate) fn queue_download_selected(app: &mut AppState, runtime: &mut Runtime) {
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }

    let entries = pane_entries_for_transfer(app, dd_ftp_app::FocusPane::Remote);
    enqueue_selected(app, runtime, entries, TransferDirection::Download);
}

fn enqueue_selected(
    app: &mut AppState,
    runtime: &mut Runtime,
    mut entries: Vec<FileEntry>,
    direction: TransferDirection,
) {
    match entries.len() {
        0 => {}
        1 => enqueue_entry(app, runtime, entries.remove(0), direction),
        _ => enqueue_entries(app, runtime, entries, direction),
    }
}

pub(crate) fn enqueue_entry(
    app: &mut AppState,
    runtime: &mut Runtime,
    entry: FileEntry,
    direction: TransferDirection,
) {
    enqueue_entries(app, runtime, vec![entry], direction);
}

fn pending_from_entry(
    app: &AppState,
    entry: &FileEntry,
    direction: TransferDirection,
) -> anyhow::Result<PendingFile> {
    let (local_path, remote_path) = match direction {
        TransferDirection::Upload => {
            let remote_path = safe_remote_child(&app.remote_cwd, &entry.name)?;
            (entry.path.clone(), remote_path)
        }
        TransferDirection::Download => {
            let local_path = safe_local_child(Path::new(&app.local_cwd), &entry.name)?
                .to_string_lossy()
                .to_string();
            let remote_path = safe_remote_child(&app.remote_cwd, &entry.name)?;
            (local_path, remote_path)
        }
    };
    Ok(PendingFile {
        local_path,
        remote_path,
        direction,
        size_bytes: Some(entry.size),
    })
}

pub(crate) fn enqueue_entries(
    app: &mut AppState,
    runtime: &mut Runtime,
    entries: Vec<FileEntry>,
    direction: TransferDirection,
) {
    if drain_busy(runtime) {
        return;
    }
    let entries: Vec<FileEntry> = entries
        .into_iter()
        .filter(|e| e.name != "." && e.name != "..")
        .collect();
    if entries.is_empty() {
        return;
    }
    // User-facing u/d/Enter: clear cancel so spawn may run. Drain enqueue does not.
    reduce(
        app,
        Action::SetWorkerView {
            active_count: runtime.worker_active_count,
            running: runtime.worker_active_count > 0,
            cancel_requested: false,
        },
    );

    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in entries {
        if entry.kind == dd_ftp_core::EntryKind::Directory {
            dirs.push(entry);
        } else {
            files.push(entry);
        }
    }

    for file in files {
        match pending_from_entry(app, &file, direction) {
            Ok(pending) => runtime.pending_scan.push_back(pending),
            Err(_) => reduce(app, Action::ShowError("path escapes directory".to_string())),
        }
    }

    if !dirs.is_empty() {
        start_scan_entries(app, runtime, dirs, direction);
        return;
    }
    drain_scan_next(app, runtime);
}

pub(crate) fn apply_scan_item(pending_scan: &mut VecDeque<PendingFile>, file: PendingFile) {
    pending_scan.push_back(file);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverwriteChoice {
    Skip,
    Overwrite,
    OverwriteAll,
    SkipAll,
    Abort,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DrainEvent {
    BeginDownload { dest_exists: bool },
    BeginUpload,
    UploadList { dest_exists: bool },
    UploadParentMissing,
    UploadParentsCreated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DrainStep {
    ListParent(String),
    CreateParents,
    Enqueue,
    Skip,
    Prompt,
}

pub(crate) fn resolve_conflict(dest_exists: bool, policy: OverwritePolicy) -> DrainStep {
    if !dest_exists {
        DrainStep::Enqueue
    } else {
        match policy {
            OverwritePolicy::Ask => DrainStep::Prompt,
            OverwritePolicy::OverwriteAll => DrainStep::Enqueue,
            OverwritePolicy::SkipAll => DrainStep::Skip,
        }
    }
}

pub(crate) fn drain_step(
    file: &PendingFile,
    event: DrainEvent,
    policy: OverwritePolicy,
) -> DrainStep {
    match (file.direction, event) {
        (TransferDirection::Download, DrainEvent::BeginDownload { dest_exists }) => {
            resolve_conflict(dest_exists, policy)
        }
        (TransferDirection::Upload, DrainEvent::BeginUpload) => {
            DrainStep::ListParent(parent_remote_path(&file.remote_path))
        }
        (TransferDirection::Upload, DrainEvent::UploadList { dest_exists }) => {
            resolve_conflict(dest_exists, policy)
        }
        (TransferDirection::Upload, DrainEvent::UploadParentMissing) => DrainStep::CreateParents,
        (TransferDirection::Upload, DrainEvent::UploadParentsCreated) => {
            resolve_conflict(false, policy)
        }
        _ => DrainStep::Skip,
    }
}

pub(crate) fn remote_mkdir_chain(cwd: &str, dest_parent: &str) -> anyhow::Result<Vec<String>> {
    if dest_parent == cwd || dest_parent.is_empty() {
        return Ok(Vec::new());
    }
    let cwd_trim = cwd.trim_end_matches('/');
    let prefix = if cwd_trim.is_empty() { "/" } else { cwd_trim };
    let rel = dest_parent
        .strip_prefix(prefix)
        .unwrap_or(dest_parent)
        .trim_start_matches('/');
    if rel.is_empty() {
        return Ok(Vec::new());
    }
    let mut acc = if prefix.is_empty() {
        "/".to_string()
    } else {
        prefix.to_string()
    };
    let mut out = Vec::new();
    for comp in rel.split('/') {
        if comp.is_empty() {
            continue;
        }
        acc = safe_remote_child(&acc, comp)?;
        out.push(acc.clone());
    }
    Ok(out)
}

fn remote_basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

pub(crate) fn enqueue_pending_file(app: &mut AppState, file: PendingFile) {
    let mut job = TransferJob::new(file.local_path, file.remote_path, file.direction);
    job.size_bytes = file.size_bytes;
    // Drain enqueue must not clear worker_cancel_requested (QueueTransfer does).
    app.queue.enqueue(job);
    reduce(
        app,
        Action::SetStatus(format!("Queue: {} pending", app.queue.pending.len())),
    );
}

/// Move leftover scan files onto parked `queue.pending` without clearing cancel.
pub(crate) fn park_pending_scan(app: &mut AppState, runtime: &mut Runtime) {
    while let Some(file) = runtime.pending_scan.pop_front() {
        enqueue_pending_file(app, file);
    }
    runtime.mkdir_queue.clear();
    runtime.overwrite_policy = OverwritePolicy::Ask;
}

pub(crate) fn maybe_resume_drain(app: &mut AppState, runtime: &mut Runtime) {
    if app.worker_cancel_requested {
        return;
    }
    if !runtime.pending_scan.is_empty() {
        drain_scan_next(app, runtime);
    }
}

pub(crate) fn drain_scan_next(app: &mut AppState, runtime: &mut Runtime) {
    if app.worker_cancel_requested {
        return;
    }
    if io_busy(runtime) {
        return;
    }
    if app.is_choice_prompt()
        || matches!(
            app.prompt_kind,
            Some(PromptKind::Text(TextPromptKind::OverwriteRename))
        )
    {
        return;
    }

    loop {
        let Some(file) = runtime.pending_scan.front().cloned() else {
            runtime.overwrite_policy = OverwritePolicy::Ask;
            runtime.mkdir_queue.clear();
            return;
        };
        match file.direction {
            TransferDirection::Download => {
                let dest_exists = Path::new(&file.local_path).exists();
                match drain_step(
                    &file,
                    DrainEvent::BeginDownload { dest_exists },
                    runtime.overwrite_policy,
                ) {
                    DrainStep::Enqueue => {
                        runtime.pending_scan.pop_front();
                        enqueue_pending_file(app, file);
                    }
                    DrainStep::Skip => {
                        runtime.pending_scan.pop_front();
                    }
                    DrainStep::Prompt => {
                        show_overwrite_prompt(app, runtime);
                        return;
                    }
                    _ => return,
                }
            }
            TransferDirection::Upload => {
                match drain_step(&file, DrainEvent::BeginUpload, runtime.overwrite_policy) {
                    DrainStep::ListParent(parent) => {
                        request_list(app, runtime, parent, SelectPolicy::PreserveName, true);
                        return;
                    }
                    _ => return,
                }
            }
        }
    }
}

pub(crate) fn handle_drain_list_result(
    app: &mut AppState,
    runtime: &mut Runtime,
    result: Result<Vec<FileEntry>, anyhow::Error>,
) {
    if app.worker_cancel_requested {
        return;
    }
    let Some(file) = runtime.pending_scan.front().cloned() else {
        return;
    };
    let dest_name = remote_basename(&file.remote_path);
    match result {
        Ok(entries) => {
            let dest_exists = entries.iter().any(|e| e.name == dest_name);
            apply_drain_conflict(app, runtime, file, dest_exists);
        }
        Err(err) => {
            let parent = parent_remote_path(&file.remote_path);
            if parent == app.remote_cwd {
                reduce(app, Action::ShowError(format!("Remote list failed: {err}")));
                runtime.pending_scan.pop_front();
                drain_scan_next(app, runtime);
                return;
            }
            match drain_step(
                &file,
                DrainEvent::UploadParentMissing,
                runtime.overwrite_policy,
            ) {
                DrainStep::CreateParents => match remote_mkdir_chain(&app.remote_cwd, &parent) {
                    Ok(chain) if !chain.is_empty() => {
                        runtime.mkdir_queue = chain.into();
                        drain_mkdir_next(app, runtime);
                    }
                    Ok(_) => apply_drain_conflict(app, runtime, file, false),
                    Err(_) => {
                        reduce(app, Action::ShowError("path escapes directory".to_string()));
                        runtime.pending_scan.pop_front();
                        drain_scan_next(app, runtime);
                    }
                },
                _ => {
                    runtime.pending_scan.pop_front();
                    drain_scan_next(app, runtime);
                }
            }
        }
    }
}

fn apply_drain_conflict(
    app: &mut AppState,
    runtime: &mut Runtime,
    file: PendingFile,
    dest_exists: bool,
) {
    match drain_step(
        &file,
        DrainEvent::UploadList { dest_exists },
        runtime.overwrite_policy,
    ) {
        DrainStep::Enqueue => {
            runtime.pending_scan.pop_front();
            enqueue_pending_file(app, file);
            drain_scan_next(app, runtime);
        }
        DrainStep::Skip => {
            runtime.pending_scan.pop_front();
            drain_scan_next(app, runtime);
        }
        DrainStep::Prompt => show_overwrite_prompt(app, runtime),
        _ => {}
    }
}

pub(crate) fn is_already_exists_error(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(ioe) = cause.downcast_ref::<std::io::Error>() {
            if ioe.kind() == std::io::ErrorKind::AlreadyExists {
                return true;
            }
        }
    }
    let msg = err.to_string().to_lowercase();
    msg.contains("already exists")
        || msg.contains("file exists")
        || msg.contains("directory exists")
        || msg.contains("folder exists")
        || msg.contains("file exist")
}

pub(crate) fn drain_mkdir_outcome(result: Result<(), anyhow::Error>) -> Result<(), anyhow::Error> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if is_already_exists_error(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn handle_drain_mkdir_result(
    app: &mut AppState,
    runtime: &mut Runtime,
    result: Result<(), anyhow::Error>,
) {
    if app.worker_cancel_requested {
        return;
    }
    match drain_mkdir_outcome(result) {
        Ok(()) => {
            runtime.mkdir_queue.pop_front();
            if runtime.mkdir_queue.is_empty() {
                if let Some(file) = runtime.pending_scan.front().cloned() {
                    match drain_step(
                        &file,
                        DrainEvent::UploadParentsCreated,
                        runtime.overwrite_policy,
                    ) {
                        DrainStep::Enqueue => {
                            runtime.pending_scan.pop_front();
                            enqueue_pending_file(app, file);
                        }
                        DrainStep::Skip => {
                            runtime.pending_scan.pop_front();
                        }
                        DrainStep::Prompt => {
                            show_overwrite_prompt(app, runtime);
                            return;
                        }
                        _ => {}
                    }
                }
                drain_scan_next(app, runtime);
            } else {
                drain_mkdir_next(app, runtime);
            }
        }
        Err(err) => {
            reduce(
                app,
                Action::ShowError(format!("Create folder failed: {err}")),
            );
            runtime.mkdir_queue.clear();
            runtime.pending_scan.pop_front();
            drain_scan_next(app, runtime);
        }
    }
}

fn drain_mkdir_next(app: &mut AppState, runtime: &mut Runtime) {
    if app.worker_cancel_requested {
        return;
    }
    let Some(path) = runtime.mkdir_queue.front().cloned() else {
        if let Some(file) = runtime.pending_scan.pop_front() {
            enqueue_pending_file(app, file);
        }
        drain_scan_next(app, runtime);
        return;
    };
    request_fs(app, runtime, FsKind::CreateFolder, path);
}

pub(crate) fn show_overwrite_prompt(app: &mut AppState, runtime: &Runtime) {
    let Some(current) = runtime.pending_scan.front().cloned() else {
        return;
    };
    let remaining: Vec<PendingFile> = runtime.pending_scan.iter().skip(1).cloned().collect();
    app.overwrite = Some(OverwritePrompt {
        current,
        remaining,
        apply_all: runtime.overwrite_policy,
    });
    reduce(app, Action::ShowChoicePrompt(ChoicePromptKind::Overwrite));
}

pub(crate) fn apply_overwrite_choice(
    app: &mut AppState,
    runtime: &mut Runtime,
    choice: OverwriteChoice,
) {
    match choice {
        OverwriteChoice::Skip => {
            reduce(app, Action::CancelPrompt);
            runtime.pending_scan.pop_front();
            drain_scan_next(app, runtime);
        }
        OverwriteChoice::Overwrite => {
            reduce(app, Action::CancelPrompt);
            if let Some(file) = runtime.pending_scan.pop_front() {
                enqueue_pending_file(app, file);
            }
            drain_scan_next(app, runtime);
        }
        OverwriteChoice::OverwriteAll => {
            runtime.overwrite_policy = OverwritePolicy::OverwriteAll;
            reduce(
                app,
                Action::SetOverwritePolicy(OverwritePolicy::OverwriteAll),
            );
            reduce(app, Action::CancelPrompt);
            if let Some(file) = runtime.pending_scan.pop_front() {
                enqueue_pending_file(app, file);
            }
            drain_scan_next(app, runtime);
        }
        OverwriteChoice::SkipAll => {
            runtime.overwrite_policy = OverwritePolicy::SkipAll;
            reduce(app, Action::SetOverwritePolicy(OverwritePolicy::SkipAll));
            reduce(app, Action::CancelPrompt);
            runtime.pending_scan.pop_front();
            drain_scan_next(app, runtime);
        }
        OverwriteChoice::Abort => {
            clear_scan_state(runtime);
            reduce(app, Action::CancelPrompt);
        }
        OverwriteChoice::Rename => {
            reduce(app, Action::CancelPrompt);
            app.show_prompt = true;
            app.prompt_kind = Some(PromptKind::Text(TextPromptKind::OverwriteRename));
            let current_name = runtime
                .pending_scan
                .front()
                .map(|f| match f.direction {
                    TransferDirection::Upload => remote_basename(&f.remote_path),
                    TransferDirection::Download => Path::new(&f.local_path)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                })
                .unwrap_or_default();
            app.prompt_value = dd_ftp_app::TextField::from_str(&current_name);
        }
    }
}

/// Apply a rename dest. `false` keeps the rename prompt open and the file at front.
pub(crate) fn apply_overwrite_rename(
    app: &mut AppState,
    runtime: &mut Runtime,
    new_name: &str,
) -> bool {
    let Some(mut file) = runtime.pending_scan.front().cloned() else {
        return true;
    };
    match file.direction {
        TransferDirection::Upload => {
            let parent = parent_remote_path(&file.remote_path);
            match safe_remote_child(&parent, new_name) {
                Ok(p) => file.remote_path = p,
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return false;
                }
            }
        }
        TransferDirection::Download => {
            let parent = Path::new(&file.local_path)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(&app.local_cwd));
            match safe_local_child(&parent, new_name) {
                Ok(p) => file.local_path = p.to_string_lossy().to_string(),
                Err(_) => {
                    reduce(app, Action::ShowError("path escapes directory".to_string()));
                    return false;
                }
            }
        }
    }
    if let Some(front) = runtime.pending_scan.front_mut() {
        *front = file;
    }
    true
}

pub(crate) fn start_scan_entries(
    app: &mut AppState,
    runtime: &mut Runtime,
    entries: Vec<FileEntry>,
    direction: TransferDirection,
) {
    if io_busy(runtime) {
        return;
    }
    let entries: Vec<FileEntry> = entries
        .into_iter()
        .filter(|e| e.name != "." && e.name != "..")
        .collect();
    if entries.is_empty() {
        return;
    }
    let gen = runtime.generation;
    runtime.in_flight = Some(InFlight::Scan { generation: gen });
    let label = if entries.len() == 1 {
        entries[0].name.clone()
    } else {
        format!("{} items", entries.len())
    };
    let msg = format!("Scanning {label}...");
    app.toast = Some(Toast::info(msg.clone()));
    reduce(app, Action::SetStatus(msg));

    let io_tx = runtime.io_tx.clone();
    match direction {
        TransferDirection::Upload => {
            let mut roots = Vec::new();
            for entry in &entries {
                match safe_remote_child(&app.remote_cwd, &entry.name) {
                    Ok(remote_root) => roots.push((PathBuf::from(&entry.path), remote_root)),
                    Err(_) => {
                        runtime.in_flight = None;
                        reduce(app, Action::ShowError("path escapes directory".to_string()));
                        drain_scan_next(app, runtime);
                        return;
                    }
                }
            }
            tokio::task::spawn_blocking(move || {
                let mut all = Vec::new();
                for (local_root, remote_root) in roots {
                    match walk_local_files(&local_root, &remote_root) {
                        Ok(files) => all.extend(files),
                        Err(err) => {
                            let _ = io_tx.send(IoMessage::ScanError {
                                generation: gen,
                                error: err.to_string(),
                            });
                            return;
                        }
                    }
                }
                for file in all {
                    let _ = io_tx.send(IoMessage::ScanItem {
                        generation: gen,
                        file,
                    });
                }
                let _ = io_tx.send(IoMessage::ScanDone { generation: gen });
            });
        }
        TransferDirection::Download => {
            let Some(info) = app.active_connection.clone() else {
                runtime.in_flight = None;
                reduce(app, Action::SetStatus("Not connected".to_string()));
                drain_scan_next(app, runtime);
                return;
            };
            let mut roots = Vec::new();
            for entry in &entries {
                let remote_root = match safe_remote_child(&app.remote_cwd, &entry.name) {
                    Ok(p) => p,
                    Err(_) => {
                        runtime.in_flight = None;
                        reduce(app, Action::ShowError("path escapes directory".to_string()));
                        drain_scan_next(app, runtime);
                        return;
                    }
                };
                let local_root = match safe_local_child(Path::new(&app.local_cwd), &entry.name) {
                    Ok(p) => p,
                    Err(_) => {
                        runtime.in_flight = None;
                        reduce(app, Action::ShowError("path escapes directory".to_string()));
                        drain_scan_next(app, runtime);
                        return;
                    }
                };
                roots.push((remote_root, local_root));
            }
            tokio::spawn(async move {
                match walk_remote_files(info, roots, gen, io_tx.clone()).await {
                    Ok(()) => {
                        let _ = io_tx.send(IoMessage::ScanDone { generation: gen });
                    }
                    Err(err) => {
                        let _ = io_tx.send(IoMessage::ScanError {
                            generation: gen,
                            error: err.to_string(),
                        });
                    }
                }
            });
        }
    }
}

/// Local recursive walk. Reads only; never creates destination directories.
pub(crate) fn walk_local_files(root: &Path, remote_root: &str) -> anyhow::Result<Vec<PendingFile>> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), remote_root.to_string())];
    while let Some((local_dir, remote_dir)) = stack.pop() {
        let rd = match std::fs::read_dir(&local_dir) {
            Ok(rd) => rd,
            Err(err) => anyhow::bail!("read_dir {}: {err}", local_dir.display()),
        };
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "." || name_str == ".." {
                continue;
            }
            let meta = match std::fs::symlink_metadata(entry.path()) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let ft = meta.file_type();
            if ft.is_symlink() {
                // Do not follow directory symlinks (cycles / escape). Skip them.
                if std::fs::metadata(entry.path())
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            if ft.is_dir() {
                let child_remote = safe_remote_child(&remote_dir, name_str.as_ref())?;
                stack.push((entry.path(), child_remote));
            } else {
                let remote_path = safe_remote_child(&remote_dir, name_str.as_ref())?;
                out.push(PendingFile {
                    local_path: entry.path().to_string_lossy().into_owned(),
                    remote_path,
                    direction: TransferDirection::Upload,
                    size_bytes: Some(meta.len()),
                });
            }
        }
    }
    Ok(out)
}

async fn walk_remote_files(
    info: ConnectionInfo,
    roots: Vec<(String, PathBuf)>,
    generation: u64,
    tx: mpsc::UnboundedSender<IoMessage>,
) -> Result<()> {
    let mut stack = roots;
    match info.protocol {
        Protocol::Sftp => {
            let mut session = SftpSession::default();
            session.connect(info).await?;
            while let Some((remote_dir, local_dir)) = stack.pop() {
                let entries = session.list_dir(&remote_dir).await?;
                push_remote_entries(
                    &mut stack,
                    &tx,
                    generation,
                    &remote_dir,
                    &local_dir,
                    entries,
                )?;
            }
            Ok(())
        }
        Protocol::Ftp | Protocol::Ftps => {
            let mut unified = UnifiedFtpSession::new();
            unified.connect(info).await?;
            let mut walk_err = None;
            while let Some((remote_dir, local_dir)) = stack.pop() {
                match unified.list_dir(&remote_dir).await {
                    Ok(entries) => {
                        if let Err(err) = push_remote_entries(
                            &mut stack,
                            &tx,
                            generation,
                            &remote_dir,
                            &local_dir,
                            entries,
                        ) {
                            walk_err = Some(err);
                            break;
                        }
                    }
                    Err(err) => {
                        walk_err = Some(err);
                        break;
                    }
                }
            }
            unified.disconnect().await.ok();
            match walk_err {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }
    }
}

fn push_remote_entries(
    stack: &mut Vec<(String, PathBuf)>,
    tx: &mpsc::UnboundedSender<IoMessage>,
    generation: u64,
    remote_dir: &str,
    local_dir: &Path,
    entries: Vec<FileEntry>,
) -> Result<()> {
    for entry in entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        if entry.is_dir() {
            let child_remote = safe_remote_child(remote_dir, &entry.name)?;
            let child_local = safe_local_child(local_dir, &entry.name)?;
            stack.push((child_remote, child_local));
        } else {
            let remote_path = safe_remote_child(remote_dir, &entry.name)?;
            let local_path = safe_local_child(local_dir, &entry.name)?;
            let _ = tx.send(IoMessage::ScanItem {
                generation,
                file: PendingFile {
                    local_path: local_path.to_string_lossy().into_owned(),
                    remote_path,
                    direction: TransferDirection::Download,
                    size_bytes: Some(entry.size),
                },
            });
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_runtime() -> (Runtime, mpsc::UnboundedReceiver<IoMessage>) {
    let (io_tx, io_rx) = mpsc::unbounded_channel();
    let mut runtime = Runtime::new(io_tx);
    runtime.generation = 1;
    (runtime, io_rx)
}

#[cfg(test)]
mod scan_tests {
    use std::path::{Component, Path};

    use super::*;
    use dd_ftp_app::{
        reduce, Action, AppState, OverwritePolicy, PendingFile, PromptKind, TextPromptKind,
    };
    use dd_ftp_core::TransferDirection;

    fn pending_upload(local: &str, remote: &str) -> PendingFile {
        PendingFile {
            local_path: local.to_string(),
            remote_path: remote.to_string(),
            direction: TransferDirection::Upload,
            size_bytes: Some(1),
        }
    }

    #[test]
    fn walk_local_tree_emits_nested_and_hidden_never_dotdot() {
        let root = std::env::temp_dir().join(format!(
            "dd_ftp_walk_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).expect("nested dir");
        std::fs::write(nested.join("c.txt"), b"hi").expect("c.txt");
        std::fs::write(root.join("a").join(".hidden"), b"dot").expect(".hidden");

        let files = walk_local_files(&root.join("a"), "/pub/a").expect("walk");
        let names: Vec<String> = files
            .iter()
            .map(|f| {
                Path::new(&f.local_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(files.len(), 2, "expected c.txt and .hidden, got {names:?}");
        assert!(names.contains(&"c.txt".to_string()));
        assert!(names.contains(&".hidden".to_string()));
        assert!(!names.iter().any(|n| n == "." || n == ".."));
        assert!(files.iter().any(|f| f.remote_path == "/pub/a/b/c.txt"));
        assert!(files.iter().any(|f| f.remote_path == "/pub/a/.hidden"));
        for f in &files {
            for comp in Path::new(&f.remote_path).components() {
                if let Component::Normal(name) = comp {
                    let n = name.to_string_lossy();
                    if n != "pub" {
                        assert!(
                            safe_remote_child("/pub", n.as_ref()).is_ok()
                                || n == "a"
                                || n == "b"
                                || n == "c.txt"
                                || n == ".hidden",
                            "unsafe dest component {n}"
                        );
                    }
                }
            }
        }

        // Walk helper reads only: fixture tree is unchanged (no dest create_dir).
        assert!(nested.join("c.txt").is_file());
        assert!(root.join("a").join(".hidden").is_file());
        assert!(!root.join("pub").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_item_only_appends_to_pending_scan() {
        let mut pending = VecDeque::new();
        let app = AppState::default();
        let file = pending_upload("/tmp/a/b/c.txt", "/pub/a/b/c.txt");
        apply_scan_item(&mut pending, file);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].remote_path, "/pub/a/b/c.txt");
        assert!(
            app.queue.pending.is_empty(),
            "ScanItem must not enqueue; queue.pending={}",
            app.queue.pending.len()
        );
    }

    #[test]
    fn nested_upload_drain_waits_for_mkdir_before_enqueue() {
        let file = pending_upload("/tmp/a/b/c.txt", "/pub/a/b/c.txt");
        assert_eq!(
            drain_step(&file, DrainEvent::BeginUpload, OverwritePolicy::Ask),
            DrainStep::ListParent("/pub/a/b".into())
        );
        assert_eq!(
            drain_step(&file, DrainEvent::UploadParentMissing, OverwritePolicy::Ask),
            DrainStep::CreateParents
        );
        let chain = remote_mkdir_chain("/pub", "/pub/a/b").expect("chain");
        assert_eq!(chain, vec!["/pub/a".to_string(), "/pub/a/b".to_string()]);
        assert_eq!(
            drain_step(
                &file,
                DrainEvent::UploadParentsCreated,
                OverwritePolicy::Ask
            ),
            DrainStep::Enqueue
        );
        assert!(matches!(
            drain_step(
                &file,
                DrainEvent::UploadList { dest_exists: false },
                OverwritePolicy::Ask
            ),
            DrainStep::Enqueue
        ));
    }

    #[test]
    fn overwrite_default_skip_and_overwrite_all_remaining() {
        let file = pending_upload("/tmp/a.txt", "/pub/a.txt");
        assert_eq!(
            resolve_conflict(true, OverwritePolicy::Ask),
            DrainStep::Prompt
        );
        assert_eq!(
            drain_step(
                &file,
                DrainEvent::UploadList { dest_exists: true },
                OverwritePolicy::Ask
            ),
            DrainStep::Prompt
        );
        assert_eq!(
            drain_step(
                &file,
                DrainEvent::UploadList { dest_exists: true },
                OverwritePolicy::OverwriteAll
            ),
            DrainStep::Enqueue
        );
        assert_eq!(
            drain_step(
                &file,
                DrainEvent::UploadList { dest_exists: true },
                OverwritePolicy::SkipAll
            ),
            DrainStep::Skip
        );
        assert_eq!(
            drain_step(
                &file,
                DrainEvent::UploadList { dest_exists: false },
                OverwritePolicy::SkipAll
            ),
            DrainStep::Enqueue
        );
    }

    #[test]
    fn scan_item_is_generation_stamped_and_types_exist() {
        let file = pending_upload("/tmp/a.txt", "/pub/a.txt");
        let msg = IoMessage::ScanItem {
            generation: 7,
            file: file.clone(),
        };
        match msg {
            IoMessage::ScanItem { generation, file } => {
                assert_eq!(generation, 7);
                assert_eq!(file.remote_path, "/pub/a.txt");
            }
            _ => panic!("expected ScanItem"),
        }
        let inflight = InFlight::Scan { generation: 7 };
        assert!(matches!(inflight, InFlight::Scan { generation: 7 }));
        let (mut runtime, _rx) = test_runtime();
        runtime.pending_scan.push_back(file);
        assert!(!runtime.pending_scan.is_empty());
        assert!(drain_busy(&runtime));
    }

    #[test]
    fn safe_child_applied_to_every_walk_dest() {
        let files = {
            let root = std::env::temp_dir().join(format!(
                "dd_ftp_walk_safe_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(root.join("a").join("b")).expect("dirs");
            std::fs::write(root.join("a").join("b").join("c.txt"), b"x").expect("file");
            let files = walk_local_files(&root.join("a"), "/pub/a").expect("walk");
            let _ = std::fs::remove_dir_all(&root);
            files
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].remote_path, "/pub/a/b/c.txt");
        assert_eq!(safe_remote_child("/pub/a", "b").unwrap(), "/pub/a/b");
        assert_eq!(
            safe_remote_child("/pub/a/b", "c.txt").unwrap(),
            "/pub/a/b/c.txt"
        );
        assert!(safe_remote_child("/pub/a", "..").is_err());
        assert!(safe_local_child(Path::new("/tmp/pane"), "..").is_err());
    }

    #[test]
    fn overwrite_esc_does_not_clear_queued_jobs() {
        let mut app = AppState::default();
        reduce(
            &mut app,
            Action::QueueTransfer(TransferJob::new(
                "/tmp/kept",
                "/pub/kept",
                TransferDirection::Upload,
            )),
        );
        let (mut runtime, _rx) = test_runtime();
        runtime
            .pending_scan
            .push_back(pending_upload("/tmp/conflict", "/pub/conflict"));
        apply_overwrite_choice(&mut app, &mut runtime, OverwriteChoice::Abort);
        assert!(runtime.pending_scan.is_empty());
        assert_eq!(app.queue.pending.len(), 1);
        assert_eq!(app.queue.pending[0].remote_path, "/pub/kept");
    }

    #[test]
    fn already_exists_mkdir_is_success_and_does_not_drop_file() {
        assert!(is_already_exists_error(&anyhow::anyhow!(
            "failed to create remote directory: File exists"
        )));
        assert!(is_already_exists_error(&anyhow::anyhow!(
            "550 Directory already exists"
        )));
        assert!(!is_already_exists_error(&anyhow::anyhow!(
            "permission denied"
        )));
        assert!(drain_mkdir_outcome(Err(anyhow::anyhow!("File exists"))).is_ok());
        assert!(drain_mkdir_outcome(Err(anyhow::anyhow!("permission denied"))).is_err());

        let mut app = AppState::default();
        let (mut runtime, _rx) = test_runtime();
        runtime
            .pending_scan
            .push_back(pending_upload("/tmp/a/b/c.txt", "/pub/a/b/c.txt"));
        runtime.mkdir_queue.push_back("/pub/a/b".into());
        handle_drain_mkdir_result(&mut app, &mut runtime, Err(anyhow::anyhow!("File exists")));
        assert!(
            runtime.pending_scan.is_empty(),
            "exists is success: file is enqueued, not dropped"
        );
        assert_eq!(app.queue.pending.len(), 1);
        assert_eq!(app.queue.pending[0].remote_path, "/pub/a/b/c.txt");
    }

    #[test]
    fn unsafe_overwrite_rename_keeps_file_and_prompt() {
        let mut app = AppState {
            connected: true,
            local_cwd: "/tmp".into(),
            remote_cwd: "/pub".into(),
            show_prompt: true,
            prompt_kind: Some(PromptKind::Text(TextPromptKind::OverwriteRename)),
            ..Default::default()
        };
        let (mut runtime, _rx) = test_runtime();
        runtime
            .pending_scan
            .push_back(pending_upload("/tmp/a.txt", "/pub/a.txt"));
        assert!(!apply_overwrite_rename(&mut app, &mut runtime, ".."));
        assert_eq!(runtime.pending_scan.len(), 1);
        assert_eq!(runtime.pending_scan[0].remote_path, "/pub/a.txt");
        assert!(app.show_prompt);
        assert_eq!(
            app.prompt_kind,
            Some(PromptKind::Text(TextPromptKind::OverwriteRename))
        );
    }

    #[cfg(unix)]
    #[test]
    fn walk_local_skips_symlink_dirs() {
        let root = std::env::temp_dir().join(format!(
            "dd_ftp_walk_symlink_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let real = root.join("a").join("real");
        std::fs::create_dir_all(&real).expect("dirs");
        std::fs::write(real.join("ok.txt"), b"ok").expect("file");
        std::os::unix::fs::symlink("..", root.join("a").join("loop")).expect("symlink dir");
        let files = walk_local_files(&root.join("a"), "/pub/a").expect("walk");
        let names: Vec<_> = files
            .iter()
            .map(|f| {
                Path::new(&f.local_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["ok.txt".to_string()]);
        assert!(!files.iter().any(|f| f.local_path.contains("/loop/")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn download_jobs_must_not_contain_a_list_line_path() {
        use dd_ftp_core::{EntryKind, FileEntry};
        let dir = std::env::temp_dir().join(format!(
            "dd_ftp_dl_path_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut app = AppState {
            connected: true,
            local_cwd: dir.to_string_lossy().into_owned(),
            remote_cwd: "/pub".into(),
            remote_entries: vec![FileEntry {
                name: "file.bin".into(),
                path: "-rw-r--r-- 1 user group 123 Jan 01 file.bin".into(),
                kind: EntryKind::File,
                size: 123,
                modified: None,
                permissions: None,
            }],
            selected_remote: 0,
            ..Default::default()
        };

        let (mut runtime, _rx) = test_runtime();
        queue_download_selected(&mut app, &mut runtime);

        let job = app.queue.pending.first().expect("download job queued");
        assert_eq!(job.remote_path, "/pub/file.bin");
        assert!(
            !job.remote_path.contains("-rw-"),
            "download remote_path must not be a LIST line, got {}",
            job.remote_path
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
