use std::path::Path;
use std::sync::atomic::Ordering;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dd_ftp_app::{
    parse_octal_mode, reduce, Action, AppState, ChoicePromptKind, FocusPane, PromptKind,
    QuickConnectField, SelectPolicy, TextPromptKind,
};
use dd_ftp_core::ConnectionInfo;
use dd_ftp_storage::SecretStore;

use crate::session::{OverwriteChoice, Runtime, SessionHandle};

pub(crate) enum LoopControl {
    Continue,
    Quit,
}

pub(crate) async fn handle_key(
    app: &mut AppState,
    runtime: &mut Runtime,
    key: KeyEvent,
) -> Result<LoopControl> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if app.show_help || app.show_filter || app.show_prompt || app.show_quick_connect {
            return Ok(LoopControl::Continue);
        }
        reduce(
            app,
            Action::SetWorkerView {
                active_count: runtime.worker_active_count,
                running: runtime.worker_active_count > 0,
                cancel_requested: true,
            },
        );
        for flag in &runtime.cancel_flags {
            flag.store(true, Ordering::Relaxed);
        }
        crate::session::park_pending_scan(app, runtime);
        reduce(
            app,
            Action::SetStatus("Cancel requested for active transfers".to_string()),
        );
        return Ok(LoopControl::Continue);
    }

    if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        if app.is_choice_prompt() {
            if let Some(PromptKind::Choice(kind)) = app.prompt_kind {
                match kind {
                    ChoicePromptKind::ConfirmQuit => {
                        crate::session::bump_generation_drop_workers(app, runtime).await;
                        return Ok(LoopControl::Quit);
                    }
                    ChoicePromptKind::Overwrite => {
                        crate::session::apply_overwrite_choice(
                            app,
                            runtime,
                            OverwriteChoice::Abort,
                        );
                    }
                    ChoicePromptKind::HostKey => {
                        crate::session::reject_host_key(runtime);
                        reduce(app, Action::CancelPrompt);
                    }
                    _ => {
                        reduce(app, Action::CancelPrompt);
                    }
                }
            }
        }
        if request_quit(app) {
            return Ok(LoopControl::Quit);
        }
        return Ok(LoopControl::Continue);
    }

    if !app.any_modal_open()
        && key.code == KeyCode::Char('k')
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        crate::bookmarks::run_keyring_health_check(app);
        return Ok(LoopControl::Continue);
    }

    if key.code == KeyCode::F(1) && (!app.any_modal_open() || app.show_help) {
        reduce(app, Action::ToggleHelp);
        return Ok(LoopControl::Continue);
    }

    if key.code == KeyCode::F(2) && (!app.any_modal_open() || app.show_theme_debug) {
        if !app.show_theme_debug {
            let _ = dd_ftp_ui::reload_theme();
        }
        reduce(app, Action::ToggleThemeDebug);
        return Ok(LoopControl::Continue);
    }

    if !app.any_modal_open() && key.code == KeyCode::Char('/') {
        reduce(app, Action::ToggleFilter);
        return Ok(LoopControl::Continue);
    }

    if app.show_help {
        match key.code {
            KeyCode::Esc => reduce(app, Action::ToggleHelp),
            KeyCode::Up | KeyCode::Char('k') => {
                reduce(app, Action::HelpScroll(-1));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                reduce(app, Action::HelpScroll(1));
            }
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if app.show_filter {
        match key.code {
            KeyCode::Esc => reduce(app, Action::ToggleFilter),
            KeyCode::Backspace => reduce(app, Action::FilterBackspace),
            KeyCode::Char(ch) => reduce(app, Action::FilterInput(ch)),
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if !app.any_modal_open() && key.code == KeyCode::Char('C') {
        reduce(app, Action::ToggleCompare);
        return Ok(LoopControl::Continue);
    }

    if app.is_choice_prompt() {
        let Some(PromptKind::Choice(kind)) = app.prompt_kind else {
            return Ok(LoopControl::Continue);
        };
        if kind == ChoicePromptKind::Overwrite {
            match key.code {
                KeyCode::Enter | KeyCode::Char('s') | KeyCode::Char('S') => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::Skip);
                }
                KeyCode::Char('o') | KeyCode::Char('O') => {
                    crate::session::apply_overwrite_choice(
                        app,
                        runtime,
                        OverwriteChoice::Overwrite,
                    );
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    crate::session::apply_overwrite_choice(
                        app,
                        runtime,
                        OverwriteChoice::OverwriteAll,
                    );
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::SkipAll);
                }
                KeyCode::Esc => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::Abort);
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::Rename);
                }
                _ => {}
            }
            return Ok(LoopControl::Continue);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                crate::session::reject_host_key(runtime);
                reduce(app, Action::CancelPrompt);
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => match kind {
                ChoicePromptKind::ConfirmQuit => {
                    crate::session::bump_generation_drop_workers(app, runtime).await;
                    return Ok(LoopControl::Quit);
                }
                ChoicePromptKind::ConfirmDelete => {
                    let target = app.prompt_target.clone();
                    reduce(app, Action::ConfirmPrompt);
                    if let Some(t) = target {
                        crate::fs_ops::delete_item(app, runtime, &t);
                    }
                }
                ChoicePromptKind::ConfirmBookmarkDelete => {
                    let name = app.prompt_target.clone();
                    reduce(app, Action::ConfirmPrompt);
                    if let Some(name) = name {
                        crate::bookmarks::delete_bookmark_named(app, &name);
                    }
                }
                ChoicePromptKind::HostKey => {
                    crate::session::accept_host_key(runtime);
                    reduce(app, Action::ConfirmPrompt);
                }
                ChoicePromptKind::Overwrite => {}
            },
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if app.is_text_prompt() {
        match key.code {
            KeyCode::Esc => {
                let renaming = matches!(
                    app.prompt_kind,
                    Some(PromptKind::Text(TextPromptKind::OverwriteRename))
                );
                reduce(app, Action::CancelPrompt);
                if renaming {
                    crate::session::show_overwrite_prompt(app, runtime);
                }
            }
            KeyCode::Tab => {
                app.prompt_kind = match app.prompt_kind {
                    Some(PromptKind::Text(TextPromptKind::CreateFile)) => {
                        Some(PromptKind::Text(TextPromptKind::CreateFolder))
                    }
                    Some(PromptKind::Text(TextPromptKind::CreateFolder)) => {
                        Some(PromptKind::Text(TextPromptKind::CreateFile))
                    }
                    other => other,
                };
            }
            KeyCode::Enter => {
                if let Some(PromptKind::Text(kind)) = app.prompt_kind {
                    match kind {
                        TextPromptKind::CreateFile => {
                            let name = app.prompt_value.value.clone();
                            reduce(app, Action::ConfirmPrompt);
                            crate::fs_ops::create_file(app, runtime, &name);
                        }
                        TextPromptKind::CreateFolder => {
                            let name = app.prompt_value.value.clone();
                            reduce(app, Action::ConfirmPrompt);
                            crate::fs_ops::create_folder(app, runtime, &name);
                        }
                        TextPromptKind::Rename => {
                            let new_name = app.prompt_value.value.clone();
                            let target = app.prompt_target.clone();
                            reduce(app, Action::ConfirmPrompt);
                            if let Some(t) = target {
                                crate::fs_ops::rename_item(app, runtime, &t, &new_name);
                            }
                        }
                        TextPromptKind::OverwriteRename => {
                            let new_name = app.prompt_value.value.clone();
                            if crate::session::apply_overwrite_rename(app, runtime, &new_name) {
                                reduce(app, Action::ConfirmPrompt);
                                crate::session::drain_scan_next(app, runtime);
                            }
                        }
                        TextPromptKind::Chmod => {
                            let value = app.prompt_value.value.clone();
                            let target = app.prompt_target.clone();
                            reduce(app, Action::ConfirmPrompt);
                            match parse_octal_mode(&value) {
                                Ok(mode) => {
                                    if let Some(path) = target {
                                        crate::fs_ops::chmod(app, runtime, &path, mode);
                                    }
                                }
                                Err(err) => reduce(app, Action::ShowError(err)),
                            }
                        }
                    }
                }
            }
            KeyCode::Left => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.prompt_value.move_cursor(-1, shift);
            }
            KeyCode::Right => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.prompt_value.move_cursor(1, shift);
            }
            KeyCode::Home => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.prompt_value.move_home(shift);
            }
            KeyCode::End => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.prompt_value.move_end(shift);
            }
            KeyCode::Delete => {
                app.prompt_value.delete();
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.prompt_value.delete_word_left();
            }
            KeyCode::Backspace => reduce(app, Action::PromptBackspace),
            KeyCode::Char(ch) => reduce(app, Action::PromptInput(ch)),
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if !app.any_modal_open()
        && key.code == KeyCode::Char('n')
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        reduce(app, Action::ShowCreatePrompt);
        return Ok(LoopControl::Continue);
    }

    if !app.any_modal_open()
        && key.code == KeyCode::Char('e')
        && key
            .modifiers
            .contains(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        reduce(app, Action::ShowRenamePrompt);
        if let Some(entry) = get_selected_entry(app) {
            app.prompt_target = Some(entry.path.clone());
            app.prompt_value = dd_ftp_app::TextField::from_str(&entry.name);
        }
        return Ok(LoopControl::Continue);
    }

    if !app.any_modal_open()
        && key.code == KeyCode::Delete
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        open_delete_prompt(app);
        return Ok(LoopControl::Continue);
    }

    if app.show_quick_connect {
        match key.code {
            KeyCode::Esc => reduce(app, Action::ToggleQuickConnect),
            KeyCode::Tab => reduce(app, Action::QuickConnectNextField),
            KeyCode::BackTab => reduce(app, Action::QuickConnectPrevField),
            KeyCode::Left => {
                if app.quick_connect_field == QuickConnectField::Protocol {
                    reduce(app, Action::QuickConnectSetProtocolPrev);
                } else {
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                    reduce(app, Action::QuickConnectMoveCursor { dir: -1, shift });
                }
            }
            KeyCode::Right => {
                if app.quick_connect_field == QuickConnectField::Protocol {
                    reduce(app, Action::QuickConnectSetProtocolNext);
                } else {
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                    reduce(app, Action::QuickConnectMoveCursor { dir: 1, shift });
                }
            }
            KeyCode::Home => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.qc_field.move_home(shift);
            }
            KeyCode::End => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                app.qc_field.move_end(shift);
            }
            KeyCode::Delete => {
                app.qc_field.delete();
                app.qc_flush();
            }
            KeyCode::Backspace => reduce(app, Action::QuickConnectBackspace),
            KeyCode::Enter => {
                let mut info = app.quick_connect.clone();
                if info.password.is_none() {
                    if let Ok(Some(secret)) = SecretStore::load_password(
                        &info.name,
                        &info.username,
                        &info.host,
                        info.port,
                    ) {
                        info.password = Some(secret);
                    }
                }
                crate::session::connect_off_thread(app, runtime, info).await;
                reduce(app, Action::ToggleQuickConnect);
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.qc_field.delete_word_left();
                app.qc_flush();
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                crate::bookmarks::save_quick_connect_bookmark(app);
            }
            KeyCode::Char(ch) => {
                reduce(app, Action::QuickConnectInput(ch));
            }
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if app.show_bookmarks {
        match key.code {
            KeyCode::Esc => reduce(app, Action::ToggleBookmarks),
            KeyCode::Char('j') | KeyCode::Down => reduce(app, Action::SelectNextBookmark),
            KeyCode::Char('k') | KeyCode::Up => reduce(app, Action::SelectPrevBookmark),
            KeyCode::Enter => {
                if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                    let bm =
                        crate::bookmarks::hydrate_password_from_keyring(app, bm, "bookmark-load");
                    reduce(app, Action::QuickConnectSetFromBookmark(bm));
                    reduce(app, Action::ToggleBookmarks);
                    reduce(app, Action::ToggleQuickConnect);
                }
            }
            KeyCode::Char('c') => {
                if let Some(mut bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                    if app.connected {
                        crate::session::disconnect_session(app, runtime).await;
                    }
                    if bm.password.is_none() {
                        if let Ok(Some(secret)) =
                            SecretStore::load_password(&bm.name, &bm.username, &bm.host, bm.port)
                        {
                            bm.password = Some(secret);
                        }
                    }
                    crate::session::connect_off_thread(app, runtime, bm).await;
                    reduce(app, Action::ToggleBookmarks);
                }
            }
            KeyCode::Char('d') => {
                if app.bookmarks.is_empty() {
                    reduce(app, Action::SetStatus("No bookmarks to delete".to_string()));
                } else if let Some(bm) = app.bookmarks.get(app.selected_bookmark) {
                    let name = bm.name.clone();
                    reduce(
                        app,
                        Action::ShowChoicePrompt(ChoicePromptKind::ConfirmBookmarkDelete),
                    );
                    app.prompt_target = Some(name);
                }
            }
            KeyCode::Char('e') => {
                if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                    let bm =
                        crate::bookmarks::hydrate_password_from_keyring(app, bm, "bookmark-edit");
                    reduce(app, Action::QuickConnectSetFromBookmark(bm));
                    reduce(app, Action::ToggleBookmarks);
                    reduce(app, Action::ToggleQuickConnect);
                }
            }
            KeyCode::Char('D') => {
                crate::bookmarks::set_default_bookmark(app);
            }
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    match key.code {
        KeyCode::Esc => {
            if app.show_compare {
                reduce(app, Action::ToggleCompare);
            }
        }
        KeyCode::Enter => match app.focus {
            FocusPane::Queue => {}
            FocusPane::Local | FocusPane::Remote => {
                if let Some(entry) = get_selected_entry(app) {
                    if entry.kind == dd_ftp_core::EntryKind::Directory {
                        navigate_into_directory(app, runtime);
                    } else if app.focus == FocusPane::Local {
                        crate::session::queue_upload_selected(app, runtime);
                    } else {
                        crate::session::queue_download_selected(app, runtime);
                    }
                }
            }
        },
        KeyCode::Char('n') => {
            reduce(app, Action::ShowCreatePrompt);
        }
        KeyCode::Char('e') => {
            reduce(app, Action::ShowRenamePrompt);
            if let Some(entry) = get_selected_entry(app) {
                app.prompt_target = Some(entry.path.clone());
                app.prompt_value = dd_ftp_app::TextField::from_str(&entry.name);
            }
        }
        KeyCode::Delete => {
            open_delete_prompt(app);
        }
        KeyCode::Tab => reduce(app, Action::FocusNextPane),
        KeyCode::Char('1') => {
            reduce(app, Action::SetFocus(FocusPane::Local));
            reduce(app, Action::SetStatus("Focus: Local".to_string()));
        }
        KeyCode::Char('2') => {
            reduce(app, Action::SetFocus(FocusPane::Remote));
            reduce(app, Action::SetStatus("Focus: Remote".to_string()));
        }
        KeyCode::Char('3') => {
            reduce(app, Action::SetFocus(FocusPane::Queue));
            reduce(app, Action::SetStatus("Focus: Queue".to_string()));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.focus == FocusPane::Queue {
                reduce(app, Action::QueueScroll(-1));
            } else {
                reduce(app, Action::SelectUp)
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.focus == FocusPane::Queue {
                reduce(app, Action::QueueScroll(1));
            } else {
                reduce(app, Action::SelectDown)
            }
        }
        KeyCode::Char('l') => {
            navigate_into_directory(app, runtime);
        }
        KeyCode::Char('h') => {
            navigate_parent_directory(app, runtime);
        }
        KeyCode::Char('r') => {
            if crate::session::drain_busy(runtime) {
                return Ok(LoopControl::Continue);
            }
            reduce(
                app,
                Action::SetLocalEntries {
                    entries: crate::paths::local_list(&app.local_cwd),
                    select: SelectPolicy::PreserveName,
                },
            );

            if app.connected {
                runtime.list_ok_status = Some("Refreshed local + remote listing".to_string());
                runtime.list_err_prefix = "Remote refresh failed".to_string();
                crate::session::list_remote(
                    app,
                    runtime,
                    app.remote_cwd.clone(),
                    SelectPolicy::PreserveName,
                );
            } else {
                reduce(
                    app,
                    Action::SetStatus("Refreshed local listing".to_string()),
                );
            }
        }
        KeyCode::Char('b') => {
            reduce(app, Action::SelectNextBookmark);
        }
        KeyCode::Char('B') => {
            crate::bookmarks::save_quick_connect_bookmark(app);
        }
        KeyCode::Char('o') => {
            reduce(app, Action::ToggleQuickConnect);
        }
        KeyCode::Char('m') => {
            reduce(app, Action::ToggleBookmarks);
        }
        KeyCode::Char('c') => {
            if app.connected {
                crate::session::disconnect_session(app, runtime).await;
            } else {
                let info = selected_or_quick_connect(app);
                crate::session::connect_off_thread(app, runtime, info).await;
            }
        }
        KeyCode::Char('u') => {
            crate::session::queue_upload_selected(app, runtime);
        }
        KeyCode::Char('d') => {
            crate::session::queue_download_selected(app, runtime);
        }
        KeyCode::Char('X') => {
            reduce(app, Action::ClearPendingTransfers);
        }
        KeyCode::Char('R') => {
            reduce(app, Action::RetryLastFailed);
        }
        KeyCode::Char(' ') => {
            reduce(app, Action::ToggleMark);
        }
        KeyCode::Char('p') => {
            open_chmod_prompt(app, runtime);
        }
        KeyCode::Char('s') => {
            reduce(app, Action::CycleSort);
        }
        KeyCode::Char('S') => {
            reduce(app, Action::ToggleSortDir);
        }
        KeyCode::Char('.') => {
            reduce(app, Action::ToggleHideDotfiles);
        }
        _ => {}
    }

    Ok(LoopControl::Continue)
}

