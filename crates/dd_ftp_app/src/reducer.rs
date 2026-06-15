use crate::{Action, AppState, FocusPane, PromptType, QuickConnectField, TextField, Toast};
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
                state.qc_hydrate();
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
            state.qc_hydrate();
        }
        Action::QuickConnectPrevField => {
            state.quick_connect_field = state.quick_connect_field.prev();
            state.qc_hydrate();
        }
        Action::QuickConnectInput(ch) => {
            if state.quick_connect_field != QuickConnectField::Port || ch.is_ascii_digit() {
                state.qc_field.insert_char(ch);
                state.qc_flush();
            }
        }
        Action::QuickConnectBackspace => {
            state.qc_field.backspace();
            state.qc_flush();
        }
        Action::QuickConnectSyncField => {
            state.qc_hydrate();
        }
        Action::QuickConnectBeginSelect(i) => {
            state.qc_field.begin_drag(i);
        }
        Action::QuickConnectExtendSelect(i) => {
            state.qc_field.extend_drag(i);
        }
        Action::QuickConnectMoveCursor { dir, shift } => {
            state.qc_field.move_cursor(dir, shift);
        }
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
            state.qc_hydrate();
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
            state.toast = Some(Toast::error(msg.clone()));
            state.status = format!("Error: {msg}");
        }
        Action::ClearError => {
            state.toast = None;
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
        Action::ToggleThemeDebug => {
            state.show_theme_debug = !state.show_theme_debug;
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
        Action::ToggleFilter => {
            state.show_filter = !state.show_filter;
            if !state.show_filter {
                state.filter_pattern.clear();
            }
        }
        Action::FilterInput(ch) => {
            state.filter_pattern.push(ch);
        }
        Action::FilterBackspace => {
            state.filter_pattern.pop();
        }
        Action::ClearFilter => {
            state.filter_pattern.clear();
        }
        Action::ToggleCompare => {
            state.show_compare = !state.show_compare;
        }
        Action::ShowCreatePrompt => {
            state.show_prompt = true;
            state.prompt_type = Some(PromptType::CreateFile);
            state.prompt_value = TextField::default();
            state.prompt_target = None;
        }
        Action::ShowRenamePrompt => {
            state.show_prompt = true;
            state.prompt_type = Some(PromptType::Rename);
            state.prompt_value = TextField::default();
            // Target will be set based on current selection
            state.prompt_target = None;
        }
        Action::ShowDeletePrompt => {
            state.show_prompt = true;
            state.prompt_type = Some(PromptType::Delete);
            state.prompt_value = TextField::default();
            state.prompt_target = None;
        }
        Action::PromptInput(ch) => {
            state.prompt_value.insert_char(ch);
        }
        Action::PromptBackspace => {
            state.prompt_value.backspace();
        }
        Action::ConfirmPrompt => {
            state.show_prompt = false;
            state.prompt_type = None;
            state.prompt_value = TextField::default();
            state.prompt_target = None;
        }
        Action::CancelPrompt => {
            state.show_prompt = false;
            state.prompt_type = None;
            state.prompt_value = TextField::default();
            state.prompt_target = None;
        }
        Action::CreateFile(_)
        | Action::CreateFolder(_)
        | Action::RenameItem(_, _)
        | Action::DeleteItem(_) => {
            // These are handled by the main loop, not the reducer
        }
    }
}

#[cfg(test)]
mod qc_tests {
    use super::*;
    use crate::{AppState, QuickConnectField};

    #[test]
    fn qc_field_change_hydrates_text_field() {
        let mut s = AppState::default();
        s.quick_connect.host = "example.com".into();
        s.quick_connect_field = QuickConnectField::Host;
        reduce(&mut s, Action::QuickConnectSyncField); // hydrate active field
        assert_eq!(s.qc_field.value, "example.com");
        assert_eq!(s.qc_field.cursor, 11);
    }

    #[test]
    fn qc_backspace_with_selection_clears_field() {
        let mut s = AppState::default();
        s.quick_connect.host = "abc".into();
        s.quick_connect_field = QuickConnectField::Host;
        reduce(&mut s, Action::QuickConnectSyncField);
        s.qc_field.anchor = Some(0);
        s.qc_field.cursor = 3; // whole value selected
        reduce(&mut s, Action::QuickConnectBackspace);
        assert_eq!(s.quick_connect.host, "");
    }
}

#[cfg(test)]
mod prompt_tests {
    use super::*;
    use crate::AppState;

    #[test]
    fn prompt_input_inserts_at_cursor() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ShowCreatePrompt);
        reduce(&mut s, Action::PromptInput('a'));
        reduce(&mut s, Action::PromptInput('b'));
        assert_eq!(s.prompt_value.value, "ab");
        assert_eq!(s.prompt_value.cursor, 2);
    }
}
