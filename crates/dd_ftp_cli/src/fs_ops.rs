use std::path::Path;

use dd_ftp_app::{reduce, Action, AppState};
use dd_ftp_core::{RemoteSession, TransferDirection, TransferJob};

use crate::paths::safe_local_child;
use crate::paths::safe_remote_child;
use crate::session::{io_busy, FsKind, Runtime, SessionHandle};

pub(crate) fn chmod(app: &mut AppState, runtime: &mut Runtime, path: &str, mode: u32) {
    if io_busy(runtime) {
        return;
    }
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }
    let path = path.to_string();
    runtime.begin_fs(app, FsKind::Chmod, true, format!("chmod {:o} {path}", mode));
    runtime.spawn_remote_fs(FsKind::Chmod, move |handle| async move {
        let mut handle = handle;
        let result = match &mut handle {
            Some(SessionHandle::Sftp(s)) => s.set_permissions(&path, mode).await,
            Some(SessionHandle::Ftp(f)) => f.set_permissions(&path, mode).await,
            None => Err(anyhow::anyhow!("not connected")),
        };
        (handle, result)
    });
}

pub(crate) fn create_file(app: &mut AppState, runtime: &mut Runtime, name: &str) {
    if io_busy(runtime) {
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
            runtime.begin_fs(
                app,
                FsKind::CreateFile,
                false,
                format!("Created file: {name}"),
            );
            runtime.spawn_local_fs(FsKind::CreateFile, move || {
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
            runtime.begin_fs(
                app,
                FsKind::CreateFile,
                true,
                format!("Created file: {name}"),
            );
            runtime.spawn_remote_fs(FsKind::CreateFile, move |handle| async move {
                let mut handle = handle;
                let result = async {
                    std::fs::File::create(&temp_file)?;
                    let job = TransferJob::new(
                        temp_file.to_string_lossy().to_string(),
                        path,
                        TransferDirection::Upload,
                    );
                    let r = match &mut handle {
                        Some(SessionHandle::Ftp(f)) => f.upload(&job).await,
                        Some(SessionHandle::Sftp(s)) => s.upload(&job).await,
                        None => Err(anyhow::anyhow!("not connected")),
                    };
                    let _ = std::fs::remove_file(&temp_file);
                    r
                }
                .await;
                (handle, result)
            });
        }
        _ => {}
    }
}

pub(crate) fn create_folder(app: &mut AppState, runtime: &mut Runtime, name: &str) {
    if io_busy(runtime) {
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
            runtime.begin_fs(
                app,
                FsKind::CreateFolder,
                false,
                format!("Created folder: {name}"),
            );
            runtime.spawn_local_fs(FsKind::CreateFolder, move || {
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
            runtime.begin_fs(
                app,
                FsKind::CreateFolder,
                true,
                format!("Created folder: {name}"),
            );
            runtime.spawn_remote_fs(FsKind::CreateFolder, move |handle| async move {
                let mut handle = handle;
                let result = match &mut handle {
                    Some(SessionHandle::Ftp(f)) => f.create_dir(&path).await,
                    Some(SessionHandle::Sftp(s)) => s.create_dir(&path).await,
                    None => Err(anyhow::anyhow!("not connected")),
                };
                (handle, result)
            });
        }
        _ => {}
    }
}

pub(crate) fn rename_item(
    app: &mut AppState,
    runtime: &mut Runtime,
    _target: &str,
    new_name: &str,
) {
    if io_busy(runtime) {
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
            runtime.begin_fs(
                app,
                FsKind::Rename,
                false,
                format!("Renamed to: {new_name}"),
            );
            runtime.spawn_local_fs(FsKind::Rename, move || {
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
            runtime.begin_fs(app, FsKind::Rename, true, format!("Renamed to: {new_name}"));
            runtime.spawn_remote_fs(FsKind::Rename, move |handle| async move {
                let mut handle = handle;
                let result = match &mut handle {
                    Some(SessionHandle::Ftp(f)) => f.rename(&from, &to).await,
                    Some(SessionHandle::Sftp(s)) => s.rename(&from, &to).await,
                    None => Err(anyhow::anyhow!("not connected")),
                };
                (handle, result)
            });
        }
        _ => {}
    }
}

pub(crate) fn delete_item(app: &mut AppState, runtime: &mut Runtime, target: &str) {
    if io_busy(runtime) {
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
            runtime.begin_fs(app, FsKind::Delete, false, format!("Deleted: {target_str}"));
            runtime.spawn_local_fs(FsKind::Delete, move || {
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
            runtime.begin_fs(app, FsKind::Delete, true, format!("Deleted: {path}"));
            runtime.spawn_remote_fs(FsKind::Delete, move |handle| async move {
                let mut handle = handle;
                let result = match &mut handle {
                    Some(SessionHandle::Ftp(f)) => {
                        if is_dir {
                            f.remove_dir(&path).await
                        } else {
                            f.remove_file(&path).await
                        }
                    }
                    Some(SessionHandle::Sftp(s)) => {
                        if is_dir {
                            s.remove_dir(&path).await
                        } else {
                            s.remove_file(&path).await
                        }
                    }
                    None => Err(anyhow::anyhow!("not connected")),
                };
                (handle, result)
            });
        }
        _ => {}
    }
}