pub(crate) fn navigate_into_directory(app: &mut AppState, runtime: &mut Runtime) {
    if crate::session::drain_busy(runtime) {
        return;
    }
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            if let Some(entry) = app.selected_local_entry().cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.local_cwd = entry.path;
                    reduce(
                        app,
                        Action::SetLocalEntries {
                            entries: crate::paths::local_list(&app.local_cwd),
                            select: SelectPolicy::Reset,
                        },
                    );
                    reduce(
                        app,
                        Action::SetStatus(format!("Local cwd: {}", app.local_cwd)),
                    );
                }
            }
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            if crate::session::io_busy(runtime) {
                return;
            }

            if let Some(entry) = app.selected_remote_entry().cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.remote_cwd = crate::paths::join_remote_path(&app.remote_cwd, &entry.name);
                    runtime.list_ok_status = Some(format!("Remote cwd: {}", app.remote_cwd));
                    runtime.list_err_prefix = "Remote enter failed".to_string();
                    crate::session::list_remote(
                        app,
                        runtime,
                        app.remote_cwd.clone(),
                        SelectPolicy::Reset,
                    );
                } else {
                    reduce(
                        app,
                        Action::SetStatus(format!("'{}' is not a directory", entry.name)),
                    );
                }
            }
        }
        dd_ftp_app::FocusPane::Queue => {}
    }
}

