use std::time::{Duration, Instant};

use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use dd_ftp_app::{reduce, Action, AppState, FocusPane, QuickConnectField, SettingsField};
use dd_ftp_ui::{
    hit_test, path_seg_action, ControlId, FieldId, Pane, PathSegAction, Region, ScrollRegion,
};

use crate::session::Runtime;

pub(crate) const SCROLL_STEP: usize = 3;

pub(crate) fn handle_mouse(
    app: &mut AppState,
    runtime: &mut Runtime,
    app_layout: &dd_ftp_ui::LayoutMap,
    mouse: MouseEvent,
    last_click: &mut Option<(u16, u16, Instant)>,
    drag: &mut Option<ScrollRegion>,
    drag_field: &mut Option<FieldId>,
) -> bool {
    let (mx, my) = (mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Moved => {
            let old = app.mouse_pos;
            let new = Some((mx, my));
            if old == new {
                return false;
            }
            app.mouse_pos = new;
            app_layout.hovered_scrollbar(old) != app_layout.hovered_scrollbar(new)
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
            match hit_test(app_layout, mx, my) {
                Some(Region::List(Pane::Local))
                | Some(Region::Scrollbar(ScrollRegion::ListLocal))
                    if !app.any_modal_open() =>
                {
                    reduce(app, Action::SetFocus(FocusPane::Local));
                    for _ in 0..SCROLL_STEP {
                        reduce(
                            app,
                            if up {
                                Action::SelectUp
                            } else {
                                Action::SelectDown
                            },
                        );
                    }
                    true
                }
                Some(Region::List(Pane::Remote))
                | Some(Region::Scrollbar(ScrollRegion::ListRemote))
                    if !app.any_modal_open() =>
                {
                    reduce(app, Action::SetFocus(FocusPane::Remote));
                    for _ in 0..SCROLL_STEP {
                        reduce(
                            app,
                            if up {
                                Action::SelectUp
                            } else {
                                Action::SelectDown
                            },
                        );
                    }
                    true
                }
                Some(Region::QueueList) | Some(Region::Scrollbar(ScrollRegion::Queue))
                    if !app.any_modal_open() =>
                {
                    reduce(app, Action::SetFocus(FocusPane::Queue));
                    for _ in 0..SCROLL_STEP {
                        reduce(
                            app,
                            if up {
                                Action::SelectUp
                            } else {
                                Action::SelectDown
                            },
                        );
                    }
                    true
                }
                Some(Region::Scrollbar(ScrollRegion::Help)) => {
                    reduce(
                        app,
                        Action::HelpScroll(if up {
                            -(SCROLL_STEP as i32)
                        } else {
                            SCROLL_STEP as i32
                        }),
                    );
                    true
                }
                Some(Region::KeyPickerList) | Some(Region::Scrollbar(ScrollRegion::KeyPicker))
                    if app.show_key_picker =>
                {
                    for _ in 0..SCROLL_STEP {
                        reduce(
                            app,
                            if up {
                                Action::KeyPickerSelectUp
                            } else {
                                Action::KeyPickerSelectDown
                            },
                        );
                    }
                    true
                }
                _ => false,
            }
        }
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            let now = Instant::now();
            let is_double = last_click
                .map(|(lx, ly, t)| {
                    lx == mx && ly == my && now.duration_since(t) < Duration::from_millis(300)
                })
                .unwrap_or(false);
            *last_click = Some((mx, my, now));
            match hit_test(app_layout, mx, my) {
                Some(Region::QueueList) if !app.any_modal_open() => {
                    reduce(app, Action::SetFocus(FocusPane::Queue));
                    let list_rect = app_layout.queue_list;
                    if my >= list_rect.y {
                        let row = (my - list_rect.y) as usize;
                        let idx = app_layout.queue_list_offset + row;
                        if idx < app.visible_queue().len() {
                            reduce(
                                app,
                                Action::SelectIndex {
                                    pane: FocusPane::Queue,
                                    index: idx,
                                },
                            );
                        }
                    }
                }
                Some(Region::List(pane)) if !app.any_modal_open() => {
                    reduce(
                        app,
                        Action::SetFocus(match pane {
                            Pane::Local => FocusPane::Local,
                            Pane::Remote => FocusPane::Remote,
                        }),
                    );
                    let (list_rect, offset, len) = match pane {
                        Pane::Local => (
                            app_layout.local_list,
                            app_layout.local_list_offset,
                            app.visible_local().len(),
                        ),
                        Pane::Remote => (
                            app_layout.remote_list,
                            app_layout.remote_list_offset,
                            app.visible_remote().len(),
                        ),
                    };
                    let content_top = list_rect.y + 1; // top border
                    if my >= content_top {
                        let row = (my - content_top) as usize;
                        let idx = offset + row;
                        if idx < len {
                            let focus_pane = match pane {
                                Pane::Local => FocusPane::Local,
                                Pane::Remote => FocusPane::Remote,
                            };
                            let prev = match pane {
                                Pane::Local => app.selected_local,
                                Pane::Remote => app.selected_remote,
                            };
                            reduce(
                                app,
                                Action::SelectIndex {
                                    pane: focus_pane,
                                    index: idx,
                                },
                            );
                            if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                                reduce(
                                    app,
                                    Action::MarkRange {
                                        pane: focus_pane,
                                        from: prev,
                                        to: idx,
                                    },
                                );
                            }
                            if is_double {
                                *last_click = None;
                                let is_dir = match pane {
                                    Pane::Local => app.selected_local_entry().is_some_and(|e| {
                                        e.kind == dd_ftp_core::EntryKind::Directory
                                    }),
                                    Pane::Remote => app.selected_remote_entry().is_some_and(|e| {
                                        e.kind == dd_ftp_core::EntryKind::Directory
                                    }),
                                };
                                if is_dir {
                                    crate::events::navigate_into_directory(app, runtime);
                                } else {
                                    match pane {
                                        Pane::Local => {
                                            crate::session::queue_upload_selected(app, runtime)
                                        }
                                        Pane::Remote => {
                                            crate::session::queue_download_selected(app, runtime)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Region::Control(ControlId::LocalPathSeg(i))) if !app.any_modal_open() => {
                    reduce(app, Action::SetFocus(FocusPane::Local));
                    match path_seg_action(&app_layout.local_path_targets, i, is_double).map(|a| {
                        match a {
                            PathSegAction::Parent => None,
                            PathSegAction::Goto(p) => Some(p.to_string()),
                        }
                    }) {
                        Some(None) => crate::events::navigate_parent_directory(app, runtime),
                        Some(Some(path)) => crate::events::navigate_local_path(app, runtime, path),
                        None => {}
                    }
                }
                Some(Region::Control(ControlId::RemotePathSeg(i))) if !app.any_modal_open() => {
                    reduce(app, Action::SetFocus(FocusPane::Remote));
                    match path_seg_action(&app_layout.remote_path_targets, i, is_double).map(|a| {
                        match a {
                            PathSegAction::Parent => None,
                            PathSegAction::Goto(p) => Some(p.to_string()),
                        }
                    }) {
                        Some(None) => crate::events::navigate_parent_directory(app, runtime),
                        Some(Some(path)) => crate::events::navigate_remote_path(app, runtime, path),
                        None => {}
                    }
                }
                Some(Region::Control(_)) if app.show_prompt => {}
                Some(Region::Control(cid)) if app.show_settings => {
                    if let Some(field) = match cid {
                        ControlId::SettingsWorkers => Some(SettingsField::Workers),
                        ControlId::SettingsHideDotfiles => Some(SettingsField::HideDotfiles),
                        ControlId::SettingsOverwrite => Some(SettingsField::Overwrite),
                        ControlId::SettingsAutoConnect => Some(SettingsField::AutoConnect),
                        _ => None,
                    } {
                        reduce(app, Action::SettingsSelectField(field));
                        reduce(app, Action::SettingsCycleValue(1));
                    }
                }
                Some(Region::Control(ControlId::QcBrowseKey)) if app.show_quick_connect => {
                    crate::events::open_key_picker(app);
                }
                Some(Region::KeyPickerList) if app.show_key_picker => {
                    let list_rect = app_layout.key_picker_list;
                    let content_top = list_rect.y + 1;
                    if my >= content_top {
                        let row = (my - content_top) as usize;
                        let idx = app_layout.key_picker_list_offset + row;
                        let len = app.visible_key_picker().len();
                        if idx < len {
                            reduce(app, Action::KeyPickerSelectIndex(idx));
                            if is_double {
                                *last_click = None;
                                crate::events::key_picker_activate_selected(app);
                            }
                        }
                    }
                }
                Some(Region::Control(ControlId::QcProtocol)) => {
                    reduce(app, Action::QuickConnectSetProtocolNext);
                }
                Some(Region::Control(ControlId::BookmarkRow(i))) => {
                    app.selected_bookmark = i.min(app.bookmarks.len().saturating_sub(1));
                    if is_double {
                        *last_click = None;
                        if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                            let bm = crate::bookmarks::hydrate_password_from_keyring(
                                app,
                                bm,
                                "bookmark-load",
                            );
                            reduce(app, Action::QuickConnectSetFromBookmark(bm));
                            reduce(app, Action::ToggleBookmarks);
                            reduce(app, Action::ToggleQuickConnect);
                        }
                    }
                }
                Some(Region::Scrollbar(sr)) => {
                    let allow = match sr {
                        ScrollRegion::Help | ScrollRegion::KeyPicker => true,
                        _ => !app.any_modal_open(),
                    };
                    if allow {
                        *drag = Some(sr);
                        apply_scrollbar_drag(app, app_layout, sr, my);
                    }
                }
                Some(Region::Field(fid)) => {
                    if let Some(fr) = app_layout.fields.iter().find(|f| f.id == fid).copied() {
                        match fid {
                            FieldId::Prompt if app.is_text_prompt() => {
                                let len = app.prompt_value.len();
                                let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                app.prompt_value.begin_drag(idx);
                                *drag_field = Some(fid);
                            }
                            FieldId::Prompt => {}
                            FieldId::SettingsEditor | FieldId::SettingsLastLocal
                                if app.show_settings =>
                            {
                                let field = match fid {
                                    FieldId::SettingsLastLocal => SettingsField::LastLocalCwd,
                                    _ => SettingsField::Editor,
                                };
                                reduce(app, Action::SettingsSelectField(field));
                                let len = app.settings_text_mut().map(|tf| tf.len()).unwrap_or(0);
                                let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                reduce(app, Action::SettingsBeginSelect(idx));
                                *drag_field = Some(fid);
                            }
                            _ => {
                                if let Some(qf) = qc_field_for(fid) {
                                    app.quick_connect_field = qf;
                                    reduce(app, Action::QuickConnectSyncField);
                                    let len = app.qc_field.len();
                                    let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                    reduce(app, Action::QuickConnectBeginSelect(idx));
                                    *drag_field = Some(fid);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            true
        }
        MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
            let dragging = drag.is_some() || drag_field.is_some();
            if let Some(sr) = *drag {
                apply_scrollbar_drag(app, app_layout, sr, my);
            }
            if let Some(fid) = *drag_field {
                if let Some(fr) = app_layout.fields.iter().find(|f| f.id == fid).copied() {
                    match fid {
                        FieldId::Prompt if app.is_text_prompt() => {
                            let len = app.prompt_value.len();
                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                            app.prompt_value.extend_drag(idx);
                        }
                        FieldId::Prompt => {}
                        FieldId::SettingsEditor | FieldId::SettingsLastLocal
                            if app.show_settings =>
                        {
                            let len = app.settings_text_mut().map(|tf| tf.len()).unwrap_or(0);
                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                            reduce(app, Action::SettingsExtendSelect(idx));
                        }
                        _ if app.show_quick_connect => {
                            let len = app.qc_field.len();
                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                            reduce(app, Action::QuickConnectExtendSelect(idx));
                        }
                        _ => {}
                    }
                }
            }
            dragging
        }
        MouseEventKind::Up(_) => {
            *drag = None;
            *drag_field = None;
            false
        }
        _ => false,
    }
}

pub(crate) fn qc_field_for(fid: dd_ftp_ui::FieldId) -> Option<QuickConnectField> {
    use dd_ftp_ui::FieldId::*;
    Some(match fid {
        QcName => QuickConnectField::Name,
        QcHost => QuickConnectField::Host,
        QcPort => QuickConnectField::Port,
        QcUsername => QuickConnectField::Username,
        QcPassword => QuickConnectField::Password,
        QcPrivateKey => QuickConnectField::PrivateKey,
        QcPath => QuickConnectField::Path,
        Prompt | SettingsEditor | SettingsLastLocal => return None,
    })
}

pub(crate) fn apply_scrollbar_drag(
    app: &mut AppState,
    layout: &dd_ftp_ui::LayoutMap,
    sr: dd_ftp_ui::ScrollRegion,
    my: u16,
) {
    let track = match sr {
        ScrollRegion::ListLocal => layout.local_scrollbar,
        ScrollRegion::ListRemote => layout.remote_scrollbar,
        ScrollRegion::Queue => layout.queue_scrollbar,
        ScrollRegion::Help => layout.help_scrollbar.unwrap_or_default(),
        ScrollRegion::KeyPicker => layout.key_picker_scrollbar,
    };
    if track.height == 0 {
        return;
    }
    let rel = my
        .saturating_sub(track.y)
        .min(track.height.saturating_sub(1));
    let denom = track.height.saturating_sub(1).max(1) as f32;
    let frac = rel as f32 / denom;
    match sr {
        ScrollRegion::ListLocal => {
            let n = app.visible_local().len().saturating_sub(1);
            reduce(
                app,
                Action::SelectIndex {
                    pane: FocusPane::Local,
                    index: (frac * n as f32).round() as usize,
                },
            );
        }
        ScrollRegion::ListRemote => {
            let n = app.visible_remote().len().saturating_sub(1);
            reduce(
                app,
                Action::SelectIndex {
                    pane: FocusPane::Remote,
                    index: (frac * n as f32).round() as usize,
                },
            );
        }
        ScrollRegion::Queue => {
            let n = app.visible_queue().len().saturating_sub(1);
            reduce(
                app,
                Action::SelectIndex {
                    pane: FocusPane::Queue,
                    index: (frac * n as f32).round() as usize,
                },
            );
        }
        ScrollRegion::Help => {
            let target = (frac * track.height as f32).round() as usize;
            reduce(
                app,
                Action::HelpScroll(target as i32 - app.help_scroll as i32),
            );
        }
        ScrollRegion::KeyPicker => {
            let n = app.visible_key_picker().len().saturating_sub(1);
            reduce(
                app,
                Action::KeyPickerSelectIndex((frac * n as f32).round() as usize),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::MouseEvent;
    use dd_ftp_ui::LayoutMap;
    use ratatui::layout::Rect;

    use crate::session::test_runtime;

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn layout_with_local_bar() -> LayoutMap {
        LayoutMap {
            local_list: Rect::new(0, 3, 40, 20),
            local_scrollbar: Rect::new(39, 3, 1, 20),
            ..Default::default()
        }
    }

    #[test]
    fn mouse_move_redraws_only_when_scrollbar_hover_changes() {
        let mut app = AppState::default();
        let (mut runtime, _rx) = test_runtime();
        let layout = layout_with_local_bar();
        let mut last_click = None;
        let mut drag = None;
        let mut drag_field = None;

        assert!(!handle_mouse(
            &mut app,
            &mut runtime,
            &layout,
            mouse(MouseEventKind::Moved, 10, 10),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        ));
        assert_eq!(app.mouse_pos, Some((10, 10)));

        assert!(!handle_mouse(
            &mut app,
            &mut runtime,
            &layout,
            mouse(MouseEventKind::Moved, 12, 11),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        ));

        assert!(handle_mouse(
            &mut app,
            &mut runtime,
            &layout,
            mouse(MouseEventKind::Moved, 39, 10),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        ));
        assert!(!handle_mouse(
            &mut app,
            &mut runtime,
            &layout,
            mouse(MouseEventKind::Moved, 39, 12),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        ));
        assert!(!handle_mouse(
            &mut app,
            &mut runtime,
            &layout,
            mouse(
                MouseEventKind::Up(crossterm::event::MouseButton::Left),
                39,
                12
            ),
            &mut last_click,
            &mut drag,
            &mut drag_field,
        ));
    }
}
