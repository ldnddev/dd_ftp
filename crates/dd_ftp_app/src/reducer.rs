use crate::{Action, AppState, FocusPane, QuickConnectField};
use dd_ftp_core::Protocol;

pub fn reduce(state: &mut AppState, action: Action) {
    match action {
        Action::Connect(info) => {
            state.status = format!("Connecting to {}...", info.host);
        }
        Action::Disconnect => {
            state.connected = false;
            state.status = "Disconnected".to_string();
        }
        Action::SetConnected(value) => {
            state.connected = value;
            state.status = if value {
                "Connected".to_string()
            } else {
                "Disconnected".to_string()
            };
        }
        Action::SetLocalEntries(entries) => {
            state.local_entries = entries;
            state.selected_local = 0;
        }
        Action::SetRemoteEntries(entries) => {
            state.remote_entries = entries;
            state.selected_remote = 0;
        }
        Action::SetBookmarks(bookmarks) => {
            state.bookmarks = bookmarks;
            state.selected_bookmark = 0;
        }
        Action::SelectNextBookmark => {
            if !state.bookmarks.is_empty() {
                state.selected_bookmark = (state.selected_bookmark + 1) % state.bookmarks.len();
                let bm = &state.bookmarks[state.selected_bookmark];
                state.status = format!("Bookmark: {} ({})", bm.name, bm.host);
            } else {
                state.status = "No bookmarks saved".to_string();
            }
        }
        Action::SelectPrevBookmark => {
            if !state.bookmarks.is_empty() {
                if state.selected_bookmark == 0 {
                    state.selected_bookmark = state.bookmarks.len().saturating_sub(1);
                } else {
                    state.selected_bookmark -= 1;
                }
                let bm = &state.bookmarks[state.selected_bookmark];
                state.status = format!("Bookmark: {} ({})", bm.name, bm.host);
            } else {
                state.status = "No bookmarks saved".to_string();
            }
        }
        Action::ToggleQuickConnect => {
            state.show_quick_connect = !state.show_quick_connect;
            if state.show_quick_connect {
                state.show_bookmarks = false;
                state.quick_connect_field = QuickConnectField::Name;
            }
        }
        Action::ToggleBookmarks => {
            state.show_bookmarks = !state.show_bookmarks;
            if state.show_bookmarks {
                state.show_quick_connect = false;
            }
        }
        Action::QuickConnectNextField => {
            state.quick_connect_field = state.quick_connect_field.next();
        }
        Action::QuickConnectPrevField => {
            state.quick_connect_field = state.quick_connect_field.prev();
        }
        Action::QuickConnectInput(ch) => match state.quick_connect_field {
            QuickConnectField::Name => state.quick_connect.name.push(ch),
            QuickConnectField::Host => state.quick_connect.host.push(ch),
            QuickConnectField::Port => {
                if ch.is_ascii_digit() {
                    let mut s = state.quick_connect.port.to_string();
                    if s == "0" {
                        s.clear();
                    }
                    s.push(ch);
                    if let Ok(p) = s.parse::<u16>() {
                        state.quick_connect.port = p;
                    }
                }
            }
            QuickConnectField::Username => state.quick_connect.username.push(ch),
            QuickConnectField::Password => {
                let mut pw = state.quick_connect.password.clone().unwrap_or_default();
                pw.push(ch);
                state.quick_connect.password = Some(pw);
            }
            QuickConnectField::PrivateKey => {
                let mut key = state.quick_connect.private_key.clone().unwrap_or_default();
                key.push(ch);
                state.quick_connect.private_key = Some(key);
            }
            QuickConnectField::Protocol => {}
            QuickConnectField::Path => state.quick_connect.initial_path.push(ch),
        },
        Action::QuickConnectBackspace => match state.quick_connect_field {
            QuickConnectField::Name => {
                state.quick_connect.name.pop();
            }
            QuickConnectField::Host => {
                state.quick_connect.host.pop();
            }
            QuickConnectField::Port => {
                let mut s = state.quick_connect.port.to_string();
                s.pop();
                state.quick_connect.port = if s.is_empty() {
                    0
                } else {
                    s.parse::<u16>().unwrap_or(state.quick_connect.port)
                };
            }
            QuickConnectField::Username => {
                state.quick_connect.username.pop();
            }
            QuickConnectField::Password => {
                let mut pw = state.quick_connect.password.clone().unwrap_or_default();
                pw.pop();
                state.quick_connect.password = Some(pw);
            }
            QuickConnectField::PrivateKey => {
                let mut key = state.quick_connect.private_key.clone().unwrap_or_default();
                key.pop();
                state.quick_connect.private_key = Some(key);
            }
            QuickConnectField::Protocol => {}
            QuickConnectField::Path => {
                state.quick_connect.initial_path.pop();
            }
        },
        Action::QuickConnectSetProtocolNext => {
            state.quick_connect.protocol = match state.quick_connect.protocol {
                Protocol::Sftp => Protocol::Ftp,
                Protocol::Ftp => Protocol::Ftps,
                Protocol::Ftps => Protocol::Sftp,
            };
        }
        Action::QuickConnectSetProtocolPrev => {
            state.quick_connect.protocol = match state.quick_connect.protocol {
                Protocol::Sftp => Protocol::Ftps,
                Protocol::Ftp => Protocol::Sftp,
                Protocol::Ftps => Protocol::Ftp,
            };
        }
        Action::QuickConnectSetFromBookmark(info) => {
            state.quick_connect = info;
            state.quick_connect_field = QuickConnectField::Name;
            state.status = "Loaded bookmark into quick connect".to_string();
        }
        Action::QueueTransfer(job) => {
            state.queue.enqueue(job);
            state.status = format!("Queue: {} pending", state.queue.pending.len());
        }
        Action::StartNextTransfer => {
            if let Some(job) = state.queue.start_next() {
                state.status = format!("Transfer active: {}", job.id);
            } else {
                state.status = "Queue is empty".to_string();
            }
        }
        Action::MarkTransferCompleted(job) => {
            state.queue.mark_completed(job);
            state.status = format!(
                "Transfer complete. Pending: {} Active: {}",
                state.queue.pending.len(),
                state.queue.active.len()
            );
        }
        Action::MarkTransferFailed(job) => {
            state.queue.mark_failed(job);
            state.status = format!(
                "Transfer failed. Pending: {} Active: {}",
                state.queue.pending.len(),
                state.queue.active.len()
            );
        }
        Action::MarkTransferCancelled(job) => {
            state.queue.mark_cancelled(job);
            state.status = format!(
                "Transfer cancelled. Pending: {} Active: {}",
                state.queue.pending.len(),
                state.queue.active.len()
            );
        }
        Action::RetryLastFailed => {
            if state.queue.retry_last_failed().is_some() {
                state.status = format!(
                    "Requeued last failed transfer. Pending: {}",
                    state.queue.pending.len()
                );
            } else {
                state.status = "No failed transfer to retry".to_string();
            }
        }
        Action::UpdateTransferProgress {
            job_id,
            transferred_bytes,
            size_bytes,
        } => {
            state
                .queue
                .update_active_progress(job_id, transferred_bytes, size_bytes);
        }
        Action::ClearPendingTransfers => {
            let removed = state.queue.clear_pending();
            state.status = format!("Cleared {removed} pending transfer(s)");
        }
        Action::SetStatus(msg) => {
            state.status = msg;
        }
        Action::ShowError(msg) => {
            state.error_modal = Some(msg.clone());
            state.status = format!("Error: {msg}");
        }
        Action::ClearError => {
            state.error_modal = None;
        }
        Action::FocusNextPane => {
            state.focus = match state.focus {
                FocusPane::Local => FocusPane::Remote,
                FocusPane::Remote => FocusPane::Queue,
                FocusPane::Queue => FocusPane::Local,
            };
        }
        Action::ToggleHelp => {
            state.show_help = !state.show_help;
        }
        Action::SelectUp => match state.focus {
            FocusPane::Local => {
                if state.selected_local > 0 {
                    state.selected_local -= 1;
                }
            }
            FocusPane::Remote => {
                if state.selected_remote > 0 {
                    state.selected_remote -= 1;
                }
            }
            FocusPane::Queue => {}
        },
        Action::SelectDown => match state.focus {
            FocusPane::Local => {
                if state.selected_local < state.local_entries.len().saturating_sub(1) {
                    state.selected_local += 1;
                }
            }
            FocusPane::Remote => {
                if state.selected_remote < state.remote_entries.len().saturating_sub(1) {
                    state.selected_remote += 1;
                }
            }
            FocusPane::Queue => {}
        },
    }
}