pub(crate) fn navigate_parent_directory(app: &mut AppState, runtime: &mut Runtime) {
    if crate::session::drain_busy(runtime) {
        return;
    }
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let parent = Path::new(&app.local_cwd)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| app.local_cwd.clone());

            app.local_cwd = parent;
            reduce(
                app,
                Action::SetLocalEntries {
                    entries: crate::paths::local_list(&app.local_cwd),
                    select: SelectPolicy::Reset,
                },
            );
            reduce(
                app,
                Action::SetStatus(format!("Local cwd: {}", app.local_cwd)),
            );
        }
        dd_ftp_app::FocusPane::Remote => {
            if !app.connected {
                reduce(app, Action::SetStatus("Not connected".to_string()));
                return;
            }
            if crate::session::io_busy(runtime) {
                return;
            }

            app.remote_cwd = crate::paths::parent_remote_path(&app.remote_cwd);
            runtime.list_ok_status = Some(format!("Remote cwd: {}", app.remote_cwd));
            runtime.list_err_prefix = "Remote parent failed".to_string();
            crate::session::list_remote(app, runtime, app.remote_cwd.clone(), SelectPolicy::Reset);
        }
        dd_ftp_app::FocusPane::Queue => {}
    }
}

pub(crate) fn selected_or_quick_connect(app: &mut AppState) -> ConnectionInfo {
    quick_connect_info(app)
}

