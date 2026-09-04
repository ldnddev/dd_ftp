use std::time::{Duration, Instant};

use crossterm::event::{MouseEvent, MouseEventKind};
use dd_ftp_app::{reduce, Action, AppState, FocusPane, QuickConnectField};
use dd_ftp_ui::{hit_test, ControlId, FieldId, Pane, Region, ScrollRegion};

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
) {
    let (mx, my) = (mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Moved => {
            app.mouse_pos = Some((mx, my));
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
                }
                Some(Region::Scrollbar(ScrollRegion::Queue)) => {
                    reduce(
                        app,
                        Action::QueueScroll(if up {
                            -(SCROLL_STEP as i32)
                        } else {
                            SCROLL_STEP as i32
                        }),
                    );
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
                }
                _ => {}
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
                            reduce(
                                app,
                                Action::SelectIndex {
                                    pane: match pane {
                                        Pane::Local => FocusPane::Local,
                                        Pane::Remote => FocusPane::Remote,
                                    },
                                    index: idx,
                                },
                            );
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
                Some(Region::Control(_)) if app.show_prompt => {}
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
                        ScrollRegion::Help => true,
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
        }
        MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
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
                        _ if app.show_quick_connect => {
                            let len = app.qc_field.len();
                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                            reduce(app, Action::QuickConnectExtendSelect(idx));
                        }
                        _ => {}
                    }
                }
            }
        }
        MouseEventKind::Up(_) => {
            *drag = None;
            *drag_field = None;
        }
        _ => {}
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
        Prompt => return None,
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
            let n = app.queue.pending.len()
                + app.queue.active.len()
                + app.queue.completed.len()
                + app.queue.failed.len()
                + app.queue.cancelled.len();
            let target = (frac * n as f32).round() as usize;
            reduce(
                app,
                Action::QueueScroll(target as i32 - app.queue_scroll as i32),
            );
        }
        ScrollRegion::Help => {
            let target = (frac * track.height as f32).round() as usize;
            reduce(
                app,
                Action::HelpScroll(target as i32 - app.help_scroll as i32),
            );
        }
    }
}
