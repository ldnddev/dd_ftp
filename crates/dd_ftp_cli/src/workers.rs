use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use dd_ftp_app::{reduce, Action, AppState, SelectPolicy};
use dd_ftp_core::{ConnectionInfo, Protocol, RemoteSession, TransferDirection};
use dd_ftp_ftp::UnifiedFtpSession;
use dd_ftp_protocols::SftpSession;
use dd_ftp_storage::SecretStore;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::session::{connection_info_from_env, list_remote, Runtime, SessionHandle};

#[derive(Debug)]
pub(crate) struct WorkerResult {
    pub job: dd_ftp_core::TransferJob,
    pub outcome: anyhow::Result<()>,
    pub was_cancelled: bool,
    pub cancel_flag: Arc<AtomicBool>,
}

#[derive(Debug)]
pub(crate) enum WorkerMessage {
    Progress {
        generation: u64,
        job_id: Uuid,
        transferred_bytes: u64,
        size_bytes: Option<u64>,
    },
    Done {
        generation: u64,
        result: WorkerResult,
        session: Option<Box<SessionHandle>>,
    },
}

pub(crate) fn should_spawn(
    connected: bool,
    cancel_requested: bool,
    active: usize,
    max: usize,
    pending_len: usize,
) -> bool {
    connected && !cancel_requested && active < max && pending_len > 0
}

pub(crate) fn accept_worker_msg(current: u64, msg_gen: u64) -> bool {
    current == msg_gen
}

fn protocol_tag(p: &Protocol) -> u8 {
    match p {
        Protocol::Sftp => 0,
        Protocol::Ftp => 1,
        Protocol::Ftps => 2,
    }
}

pub(crate) fn session_identity_eq(a: &ConnectionInfo, b: &ConnectionInfo) -> bool {
    protocol_tag(&a.protocol) == protocol_tag(&b.protocol)
        && a.host == b.host
        && a.port == b.port
        && a.username == b.username
        && a.private_key == b.private_key
}

fn handle_matches_info(handle: &SessionHandle, info: &ConnectionInfo) -> bool {
    let current = match handle {
        SessionHandle::Sftp(s) => s.connection_info(),
        SessionHandle::Ftp(f) => f.connection_info(),
    };
    current.is_some_and(|c| session_identity_eq(c, info))
}

pub(crate) fn take_pooled_session(
    runtime: &mut Runtime,
    info: &ConnectionInfo,
) -> Option<SessionHandle> {
    let idx = runtime
        .worker_sessions
        .iter()
        .position(|h| handle_matches_info(h, info))?;
    Some(runtime.worker_sessions.swap_remove(idx))
}

pub(crate) fn return_pooled_session(
    runtime: &mut Runtime,
    session: SessionHandle,
    info: &ConnectionInfo,
    max: usize,
) {
    if runtime.worker_sessions.len() >= max.max(1) {
        return;
    }
    if !handle_matches_info(&session, info) {
        return;
    }
    runtime.worker_sessions.push(session);
}

pub(crate) fn spawn_pending_workers(
    app: &mut AppState,
    runtime: &mut Runtime,
    tx: &mpsc::UnboundedSender<WorkerMessage>,
) {
    while should_spawn(
        app.connected,
        app.worker_cancel_requested,
        runtime.worker_active_count,
        app.worker_max_concurrency,
        app.queue.pending.len(),
    ) {
        let Some(job) = app.queue.start_next() else {
            break;
        };

        runtime.worker_active_count += 1;
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
        runtime.cancel_flags.push(cancel.clone());
        runtime.job_cancels.insert(job.id, cancel.clone());
        let generation = runtime.generation;
        let pooled = take_pooled_session(runtime, &info);

        let handle = tokio::spawn(async move {
            let (outcome, session) = run_pooled_job(
                pooled,
                info,
                &job,
                cancel.clone(),
                tx_clone.clone(),
                generation,
            )
            .await;
            let _ = tx_clone.send(WorkerMessage::Done {
                generation,
                result: WorkerResult {
                    job,
                    outcome,
                    was_cancelled: cancel.load(Ordering::Relaxed),
                    cancel_flag: cancel,
                },
                session: session.map(Box::new),
            });
        });
        runtime.worker_handles.push(handle);
    }
}

const PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(100);

fn should_emit_progress(last: Option<Instant>, now: Instant, complete: bool) -> bool {
    if complete {
        return true;
    }
    match last {
        None => true,
        Some(t) => now.saturating_duration_since(t) >= PROGRESS_MIN_INTERVAL,
    }
}

fn progress_cb(
    tx: mpsc::UnboundedSender<WorkerMessage>,
    generation: u64,
) -> impl Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static {
    let last = Mutex::new(None::<Instant>);
    move |job_id: Uuid, transferred, size| {
        let complete = size.is_some_and(|s| transferred >= s);
        let now = Instant::now();
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        if !should_emit_progress(*last, now, complete) {
            return;
        }
        *last = Some(now);
        let _ = tx.send(WorkerMessage::Progress {
            generation,
            job_id,
            transferred_bytes: transferred,
            size_bytes: size,
        });
    }
}

async fn connect_worker_session(info: ConnectionInfo) -> anyhow::Result<SessionHandle> {
    match info.protocol {
        Protocol::Sftp => {
            let mut session = SftpSession::default();
            session.connect(info).await?;
            Ok(SessionHandle::Sftp(session))
        }
        Protocol::Ftp | Protocol::Ftps => {
            let mut session = UnifiedFtpSession::new();
            session.connect(info).await?;
            Ok(SessionHandle::Ftp(session))
        }
    }
}

async fn disconnect_worker_session(session: &mut SessionHandle) {
    match session {
        SessionHandle::Sftp(s) => {
            let _ = s.disconnect().await;
        }
        SessionHandle::Ftp(f) => {
            let _ = f.disconnect().await;
        }
    }
}

async fn transfer_with_handle(
    session: &mut SessionHandle,
    job: &dd_ftp_core::TransferJob,
    cancel: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<WorkerMessage>,
    generation: u64,
) -> anyhow::Result<()> {
    let progress = progress_cb(tx, generation);
    match session {
        SessionHandle::Sftp(s) => match job.direction {
            TransferDirection::Upload => {
                SftpSession::upload_with_progress(s, job, cancel, progress).await
            }
            TransferDirection::Download => {
                SftpSession::download_with_progress(s, job, cancel, progress).await
            }
        },
        SessionHandle::Ftp(f) => match job.direction {
            TransferDirection::Upload => {
                UnifiedFtpSession::upload_with_progress(f, job, cancel, progress).await
            }
            TransferDirection::Download => {
                UnifiedFtpSession::download_with_progress(f, job, cancel, progress).await
            }
        },
    }
}

async fn run_pooled_job(
    pooled: Option<SessionHandle>,
    info: ConnectionInfo,
    job: &dd_ftp_core::TransferJob,
    cancel: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<WorkerMessage>,
    generation: u64,
) -> (anyhow::Result<()>, Option<SessionHandle>) {
    let reused = pooled.is_some();
    let mut session = match pooled {
        Some(s) => s,
        None => match connect_worker_session(info.clone()).await {
            Ok(s) => s,
            Err(err) => return (Err(err), None),
        },
    };

    let mut outcome =
        transfer_with_handle(&mut session, job, cancel.clone(), tx.clone(), generation).await;

    if outcome.is_err() && reused && !cancel.load(Ordering::Relaxed) {
        disconnect_worker_session(&mut session).await;
        match connect_worker_session(info).await {
            Ok(mut fresh) => {
                outcome =
                    transfer_with_handle(&mut fresh, job, cancel.clone(), tx, generation).await;
                session = fresh;
            }
            Err(err) => return (Err(err), None),
        }
    }

    if outcome.is_ok() && !cancel.load(Ordering::Relaxed) {
        (outcome, Some(session))
    } else {
        disconnect_worker_session(&mut session).await;
        (outcome, None)
    }
}