pub(crate) fn quick_connect_info(app: &mut AppState) -> ConnectionInfo {
    crate::bookmarks::hydrate_password_from_keyring(app, app.quick_connect.clone(), "quick-connect")
}

pub(crate) fn request_quit(app: &mut AppState) -> bool {
    if app.worker_running || !app.queue.active.is_empty() {
        reduce(app, Action::ShowChoicePrompt(ChoicePromptKind::ConfirmQuit));
        false
    } else {
        true
    }
}

pub(crate) fn get_selected_entry(app: &AppState) -> Option<dd_ftp_core::FileEntry> {
    match app.focus {
        dd_ftp_app::FocusPane::Local => app.selected_local_entry().cloned(),
        dd_ftp_app::FocusPane::Remote => app.selected_remote_entry().cloned(),
        _ => None,
    }
}

pub(crate) fn open_chmod_prompt(app: &mut AppState, runtime: &Runtime) {
    if app.focus != FocusPane::Remote {
        return;
    }
    match runtime.handle.as_ref() {
        Some(SessionHandle::Sftp(_)) => {
            if let Some(entry) = app.selected_remote_entry() {
                if entry.name == "." || entry.name == ".." {
                    return;
                }
                let mode = entry.permissions.clone().unwrap_or_default();
                let path = entry.path.clone();
                reduce(app, Action::ShowChmodPrompt { mode });
                app.prompt_target = Some(path);
            } else {
                reduce(app, Action::SetStatus("Nothing selected".to_string()));
            }
        }
        Some(SessionHandle::Ftp(_)) => {
            reduce(app, Action::ShowError("chmod is SFTP-only".to_string()));
        }
        None => {
            reduce(app, Action::SetStatus("Not connected".to_string()));
        }
    }
}

