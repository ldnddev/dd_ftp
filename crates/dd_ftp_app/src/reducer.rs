use crate::{
    is_dot_or_dotdot, Action, AppState, ChoicePromptKind, FocusPane, PromptKind, QuickConnectField,
    SelectPolicy, TextField, TextPromptKind, Toast,
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
            if select == SelectPolicy::Reset {
                state.marked_local.clear();
            }
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
            if select == SelectPolicy::Reset {
                state.marked_remote.clear();
            }
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
            state.worker_cancel_requested = false;
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
            state.worker_cancel_requested = false;
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
        Action::SetWorkerView {
            active_count,
            running,
            cancel_requested,
        } => {
            state.worker_active_count = active_count;
            state.worker_running = running;
            state.worker_cancel_requested = cancel_requested;
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
        Action::SetFocus(pane) => {
            state.set_focus(pane);
        }
        Action::SelectIndex { pane, index } => match pane {
            FocusPane::Local => {
                let last = state.visible_local().len().saturating_sub(1);
                state.selected_local = index.min(last);
            }
            FocusPane::Remote => {
                let last = state.visible_remote().len().saturating_sub(1);
                state.selected_remote = index.min(last);
            }
            FocusPane::Queue => {}
        },
        Action::HelpScroll(delta) => {
            if delta < 0 {
                state.help_scroll = state
                    .help_scroll
                    .saturating_sub(delta.unsigned_abs() as usize);
            } else {
                state.help_scroll = state.help_scroll.saturating_add(delta as usize);
            }
        }
        Action::QueueScroll(delta) => {
            if delta < 0 {
                state.queue_scroll = state
                    .queue_scroll
                    .saturating_sub(delta.unsigned_abs() as usize);
            } else {
                state.queue_scroll = state.queue_scroll.saturating_add(delta as usize);
            }
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
        Action::ShowChmodPrompt { mode } => {
            state.show_prompt = true;
            state.prompt_kind = Some(PromptKind::Text(TextPromptKind::Chmod));
            state.prompt_value = TextField::from_str(&mode);
            state.prompt_target = None;
        }
        Action::ToggleMark => {
            let info = match state.focus {
                FocusPane::Local => state
                    .selected_local_entry()
                    .map(|e| (e.path.clone(), is_dot_or_dotdot(&e.name))),
                FocusPane::Remote => state
                    .selected_remote_entry()
                    .map(|e| (e.path.clone(), is_dot_or_dotdot(&e.name))),
                FocusPane::Queue => None,
            };
            if let Some((path, skip)) = info {
                if !skip {
                    let marks = match state.focus {
                        FocusPane::Local => &mut state.marked_local,
                        FocusPane::Remote => &mut state.marked_remote,
                        FocusPane::Queue => return,
                    };
                    if !marks.remove(&path) {
                        marks.insert(path);
                    }
                }
            }
        }
        Action::ClearMarks { pane } => match pane {
            FocusPane::Local => state.marked_local.clear(),
            FocusPane::Remote => state.marked_remote.clear(),
            FocusPane::Queue => {}
        },
        Action::CycleSort => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.sort_key = state.sort_key.next();
            preserve_filter_selection(state, local_name, remote_name);
        }
        Action::ToggleSortDir => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.sort_asc = !state.sort_asc;
            preserve_filter_selection(state, local_name, remote_name);
        }
        Action::ToggleHideDotfiles => {
            let local_name = state.selected_local_entry().map(|e| e.name.clone());
            let remote_name = state.selected_remote_entry().map(|e| e.name.clone());
            state.hide_dotfiles = !state.hide_dotfiles;
            preserve_filter_selection(state, local_name, remote_name);
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
        Action::SetOverwritePolicy(policy) => {
            if let Some(ow) = state.overwrite.as_mut() {
                ow.apply_all = policy;
            }
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
            state.overwrite = None;
        }
        Action::CancelPrompt => {
            state.show_prompt = false;
            state.prompt_kind = None;
            state.prompt_value = TextField::default();
            state.prompt_target = None;
            state.host_key = None;
            state.overwrite = None;
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
    use crate::{AppState, OverwritePolicy};

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

    fn pending(name: &str) -> crate::PendingFile {
        crate::PendingFile {
            local_path: format!("/tmp/{name}"),
            remote_path: format!("/pub/{name}"),
            direction: dd_ftp_core::TransferDirection::Upload,
            size_bytes: Some(1),
        }
    }

    #[test]
    fn overwrite_default_policy_is_skip() {
        let mut s = AppState {
            prompt_value: TextField::from_str("keep-me"),
            overwrite: Some(crate::OverwritePrompt {
                current: pending("a.txt"),
                remaining: vec![pending("b.txt")],
                apply_all: OverwritePolicy::Ask,
            }),
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::ShowChoicePrompt(ChoicePromptKind::Overwrite),
        );
        assert!(s.show_prompt);
        assert_eq!(
            s.prompt_kind,
            Some(PromptKind::Choice(ChoicePromptKind::Overwrite))
        );
        assert_eq!(s.prompt_value.value, "keep-me");
        assert_eq!(
            s.overwrite.as_ref().map(|o| o.apply_all),
            Some(OverwritePolicy::Ask)
        );
    }

    #[test]
    fn overwrite_all_applies_to_remaining() {
        let mut s = AppState {
            overwrite: Some(crate::OverwritePrompt {
                current: pending("a.txt"),
                remaining: vec![pending("b.txt"), pending("c.txt")],
                apply_all: OverwritePolicy::Ask,
            }),
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::ShowChoicePrompt(ChoicePromptKind::Overwrite),
        );
        reduce(
            &mut s,
            Action::SetOverwritePolicy(OverwritePolicy::OverwriteAll),
        );
        let ow = s.overwrite.as_ref().expect("overwrite snapshot");
        assert_eq!(ow.apply_all, OverwritePolicy::OverwriteAll);
        assert_eq!(ow.remaining.len(), 2);
        assert_eq!(ow.remaining[0].remote_path, "/pub/b.txt");
        assert_eq!(ow.remaining[1].remote_path, "/pub/c.txt");
    }

    #[test]
    fn overwrite_esc_leaves_already_queued_jobs() {
        use dd_ftp_core::{TransferDirection, TransferJob};
        let mut s = AppState::default();
        reduce(
            &mut s,
            Action::QueueTransfer(TransferJob::new(
                "/tmp/kept",
                "/pub/kept",
                TransferDirection::Upload,
            )),
        );
        assert_eq!(s.queue.pending.len(), 1);
        s.overwrite = Some(crate::OverwritePrompt {
            current: pending("conflict.txt"),
            remaining: vec![pending("later.txt")],
            apply_all: OverwritePolicy::Ask,
        });
        reduce(
            &mut s,
            Action::ShowChoicePrompt(ChoicePromptKind::Overwrite),
        );
        reduce(&mut s, Action::CancelPrompt);
        assert!(!s.show_prompt);
        assert_eq!(s.prompt_kind, None);
        assert!(s.overwrite.is_none());
        assert_eq!(s.queue.pending.len(), 1);
        assert_eq!(s.queue.pending[0].remote_path, "/pub/kept");
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
        assert_eq!(s.selected_remote_entry().unwrap().name, "keep");
        assert_eq!(s.selected_remote, 0);
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

#[cfg(test)]
mod worker_flag_tests {
    use super::*;
    use crate::AppState;
    use dd_ftp_core::{TransferDirection, TransferJob};

    #[test]
    fn queue_transfer_clears_worker_cancel_requested() {
        let mut s = AppState {
            worker_cancel_requested: true,
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::QueueTransfer(TransferJob::new(
                "/tmp/a",
                "/pub/a",
                TransferDirection::Upload,
            )),
        );
        assert!(!s.worker_cancel_requested);
        assert_eq!(s.queue.pending.len(), 1);
    }

    #[test]
    fn retry_last_failed_clears_worker_cancel_requested() {
        let mut s = AppState {
            worker_cancel_requested: true,
            ..Default::default()
        };
        s.queue.mark_failed(TransferJob::new(
            "/tmp/a",
            "/pub/a",
            TransferDirection::Upload,
        ));
        reduce(&mut s, Action::RetryLastFailed);
        assert!(!s.worker_cancel_requested);
        assert_eq!(s.queue.pending.len(), 1);
    }

    #[test]
    fn set_connected_true_does_not_clear_worker_cancel_requested() {
        let mut s = AppState {
            worker_cancel_requested: true,
            ..Default::default()
        };
        reduce(&mut s, Action::SetConnected(true));
        assert!(s.connected);
        assert!(s.worker_cancel_requested);
    }

    #[test]
    fn set_worker_view_zeros_and_sets_cancel() {
        let mut s = AppState {
            worker_active_count: 2,
            worker_running: true,
            worker_cancel_requested: false,
            ..Default::default()
        };
        reduce(
            &mut s,
            Action::SetWorkerView {
                active_count: 0,
                running: false,
                cancel_requested: true,
            },
        );
        assert_eq!(s.worker_active_count, 0);
        assert!(!s.worker_running);
        assert!(s.worker_cancel_requested);
    }
}

#[cfg(test)]
mod reduce_table_tests {
    use super::*;
    use crate::{AppState, SelectPolicy};
    use dd_ftp_core::{EntryKind, FileEntry, TransferDirection, TransferJob, TransferStatus};

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

    fn job(local: &str, remote: &str) -> TransferJob {
        TransferJob::new(local, remote, TransferDirection::Upload)
    }

    #[test]
    fn set_local_entries_policies() {
        struct Case {
            name: &'static str,
            initial: &'static [&'static str],
            selected: usize,
            next: &'static [&'static str],
            policy: SelectPolicy,
            want_name: &'static str,
            want_idx: usize,
        }
        let cases = [
            Case {
                name: "preserve_name",
                initial: &["a", "keep", "z"],
                selected: 1,
                next: &["new", "keep", "z"],
                policy: SelectPolicy::PreserveName,
                want_name: "keep",
                want_idx: 0,
            },
            Case {
                name: "reset",
                initial: &["a", "b"],
                selected: 1,
                next: &["x", "y", "z"],
                policy: SelectPolicy::Reset,
                want_name: "x",
                want_idx: 0,
            },
            Case {
                name: "clamp",
                initial: &["a", "b", "c"],
                selected: 2,
                next: &["only"],
                policy: SelectPolicy::Clamp,
                want_name: "only",
                want_idx: 0,
            },
        ];
        for case in cases {
            let mut s = AppState {
                local_entries: case.initial.iter().map(|n| fe(n)).collect(),
                selected_local: case.selected,
                ..Default::default()
            };
            reduce(
                &mut s,
                Action::SetLocalEntries {
                    entries: case.next.iter().map(|n| fe(n)).collect(),
                    select: case.policy,
                },
            );
            assert_eq!(s.selected_local, case.want_idx, "{}", case.name);
            assert_eq!(
                s.selected_local_entry().unwrap().name,
                case.want_name,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn set_remote_entries_policies() {
        struct Case {
            name: &'static str,
            initial: &'static [&'static str],
            selected: usize,
            next: &'static [&'static str],
            policy: SelectPolicy,
            want_name: &'static str,
            want_idx: usize,
        }
        let cases = [
            Case {
                name: "preserve_name",
                initial: &["a", "keep", "z"],
                selected: 1,
                next: &["new", "keep", "z"],
                policy: SelectPolicy::PreserveName,
                want_name: "keep",
                want_idx: 0,
            },
            Case {
                name: "reset",
                initial: &["a", "b"],
                selected: 1,
                next: &["x", "y", "z"],
                policy: SelectPolicy::Reset,
                want_name: "x",
                want_idx: 0,
            },
            Case {
                name: "clamp",
                initial: &["a", "b", "c"],
                selected: 2,
                next: &["only"],
                policy: SelectPolicy::Clamp,
                want_name: "only",
                want_idx: 0,
            },
        ];
        for case in cases {
            let mut s = AppState {
                remote_entries: case.initial.iter().map(|n| fe(n)).collect(),
                selected_remote: case.selected,
                ..Default::default()
            };
            reduce(
                &mut s,
                Action::SetRemoteEntries {
                    entries: case.next.iter().map(|n| fe(n)).collect(),
                    select: case.policy,
                },
            );
            assert_eq!(s.selected_remote, case.want_idx, "{}", case.name);
            assert_eq!(
                s.selected_remote_entry().unwrap().name,
                case.want_name,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn select_up_down_at_bounds() {
        let mut s = AppState {
            local_entries: vec![fe("a"), fe("b"), fe("c")],
            selected_local: 0,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::SelectUp);
        assert_eq!(s.selected_local, 0);
        reduce(&mut s, Action::SelectDown);
        reduce(&mut s, Action::SelectDown);
        reduce(&mut s, Action::SelectDown);
        assert_eq!(s.selected_local, 2);
        reduce(&mut s, Action::SelectDown);
        assert_eq!(s.selected_local, 2);

        s.focus = FocusPane::Remote;
        s.remote_entries = vec![fe("r0"), fe("r1")];
        s.selected_remote = 0;
        reduce(&mut s, Action::SelectUp);
        assert_eq!(s.selected_remote, 0);
        reduce(&mut s, Action::SelectDown);
        reduce(&mut s, Action::SelectDown);
        assert_eq!(s.selected_remote, 1);
    }

    #[test]
    fn toggle_filter_clears_pattern() {
        let mut s = AppState {
            filter_pattern: "foo".into(),
            show_filter: true,
            ..Default::default()
        };
        reduce(&mut s, Action::ToggleFilter);
        assert!(!s.show_filter);
        assert!(s.filter_pattern.is_empty());
    }

    #[test]
    fn queue_transfer_increments_pending_and_clears_cancel() {
        let mut s = AppState {
            worker_cancel_requested: true,
            ..Default::default()
        };
        reduce(&mut s, Action::QueueTransfer(job("/tmp/a", "/pub/a")));
        assert_eq!(s.queue.pending.len(), 1);
        assert!(!s.worker_cancel_requested);
    }

    #[test]
    fn set_connected_true_does_not_clear_cancel_flag() {
        let mut s = AppState {
            worker_cancel_requested: true,
            ..Default::default()
        };
        reduce(&mut s, Action::SetConnected(true));
        assert!(s.connected);
        assert!(s.worker_cancel_requested);
    }

    #[test]
    fn mark_transfer_completed_moves_active() {
        let mut s = AppState::default();
        let j = job("/tmp/a", "/pub/a");
        reduce(&mut s, Action::QueueTransfer(j.clone()));
        let started = s.queue.start_next().expect("started");
        assert_eq!(s.queue.active.len(), 1);
        reduce(&mut s, Action::MarkTransferCompleted(started));
        assert!(s.queue.active.is_empty());
        assert_eq!(s.queue.completed.len(), 1);
        assert_eq!(s.queue.completed[0].status, TransferStatus::Completed);
    }

    #[test]
    fn update_transfer_progress_writes_active_bytes() {
        let mut s = AppState::default();
        let j = job("/tmp/a", "/pub/a");
        reduce(&mut s, Action::QueueTransfer(j));
        let started = s.queue.start_next().expect("started");
        reduce(
            &mut s,
            Action::UpdateTransferProgress {
                job_id: started.id,
                transferred_bytes: 42,
                size_bytes: Some(100),
            },
        );
        assert_eq!(s.queue.active[0].transferred_bytes, 42);
        assert_eq!(s.queue.active[0].size_bytes, Some(100));
    }

    #[test]
    fn clear_pending_transfers_empties_pending() {
        let mut s = AppState::default();
        reduce(&mut s, Action::QueueTransfer(job("/tmp/a", "/pub/a")));
        reduce(&mut s, Action::QueueTransfer(job("/tmp/b", "/pub/b")));
        reduce(&mut s, Action::ClearPendingTransfers);
        assert!(s.queue.pending.is_empty());
    }

    #[test]
    fn show_error_sets_toast_and_status() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ShowError("boom".into()));
        assert!(s.toast.is_some());
        assert!(s.status.contains("boom"));
    }

    #[test]
    fn set_focus_updates_focus_and_last_file_pane() {
        let mut s = AppState::default();
        reduce(&mut s, Action::SetFocus(FocusPane::Remote));
        assert_eq!(s.focus, FocusPane::Remote);
        assert_eq!(s.last_file_pane, FocusPane::Remote);
        reduce(&mut s, Action::SetFocus(FocusPane::Queue));
        assert_eq!(s.focus, FocusPane::Queue);
        assert_eq!(s.last_file_pane, FocusPane::Remote);
    }

    #[test]
    fn toggle_compare_flips() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ToggleCompare);
        assert!(s.show_compare);
        reduce(&mut s, Action::ToggleCompare);
        assert!(!s.show_compare);
    }

    #[test]
    fn set_worker_view_copies_counters() {
        let mut s = AppState::default();
        reduce(
            &mut s,
            Action::SetWorkerView {
                active_count: 2,
                running: true,
                cancel_requested: true,
            },
        );
        assert_eq!(s.worker_active_count, 2);
        assert!(s.worker_running);
        assert!(s.worker_cancel_requested);
    }
}

#[cfg(test)]
mod mark_sort_chmod_tests {
    use super::*;
    use crate::{is_dot_or_dotdot, parse_octal_mode, AppState, SelectPolicy, SortKey};
    use dd_ftp_core::{EntryKind, FileEntry};

    fn fe(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: format!("/{name}"),
            kind: EntryKind::File,
            size: 0,
            modified: None,
            permissions: None,
        }
    }

    fn dir(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: format!("/{name}"),
            kind: EntryKind::Directory,
            size: 0,
            modified: None,
            permissions: None,
        }
    }

    #[test]
    fn toggle_mark_adds_and_removes_path() {
        let mut s = AppState {
            local_entries: vec![fe("a"), fe("b")],
            selected_local: 0,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::ToggleMark);
        assert!(s.marked_local.contains("/a"));
        reduce(&mut s, Action::ToggleMark);
        assert!(!s.marked_local.contains("/a"));
        assert!(s.marked_local.is_empty());
    }

    #[test]
    fn set_entries_reset_clears_marks() {
        let mut s = AppState {
            local_entries: vec![fe("a")],
            selected_local: 0,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::ToggleMark);
        assert!(!s.marked_local.is_empty());
        reduce(
            &mut s,
            Action::SetLocalEntries {
                entries: vec![fe("a"), fe("b")],
                select: SelectPolicy::Reset,
            },
        );
        assert!(s.marked_local.is_empty());

        s.remote_entries = vec![fe("r")];
        s.selected_remote = 0;
        s.focus = FocusPane::Remote;
        reduce(&mut s, Action::ToggleMark);
        assert!(!s.marked_remote.is_empty());
        reduce(
            &mut s,
            Action::SetRemoteEntries {
                entries: vec![fe("r"), fe("s")],
                select: SelectPolicy::Reset,
            },
        );
        assert!(s.marked_remote.is_empty());
    }

    #[test]
    fn set_entries_preserve_name_keeps_marks() {
        let mut s = AppState {
            local_entries: vec![fe("a")],
            selected_local: 0,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::ToggleMark);
        reduce(
            &mut s,
            Action::SetLocalEntries {
                entries: vec![fe("a"), fe("b")],
                select: SelectPolicy::PreserveName,
            },
        );
        assert!(s.marked_local.contains("/a"));
    }

    #[test]
    fn toggle_mark_does_not_mark_dot_or_dotdot() {
        let mut s = AppState {
            local_entries: vec![dir("."), dir(".."), fe("a")],
            selected_local: 0,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::ToggleMark);
        assert!(s.marked_local.is_empty());
        s.selected_local = 1;
        reduce(&mut s, Action::ToggleMark);
        assert!(s.marked_local.is_empty());
        s.selected_local = 2;
        reduce(&mut s, Action::ToggleMark);
        assert!(s.marked_local.contains("/a"));
        assert!(is_dot_or_dotdot("."));
        assert!(is_dot_or_dotdot(".."));
    }

    #[test]
    fn show_chmod_prompt_opens_with_selected_mode_string() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ShowChmodPrompt { mode: "755".into() });
        assert!(s.show_prompt);
        assert_eq!(s.prompt_kind, Some(PromptKind::Text(TextPromptKind::Chmod)));
        assert_eq!(s.prompt_value.value, "755");
    }

    #[test]
    fn octal_parse_table_and_mask() {
        assert_eq!(parse_octal_mode("755").unwrap(), 0o755);
        assert_eq!(parse_octal_mode("0755").unwrap(), 0o755);
        assert_eq!(parse_octal_mode("0o755").unwrap(), 0o755);
        assert!(parse_octal_mode("zzz").is_err());
        assert_eq!(parse_octal_mode("17777").unwrap(), 0o7777);
        assert_eq!(parse_octal_mode("77777").unwrap() & 0o7777, 0o7777);
    }

    #[test]
    fn cycle_sort_and_hide_dotfiles() {
        let mut s = AppState::default();
        assert_eq!(s.sort_key, SortKey::Name);
        assert!(s.sort_asc);
        reduce(&mut s, Action::CycleSort);
        assert_eq!(s.sort_key, SortKey::Size);
        reduce(&mut s, Action::CycleSort);
        assert_eq!(s.sort_key, SortKey::Date);
        reduce(&mut s, Action::CycleSort);
        assert_eq!(s.sort_key, SortKey::Name);
        reduce(&mut s, Action::ToggleSortDir);
        assert!(!s.sort_asc);
        reduce(&mut s, Action::ToggleHideDotfiles);
        assert!(s.hide_dotfiles);
    }
}
