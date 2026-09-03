use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use dd_ftp_app::{reduce, Action, AppState, SelectPolicy};
use dd_ftp_core::{Protocol, RemoteSession, TransferDirection};
use dd_ftp_ftp::UnifiedFtpSession;
use dd_ftp_protocols::SftpSession;
use dd_ftp_storage::SecretStore;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::paths;
use crate::session::{connection_info_from_env, list_remote, Runtime};

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
        let generation = runtime.generation;

        let handle = tokio::spawn(async move {
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
                                        generation,
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
                                        .download_with_progress(&job, cancel.clone(), progress_tx)
                                        .await
                                }
                            }
                        }
                        Err(err) => Err(err),
                    }
                }
                Protocol::Ftp | Protocol::Ftps => {
                    let mut unified = UnifiedFtpSession::new();

                    match unified.connect(info.clone()).await {
                        Ok(_) => {
                            let progress_tx = {
                                let tx_progress = tx_clone.clone();
                                move |job_id: Uuid, transferred, size| {
                                    let _ = tx_progress.send(WorkerMessage::Progress {
                                        generation,
                                        job_id,
                                        transferred_bytes: transferred,
                                        size_bytes: size,
                                    });
                                }
                            };
                            let result = match job.direction {
                                TransferDirection::Upload => {
                                    unified
                                        .upload_with_progress(&job, cancel.clone(), progress_tx)
                                        .await
                                }
                                TransferDirection::Download => {
                                    unified
                                        .download_with_progress(&job, cancel.clone(), progress_tx)
                                        .await
                                }
                            };
                            unified.disconnect().await.ok();
                            result
                        }
                        Err(err) => Err(err),
                    }
                }
            };

            let _ = tx_clone.send(WorkerMessage::Done {
                generation,
                result: WorkerResult {
                    job,
                    outcome,
                    was_cancelled: cancel.load(Ordering::Relaxed),
                    cancel_flag: cancel,
                },
            });
        });
        runtime.worker_handles.push(handle);
    }
}

pub(crate) fn handle_worker_result(
    app: &mut AppState,
    runtime: &mut Runtime,
    mut msg: WorkerResult,
) {
    if app.worker_cancel_requested || msg.was_cancelled {
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
                    entries: paths::local_list(&app.local_cwd),
                    select: SelectPolicy::PreserveName,
                },
            );
            if app.connected && runtime.pending_scan.is_empty() {
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
        PendingFile {
            local_path: local.to_string(),
            remote_path: remote.to_string(),
            direction: TransferDirection::Upload,
            size_bytes: Some(1),
        }
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