pub(crate) fn open_delete_prompt(app: &mut AppState) {
    if let Some(entry) = get_selected_entry(app) {
        reduce(app, Action::ShowDeletePrompt);
        app.prompt_target = Some(entry.path.clone());
    } else {
        reduce(
            app,
            Action::SetStatus("Nothing selected to delete".to_string()),
        );
    }
}

#[cfg(test)]
mod connect_info_tests {
    use super::*;
    use dd_ftp_core::Protocol;

    #[test]
    fn selected_or_quick_connect_uses_form_not_bookmark() {
        let mut app = AppState {
            bookmarks: vec![ConnectionInfo {
                name: "saved".into(),
                host: "bookmark.example".into(),
                port: 22,
                protocol: Protocol::Sftp,
                username: "bmuser".into(),
                password: Some("bmpass".into()),
                private_key: None,
                initial_path: "/".into(),
            }],
            selected_bookmark: 0,
            quick_connect: ConnectionInfo {
                host: "form.example".into(),
                username: "formuser".into(),
                password: Some("formpass".into()),
                ..ConnectionInfo::default()
            },
            ..Default::default()
        };

        let info = selected_or_quick_connect(&mut app);
        assert_eq!(info.host, "form.example");
        assert_eq!(info.username, "formuser");

        let info = quick_connect_info(&mut app);
        assert_eq!(info.host, "form.example");
        assert_eq!(info.username, "formuser");
    }
}
