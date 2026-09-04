use crate::{
    Action, AppState, ChoicePromptKind, FocusPane, PromptKind, QuickConnectField, SelectPolicy,
    TextField, TextPromptKind, Toast,
};
use dd_ftp_core::{FileEntry, Protocol};

fn resolve_select(
    current: usize,
    visible: &[&FileEntry],
    previous_name: Option<&str>,
    policy: SelectPolicy,
) -> usize {
    let last = visible.len().saturating_sub(1);
    match policy {
        SelectPolicy::Reset => 0,
        SelectPolicy::Clamp => current.min(last),
        SelectPolicy::PreserveName => previous_name
            .and_then(|name| visible.iter().position(|e| e.name == name))
            .unwrap_or(current.min(last)),
    }
}

fn preserve_filter_selection(
    state: &mut AppState,
    local_name: Option<String>,
    remote_name: Option<String>,
) {
    let idx = {
        let visible = state.visible_local();
        resolve_select(
            state.selected_local,
            &visible,
            local_name.as_deref(),
            SelectPolicy::PreserveName,
        )
    };
    state.selected_local = idx;
    let idx = {
        let visible = state.visible_remote();
        resolve_select(
            state.selected_remote,
            &visible,
            remote_name.as_deref(),
            SelectPolicy::PreserveName,
        )
    };
    state.selected_remote = idx;
}

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
        Action::SetLocalEntries { entries, select } => {
            let previous_name = state.selected_local_entry().map(|e| e.name.clone());
            state.local_entries = entries;
            let idx = {
                let visible = state.visible_local();
                resolve_select(
                    state.selected_local,
                    &visible,
                    previous_name.as_deref(),
                    select,
                )
            };
            state.selected_local = idx;
        }
        Action::SetRemoteEntries { entries, select } => {
            let previous_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.remote_entries = entries;
            let idx = {
                let visible = state.visible_remote();
                resolve_select(
                    state.selected_remote,
                    &visible,
                    previous_name.as_deref(),
                    select,
                )
            };
            state.selected_remote = idx;
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
            let next = match state.focus {
                FocusPane::Local => FocusPane::Remote,
                FocusPane::Remote => FocusPane::Queue,
                FocusPane::Queue => FocusPane::Local,
            };
            state.set_focus(next);
        }
        Action::ToggleHelp => {
            state.show_help = !state.show_help;
            if state.show_help {
                state.show_theme_debug = false;
            }
        }
        Action::ToggleThemeDebug => {
            state.show_theme_debug = !state.show_theme_debug;
            if state.show_theme_debug {
                state.show_help = false;
            }
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
                if state.selected_local < state.visible_local().len().saturating_sub(1) {
                    state.selected_local += 1;
                }
            }
            FocusPane::Remote => {
                if state.selected_remote < state.visible_remote().len().saturating_sub(1) {
                    state.selected_remote += 1;
                }
            }
            FocusPane::Queue => {}
        },
        Action::ToggleFilter => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.show_filter = !state.show_filter;
            if !state.show_filter {
                state.filter_pattern.clear();
            }
            preserve_filter_selection(state, local_name, remote_name);
        }
        Action::FilterInput(ch) => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.filter_pattern.push(ch);
            preserve_filter_selection(state, local_name, remote_name);
        }
        Action::FilterBackspace => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.filter_pattern.pop();
            preserve_filter_selection(state, local_name, remote_name);
        }
        Action::ClearFilter => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.filter_pattern.clear();
            preserve_filter_selection(state, local_name, remote_name);
        }
        Action::ToggleCompare => {
            state.show_compare = !state.show_compare;
        }
        Action::ShowCreatePrompt => {
            state.show_prompt = true;
            state.prompt_kind = Some(PromptKind::Text(TextPromptKind::CreateFile));
            state.prompt_value = TextField::default();
            state.prompt_target = None;
        }
        Action::ShowRenamePrompt => {
            state.show_prompt = true;
            state.prompt_kind = Some(PromptKind::Text(TextPromptKind::Rename));
            state.prompt_value = TextField::default();
            // Target will be set based on current selection
            state.prompt_target = None;
        }
        Action::ShowDeletePrompt => {
            state.show_prompt = true;
            state.prompt_kind = Some(PromptKind::Choice(ChoicePromptKind::ConfirmDelete));
            state.prompt_target = None;
        }
        Action::ShowChoicePrompt(kind) => {
            state.show_prompt = true;
            state.prompt_kind = Some(PromptKind::Choice(kind));
        }
        Action::PromptInput(ch) => {
            state.prompt_value.insert_char(ch);
        }
        Action::PromptBackspace => {
            state.prompt_value.backspace();
        }
        Action::ConfirmPrompt => {
            state.show_prompt = false;
            state.prompt_kind = None;
            state.prompt_value = TextField::default();
            state.prompt_target = None;
            state.host_key = None;
        }
        Action::CancelPrompt => {
            state.show_prompt = false;
            state.prompt_kind = None;
            state.prompt_value = TextField::default();
            state.prompt_target = None;
            state.host_key = None;
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

    #[test]
    fn confirm_quit_is_choice_and_does_not_touch_prompt_value() {
        let mut s = AppState {
            prompt_value: TextField::from_str("keep-me"),
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::ShowChoicePrompt(ChoicePromptKind::ConfirmQuit),
        );
        assert!(s.show_prompt);
        assert_eq!(
            s.prompt_kind,
            Some(PromptKind::Choice(ChoicePromptKind::ConfirmQuit))
        );
        assert_eq!(s.prompt_value.value, "keep-me");
        reduce(&mut s, Action::CancelPrompt);
        assert!(!s.show_prompt);
        assert_eq!(s.prompt_kind, None);
    }

    #[test]
    fn confirm_delete_is_choice_and_does_not_touch_prompt_value() {
        let mut s = AppState {
            prompt_value: TextField::from_str("keep-me"),
            ..Default::default()
        };
        reduce(&mut s, Action::ShowDeletePrompt);
        assert!(s.show_prompt);
        assert_eq!(
            s.prompt_kind,
            Some(PromptKind::Choice(ChoicePromptKind::ConfirmDelete))
        );
        assert_eq!(s.prompt_value.value, "keep-me");
        reduce(&mut s, Action::CancelPrompt);
        assert!(!s.show_prompt);
        assert_eq!(s.prompt_kind, None);
    }

    #[test]
    fn confirm_bookmark_delete_is_choice_and_does_not_touch_prompt_value() {
        let mut s = AppState {
            prompt_value: TextField::from_str("keep-me"),
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::ShowChoicePrompt(ChoicePromptKind::ConfirmBookmarkDelete),
        );
        assert!(s.show_prompt);
        assert_eq!(
            s.prompt_kind,
            Some(PromptKind::Choice(ChoicePromptKind::ConfirmBookmarkDelete))
        );
        assert_eq!(s.prompt_value.value, "keep-me");
        reduce(&mut s, Action::CancelPrompt);
        assert!(!s.show_prompt);
        assert_eq!(s.prompt_kind, None);
    }

    #[test]
    fn host_key_is_choice_and_does_not_touch_prompt_value() {
        let mut s = AppState {
            prompt_value: TextField::from_str("keep-me"),
            host_key: Some(crate::HostKeyView {
                host: "example.com".into(),
                port: 22,
                fingerprint: "abcd".into(),
                changed: false,
            }),
            ..Default::default()
        };
        reduce(&mut s, Action::ShowChoicePrompt(ChoicePromptKind::HostKey));
        assert!(s.show_prompt);
        assert_eq!(
            s.prompt_kind,
            Some(PromptKind::Choice(ChoicePromptKind::HostKey))
        );
        assert_eq!(s.prompt_value.value, "keep-me");
        reduce(&mut s, Action::CancelPrompt);
        assert!(!s.show_prompt);
        assert_eq!(s.prompt_kind, None);
        assert!(s.host_key.is_none());
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::*;
    use crate::AppState;

    #[test]
    fn toggle_help_twice_closes() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ToggleHelp);
        assert!(s.show_help);
        reduce(&mut s, Action::ToggleHelp);
        assert!(!s.show_help);
    }

    #[test]
    fn toggle_theme_debug_twice_closes() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ToggleThemeDebug);
        assert!(s.show_theme_debug);
        reduce(&mut s, Action::ToggleThemeDebug);
        assert!(!s.show_theme_debug);
    }

    #[test]
    fn toggle_help_clears_theme_debug() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ToggleThemeDebug);
        assert!(s.show_theme_debug);
        reduce(&mut s, Action::ToggleHelp);
        assert!(s.show_help);
        assert!(!s.show_theme_debug);
    }

    #[test]
    fn toggle_compare_closes_compare() {
        let mut s = AppState {
            show_compare: true,
            ..Default::default()
        };
        reduce(&mut s, Action::ToggleCompare);
        assert!(!s.show_compare);
    }

    #[test]
    fn toggle_theme_debug_clears_help() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ToggleHelp);
        reduce(&mut s, Action::ToggleThemeDebug);
        assert!(s.show_theme_debug);
        assert!(!s.show_help);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::{AppState, SelectPolicy};
    use dd_ftp_core::{EntryKind, FileEntry};

    fn fe(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: name.to_string(),
            kind: EntryKind::File,
            size: 0,
            modified: None,
            permissions: None,
        }
    }

    #[test]
    fn select_down_does_not_walk_into_filtered_out_rows() {
        let mut s = AppState {
            local_entries: vec![fe("foo"), fe("skip"), fe("food")],
            filter_pattern: "foo".into(),
            selected_local: 0,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::SelectDown);
        assert_eq!(s.selected_local, 1);
        assert_eq!(s.selected_local_entry().unwrap().name, "food");
        reduce(&mut s, Action::SelectDown);
        assert_eq!(s.selected_local, 1);
        assert_eq!(s.selected_local_entry().unwrap().name, "food");
    }

    #[test]
    fn set_remote_entries_reset_selects_index_zero() {
        let mut s = AppState {
            remote_entries: vec![fe("old")],
            selected_remote: 0,
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::SetRemoteEntries {
                entries: vec![fe("a"), fe("b"), fe("c")],
                select: SelectPolicy::Reset,
            },
        );
        assert_eq!(s.selected_remote, 0);
        assert_eq!(s.selected_remote_entry().unwrap().name, "a");
    }

    #[test]
    fn set_remote_entries_preserve_name_keeps_row_when_refresh_prepends() {
        let mut s = AppState {
            remote_entries: vec![fe("keep"), fe("z")],
            selected_remote: 0,
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::SetRemoteEntries {
                entries: vec![fe("new"), fe("keep"), fe("z")],
                select: SelectPolicy::PreserveName,
            },
        );
        assert_eq!(s.selected_remote, 1);
        assert_eq!(s.selected_remote_entry().unwrap().name, "keep");
    }

    #[test]
    fn focus_next_pane_remembers_last_file_pane_across_queue() {
        let mut s = AppState::default();
        reduce(&mut s, Action::FocusNextPane);
        assert_eq!(s.focus, FocusPane::Remote);
        assert_eq!(s.last_file_pane, FocusPane::Remote);
        reduce(&mut s, Action::FocusNextPane);
        assert_eq!(s.focus, FocusPane::Queue);
        assert_eq!(s.last_file_pane, FocusPane::Remote);
        assert_eq!(s.filter_count_pane(), FocusPane::Remote);
        reduce(&mut s, Action::FocusNextPane);
        assert_eq!(s.focus, FocusPane::Local);
        assert_eq!(s.last_file_pane, FocusPane::Local);
    }

    #[test]
    fn set_local_entries_preserve_name_clamps_when_row_vanishes() {
        let mut s = AppState {
            local_entries: vec![fe("keep"), fe("gone")],
            selected_local: 1,
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::SetLocalEntries {
                entries: vec![fe("keep")],
                select: SelectPolicy::PreserveName,
            },
        );
        assert_eq!(s.selected_local, 0);
        assert_eq!(s.selected_local_entry().unwrap().name, "keep");
    }
}
