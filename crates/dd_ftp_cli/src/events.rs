use std::path::Path;
use std::sync::atomic::Ordering;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dd_ftp_app::{
    parse_octal_mode, reduce, Action, AppState, ChoicePromptKind, FocusPane, PromptKind,
    QuickConnectField, SelectPolicy, TextPromptKind,
};
use dd_ftp_core::{ConnectionInfo, TransferStatus};
use dd_ftp_storage::SecretStore;
use dd_ftp_ui::LayoutMap;
use ratatui::layout::Rect;

use crate::session::{OverwriteChoice, Runtime, SessionHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopControl {
    Continue,
    Quit,
}

pub(crate) fn accept_key_event(key: KeyEvent, app: &AppState) -> bool {
    match key.kind {
        KeyEventKind::Release => false,
        KeyEventKind::Press => true,
        KeyEventKind::Repeat => key_repeat_is_useful(key.code, app),
    }
}

fn key_repeat_is_useful(code: KeyCode, app: &AppState) -> bool {
    if matches!(
        code,
        KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete
    ) {
        return true;
    }
    let typing = app.show_filter
        || app.is_text_prompt()
        || app.show_quick_connect
        || app.show_settings
        || app.show_theme_debug;
    match code {
        KeyCode::Char(_) if typing => true,
        KeyCode::Char('j' | 'k' | 'h' | 'l') if !typing => true,
        _ => false,
    }
}

pub(crate) fn page_rows_for(rect: Rect) -> usize {
    rect.height.saturating_sub(2).max(1) as usize
}

pub(crate) fn offset_index(current: usize, last: usize, delta: i32) -> usize {
    let next = (current as i64).saturating_add(delta as i64);
    next.clamp(0, last as i64) as usize
}

pub(crate) fn handle_key(
    app: &mut AppState,
    runtime: &mut Runtime,
    key: KeyEvent,
    layout: &LayoutMap,
) -> Result<LoopControl> {
    if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if app.show_help
            || app.show_prompt
            || app.show_quick_connect
            || app.show_settings
            || app.show_bookmarks
            || app.show_key_picker
        {
            return Ok(LoopControl::Continue);
        }
        reduce(app, Action::MarkAllVisible);
        return Ok(LoopControl::Continue);
    }

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
                        crate::session::bump_generation_drop_workers(app, runtime);
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
                        if runtime.edit_session.is_some() {
                            let keep = runtime
                                .edit_session
                                .as_ref()
                                .is_some_and(|e| e.fingerprint.is_some());
                            crate::session::cancel_edit(app, runtime, keep);
                        }
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
        if app.show_theme_debug {
            if let Some(mut editor) = app.theme_editor.take() {
                editor.revert();
            }
            reduce(app, Action::ToggleThemeDebug);
        } else {
            let loaded = dd_ftp_ui::cached_theme();
            app.theme_editor = Some(ldnddev_theme::ThemeEditor::new(
                dd_ftp_ui::palette_from_theme(&loaded.theme, loaded.header_quotes.clone()),
                dd_ftp_ui::extra_theme_fields(),
            ));
            let _ = dd_ftp_ui::reload_theme();
            reduce(app, Action::ToggleThemeDebug);
        }
        return Ok(LoopControl::Continue);
    }

    if app.show_theme_debug && app.theme_editor.is_some() && key.code != KeyCode::F(1) {
        handle_ftp_theme_editor(app, key);
        return Ok(LoopControl::Continue);
    }

    if key.code == KeyCode::F(3)
        && (!app.any_modal_open() || app.show_settings || app.show_help || app.show_theme_debug)
    {
        reduce(app, Action::ToggleSettings);
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
            KeyCode::PageUp => jump_help(app, layout, Jump::Page(-1)),
            KeyCode::PageDown => jump_help(app, layout, Jump::Page(1)),
            KeyCode::Home => jump_help(app, layout, Jump::Home),
            KeyCode::End => jump_help(app, layout, Jump::End),
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if app.show_filter {
        match key.code {
            KeyCode::Esc => reduce(app, Action::ToggleFilter),
            KeyCode::Enter => reduce(app, Action::CloseFilterKeep),
            KeyCode::Backspace => reduce(app, Action::FilterBackspace),
            KeyCode::Char(ch) => reduce(app, Action::FilterInput(ch)),
            _ => {}
        }
        return Ok(LoopControl::Continue);
    }

    if app.show_settings {
        match key.code {
            KeyCode::Esc | KeyCode::F(3) => reduce(app, Action::ToggleSettings),
            KeyCode::Enter => save_settings(app, runtime),
            KeyCode::Tab | KeyCode::Down => reduce(app, Action::SettingsNextField),
            KeyCode::BackTab | KeyCode::Up => reduce(app, Action::SettingsPrevField),
            KeyCode::Backspace if app.settings_field.is_text() => {
                reduce(app, Action::SettingsBackspace);
            }
            KeyCode::Left => {
                if app.settings_field.is_text() {
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                    reduce(app, Action::SettingsMoveCursor { dir: -1, shift });
                } else {
                    reduce(app, Action::SettingsCycleValue(-1));
                }
            }
            KeyCode::Right => {
                if app.settings_field.is_text() {
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                    reduce(app, Action::SettingsMoveCursor { dir: 1, shift });
                } else {
                    reduce(app, Action::SettingsCycleValue(1));
                }
            }
            KeyCode::Home if app.settings_field.is_text() => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                if let Some(tf) = app.settings_text_mut() {
                    tf.move_home(shift);
                }
            }
            KeyCode::End if app.settings_field.is_text() => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                if let Some(tf) = app.settings_text_mut() {
                    tf.move_end(shift);
                }
            }
            KeyCode::Delete if app.settings_field.is_text() => {
                if let Some(tf) = app.settings_text_mut() {
                    tf.delete();
                }
            }
            KeyCode::Char(' ') if !app.settings_field.is_text() => {
                reduce(app, Action::SettingsCycleValue(1));
            }
            KeyCode::Char(ch) if app.settings_field.is_text() => {
                reduce(app, Action::SettingsInput(ch));
            }
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
        if kind == ChoicePromptKind::EditConflict {
            match key.code {
                KeyCode::Char('o') | KeyCode::Char('O') | KeyCode::Char('y') => {
                    reduce(app, Action::ConfirmPrompt);
                    if let Some(edit) = runtime.edit_session.clone() {
                        crate::session::enqueue_edit_upload(app, runtime, edit.remote_path);
                    }
                }
                KeyCode::Char('s')
                | KeyCode::Char('S')
                | KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Esc => {
                    reduce(app, Action::CancelPrompt);
                    crate::session::cancel_edit(app, runtime, true);
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    reduce(app, Action::CancelPrompt);
                    app.show_prompt = true;
                    app.prompt_kind = Some(PromptKind::Text(TextPromptKind::EditSaveAs));
                    let name = runtime
                        .edit_session
                        .as_ref()
                        .map(|e| {
                            e.remote_path
                                .rsplit('/')
                                .next()
                                .unwrap_or(&e.remote_path)
                                .to_string()
                        })
                        .unwrap_or_default();
                    app.prompt_value = dd_ftp_app::TextField::from_str(&name);
                }
                _ => {}
            }
            return Ok(LoopControl::Continue);
        }
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
                KeyCode::Char('a') => {
                    crate::session::apply_overwrite_choice(
                        app,
                        runtime,
                        OverwriteChoice::OverwriteAll,
                    );
                }
                KeyCode::Char('A') => {
                    crate::session::apply_overwrite_choice(
                        app,
                        runtime,
                        OverwriteChoice::OverwriteNewer,
                    );
                }
                KeyCode::Char('n') => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::SkipAll);
                }
                KeyCode::Char('N') => {
                    crate::session::apply_overwrite_choice(
                        app,
                        runtime,
                        OverwriteChoice::SkipNewer,
                    );
                }
                KeyCode::Esc => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::Abort);
                }
                KeyCode::Char('r') => {
                    crate::session::apply_overwrite_choice(app, runtime, OverwriteChoice::Rename);
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    crate::session::apply_overwrite_choice(
                        app,
                        runtime,
                        OverwriteChoice::RenameNewer,
                    );
                }
                _ => {}
            }
            return Ok(LoopControl::Continue);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                if kind == ChoicePromptKind::ConfirmDelete {
                    runtime.pending_delete.clear();
                }
                if kind == ChoicePromptKind::ConfirmEditLarge {
                    crate::session::cancel_edit(app, runtime, false);
                }
                if kind == ChoicePromptKind::ConfirmEditBinary {
                    crate::session::cancel_edit(app, runtime, true);
                }
                crate::session::reject_host_key(runtime);
                reduce(app, Action::CancelPrompt);
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => match kind {
                ChoicePromptKind::ConfirmQuit => {
                    crate::session::bump_generation_drop_workers(app, runtime);
                    return Ok(LoopControl::Quit);
                }
                ChoicePromptKind::ConfirmDelete => {
                    reduce(app, Action::ConfirmPrompt);
                    crate::fs_ops::start_next_delete(app, runtime);
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
                ChoicePromptKind::Overwrite | ChoicePromptKind::EditConflict => {}
                ChoicePromptKind::ConfirmEditLarge => {
                    reduce(app, Action::ConfirmPrompt);
                    crate::session::begin_edit_download(app, runtime);
                }
                ChoicePromptKind::ConfirmEditBinary => {
                    reduce(app, Action::ConfirmPrompt);
                    runtime.editor_ready = true;
                }
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
                let edit_save = matches!(
                    app.prompt_kind,
                    Some(PromptKind::Text(TextPromptKind::EditSaveAs))
                );
                reduce(app, Action::CancelPrompt);
                if renaming {
                    crate::session::show_overwrite_prompt(app, runtime);
                }
                if edit_save {
                    crate::session::cancel_edit(app, runtime, true);
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
                        TextPromptKind::EditSaveAs => {
                            let new_name = app.prompt_value.value.clone();
                            reduce(app, Action::ConfirmPrompt);
                            match crate::paths::safe_remote_child(&app.remote_cwd, &new_name) {
                                Ok(path) => {
                                    crate::session::enqueue_edit_upload(app, runtime, path);
                                }
                                Err(_) => {
                                    crate::session::cancel_edit(app, runtime, true);
                                    reduce(
                                        app,
                                        Action::ShowError("path escapes directory".to_string()),
                                    );
                                }
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
        open_delete_prompt(app, runtime);
        return Ok(LoopControl::Continue);
    }

    if app.show_key_picker {
        match key.code {
            KeyCode::Esc => reduce(app, Action::CloseKeyPicker),
            KeyCode::Up | KeyCode::Char('k') => reduce(app, Action::KeyPickerSelectUp),
            KeyCode::Down | KeyCode::Char('j') => reduce(app, Action::KeyPickerSelectDown),
            KeyCode::PageUp => jump_key_picker(app, layout, Jump::Page(-1)),
            KeyCode::PageDown => jump_key_picker(app, layout, Jump::Page(1)),
            KeyCode::Home => jump_key_picker(app, layout, Jump::Home),
            KeyCode::End => jump_key_picker(app, layout, Jump::End),
            KeyCode::Enter => key_picker_activate_selected(app),
            KeyCode::Char('l') | KeyCode::Right => key_picker_enter_dir(app),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => key_picker_go_parent(app),
            _ => {}
        }
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
                crate::session::connect_off_thread(app, runtime, info);
                reduce(app, Action::ToggleQuickConnect);
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.qc_field.delete_word_left();
                app.qc_flush();
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                crate::bookmarks::save_quick_connect_bookmark(app);
            }
            KeyCode::Char('p') | KeyCode::Char('P')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if app.quick_connect_field == QuickConnectField::PrivateKey {
                    open_key_picker(app);
                }
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
            KeyCode::PageUp => jump_bookmarks(app, layout, Jump::Page(-1)),
            KeyCode::PageDown => jump_bookmarks(app, layout, Jump::Page(1)),
            KeyCode::Home => jump_bookmarks(app, layout, Jump::Home),
            KeyCode::End => jump_bookmarks(app, layout, Jump::End),
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
                        crate::session::disconnect_session(app, runtime);
                    }
                    if bm.password.is_none() {
                        if let Ok(Some(secret)) =
                            SecretStore::load_password(&bm.name, &bm.username, &bm.host, bm.port)
                        {
                            bm.password = Some(secret);
                        }
                    }
                    crate::session::connect_off_thread(app, runtime, bm);
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
            } else if app.marked_live_count(app.focus) > 0 {
                reduce(app, Action::ClearMarks { pane: app.focus });
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
        KeyCode::Char('E') => {
            crate::session::start_remote_edit(app, runtime);
        }
        KeyCode::Delete => {
            if app.focus == FocusPane::Queue {
                cancel_highlighted_queue_job(app, runtime);
            } else {
                open_delete_prompt(app, runtime);
            }
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
                reduce(app, Action::SelectUp);
            } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                reduce(app, Action::ExtendMark { dir: -1 });
            } else {
                reduce(app, Action::SelectUp)
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.focus == FocusPane::Queue {
                reduce(app, Action::SelectDown);
            } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                reduce(app, Action::ExtendMark { dir: 1 });
            } else {
                reduce(app, Action::SelectDown)
            }
        }
        KeyCode::PageUp => jump_focused(app, layout, Jump::Page(-1)),
        KeyCode::PageDown => jump_focused(app, layout, Jump::Page(1)),
        KeyCode::Home => jump_focused(app, layout, Jump::Home),
        KeyCode::End => jump_focused(app, layout, Jump::End),
        KeyCode::Char('l') => {
            navigate_into_directory(app, runtime);
        }
        KeyCode::Char('h') => {
            navigate_parent_directory(app, runtime);
        }
        KeyCode::Char('r') => {
            let local_status = if app.connected && !crate::session::io_busy(runtime) {
                None
            } else {
                Some("Refreshed local listing".to_string())
            };
            crate::session::request_local_list(
                runtime,
                app.local_cwd.clone(),
                SelectPolicy::PreserveName,
                local_status,
            );

            if app.connected && !crate::session::io_busy(runtime) {
                runtime.list_ok_status = Some("Refreshed local + remote listing".to_string());
                runtime.list_err_prefix = "Remote refresh failed".to_string();
                crate::session::list_remote(
                    app,
                    runtime,
                    app.remote_cwd.clone(),
                    SelectPolicy::PreserveName,
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
        KeyCode::Char('M') => {
            crate::session::queue_move_selected(app, runtime);
        }
        KeyCode::Char('c') => {
            if app.connected {
                crate::session::disconnect_session(app, runtime);
            } else {
                let info = selected_or_quick_connect(app);
                crate::session::connect_off_thread(app, runtime, info);
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
        KeyCode::Char('x') if app.focus == FocusPane::Queue => {
            cancel_highlighted_queue_job(app, runtime);
        }
        KeyCode::Char('R') => {
            if app.focus == FocusPane::Queue {
                if let Some(id) = app
                    .selected_queue_job()
                    .and_then(|j| (j.status == TransferStatus::Failed).then_some(j.id))
                {
                    reduce(app, Action::RetryFailed(id));
                    return Ok(LoopControl::Continue);
                }
            }
            reduce(app, Action::RetryLastFailed);
        }
        KeyCode::Char('A') if app.focus == FocusPane::Queue => {
            reduce(app, Action::RetryAllFailed);
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

pub(crate) fn navigate_local_path(app: &mut AppState, runtime: &mut Runtime, path: String) {
    if path == app.local_cwd {
        return;
    }
    app.local_cwd = path;
    crate::session::request_local_list(
        runtime,
        app.local_cwd.clone(),
        SelectPolicy::Reset,
        Some(format!("Local cwd: {}", app.local_cwd)),
    );
}

pub(crate) fn navigate_remote_path(app: &mut AppState, runtime: &mut Runtime, path: String) {
    if !app.connected {
        reduce(app, Action::SetStatus("Not connected".to_string()));
        return;
    }
    if crate::session::io_busy(runtime) {
        return;
    }
    if path == app.remote_cwd {
        return;
    }
    app.remote_cwd = path;
    runtime.list_ok_status = Some(format!("Remote cwd: {}", app.remote_cwd));
    runtime.list_err_prefix = "Remote enter failed".to_string();
    crate::session::list_remote(app, runtime, app.remote_cwd.clone(), SelectPolicy::Reset);
}

pub(crate) fn navigate_into_directory(app: &mut AppState, runtime: &mut Runtime) {
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            if let Some(entry) = app.selected_local_entry().cloned() {
                if entry.kind == dd_ftp_core::EntryKind::Directory {
                    app.local_cwd = entry.path;
                    crate::session::request_local_list(
                        runtime,
                        app.local_cwd.clone(),
                        SelectPolicy::Reset,
                        Some(format!("Local cwd: {}", app.local_cwd)),
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
    match app.focus {
        dd_ftp_app::FocusPane::Local => {
            let parent = Path::new(&app.local_cwd)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| app.local_cwd.clone());

            app.local_cwd = parent;
            crate::session::request_local_list(
                runtime,
                app.local_cwd.clone(),
                SelectPolicy::Reset,
                Some(format!("Local cwd: {}", app.local_cwd)),
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

fn cancel_highlighted_queue_job(app: &mut AppState, runtime: &mut Runtime) {
    let Some((id, status)) = app.selected_queue_job().map(|j| (j.id, j.status)) else {
        return;
    };
    match status {
        TransferStatus::Pending | TransferStatus::Failed => {
            reduce(app, Action::RemoveQueueJob(id));
        }
        TransferStatus::Active => {
            if let Some(flag) = runtime.job_cancels.get(&id) {
                flag.store(true, Ordering::Relaxed);
                reduce(
                    app,
                    Action::SetStatus("Cancel requested for highlighted transfer".to_string()),
                );
            }
        }
        _ => {}
    }
}

fn save_settings(app: &mut AppState, runtime: &mut Runtime) {
    let editor = app.settings_editor.value.trim().to_string();
    let last_local = app.settings_last_local.value.trim().to_string();
    let overwrite = app.settings_overwrite.to_config();
    let cfg = dd_ftp_storage::AppConfig {
        editor: if editor.is_empty() {
            None
        } else {
            Some(editor.clone())
        },
        worker_max_concurrency: Some(app.settings_workers as u32),
        last_local_cwd: if last_local.is_empty() {
            None
        } else {
            Some(last_local.clone())
        },
        hide_dotfiles: app.settings_hide_dotfiles,
        overwrite_policy: if overwrite == "ask" {
            None
        } else {
            Some(overwrite.to_string())
        },
        auto_connect_default: app.settings_auto_connect,
    };
    match cfg.save_to_default_path() {
        Ok(path) => {
            reduce(
                app,
                Action::ApplySettings {
                    editor: cfg.editor_or_empty(),
                    worker_max_concurrency: app.settings_workers,
                    hide_dotfiles: app.settings_hide_dotfiles,
                    overwrite: app.settings_overwrite,
                    auto_connect: app.settings_auto_connect,
                },
            );
            reduce(app, Action::ToggleSettings);
            runtime.default_overwrite_policy = app.default_overwrite_policy;
            if !crate::session::drain_busy(runtime) {
                runtime.overwrite_policy = app.default_overwrite_policy;
            }
            if !last_local.is_empty()
                && Path::new(&last_local).is_dir()
                && last_local != app.local_cwd
            {
                app.local_cwd = last_local;
                crate::session::request_local_list(
                    runtime,
                    app.local_cwd.clone(),
                    SelectPolicy::Reset,
                    Some(format!("Local cwd: {}", app.local_cwd)),
                );
            }
            reduce(
                app,
                Action::SetStatus(format!("Settings saved to {}", path.display())),
            );
        }
        Err(err) => {
            reduce(
                app,
                Action::ShowError(format!("Could not save settings: {err}")),
            );
        }
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

pub(crate) fn open_delete_prompt(app: &mut AppState, runtime: &mut Runtime) {
    let n = crate::fs_ops::queue_deletes_for_pane(app, runtime);
    if n == 0 {
        reduce(
            app,
            Action::SetStatus("Nothing selected to delete".to_string()),
        );
        return;
    }
    let summary = crate::fs_ops::delete_summary(runtime);
    reduce(app, Action::ShowDeletePrompt);
    app.prompt_target = Some(summary);
}

pub(crate) fn open_key_picker(app: &mut AppState) {
    app.qc_flush();
    let start = crate::paths::key_picker_start_dir(app.quick_connect.private_key.as_deref());
    let entries = crate::paths::local_list(&start);
    let prefer =
        crate::paths::key_picker_prefer_name(app.quick_connect.private_key.as_deref(), &start);
    reduce(
        app,
        Action::OpenKeyPicker {
            cwd: start,
            entries,
            prefer,
        },
    );
}

fn key_picker_load(app: &mut AppState, cwd: String, select: SelectPolicy, prefer: Option<String>) {
    let entries = crate::paths::local_list(&cwd);
    reduce(
        app,
        Action::SetKeyPickerEntries {
            cwd,
            entries,
            select,
            prefer,
        },
    );
}

pub(crate) fn key_picker_activate_selected(app: &mut AppState) {
    let Some(entry) = app.selected_key_picker_entry().cloned() else {
        return;
    };
    if entry.is_dir() {
        if entry.name == "." {
            key_picker_load(
                app,
                app.key_picker_cwd.clone(),
                SelectPolicy::PreserveName,
                None,
            );
            return;
        }
        let came_from = if entry.name == ".." {
            Path::new(&app.key_picker_cwd)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        } else {
            None
        };
        key_picker_load(app, entry.path, SelectPolicy::Reset, came_from);
        return;
    }
    reduce(app, Action::KeyPickerConfirm);
}

fn key_picker_enter_dir(app: &mut AppState) {
    if app.selected_key_picker_entry().is_some_and(|e| e.is_dir()) {
        key_picker_activate_selected(app);
    }
}

pub(crate) fn key_picker_go_parent(app: &mut AppState) {
    let parent = Path::new(&app.key_picker_cwd)
        .parent()
        .map(|p| p.to_string_lossy().into_owned());
    let Some(parent) = parent.filter(|p| !p.is_empty() && p != &app.key_picker_cwd) else {
        return;
    };
    let came_from = Path::new(&app.key_picker_cwd)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    key_picker_load(app, parent, SelectPolicy::Reset, came_from);
}

fn handle_ftp_theme_editor(app: &mut AppState, key: KeyEvent) {
    let Some(ek) = map_editor_key(key) else {
        return;
    };
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let outcome = {
        let Some(editor) = app.theme_editor.as_mut() else {
            return;
        };
        editor.handle(ek, shift)
    };
    match outcome {
        ldnddev_theme::EditorOutcome::PaletteChanged => {
            if let Some(editor) = &app.theme_editor {
                dd_ftp_ui::apply_live_theme(&editor.palette);
            }
        }
        ldnddev_theme::EditorOutcome::RequestSave => {
            if let Some(editor) = &app.theme_editor {
                let root =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                match ldnddev_theme::save_theme(
                    &editor.palette,
                    &root,
                    "dd_ftp_theme.yml",
                    editor.save_target,
                    ldnddev_theme::default_config_home().as_deref(),
                    &editor.fields,
                ) {
                    Ok(path) => {
                        dd_ftp_ui::apply_live_theme(&editor.palette);
                        app.theme_editor = None;
                        reduce(app, Action::ToggleThemeDebug);
                        reduce(
                            app,
                            Action::SetStatus(format!("Theme saved to {}", path.display())),
                        );
                    }
                    Err(err) => {
                        reduce(
                            app,
                            Action::ShowError(format!("Could not save theme: {err}")),
                        );
                    }
                }
            }
        }
        ldnddev_theme::EditorOutcome::Closed { .. } => {
            if let Some(mut editor) = app.theme_editor.take() {
                editor.revert();
                dd_ftp_ui::apply_live_theme(&editor.palette);
            }
            if app.show_theme_debug {
                reduce(app, Action::ToggleThemeDebug);
            }
        }
        ldnddev_theme::EditorOutcome::HexError(_) | ldnddev_theme::EditorOutcome::None => {}
    }
}

fn map_editor_key(key: KeyEvent) -> Option<ldnddev_theme::EditorKey> {
    Some(match key.code {
        KeyCode::Up => ldnddev_theme::EditorKey::Up,
        KeyCode::Down => ldnddev_theme::EditorKey::Down,
        KeyCode::Left => ldnddev_theme::EditorKey::Left,
        KeyCode::Right => ldnddev_theme::EditorKey::Right,
        KeyCode::Tab => ldnddev_theme::EditorKey::Tab,
        KeyCode::Enter => ldnddev_theme::EditorKey::Enter,
        KeyCode::Esc => ldnddev_theme::EditorKey::Esc,
        KeyCode::Backspace => ldnddev_theme::EditorKey::Backspace,
        KeyCode::Char(c) => ldnddev_theme::EditorKey::Char(c),
        _ => return None,
    })
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

#[cfg(test)]
mod key_picker_io_tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};

    use crate::session::test_runtime;
    use dd_ftp_core::{ConnectionInfo, TransferDirection, TransferJob};

    #[test]
    fn navigate_local_path_same_cwd_is_noop() {
        let mut app = AppState {
            local_cwd: "/tmp".into(),
            ..Default::default()
        };
        let (mut runtime, _rx) = test_runtime();
        navigate_local_path(&mut app, &mut runtime, "/tmp".into());
        assert!(!runtime.local_list_pending);
        assert_eq!(app.local_cwd, "/tmp");
    }

    #[test]
    fn navigate_remote_path_disconnected_sets_status() {
        let mut app = AppState {
            connected: false,
            remote_cwd: "/".into(),
            ..Default::default()
        };
        let (mut runtime, _rx) = test_runtime();
        navigate_remote_path(&mut app, &mut runtime, "/pub".into());
        assert_eq!(app.remote_cwd, "/");
        assert_eq!(app.status, "Not connected");
    }

    #[tokio::test]
    async fn navigate_local_path_sets_cwd() {
        use std::time::Duration;

        let dir = unique_temp("dd_ftp_path_click");
        let nested = dir.join("a").join("b");
        std::fs::create_dir_all(&nested).expect("nested");
        let mut app = AppState::default();
        let (mut runtime, mut rx) = test_runtime();
        navigate_local_path(
            &mut app,
            &mut runtime,
            nested.to_string_lossy().into_owned(),
        );
        assert_eq!(app.local_cwd, nested.to_string_lossy());
        assert!(runtime.local_list_pending);
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("local list")
            .expect("message");
        match msg {
            crate::session::IoMessage::LocalListDone {
                path, generation, ..
            } => {
                assert_eq!(Path::new(&path), nested.as_path());
                assert_eq!(generation, runtime.local_list_gen);
            }
            _ => panic!("expected LocalListDone"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_highlighted_pending_removes_and_active_sets_flag() {
        let mut app = AppState::default();
        app.set_focus(FocusPane::Queue);
        let pending = TransferJob::new("/tmp/p", "/pub/p", TransferDirection::Upload);
        let active = TransferJob::new("/tmp/a", "/pub/a", TransferDirection::Upload);
        let active_id = active.id;
        app.queue.enqueue(active);
        app.queue.start_next();
        app.queue.enqueue(pending);
        app.selected_queue = 1;
        let (mut runtime, _rx) = test_runtime();
        cancel_highlighted_queue_job(&mut app, &mut runtime);
        assert_eq!(app.queue.pending.len(), 0);
        assert_eq!(app.queue.active.len(), 1);

        app.selected_queue = 0;
        let flag = Arc::new(AtomicBool::new(false));
        runtime.job_cancels.insert(active_id, flag.clone());
        cancel_highlighted_queue_job(&mut app, &mut runtime);
        assert!(flag.load(Ordering::Relaxed));
        assert_eq!(app.queue.active.len(), 1);
    }

    fn unique_temp(prefix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn open_key_picker_starts_in_existing_key_parent() {
        let dir = unique_temp("dd_ftp_picker_open");
        let ssh = dir.join(".ssh");
        std::fs::create_dir(&ssh).expect(".ssh");
        let key = ssh.join("id_ed25519");
        std::fs::write(&key, b"x").expect("key");

        let mut app = AppState {
            show_quick_connect: true,
            quick_connect: ConnectionInfo {
                private_key: Some(key.to_string_lossy().into_owned()),
                ..ConnectionInfo::default()
            },
            quick_connect_field: QuickConnectField::PrivateKey,
            ..Default::default()
        };
        app.qc_hydrate();
        open_key_picker(&mut app);

        assert!(app.show_key_picker);
        assert_eq!(
            Path::new(&app.key_picker_cwd),
            ssh.canonicalize().expect("canon").as_path()
        );
        assert_eq!(
            app.selected_key_picker_entry().map(|e| e.name.as_str()),
            Some("id_ed25519")
        );

        key_picker_go_parent(&mut app);
        assert_eq!(
            Path::new(&app.key_picker_cwd),
            dir.canonicalize().expect("canon parent").as_path()
        );
        assert_eq!(
            app.selected_key_picker_entry().map(|e| e.name.as_str()),
            Some(".ssh")
        );

        key_picker_activate_selected(&mut app);
        assert_eq!(
            Path::new(&app.key_picker_cwd),
            ssh.canonicalize().expect("canon ssh").as_path()
        );

        key_picker_activate_selected(&mut app);
        assert!(!app.show_key_picker);
        let got = app.quick_connect.private_key.expect("key path");
        assert_eq!(
            Path::new(&got).file_name().and_then(|n| n.to_str()),
            Some("id_ed25519")
        );
        assert!(Path::new(&got).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offset_index_clamps_and_pages() {
        assert_eq!(offset_index(0, 9, -1), 0);
        assert_eq!(offset_index(0, 9, 5), 5);
        assert_eq!(offset_index(8, 9, 5), 9);
        assert_eq!(offset_index(3, 9, -10), 0);
        assert_eq!(offset_index(0, 0, 5), 0);
    }

    #[tokio::test]
    async fn local_enter_during_scan_requests_list() {
        use dd_ftp_core::{EntryKind, FileEntry};
        use std::time::Duration;

        let dir = unique_temp("dd_ftp_nav_scan");
        let child = dir.join("sub");
        std::fs::create_dir(&child).expect("sub");
        let mut app = AppState {
            focus: FocusPane::Local,
            local_cwd: dir.to_string_lossy().into_owned(),
            local_entries: vec![FileEntry {
                name: "sub".into(),
                path: child.to_string_lossy().into_owned(),
                kind: EntryKind::Directory,
                size: 0,
                modified: None,
                permissions: None,
            }],
            selected_local: 0,
            ..Default::default()
        };
        let (mut runtime, mut rx) = crate::session::test_runtime();
        runtime.scan_generation = Some(1);
        assert!(crate::session::drain_busy(&runtime));
        navigate_into_directory(&mut app, &mut runtime);
        assert_eq!(app.local_cwd, child.to_string_lossy());
        assert!(runtime.local_list_pending);
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("local list")
            .expect("message");
        match msg {
            crate::session::IoMessage::LocalListDone {
                path, generation, ..
            } => {
                assert_eq!(Path::new(&path), child.as_path());
                assert_eq!(generation, runtime.local_list_gen);
            }
            _ => panic!("expected LocalListDone"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accept_key_event_drops_release_and_toggle_repeat() {
        let app = AppState::default();
        let press = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(accept_key_event(press, &app));
        let mut release = press;
        release.kind = KeyEventKind::Release;
        assert!(!accept_key_event(release, &app));
        let mut repeat_space = press;
        repeat_space.kind = KeyEventKind::Repeat;
        assert!(!accept_key_event(repeat_space, &app));
        let mut repeat_down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        repeat_down.kind = KeyEventKind::Repeat;
        assert!(accept_key_event(repeat_down, &app));
    }
}

#[derive(Debug, Clone, Copy)]
enum Jump {
    Page(i32),
    Home,
    End,
}

fn jump_help(app: &mut AppState, layout: &LayoutMap, jump: Jump) {
    let rows = page_rows_for(layout.help.unwrap_or_default());
    let last = layout.help_line_count.saturating_sub(rows);
    let target = match jump {
        Jump::Home => 0,
        Jump::End => last,
        Jump::Page(dir) => offset_index(app.help_scroll, last, dir.saturating_mul(rows as i32)),
    };
    reduce(
        app,
        Action::HelpScroll(target as i32 - app.help_scroll as i32),
    );
}

fn jump_key_picker(app: &mut AppState, layout: &LayoutMap, jump: Jump) {
    let last = app.visible_key_picker().len().saturating_sub(1);
    let rows = page_rows_for(layout.key_picker_list);
    let index = match jump {
        Jump::Home => 0,
        Jump::End => last,
        Jump::Page(dir) => offset_index(
            app.key_picker_selected,
            last,
            dir.saturating_mul(rows as i32),
        ),
    };
    reduce(app, Action::KeyPickerSelectIndex(index));
}

fn jump_bookmarks(app: &mut AppState, _layout: &LayoutMap, jump: Jump) {
    if app.bookmarks.is_empty() {
        return;
    }
    let last = app.bookmarks.len().saturating_sub(1);
    let rows = 8usize;
    app.selected_bookmark = match jump {
        Jump::Home => 0,
        Jump::End => last,
        Jump::Page(dir) => {
            offset_index(app.selected_bookmark, last, dir.saturating_mul(rows as i32))
        }
    };
}

fn jump_focused(app: &mut AppState, layout: &LayoutMap, jump: Jump) {
    match app.focus {
        FocusPane::Queue => {
            let last = app.visible_queue().len().saturating_sub(1);
            let rows = page_rows_for(layout.queue_list);
            let target = match jump {
                Jump::Home => 0,
                Jump::End => last,
                Jump::Page(dir) => {
                    offset_index(app.selected_queue, last, dir.saturating_mul(rows as i32))
                }
            };
            reduce(
                app,
                Action::SelectIndex {
                    pane: FocusPane::Queue,
                    index: target,
                },
            );
        }
        pane => {
            let (current, last, rows) = match pane {
                FocusPane::Local => (
                    app.selected_local,
                    app.visible_local().len().saturating_sub(1),
                    page_rows_for(layout.local_list),
                ),
                FocusPane::Remote => (
                    app.selected_remote,
                    app.visible_remote().len().saturating_sub(1),
                    page_rows_for(layout.remote_list),
                ),
                FocusPane::Queue => unreachable!(),
            };
            let index = match jump {
                Jump::Home => 0,
                Jump::End => last,
                Jump::Page(dir) => offset_index(current, last, dir.saturating_mul(rows as i32)),
            };
            reduce(app, Action::SelectIndex { pane, index });
        }
    }
}