pub(crate) fn handle_worker_result(
    app: &mut AppState,
    runtime: &mut Runtime,
    mut msg: WorkerResult,
) {
    if app.worker_cancel_requested || msg.was_cancelled {
        if msg.job.edit_after {
            crate::session::cancel_edit(app, runtime, true);
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
            let delete_source = msg.job.delete_source;
            let edit_after = msg.job.edit_after;
            let direction = msg.job.direction;
            let local_path = msg.job.local_path.clone();
            let remote_path = msg.job.remote_path.clone();
            msg.job.last_error = None;
            reduce(app, Action::MarkTransferCompleted(msg.job));
            reduce(app, Action::SetStatus(format!("{name} complete")));

            if edit_after {
                crate::session::on_edit_download_complete(app, runtime);
                return;
            }
            if runtime.edit_session.as_ref().is_some_and(|e| {
                e.local_path.to_string_lossy() == local_path
                    && direction == TransferDirection::Upload
            }) {
                let _ = std::fs::remove_file(&local_path);
                runtime.edit_session = None;
            }

            if delete_source {
                match direction {
                    TransferDirection::Upload => {
                        runtime
                            .pending_delete
                            .push_back(crate::session::PendingDelete {
                                name: std::path::Path::new(&local_path)
                                    .file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| local_path.clone()),
                                path: local_path,
                                is_dir: false,
                                remote: false,
                            });
                    }
                    TransferDirection::Download => {
                        runtime
                            .pending_delete
                            .push_back(crate::session::PendingDelete {
                                name: remote_path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&remote_path)
                                    .to_string(),
                                path: remote_path,
                                is_dir: false,
                                remote: true,
                            });
                    }
                }
            }

            crate::session::request_local_list(
                runtime,
                app.local_cwd.clone(),
                SelectPolicy::PreserveName,
                None,
            );
            if !runtime.pending_delete.is_empty() && runtime.in_flight.is_none() {
                crate::fs_ops::start_next_delete(app, runtime);
            } else if app.connected && runtime.pending_scan.is_empty() {
                runtime.list_ok_status = None;
                runtime.list_err_prefix = "Remote list failed".to_string();
                list_remote(
                    app,
                    runtime,
                    app.remote_cwd.clone(),
                    SelectPolicy::PreserveName,
                );
            }
        }
        Err(err) => {
            if msg.job.edit_after {
                crate::session::cancel_edit(app, runtime, true);
            }
            msg.job.last_error = Some(err.to_string());
            reduce(app, Action::MarkTransferFailed(msg.job));
            reduce(app, Action::ShowError(format!("Transfer failed: {err}")));
        }
    }
}

#[cfg(test)]
mod worker_gate_tests {
    use super::*;
    use crate::session::test_runtime;
    use dd_ftp_app::{reduce, Action, PendingFile};
    use dd_ftp_core::TransferJob;

    #[test]
    fn progress_emits_first_complete_and_throttles() {
        let t0 = Instant::now();
        assert!(should_emit_progress(None, t0, false));
        assert!(!should_emit_progress(
            Some(t0),
            t0 + Duration::from_millis(50),
            false
        ));
        assert!(should_emit_progress(
            Some(t0),
            t0 + Duration::from_millis(100),
            false
        ));
        assert!(should_emit_progress(
            Some(t0),
            t0 + Duration::from_millis(10),
            true
        ));
    }

    #[test]
    fn should_spawn_false_when_cancel_requested() {
        assert!(!should_spawn(true, true, 0, 2, 3));
    }

    #[test]
    fn should_spawn_true_when_idle_with_pending() {
        assert!(should_spawn(true, false, 0, 2, 3));
    }

    #[test]
    fn accept_worker_msg_same_generation() {
        assert!(accept_worker_msg(4, 4));
        assert!(!accept_worker_msg(5, 4));
    }

    fn pending_upload(local: &str, remote: &str) -> PendingFile {
        let mut file = PendingFile::new(local, remote, TransferDirection::Upload);
        file.size_bytes = Some(1);
        file
    }

    #[test]
    fn drain_enqueue_does_not_clear_worker_cancel_requested() {
        let mut app = AppState {
            worker_cancel_requested: true,
            ..Default::default()
        };
        crate::session::enqueue_pending_file(&mut app, pending_upload("/tmp/a.txt", "/pub/a.txt"));
        assert!(
            app.worker_cancel_requested,
            "drain enqueue must not resume spawn"
        );
        assert_eq!(app.queue.pending.len(), 1);
        assert_eq!(app.queue.pending[0].remote_path, "/pub/a.txt");
    }

    #[test]
    fn drain_scan_next_returns_while_cancel_requested() {
        let mut app = AppState {
            connected: true,
            worker_cancel_requested: true,
            ..Default::default()
        };
        let (mut runtime, _rx) = test_runtime();
        runtime
            .pending_scan
            .push_back(pending_upload("/tmp/a.txt", "/pub/a.txt"));
        crate::session::drain_scan_next(&mut app, &mut runtime);
        assert_eq!(runtime.pending_scan.len(), 1);
        assert!(
            app.queue.pending.is_empty(),
            "cancel must not auto-enqueue the rest of a folder drain"
        );
    }

    #[test]
    fn park_pending_scan_parks_without_clearing_cancel_or_spawning() {
        let mut app = AppState {
            connected: true,
            worker_cancel_requested: true,
            ..Default::default()
        };
        let (mut runtime, _rx) = test_runtime();
        runtime
            .pending_scan
            .push_back(pending_upload("/tmp/a.txt", "/pub/a.txt"));
        runtime
            .pending_scan
            .push_back(pending_upload("/tmp/b.txt", "/pub/b.txt"));
        runtime.mkdir_queue.push_back("/pub/a".into());
        crate::session::park_pending_scan(&mut app, &mut runtime);
        assert!(runtime.pending_scan.is_empty());
        assert!(runtime.mkdir_queue.is_empty());
        assert!(!crate::session::drain_busy(&runtime));
        assert_eq!(app.queue.pending.len(), 2);
        assert!(app.worker_cancel_requested);
        assert!(
            !should_spawn(
                app.connected,
                app.worker_cancel_requested,
                app.worker_active_count,
                app.worker_max_concurrency,
                app.queue.pending.len(),
            ),
            "parked drain files must not auto-start"
        );
    }

    #[test]
    fn session_identity_ignores_password_and_label() {
        let a = ConnectionInfo {
            name: "prod".into(),
            host: "box".into(),
            port: 22,
            protocol: Protocol::Sftp,
            username: "u".into(),
            password: Some("secret".into()),
            private_key: None,
            initial_path: "/".into(),
        };
        let mut b = a.clone();
        b.name = "other".into();
        b.password = Some("other".into());
        b.initial_path = "/var".into();
        assert!(session_identity_eq(&a, &b));
        b.host = "other".into();
        assert!(!session_identity_eq(&a, &b));
    }

    #[test]
    fn pool_takes_matching_session_and_caps_returns() {
        let (mut runtime, _rx) = test_runtime();
        let info = ConnectionInfo {
            name: "site".into(),
            host: "box".into(),
            port: 22,
            protocol: Protocol::Sftp,
            username: "u".into(),
            password: None,
            private_key: None,
            initial_path: "/".into(),
        };
        let other = ConnectionInfo {
            host: "other".into(),
            ..info.clone()
        };
        runtime
            .worker_sessions
            .push(SessionHandle::Sftp(SftpSession::disconnected_for_tests(
                info.clone(),
            )));
        assert!(take_pooled_session(&mut runtime, &other).is_none());
        assert_eq!(runtime.worker_sessions.len(), 1);
        let taken = take_pooled_session(&mut runtime, &info).expect("matching session");
        assert!(runtime.worker_sessions.is_empty());
        return_pooled_session(&mut runtime, taken, &info, 1);
        assert_eq!(runtime.worker_sessions.len(), 1);
        let extra = SessionHandle::Sftp(SftpSession::disconnected_for_tests(info.clone()));
        return_pooled_session(&mut runtime, extra, &info, 1);
        assert_eq!(
            runtime.worker_sessions.len(),
            1,
            "cap rejects extra session"
        );
    }

    #[test]
    fn queue_transfer_still_clears_cancel_for_spawn_gate() {
        let mut app = AppState {
            connected: true,
            worker_cancel_requested: true,
            ..Default::default()
        };
        reduce(
            &mut app,
            Action::QueueTransfer(TransferJob::new(
                "/tmp/a",
                "/pub/a",
                TransferDirection::Upload,
            )),
        );
        assert!(!app.worker_cancel_requested);
        assert!(should_spawn(
            app.connected,
            app.worker_cancel_requested,
            0,
            2,
            app.queue.pending.len(),
        ));
    }
}
