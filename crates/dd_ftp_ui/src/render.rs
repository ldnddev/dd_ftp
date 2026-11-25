use std::time::Duration;

use dd_ftp_app::{
    is_dot_or_dotdot, is_public_key_name, AppState, ChoicePromptKind, FocusPane, PromptKind,
    QuickConnectField, SettingsField, TextPromptKind, ToastLevel, SETTINGS_HELP, SETTINGS_LAST,
    SETTINGS_SESSION, SETTINGS_THEMES,
};
use dd_ftp_core::{Protocol, TransferJob, TransferStatus};
use dd_ftp_transfer::TransferQueue;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

use crate::compare::{classify_compare_indexed, index_by_name, CompareBadge};
use crate::keys::{KeyGroup, KEYMAP};
use crate::layout::{
    split_fs_path, split_remote_path, ControlId, ControlRegion, FieldId, FieldRegion, LayoutMap,
    PathSeg,
};
use crate::theme::{cached_theme, Theme};

/// Pulse bars for the working loader: ▃ d_d ▃ → ▅ d_d ▅ → █ d_d █ → ▅ d_d ▅
const BUSY_BARS: [char; 4] = ['▃', '▅', '█', '▅'];
const BUSY_FRAME_MS: u128 = 150;

fn busy_now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn busy_bar(now_ms: u128) -> char {
    BUSY_BARS[(now_ms / BUSY_FRAME_MS) as usize % BUSY_BARS.len()]
}

#[cfg(test)]
fn busy_label(now_ms: u128) -> String {
    let bar = busy_bar(now_ms);
    format!("{bar} d_d {bar}")
}

fn busy_spans(now_ms: u128, t: &Theme) -> Vec<Span<'static>> {
    let bar = busy_bar(now_ms).to_string();
    vec![
        Span::styled(bar.clone(), Style::default().fg(t.info)),
        Span::styled(
            " d_d ",
            Style::default()
                .fg(t.text_active_focus)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(bar, Style::default().fg(t.info)),
    ]
}

fn render_field_line(tf: &dd_ftp_app::TextField, masked: bool, t: &Theme) -> Vec<Span<'static>> {
    let display: Vec<char> = if masked {
        std::iter::repeat_n('*', tf.len()).collect()
    } else {
        tf.value.chars().collect()
    };
    let sel = tf.selected_range();
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, ch) in display.iter().enumerate() {
        let selected = sel.is_some_and(|(lo, hi)| i >= lo && i < hi);
        let mut style = if selected {
            Style::default().fg(t.input_text_focus).bg(t.selection)
        } else {
            Style::default().fg(t.input_text_focus)
        };
        if i == tf.cursor {
            // block caret over this char: no extra column, so hit-testing stays aligned
            style = style
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::RAPID_BLINK);
        }
        spans.push(Span::styled(ch.to_string(), style));
    }
    if tf.cursor >= display.len() {
        spans.push(Span::styled(
            "█".to_string(),
            Style::default()
                .fg(t.cursor)
                .add_modifier(Modifier::RAPID_BLINK),
        ));
    }
    spans
}

pub fn render(frame: &mut Frame, app: &mut AppState, map: &mut LayoutMap) {
    *map = LayoutMap::default();
    let loaded = cached_theme();
    let t = loaded.theme;

    // Full app background (header + footer included)
    frame.render_widget(
        Block::default().style(Style::default().bg(t.base_background)),
        frame.area(),
    );

    // Idle queue is a 3-row strip (title, worker summary, bottom border), even
    // when focused. With jobs the panel grows by one row each, capped at 8,
    // or 12 while the queue pane is focused.
    let queue_height = queue_panel_height(app.visible_queue().len(), app.focus == FocusPane::Queue);
    let filter_height = if app.show_filter { 1 } else { 0 };

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(filter_height),
            Constraint::Min(1),
            Constraint::Length(queue_height),
            Constraint::Length(1),
        ])
        .split(frame.area());

    if app.show_filter {
        let filter_bg = if app.show_filter {
            t.modal_background
        } else {
            t.base_background
        };
        let (visible, total) = match app.filter_count_pane() {
            FocusPane::Remote => (app.visible_remote().len(), app.remote_entries.len()),
            _ => (app.visible_local().len(), app.local_entries.len()),
        };
        let count = format!("  ({visible}/{total})");
        let filter = Paragraph::new(Line::from(vec![
            Span::styled(" Filter: ", Style::default().fg(t.text_labels)),
            Span::styled(&app.filter_pattern, Style::default().fg(t.input_text_focus)),
            Span::styled(
                "█",
                Style::default()
                    .fg(t.cursor)
                    .add_modifier(Modifier::RAPID_BLINK),
            ),
            Span::styled(count, Style::default().fg(t.text_secondary)),
        ]))
        .style(Style::default().bg(filter_bg));
        frame.render_widget(filter, vertical[1]);
    }

    // Header: fixed 3-line bordered shell. Title-left brands the app,
    // title-right shows live connection state, body line is a random tagline.
    let conn_label = app
        .active_connection
        .as_ref()
        .map(|c| {
            if c.name.trim().is_empty() {
                format!(" {}@{}:{} ", c.username, c.host, c.port)
            } else {
                format!(" {} ", c.name)
            }
        })
        .unwrap_or_else(|| " disconnected ".to_string());

    let tick_ms = busy_now_ms();
    let mut header_title = vec![Span::styled(
        " dd_ftp ",
        Style::default()
            .fg(t.text_active_focus)
            .add_modifier(Modifier::BOLD),
    )];
    if app.busy {
        header_title.push(Span::raw(" "));
        header_title.extend(busy_spans(tick_ms, &t));
        header_title.push(Span::raw(" "));
    }

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_active))
        .style(Style::default().bg(t.base_background))
        .title(Line::from(header_title))
        .title_top(
            Line::from(vec![Span::styled(
                conn_label,
                Style::default().fg(if app.connected {
                    t.success
                } else {
                    t.text_secondary
                }),
            )])
            .right_aligned(),
        );
    let header = Paragraph::new(Line::from(Span::styled(
        app.header_copy.as_str(),
        Style::default().fg(t.text_secondary),
    )))
    .block(header_block)
    .style(Style::default().bg(t.base_background));
    frame.render_widget(header, vertical[0]);

    // Main content background (local/remote/queue region)
    let content_area = vertical[2];
    let queue_area = vertical[3];
    let status_area = vertical[4];
    map.queue = queue_area;

    frame.render_widget(
        Block::default().style(Style::default().bg(t.body_background)),
        content_area,
    );
    frame.render_widget(
        Block::default().style(Style::default().bg(t.body_background)),
        queue_area,
    );

    let content_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(content_area);

    let path_panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_rows[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_rows[1]);
    map.local_list = panes[0];
    map.remote_list = panes[1];

    let local_visible = app.visible_local();
    let local_visible_len = local_visible.len();
    let local_row_width = panes[0].width.saturating_sub(4) as usize;
    let local_cols = meta_columns(local_row_width.saturating_sub(6));
    let compare_local = if app.show_compare {
        Some(index_by_name(&app.local_entries))
    } else {
        None
    };
    let compare_remote = if app.show_compare {
        Some(index_by_name(&app.remote_entries))
    } else {
        None
    };
    let local_items: Vec<ListItem> = local_visible
        .into_iter()
        .map(|e| {
            let badge = match (&compare_local, &compare_remote) {
                (Some(local), Some(remote)) => classify_compare_indexed(&e.name, local, remote),
                _ => None,
            };
            file_list_item(
                e,
                local_row_width,
                local_cols,
                app.marked_local.contains(&e.path),
                badge,
                &t,
            )
        })
        .collect();
    let remote_visible = if app.connected {
        app.visible_remote()
    } else {
        Vec::new()
    };
    let remote_visible_len = remote_visible.len();
    let remote_row_width = panes[1].width.saturating_sub(4) as usize;
    let remote_cols = meta_columns(remote_row_width.saturating_sub(6));
    let remote_items: Vec<ListItem> = remote_visible
        .into_iter()
        .map(|e| {
            let badge = match (&compare_local, &compare_remote) {
                (Some(local), Some(remote)) => classify_compare_indexed(&e.name, local, remote),
                _ => None,
            };
            file_list_item(
                e,
                remote_row_width,
                remote_cols,
                app.marked_remote.contains(&e.path),
                badge,
                &t,
            )
        })
        .collect();

    let local_style = if app.focus == FocusPane::Local {
        Style::default().fg(t.border_active)
    } else {
        Style::default().fg(t.border_default)
    };

    let remote_style = if app.focus == FocusPane::Remote {
        Style::default().fg(t.border_active)
    } else {
        Style::default().fg(t.border_default)
    };

    let local_title_style = if app.focus == FocusPane::Local {
        Style::default()
            .fg(t.text_active_focus)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.text_labels)
    };

    let mut path_controls = Vec::new();
    let local_targets = render_path_bar(
        frame,
        path_panes[0],
        " Local Path ",
        &split_fs_path(&app.local_cwd),
        &t,
        &mut path_controls,
        ControlId::LocalPathSeg,
    );
    let remote_targets = render_path_bar(
        frame,
        path_panes[1],
        " Remote Path ",
        &split_remote_path(&app.remote_cwd),
        &t,
        &mut path_controls,
        ControlId::RemotePathSeg,
    );
    map.local_path_targets = local_targets;
    map.remote_path_targets = remote_targets;
    map.controls.extend(path_controls);

    let local_marked = app.marked_live_count(FocusPane::Local);
    let local_title = if local_marked > 0 {
        format!(
            " [1] Local  {}  {} marked ",
            app.sort_title_suffix(),
            local_marked
        )
    } else {
        format!(" [1] Local  {} ", app.sort_title_suffix())
    };
    let local = List::new(local_items)
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    local_title,
                    local_title_style,
                )]))
                .borders(Borders::ALL)
                .border_style(local_style),
        )
        .highlight_symbol("▶ ")
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(
            Style::default()
                .bg(t.selected_background)
                .fg(t.text_active_focus)
                .add_modifier(Modifier::BOLD),
        );

    let remote_title_style = if app.focus == FocusPane::Remote {
        Style::default()
            .fg(t.text_active_focus)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.text_labels)
    };

    let remote_marked = app.marked_live_count(FocusPane::Remote);
    let remote_title = if remote_marked > 0 {
        format!(
            " [2] Remote  {}  {} marked ",
            app.sort_title_suffix(),
            remote_marked
        )
    } else {
        format!(" [2] Remote  {} ", app.sort_title_suffix())
    };
    let remote = List::new(remote_items)
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    remote_title,
                    remote_title_style,
                )]))
                .borders(Borders::ALL)
                .border_style(remote_style),
        )
        .highlight_symbol("▶ ")
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(
            Style::default()
                .bg(t.selected_background)
                .fg(t.text_active_focus)
                .add_modifier(Modifier::BOLD),
        );

    let mut local_state = ListState::default();
    local_state.select(Some(app.selected_local));

    let mut remote_state = ListState::default();
    remote_state.select(Some(app.selected_remote));

    frame.render_stateful_widget(local, panes[0], &mut local_state);
    map.local_list_offset = local_state.offset();
    frame.render_stateful_widget(remote, panes[1], &mut remote_state);
    map.remote_list_offset = remote_state.offset();

    if !app.connected {
        frame.render_widget(
            Paragraph::new("Not connected — o Quick Connect · m Bookmarks · F1 Settings")
                .style(Style::default().fg(t.text_secondary).bg(t.body_background))
                .wrap(Wrap { trim: true }),
            inner_list_rect(panes[1]),
        );
    } else if remote_visible_len == 0 {
        frame.render_widget(
            Paragraph::new("empty directory")
                .style(Style::default().fg(t.text_secondary).bg(t.body_background)),
            inner_list_rect(panes[1]),
        );
    }

    render_scrollbar(
        frame,
        panes[0],
        app.selected_local,
        local_visible_len,
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );
    render_scrollbar(
        frame,
        panes[1],
        app.selected_remote,
        remote_visible_len,
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );
    map.local_scrollbar = Rect {
        x: panes[0].x + panes[0].width.saturating_sub(1),
        y: panes[0].y,
        width: 1,
        height: panes[0].height,
    };
    map.remote_scrollbar = Rect {
        x: panes[1].x + panes[1].width.saturating_sub(1),
        y: panes[1].y,
        width: 1,
        height: panes[1].height,
    };
    map.queue_scrollbar = Rect {
        x: queue_area.x + queue_area.width.saturating_sub(1),
        y: queue_area.y,
        width: 1,
        height: queue_area.height,
    };

    let queue_style = if app.focus == FocusPane::Queue {
        Style::default().fg(t.border_active)
    } else {
        Style::default().fg(t.border_default)
    };

    let mut worker_spans = vec![Span::styled(
        "Worker: ",
        Style::default().fg(t.text_secondary),
    )];
    if app.busy && app.worker_running {
        worker_spans.extend(busy_spans(tick_ms, &t));
    } else if app.worker_running {
        worker_spans.push(Span::styled("running", Style::default().fg(t.info)));
    } else {
        worker_spans.push(Span::styled("idle", Style::default().fg(t.info)));
    }

    worker_spans.push(Span::raw(format!(
        " ({}/{}) | Pending: {} | Active: {} | Complete: {} | Failed: {} | Cancelled: {}",
        app.worker_active_count,
        app.worker_max_concurrency,
        app.queue.pending.len(),
        app.queue.active.len(),
        app.queue.completed.len(),
        app.queue.failed.len(),
        app.queue.cancelled.len()
    )));
    let queue_jobs = app.visible_queue();
    map.queue_line_count = queue_jobs.len();

    let queue_title_style = if app.focus == FocusPane::Queue {
        Style::default()
            .fg(t.text_active_focus)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.text_labels)
    };

    frame.render_widget(
        Block::default()
            .title(Line::from(vec![Span::styled(
                " [3] Queue ",
                queue_title_style,
            )]))
            .borders(Borders::ALL)
            .border_style(queue_style)
            .style(Style::default().bg(t.body_background)),
        queue_area,
    );

    let inner = queue_area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let worker_line = Paragraph::new(Line::from(worker_spans))
        .style(Style::default().bg(t.body_background).fg(t.text_primary));

    if queue_jobs.is_empty() {
        // One content row: the summary. A second chunk would take that row and
        // hide "Worker: idle …".
        frame.render_widget(worker_line, inner);
        map.queue_list = inner;
        map.queue_list_offset = 0;
    } else {
        let queue_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);
        frame.render_widget(worker_line, queue_chunks[0]);
        map.queue_list = queue_chunks[1];
        let items: Vec<ListItem> = queue_jobs
            .iter()
            .map(|job| {
                let (tag, color) = match job.status {
                    TransferStatus::Active => ("A ", t.info),
                    TransferStatus::Failed => ("F ", t.error),
                    _ => ("P ", t.text_secondary),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(tag, Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::raw(format_job_summary(job, &app.queue)),
                ]))
            })
            .collect();
        let list = List::new(items)
            .style(Style::default().bg(t.body_background).fg(t.text_primary))
            .highlight_symbol("▶ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(
                Style::default()
                    .bg(t.selected_background)
                    .fg(t.text_active_focus)
                    .add_modifier(Modifier::BOLD),
            );
        let mut list_state = ListState::default();
        list_state.select(Some(app.selected_queue));
        frame.render_stateful_widget(list, queue_chunks[1], &mut list_state);
        map.queue_list_offset = list_state.offset();
    }
    render_scrollbar(
        frame,
        queue_area,
        app.selected_queue,
        map.queue_line_count.saturating_add(1),
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );

    let footer_keys = Paragraph::new(footer_hint(app, status_area.width))
        .style(Style::default().fg(t.text_secondary).bg(t.base_background));
    frame.render_widget(footer_keys, status_area);

    if app.busy {
        let mut spans = busy_spans(tick_ms, &t);
        if !app.status.is_empty() {
            let s = app.status.to_lowercase();
            let status_color = if s.contains("failed") || s.contains("error") {
                t.error
            } else if s.contains("saved") || s.contains("connected") || s.contains("complete") {
                t.success
            } else if s.contains("loading") || s.contains("refresh") || s.contains("processing") {
                t.info
            } else {
                t.info
            };
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                app.status.clone(),
                Style::default().fg(status_color),
            ));
        }
        spans.push(Span::raw(" "));
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .alignment(Alignment::Right)
                .style(Style::default().bg(t.base_background)),
            status_area,
        );
    }

    if app.show_settings {
        render_settings(frame, app, map, &t);
    }

    if app.show_quick_connect {
        let area = centered_rect(80, 75, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let protocol = match app.quick_connect.protocol {
            Protocol::Sftp => "SFTP",
            Protocol::Ftp => "FTP",
            Protocol::Ftps => "FTPS",
        };

        let password_mask = app
            .quick_connect
            .password
            .as_ref()
            .map(|p| "*".repeat(p.len()))
            .unwrap_or_default();

        let private_key = app.quick_connect.private_key.clone().unwrap_or_default();

        let fields = [
            (
                QuickConnectField::Name,
                "Name",
                app.quick_connect.name.clone(),
            ),
            (
                QuickConnectField::Host,
                "Host",
                app.quick_connect.host.clone(),
            ),
            (
                QuickConnectField::Port,
                "Port",
                app.quick_connect.port.to_string(),
            ),
            (
                QuickConnectField::Username,
                "User",
                app.quick_connect.username.clone(),
            ),
            (QuickConnectField::Password, "Pass", password_mask),
            (QuickConnectField::PrivateKey, "SSH Key", private_key),
            (
                QuickConnectField::Protocol,
                "Protocol",
                protocol.to_string(),
            ),
            (
                QuickConnectField::Path,
                "Path",
                app.quick_connect.initial_path.clone(),
            ),
        ];

        let outer = Block::default()
            .title(Line::from(vec![Span::styled(
                " Connection ",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_active));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(3),
            ])
            .split(inner);

        let header = Paragraph::new("Quick Connect")
            .style(Style::default().fg(t.modal_labels).bg(t.modal_background));
        frame.render_widget(header, chunks[0]);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(chunks[1]);

        for row_idx in 0..4 {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(rows[row_idx]);

            for col_idx in 0..2 {
                let idx = row_idx * 2 + col_idx;
                let (field, label, value) = &fields[idx];
                let focused = *field == app.quick_connect_field;
                let border_color = if focused {
                    t.input_border_focus
                } else {
                    t.input_border_default
                };
                let text_color = if focused {
                    t.input_text_focus
                } else {
                    t.input_text_default
                };

                let input_content = if focused && *field != QuickConnectField::Protocol {
                    let masked = *field == QuickConnectField::Password;
                    Line::from(render_field_line(&app.qc_field, masked, &t))
                } else if focused {
                    // Protocol focused: show its value with a trailing caret as before
                    Line::from(vec![
                        Span::styled(value.clone(), Style::default().fg(text_color)),
                        Span::styled(
                            "█".to_string(),
                            Style::default()
                                .fg(t.cursor)
                                .add_modifier(Modifier::RAPID_BLINK),
                        ),
                    ])
                } else {
                    Line::from(Span::styled(value.clone(), Style::default().fg(text_color)))
                };

                let cell = cols[col_idx];
                let (input_area, browse_area) = if *field == QuickConnectField::PrivateKey {
                    let split = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Min(8), Constraint::Length(10)])
                        .split(cell);
                    (split[0], Some(split[1]))
                } else {
                    (cell, None)
                };

                let input = Paragraph::new(input_content)
                    .style(Style::default().bg(t.modal_background))
                    .block(
                        Block::default()
                            .title(Line::from(vec![Span::styled(
                                format!(" {} ", label),
                                Style::default().fg(t.text_labels_active),
                            )]))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(border_color)),
                    );

                frame.render_widget(input, input_area);

                if let Some(browse) = browse_area {
                    let browse_widget = Paragraph::new(Line::from(Span::styled(
                        "…",
                        Style::default().fg(t.modal_text),
                    )))
                    .alignment(Alignment::Center)
                    .style(Style::default().bg(t.modal_background))
                    .block(
                        Block::default()
                            .title(Line::from(vec![Span::styled(
                                " Browse ",
                                Style::default().fg(t.text_labels_active),
                            )]))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(t.input_border_default)),
                    );
                    frame.render_widget(browse_widget, browse);
                }

                if !app.show_key_picker {
                    match field {
                        QuickConnectField::Protocol => {
                            map.controls.push(ControlRegion {
                                id: ControlId::QcProtocol,
                                area: cell,
                            });
                        }
                        QuickConnectField::PrivateKey => {
                            map.fields.push(FieldRegion {
                                id: FieldId::QcPrivateKey,
                                area: input_area,
                                text_x: input_area.x + 1,
                            });
                            if let Some(browse) = browse_area {
                                map.controls.push(ControlRegion {
                                    id: ControlId::QcBrowseKey,
                                    area: browse,
                                });
                            }
                        }
                        other => {
                            let fid = match other {
                                QuickConnectField::Name => FieldId::QcName,
                                QuickConnectField::Host => FieldId::QcHost,
                                QuickConnectField::Port => FieldId::QcPort,
                                QuickConnectField::Username => FieldId::QcUsername,
                                QuickConnectField::Password => FieldId::QcPassword,
                                QuickConnectField::Path => FieldId::QcPath,
                                QuickConnectField::PrivateKey | QuickConnectField::Protocol => {
                                    unreachable!()
                                }
                            };
                            map.fields.push(FieldRegion {
                                id: fid,
                                area: cell,
                                text_x: cell.x + 1,
                            });
                        }
                    }
                }
            }
        }

        let footer = Paragraph::new(vec![
            Line::from("Tab/Shift+Tab move field | ←/→ protocol | Enter connect"),
            Line::from("Ctrl+S save bookmark | Ctrl+P browse key | Esc close"),
        ])
        .style(Style::default().fg(t.modal_text).bg(t.modal_background));
        frame.render_widget(footer, chunks[2]);
    }

    if app.show_key_picker {
        let area = centered_rect(70, 65, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let outer = Block::default()
            .title(Line::from(vec![Span::styled(
                " SSH Key ",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_active));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(inner);

        let cwd = Paragraph::new(shorten_middle(&app.key_picker_cwd, 70))
            .style(Style::default().fg(t.modal_labels).bg(t.modal_background));
        frame.render_widget(cwd, chunks[0]);

        let visible = app.visible_key_picker();
        let visible_len = visible.len();
        let items: Vec<ListItem> = visible
            .into_iter()
            .map(|e| {
                let mut color = match e.kind {
                    dd_ftp_core::EntryKind::Directory => t.folder,
                    dd_ftp_core::EntryKind::Symlink => t.link,
                    _ => t.file,
                };
                if is_public_key_name(&e.name) {
                    color = t.text_secondary;
                }
                let suffix = if e.is_dir() { "/" } else { "" };
                ListItem::new(Line::from(Span::styled(
                    format!("{}{suffix}", e.name),
                    Style::default().fg(color),
                )))
            })
            .collect();

        let list = List::new(items)
            .style(Style::default().bg(t.modal_background).fg(t.modal_text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.border_active)),
            )
            .highlight_symbol("▶ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(
                Style::default()
                    .bg(t.selected_background)
                    .fg(t.text_active_focus)
                    .add_modifier(Modifier::BOLD),
            );

        let mut list_state = ListState::default();
        list_state.select(Some(app.key_picker_selected));
        frame.render_stateful_widget(list, chunks[1], &mut list_state);
        map.key_picker_list = chunks[1];
        map.key_picker_list_offset = list_state.offset();
        map.key_picker_scrollbar = Rect {
            x: chunks[1].x + chunks[1].width.saturating_sub(1),
            y: chunks[1].y,
            width: 1,
            height: chunks[1].height,
        };
        render_scrollbar(
            frame,
            chunks[1],
            app.key_picker_selected,
            visible_len,
            t.scrollbar,
            t.scrollbar_hover,
            app.mouse_pos,
        );

        let footer = Paragraph::new(vec![
            Line::from("j/k move | Enter open/select | l enter dir | h parent"),
            Line::from("Esc cancel"),
        ])
        .style(Style::default().fg(t.modal_text).bg(t.modal_background));
        frame.render_widget(footer, chunks[2]);
    }

    if app.show_bookmarks {
        let area = centered_rect(70, 65, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let mut lines = vec![
            Line::from(vec![Span::styled(
                "Bookmarks",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
        ];

        if app.bookmarks.is_empty() {
            lines.push(Line::from("No bookmarks saved."));
        } else {
            for (i, b) in app.bookmarks.iter().enumerate() {
                let prefix = if i == app.selected_bookmark { ">" } else { " " };
                let default_mark = if i == 0 { "*" } else { " " };
                let display_name = if b.name.trim().is_empty() {
                    b.host.clone()
                } else {
                    b.name.clone()
                };
                lines.push(Line::from(format!(
                    "{}{} {} [{}:{}] ({:?})",
                    prefix, default_mark, display_name, b.host, b.port, b.protocol
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(
            "j/k move | Enter load into quick connect | c connect",
        ));
        lines.push(Line::from(
            "e edit | d delete (confirm) | D set default | Esc close",
        ));

        let modal = Paragraph::new(lines.clone())
            .style(Style::default().bg(t.modal_background).fg(t.modal_text))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Bookmarks ",
                        Style::default()
                            .fg(t.modal_labels)
                            .add_modifier(Modifier::BOLD),
                    )]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.border_active)),
            );

        frame.render_widget(modal, area);
        render_scrollbar(
            frame,
            area,
            app.selected_bookmark,
            lines.len(),
            t.scrollbar,
            t.scrollbar_hover,
            app.mouse_pos,
        );

        // Record one ControlRegion per bookmark row. Each bookmark occupies one
        // line (line0 = "Bookmarks", line1 = blank, then bookmarks starting at
        // line2 with border offset +1 = y + 3 + i). Wrap{trim:true} could shift
        // rows for very long names; this is the accepted single-line approximation.
        if !app.bookmarks.is_empty() {
            for i in 0..app.bookmarks.len() {
                let y = area.y.saturating_add(3 + i as u16);
                if y < area.y + area.height.saturating_sub(1) {
                    map.controls.push(ControlRegion {
                        id: ControlId::BookmarkRow(i),
                        area: Rect {
                            x: area.x + 1,
                            y,
                            width: area.width.saturating_sub(2),
                            height: 1,
                        },
                    });
                }
            }
        }
    }

    if app.show_prompt {
        match app.prompt_kind {
            Some(PromptKind::Choice(kind)) => {
                let area = centered_rect(70, 32, frame.area());
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Block::default().style(Style::default().bg(t.modal_background)),
                    area,
                );

                let target = app.prompt_target.as_deref().unwrap_or("");
                let (title, lines) = match kind {
                    ChoicePromptKind::ConfirmQuit => (
                        " Quit ",
                        vec![
                            Line::from(vec![Span::styled(
                                "Quit anyway? Active transfers will be interrupted (y/n)",
                                Style::default().fg(t.modal_labels),
                            )]),
                            Line::from(""),
                            Line::from(vec![Span::styled(
                                "y confirm | n / Esc cancel",
                                Style::default().fg(t.warning),
                            )]),
                        ],
                    ),
                    ChoicePromptKind::ConfirmDelete => (
                        " Delete ",
                        vec![
                            Line::from(vec![Span::styled(
                                format!("Delete '{target}'? (y/n)"),
                                Style::default().fg(t.modal_labels),
                            )]),
                            Line::from(""),
                            Line::from(vec![Span::styled(
                                "y confirm | n / Esc cancel",
                                Style::default().fg(t.warning),
                            )]),
                        ],
                    ),
                    ChoicePromptKind::ConfirmBookmarkDelete => (
                        " Delete bookmark ",
                        vec![
                            Line::from(vec![Span::styled(
                                format!("Delete bookmark '{target}'? (y/n)"),
                                Style::default().fg(t.modal_labels),
                            )]),
                            Line::from(""),
                            Line::from(vec![Span::styled(
                                "y confirm | n / Esc cancel",
                                Style::default().fg(t.warning),
                            )]),
                        ],
                    ),
                    ChoicePromptKind::Overwrite => {
                        let ow = app.overwrite.as_ref();
                        let local = ow.map(|o| o.current.local_path.as_str()).unwrap_or("");
                        let remote = ow.map(|o| o.current.remote_path.as_str()).unwrap_or("");
                        let dest_newer = ow.is_some_and(|o| o.current.dest_is_newer());
                        let remaining = ow.map(|o| o.remaining.len()).unwrap_or(0);
                        let header = if dest_newer {
                            "Destination exists and is newer"
                        } else {
                            "Destination exists"
                        };
                        (
                            " Overwrite ",
                            vec![
                                Line::from(vec![Span::styled(
                                    header,
                                    Style::default().fg(if dest_newer {
                                        t.warning
                                    } else {
                                        t.modal_labels
                                    }),
                                )]),
                                Line::from(vec![Span::styled(
                                    format!("local: {local}"),
                                    Style::default().fg(t.modal_text),
                                )]),
                                Line::from(vec![Span::styled(
                                    format!("remote: {remote}"),
                                    Style::default().fg(t.modal_text),
                                )]),
                                Line::from(vec![Span::styled(
                                    format!("{remaining} more in queue"),
                                    Style::default().fg(t.text_secondary),
                                )]),
                                Line::from(""),
                                Line::from(vec![Span::styled(
                                    "s skip  o overwrite  r rename this",
                                    Style::default().fg(t.warning),
                                )]),
                                Line::from(vec![Span::styled(
                                    "a overwrite-all  n skip-all  A overwrite-newer  N skip-newer  t rename-newer  Esc abort",
                                    Style::default().fg(t.warning),
                                )]),
                            ],
                        )
                    }
                    ChoicePromptKind::ConfirmEditLarge => (
                        " Edit large file ",
                        vec![
                            Line::from(vec![Span::styled(
                                format!("File is {target}. Download and edit anyway? (y/n)"),
                                Style::default().fg(t.modal_labels),
                            )]),
                            Line::from(""),
                            Line::from(vec![Span::styled(
                                "y confirm | n / Esc cancel",
                                Style::default().fg(t.warning),
                            )]),
                        ],
                    ),
                    ChoicePromptKind::ConfirmEditBinary => (
                        " Binary file ",
                        vec![
                            Line::from(vec![Span::styled(
                                "This file looks binary. Open it in $EDITOR anyway? (y/n)",
                                Style::default().fg(t.modal_labels),
                            )]),
                            Line::from(""),
                            Line::from(vec![Span::styled(
                                "y confirm | n / Esc cancel (keeps the downloaded copy)",
                                Style::default().fg(t.warning),
                            )]),
                        ],
                    ),
                    ChoicePromptKind::EditConflict => (
                        " Remote changed ",
                        vec![
                            Line::from(vec![Span::styled(
                                "The remote file changed while you were editing.",
                                Style::default().fg(t.warning),
                            )]),
                            Line::from(""),
                            Line::from(vec![Span::styled(
                                "o overwrite  s skip  r save as  Esc cancel",
                                Style::default().fg(t.warning),
                            )]),
                        ],
                    ),
                    ChoicePromptKind::HostKey => {
                        let hk = app.host_key.as_ref();
                        let host = hk.map(|h| h.host.as_str()).unwrap_or("");
                        let port = hk.map(|h| h.port).unwrap_or(0);
                        let fp = hk.map(|h| h.fingerprint.as_str()).unwrap_or("");
                        let changed = hk.is_some_and(|h| h.changed);
                        let header = if changed {
                            format!("WARNING: HOST KEY CHANGED for {host}:{port}")
                        } else {
                            format!("Host key unknown for {host}:{port}")
                        };
                        let ask = if changed {
                            "This may be a MITM. Trust the new key and update known_hosts? (y/n)"
                        } else {
                            "Trust and save to ~/.ssh/known_hosts? (y/n)"
                        };
                        let header_style = if changed {
                            Style::default().fg(t.error)
                        } else {
                            Style::default().fg(t.modal_labels)
                        };
                        (
                            " Host key ",
                            vec![
                                Line::from(vec![Span::styled(header, header_style)]),
                                Line::from(vec![Span::styled(
                                    format!("SHA256:{fp}"),
                                    Style::default().fg(t.modal_text),
                                )]),
                                Line::from(vec![Span::styled(
                                    ask,
                                    Style::default().fg(t.modal_labels),
                                )]),
                                Line::from(""),
                                Line::from(vec![Span::styled(
                                    "y accept | n / Esc reject",
                                    Style::default().fg(t.warning),
                                )]),
                            ],
                        )
                    }
                };

                let modal = Paragraph::new(lines)
                    .style(Style::default().bg(t.modal_background).fg(t.modal_text))
                    .wrap(Wrap { trim: true })
                    .block(
                        Block::default()
                            .title(Line::from(vec![Span::styled(
                                title,
                                Style::default()
                                    .fg(t.modal_labels)
                                    .add_modifier(Modifier::BOLD),
                            )]))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(t.border_active)),
                    );

                frame.render_widget(modal, area);
            }
            Some(PromptKind::Text(kind)) => {
                let area = centered_rect(60, 30, frame.area());
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Block::default().style(Style::default().bg(t.modal_background)),
                    area,
                );

                let (title, message) = match kind {
                    TextPromptKind::CreateFile => (" Create File ", "Enter file name:"),
                    TextPromptKind::CreateFolder => (" Create Folder ", "Enter folder name:"),
                    TextPromptKind::Rename => (" Rename ", "Enter new name:"),
                    TextPromptKind::Chmod => (" Chmod ", "Enter octal mode (e.g. 755):"),
                    TextPromptKind::OverwriteRename => (" Rename destination ", "Enter new name:"),
                    TextPromptKind::EditSaveAs => (" Save edit as ", "Enter remote file name:"),
                };

                let mut lines = vec![Line::from(vec![Span::styled(
                    message,
                    Style::default().fg(t.modal_labels),
                )])];

                lines.push(Line::from(""));
                let mut prompt_spans =
                    vec![Span::styled("> ", Style::default().fg(t.input_text_focus))];
                prompt_spans.extend(render_field_line(&app.prompt_value, false, &t));
                lines.push(Line::from(prompt_spans));
                lines.push(Line::from(""));
                match kind {
                    TextPromptKind::CreateFile => {
                        lines.push(Line::from(vec![Span::styled(
                            "Tab: switch to Folder",
                            Style::default().fg(t.text_secondary),
                        )]));
                    }
                    TextPromptKind::CreateFolder => {
                        lines.push(Line::from(vec![Span::styled(
                            "Tab: switch to File",
                            Style::default().fg(t.text_secondary),
                        )]));
                    }
                    _ => {}
                }
                lines.push(Line::from(vec![Span::styled(
                    "Enter to confirm | Esc to cancel",
                    Style::default().fg(t.warning),
                )]));

                let modal = Paragraph::new(lines)
                    .style(Style::default().bg(t.modal_background).fg(t.modal_text))
                    .block(
                        Block::default()
                            .title(Line::from(vec![Span::styled(
                                title,
                                Style::default()
                                    .fg(t.modal_labels)
                                    .add_modifier(Modifier::BOLD),
                            )]))
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(t.border_active)),
                    );

                frame.render_widget(modal, area);

                let input_line_offset = 1 /* message */ + 1 /* blank */;
                let prompt_input_area = Rect {
                    x: area.x + 1,
                    y: area.y + 1 + input_line_offset as u16,
                    width: area.width.saturating_sub(2),
                    height: 1,
                };
                // hitbox starts at the modal inner edge (area.x+1); text_x adds the 2-col "> " prefix
                map.fields.push(FieldRegion {
                    id: FieldId::Prompt,
                    area: prompt_input_area,
                    text_x: area.x + 3,
                });
            }
            None => {}
        }
    }

    if let Some(toast) = &app.toast {
        render_toast(frame, frame.area(), toast, &t);
    }
}

fn render_toast(frame: &mut Frame, area: Rect, toast: &dd_ftp_app::Toast, t: &Theme) {
    let kind = match toast.level {
        ToastLevel::Info => crate::toast_chrome::ToastKind::Info,
        ToastLevel::Success => crate::toast_chrome::ToastKind::Success,
        ToastLevel::Warning => crate::toast_chrome::ToastKind::Warning,
        ToastLevel::Error => crate::toast_chrome::ToastKind::Error,
    };
    let _ = crate::toast_chrome::render_toast_stack(
        frame,
        area,
        &[crate::toast_chrome::ToastLine {
            kind,
            message: toast.message.as_str(),
        }],
        crate::toast_chrome::ToastPaint {
            modal_background: t.modal_background,
            text: t.text_primary,
            success: t.success,
            info: t.info,
            warning: t.warning,
            error: t.error,
        },
        0,
    );
}

fn format_speed(bps: f64) -> String {
    const K: f64 = 1024.0;
    if bps >= K * K * K {
        format!("{:.1} GB/s", bps / (K * K * K))
    } else if bps >= K * K {
        format!("{:.1} MB/s", bps / (K * K))
    } else if bps >= K {
        format!("{:.1} KB/s", bps / K)
    } else {
        format!("{:.0} B/s", bps)
    }
}

/// Height of the bordered queue panel.
///
/// Zero visible jobs (pending, active, or failed) stay at 3 rows: the title
/// border, the worker summary, and the bottom border. Each visible job adds a
/// row, up to 8, or 12 when the queue pane is focused.
fn queue_panel_height(job_count: usize, focused: bool) -> u16 {
    if job_count == 0 {
        return 3;
    }
    let cap = if focused { 12 } else { 8 };
    (3 + job_count).min(cap) as u16
}

fn format_eta(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

fn format_job_summary(job: &TransferJob, queue: &TransferQueue) -> String {
    let direction = match job.direction {
        dd_ftp_core::TransferDirection::Upload => "up",
        dd_ftp_core::TransferDirection::Download => "down",
    };

    let progress = format_progress(job);

    let local = shorten_middle(&job.local_path, 22);
    let remote = shorten_middle(&job.remote_path, 22);

    let speed_eta = match queue.speed_and_eta(job.id) {
        Some((speed, Some(eta))) if speed > 0.0 && job.size_bytes.is_some() => {
            format!("{} ETA {}", format_speed(speed), format_eta(eta))
        }
        _ => "ETA --:--".to_string(),
    };

    format!(
        "{} {} -> {} [{}] {} r{}",
        direction, local, remote, progress, speed_eta, job.retries
    )
}

fn format_progress(job: &TransferJob) -> String {
    if let Some(size) = job.size_bytes {
        if size > 0 {
            let pct = ((job.transferred_bytes as f64 / size as f64) * 100.0).clamp(0.0, 100.0);
            let bars_total = 8usize;
            let filled = ((pct / 100.0) * bars_total as f64).round() as usize;
            let empty = bars_total.saturating_sub(filled);
            format!("{}{} {:>3.0}%", "#".repeat(filled), "-".repeat(empty), pct)
        } else {
            "-------- 0%".to_string()
        }
    } else {
        format!("{}B", job.transferred_bytes)
    }
}

fn inner_list_rect(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

#[derive(Debug, Clone, Copy)]
struct MetaColumns {
    size: bool,
    date: bool,
    perms: bool,
}

const SIZE_COL: usize = 5;
const DATE_COL: usize = 16;
const PERMS_COL: usize = 4;
const META_GAP: usize = 2;

fn meta_columns(max_width: usize) -> MetaColumns {
    let size_w = META_GAP + SIZE_COL;
    let date_w = META_GAP + DATE_COL;
    let perms_w = META_GAP + PERMS_COL;
    if max_width >= size_w + date_w + perms_w {
        MetaColumns {
            size: true,
            date: true,
            perms: true,
        }
    } else if max_width >= size_w + date_w {
        MetaColumns {
            size: true,
            date: true,
            perms: false,
        }
    } else if max_width >= size_w {
        MetaColumns {
            size: true,
            date: false,
            perms: false,
        }
    } else {
        MetaColumns {
            size: false,
            date: false,
            perms: false,
        }
    }
}

fn file_list_item(
    entry: &dd_ftp_core::FileEntry,
    width: usize,
    cols: MetaColumns,
    marked: bool,
    badge: Option<CompareBadge>,
    t: &Theme,
) -> ListItem<'static> {
    let color = match entry.kind {
        dd_ftp_core::EntryKind::Directory => t.folder,
        dd_ftp_core::EntryKind::Symlink => t.link,
        _ => t.file,
    };
    let prefix = if entry.kind == dd_ftp_core::EntryKind::Directory {
        "> "
    } else {
        "  "
    };
    let badge_text = match badge {
        Some(b) if !is_dot_or_dotdot(&entry.name) => format!("{} ", b.label()),
        _ => String::new(),
    };
    let mark = if marked { "*" } else { "" };
    let meta = entry_meta(entry, cols);
    let meta_len = meta.chars().count();
    let lead_len = badge_text.chars().count() + mark.chars().count() + prefix.chars().count();
    let name_max = width.saturating_sub(lead_len + meta_len).max(1);
    let name = shorten_middle(&entry.name, name_max);
    let pad = width.saturating_sub(lead_len + name.chars().count() + meta_len);
    let padding = " ".repeat(pad);

    let badge_style = match badge {
        Some(CompareBadge::LocalOnly) => Style::default().fg(t.warning),
        Some(CompareBadge::RemoteOnly) => Style::default().fg(t.info),
        Some(CompareBadge::Equal) => Style::default().fg(t.success),
        Some(CompareBadge::Differ) => Style::default().fg(t.error),
        None => Style::default(),
    };

    ListItem::new(Line::from(vec![
        Span::styled(badge_text, badge_style),
        Span::styled(mark.to_string(), Style::default().fg(t.text_active_focus)),
        Span::styled(format!("{prefix}{name}"), Style::default().fg(color)),
        Span::styled(
            format!("{padding}{meta}"),
            Style::default().fg(t.text_secondary),
        ),
    ]))
}

fn entry_meta(entry: &dd_ftp_core::FileEntry, cols: MetaColumns) -> String {
    let mut meta = String::new();
    if cols.size {
        meta.push_str(&format!(
            "  {:>width$}",
            format_size(entry.size),
            width = SIZE_COL
        ));
    }
    if cols.date {
        let date = entry
            .modified
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        meta.push_str(&format!("  {date:>width$}", width = DATE_COL));
    }
    if cols.perms {
        let perms = entry.permissions.as_deref().unwrap_or("");
        meta.push_str(&format!("  {perms:<width$}", width = PERMS_COL));
    }
    meta
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return bytes.to_string();
    }
    let (scaled, label) = if bytes >= 1024u64.pow(4) {
        (bytes as f64 / 1024f64.powi(4), "T")
    } else if bytes >= 1024u64.pow(3) {
        (bytes as f64 / 1024f64.powi(3), "G")
    } else if bytes >= 1024u64.pow(2) {
        (bytes as f64 / 1024f64.powi(2), "M")
    } else {
        (bytes as f64 / 1024.0, "K")
    };
    if scaled >= 10.0 {
        format!("{scaled:.0}{label}")
    } else {
        format!("{scaled:.1}{label}")
    }
}

fn render_path_bar(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    segs: &[PathSeg],
    t: &Theme,
    controls: &mut Vec<ControlRegion>,
    make_id: fn(usize) -> ControlId,
) -> Vec<String> {
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let mut spans = Vec::new();
    let mut targets = Vec::new();
    let mut x = inner.x;
    let max_x = inner.x.saturating_add(inner.width);
    for (i, seg) in segs.iter().enumerate() {
        let show_slash = i > 0 && segs[i - 1].label != "/" && !seg.label.starts_with('/');
        if show_slash {
            spans.push(Span::styled("/", Style::default().fg(t.text_secondary)));
            x = x.saturating_add(1);
        }
        let w = seg.label.chars().count() as u16;
        if w > 0 && x < max_x {
            let vis_w = w.min(max_x.saturating_sub(x));
            controls.push(ControlRegion {
                id: make_id(targets.len()),
                area: Rect {
                    x,
                    y: inner.y,
                    width: vis_w,
                    height: 1,
                },
            });
            targets.push(seg.target.clone());
        }
        spans.push(Span::styled(
            seg.label.clone(),
            Style::default()
                .fg(t.text_primary)
                .add_modifier(Modifier::UNDERLINED),
        ));
        x = x.saturating_add(w);
    }
    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    title.to_string(),
                    Style::default().fg(t.text_labels),
                )]))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_default)),
        );
    frame.render_widget(bar, area);
    targets
}

fn shorten_middle(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }

    if max_chars < 8 {
        return "...".to_string();
    }

    let keep = (max_chars - 3) / 2;
    let start: String = input.chars().take(keep).collect();
    let end: String = input
        .chars()
        .rev()
        .take(keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    format!("{}...{}", start, end)
}

fn render_scrollbar(
    frame: &mut Frame,
    area: Rect,
    position: usize,
    content_len: usize,
    color: ratatui::style::Color,
    hover_color: ratatui::style::Color,
    mouse_pos: Option<(u16, u16)>,
) {
    let viewport_rows = area.height.saturating_sub(2) as usize;
    let content_lines = content_len.max(1);

    if content_lines <= viewport_rows {
        return;
    }

    let max_scroll = content_lines.saturating_sub(viewport_rows);
    let scroll_pos = position.min(max_scroll);

    // Check if mouse is over scrollbar area (rightmost column of the area)
    let is_hovered = mouse_pos.is_some_and(|(x, y)| {
        x == area.x + area.width.saturating_sub(1) && y >= area.y && y < area.y + area.height
    });

    let effective_color = if is_hovered { hover_color } else { color };

    let mut state = ScrollbarState::new(content_lines).position(scroll_pos);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::default().fg(effective_color))
        .track_style(Style::default().fg(effective_color))
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn footer_hint(app: &AppState, width: u16) -> String {
    let blocking = app.show_settings
        || app.show_prompt
        || app.show_quick_connect
        || app.show_bookmarks
        || app.show_key_picker;
    if blocking {
        return if width < 80 {
            "F1:Settings  Esc:Close  ^Q:Quit".to_string()
        } else {
            "F1: Settings  Esc: Close  Ctrl+Q: Quit".to_string()
        };
    }
    let (f1, quit) = if width < 80 {
        ("F1:Settings", "^Q:Quit")
    } else {
        ("F1: Settings", "Ctrl+Q: Quit")
    };
    let middle: &[&str] = if width < 80 {
        &[
            "Tab:Pane",
            "j/k:Nav",
            "/:Filter",
            "u/d:Xfer",
            "m:Sites",
            "F3:Session",
        ]
    } else {
        &[
            "F3:Session",
            "Tab: Pane",
            "j/k: Nav",
            "h/l: Dir",
            "/: Filter",
            "u/d: Up/Down",
            "m: Bookmarks",
            "o: Connect",
        ]
    };
    fit_footer(width, &[f1], middle, &[quit])
}

/// One footer row. Drops middle tokens from the right so F1 and quit stay.
fn fit_footer(width: u16, left: &[&str], middle: &[&str], right: &[&str]) -> String {
    let limit = width as usize;
    let mut mid: Vec<&str> = middle.to_vec();
    loop {
        let mut parts = Vec::with_capacity(left.len() + mid.len() + right.len());
        parts.extend_from_slice(left);
        parts.extend(mid.iter().copied());
        parts.extend_from_slice(right);
        let line = parts.join("  ");
        if line.chars().count() <= limit || mid.is_empty() {
            return line;
        }
        mid.pop();
    }
}

const SETTINGS_NAV: [&str; 3] = ["Themes", "Session", "Help"];

struct SessionMarks {
    editor: usize,
    workers: usize,
    hide: usize,
    overwrite: usize,
    auto: usize,
    last_local: usize,
    selected: usize,
}

fn render_settings(frame: &mut Frame, app: &mut AppState, map: &mut LayoutMap, t: &Theme) {
    let frame_area = frame.area();
    if frame_area.height < 4 || frame_area.width == 0 {
        return;
    }
    let width = (frame_area.width / 2).max(1).min(frame_area.width);
    let height = frame_area.height - 4;
    if height == 0 {
        return;
    }
    let area = Rect {
        x: frame_area.x + frame_area.width - width,
        y: frame_area.y + 3,
        width,
        height,
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(Line::from(Span::styled(
            " Settings ",
            Style::default()
                .fg(t.modal_header)
                .add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_active))
        .style(Style::default().bg(t.modal_background).fg(t.modal_text));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    map.help = Some(area);
    map.help_scrollbar = Some(Rect {
        x: area.x + area.width.saturating_sub(1),
        y: area.y,
        width: 1,
        height: area.height,
    });
    if app.settings_page > SETTINGS_LAST {
        app.settings_page = SETTINGS_THEMES;
    }
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let nav_w = {
        let max_nav = inner.width.saturating_sub(1).max(1).min(inner.width);
        let preferred = if area.width < 22 {
            let spare = inner.width.saturating_sub(8);
            if spare == 0 {
                1
            } else {
                14.min(spare).max(1)
            }
        } else {
            14
        };
        preferred.min(max_nav).max(1).min(inner.width)
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(nav_w), Constraint::Min(1)])
        .split(inner);
    let nav = cols[0];
    let page = cols[1];
    paint_settings_nav(frame, app, nav, t, map);

    let section = SETTINGS_NAV
        .get(app.settings_page as usize)
        .copied()
        .unwrap_or("Themes");
    let title_h = u16::from(page.height > 0);
    let title_area = Rect {
        x: page.x,
        y: page.y,
        width: page.width,
        height: title_h,
    };
    let content = Rect {
        x: page.x,
        y: page.y.saturating_add(title_h),
        width: page.width,
        height: page.height.saturating_sub(title_h),
    };
    if title_h > 0 {
        let shown: String = section.chars().take(page.width as usize).collect();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                shown,
                Style::default()
                    .fg(t.modal_header)
                    .add_modifier(Modifier::BOLD),
            )))
            .style(Style::default().bg(t.modal_background).fg(t.modal_header)),
            title_area,
        );
    }

    let page_kind = app.settings_page;
    let (lines, selected, marks) = if page_kind == SETTINGS_HELP {
        (help_lines(t, content.width as usize), None, None)
    } else if page_kind == SETTINGS_SESSION {
        let (lines, marks) = session_lines(app, t);
        let selected = marks.selected;
        (lines, Some(selected), Some(marks))
    } else {
        let (lines, selected) = theme_lines(app, t, content.width as usize);
        (lines, selected, None)
    };
    let line_count = lines.len();
    map.help_line_count = line_count;
    paint_settings_lines(frame, app, content, lines, selected, t);
    if let Some(marks) = marks {
        record_session_hits(map, content, app.help_scroll, &marks);
    }
    render_scrollbar(
        frame,
        area,
        app.help_scroll,
        line_count,
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );
}

fn paint_settings_nav(
    frame: &mut Frame,
    app: &AppState,
    nav: Rect,
    t: &Theme,
    map: &mut LayoutMap,
) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(t.border_default))
        .style(Style::default().bg(t.body_background).fg(t.modal_text));
    let inner = block.inner(nav);
    frame.render_widget(block, nav);
    map.settings_nav = Some(inner);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let selected_style = Style::default()
        .fg(t.text_active_focus)
        .bg(t.selected_background);
    let idle = Style::default().fg(t.modal_text).bg(t.body_background);
    let width = inner.width as usize;
    let mut lines = Vec::new();
    for (index, label) in SETTINGS_NAV.iter().enumerate() {
        let on = app.settings_page == index as u8;
        let marker = if on { ">" } else { " " };
        let mut text = format!("{marker} {label}");
        let count = text.chars().count();
        if count < width {
            text.push_str(&" ".repeat(width - count));
        } else {
            text = text.chars().take(width).collect();
        }
        let style = if on { selected_style } else { idle };
        lines.push(Line::from(Span::styled(text, style)));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.body_background).fg(t.modal_text)),
        inner,
    );
}

fn settings_top(title: &str, width: usize, edge: Style, title_style: Style) -> Line<'static> {
    let width = width.max(2);
    let inner = width - 2;
    let mut label = format!(" {title} ");
    if label.chars().count() > inner {
        label = label.chars().take(inner).collect();
    }
    let pad = inner - label.chars().count();
    Line::from(vec![
        Span::styled("┌", edge),
        Span::styled(label, title_style),
        Span::styled(format!("{}┐", "─".repeat(pad)), edge),
    ])
}

fn settings_bottom(width: usize, edge: Style) -> Line<'static> {
    let width = width.max(2);
    Line::from(Span::styled(format!("└{}┘", "─".repeat(width - 2)), edge))
}

fn settings_row(body: &str, width: usize, edge: Style, fill: Style) -> Line<'static> {
    let width = width.max(2);
    let inner = width - 2;
    let mut text: String = body.chars().take(inner).collect();
    let pad = inner - text.chars().count();
    text.push_str(&" ".repeat(pad));
    Line::from(vec![
        Span::styled("│", edge),
        Span::styled(text, fill),
        Span::styled("│", edge),
    ])
}

fn paint_settings_lines(
    frame: &mut Frame,
    app: &mut AppState,
    area: Rect,
    lines: Vec<Line<'static>>,
    selected_line: Option<usize>,
    t: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        app.help_scroll_max = 0;
        app.help_scroll = 0;
        return;
    }
    let view_h = area.height as usize;
    let max_scroll = lines.len().saturating_sub(view_h);
    app.help_scroll_max = max_scroll;
    if app.settings_follow {
        if let Some(sel) = selected_line {
            let top = app.help_scroll;
            if sel < top {
                app.help_scroll = sel;
            } else if sel >= top + view_h {
                app.help_scroll = sel + 1 - view_h;
            }
        }
        app.settings_follow = false;
    }
    if app.help_scroll > max_scroll {
        app.help_scroll = max_scroll;
    }
    let scroll = u16::try_from(app.help_scroll).unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(t.modal_text).bg(t.modal_background))
            .scroll((scroll, 0)),
        area,
    );
}

fn save_target_word(target: ldnddev_theme::ThemeSaveTarget) -> &'static str {
    match target {
        ldnddev_theme::ThemeSaveTarget::Local => "Local",
        ldnddev_theme::ThemeSaveTarget::Global => "Global",
    }
}

fn theme_lines(app: &AppState, t: &Theme, width: usize) -> (Vec<Line<'static>>, Option<usize>) {
    use ldnddev_theme::{theme_editor_rows, PickerRow, ThemeEditorRow};
    let editor = app.theme_editor.clone();
    let picker = app.theme_picker.clone();
    let box_w = width.max(2);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut selected_line = None;
    let rule = Style::default().fg(t.border_default);
    let header = Style::default()
        .fg(t.modal_header)
        .add_modifier(Modifier::BOLD);
    let hint = if let Some(editor) = &editor {
        let word = save_target_word(editor.save_target);
        let esc = if picker.is_some() {
            "Esc back"
        } else {
            "Esc revert"
        };
        format!("j/k move   t {word}   y/s save   R reset   {esc}")
    } else if let Some(picker) = &picker {
        let word = save_target_word(picker.save_target);
        format!("j/k move   Enter apply   e colors   t {word}   y/s save")
    } else {
        "j/k move   Enter apply   e colors   t Global/Local   y/s save".to_string()
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(t.text_secondary),
    )));

    if let Some(editor) = &editor {
        let channel = ["R", "G", "B"][editor.channel.min(2)];
        if editor.editing_hex {
            lines.push(Line::from(format!(
                "Hex: {}█   Channel: {channel}",
                editor.hex_draft
            )));
        } else {
            lines.push(Line::from(format!(
                "Hex: {}   Channel: {channel}   Enter edit",
                editor.hex_draft
            )));
        }
        let selected_style = Style::default()
            .fg(t.text_active_focus)
            .bg(t.selected_background);
        let idle = Style::default().fg(t.modal_text).bg(t.modal_background);
        let mut open = false;
        for row in theme_editor_rows(&editor.fields) {
            match row {
                ThemeEditorRow::Header(name) => {
                    if open {
                        lines.push(settings_bottom(box_w, rule));
                    }
                    lines.push(Line::from(""));
                    lines.push(settings_top(name, box_w, rule, header));
                    open = true;
                }
                ThemeEditorRow::Color(idx) => {
                    if idx == editor.selected {
                        selected_line = Some(lines.len());
                    }
                    let field = editor.fields[idx];
                    let hex = editor
                        .palette
                        .get(field.key)
                        .map(|color| color.to_hex())
                        .unwrap_or_else(|| "#000000".into());
                    let cursor = if idx == editor.selected { ">" } else { " " };
                    let style = if idx == editor.selected {
                        selected_style
                    } else {
                        idle
                    };
                    lines.push(settings_row(
                        &format!("{cursor} {:<22} {hex}", field.key),
                        box_w,
                        rule,
                        style,
                    ));
                }
            }
        }
        if open {
            lines.push(settings_bottom(box_w, rule));
        }
    } else if let Some(picker) = &picker {
        lines.push(Line::from(""));
        lines.push(settings_top("Themes", box_w, rule, header));
        let selected_style = Style::default()
            .fg(t.text_active_focus)
            .bg(t.selected_background);
        let idle = Style::default().fg(t.modal_text).bg(t.modal_background);
        for (idx, row) in picker.rows.iter().enumerate() {
            if idx == picker.selected {
                selected_line = Some(lines.len());
            }
            let (label, detail) = match row {
                PickerRow::Follow { name } => ("Follow desktop".to_string(), name.clone()),
                PickerRow::Theme(theme) => (theme.name.clone(), theme.mode.label().to_string()),
                PickerRow::Custom => ("Custom".to_string(), "built-in".to_string()),
            };
            let cursor = if idx == picker.selected { ">" } else { " " };
            let style = if idx == picker.selected {
                selected_style
            } else {
                idle
            };
            lines.push(settings_row(
                &format!("{cursor} {label:<22} {detail}"),
                box_w,
                rule,
                style,
            ));
        }
        lines.push(settings_bottom(box_w, rule));
    }
    (lines, selected_line)
}

fn session_lines(app: &AppState, t: &Theme) -> (Vec<Line<'static>>, SessionMarks) {
    let env_override = std::env::var("VISUAL")
        .ok()
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|s| !s.trim().is_empty());
    let active = env_override.as_deref().unwrap_or_else(|| {
        if app.editor.trim().is_empty() {
            "vi"
        } else {
            app.editor.trim()
        }
    });
    let row = |field: SettingsField| -> Style {
        if app.settings_field == field {
            Style::default()
                .fg(t.input_text_focus)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.modal_text)
        }
    };
    let cycle_line = |field: SettingsField, label: &str, value: &str| -> Line<'static> {
        let mark = if app.settings_field == field {
            ">"
        } else {
            " "
        };
        Line::from(Span::styled(
            format!("{mark} {label:<16} {value}"),
            row(field),
        ))
    };
    let mut editor_spans = vec![Span::styled(
        if app.settings_field == SettingsField::Editor {
            "> "
        } else {
            "  "
        },
        row(SettingsField::Editor),
    )];
    editor_spans.extend(render_field_line(&app.settings_editor, false, t));
    let mut last_spans = vec![Span::styled(
        if app.settings_field == SettingsField::LastLocalCwd {
            "> "
        } else {
            "  "
        },
        row(SettingsField::LastLocalCwd),
    )];
    last_spans.extend(render_field_line(&app.settings_last_local, false, t));
    let hide = if app.settings_hide_dotfiles {
        "on"
    } else {
        "off"
    };
    let auto = if app.settings_auto_connect {
        "on"
    } else {
        "off"
    };

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        "Editor  (empty = $VISUAL, then $EDITOR, then vi)",
        Style::default().fg(t.modal_labels),
    )));
    let editor = lines.len();
    lines.push(Line::from(editor_spans));
    lines.push(Line::from(Span::styled(
        format!("Active now: {active}"),
        Style::default().fg(t.text_secondary),
    )));
    if env_override.is_some() {
        lines.push(Line::from(Span::styled(
            "Environment variable overrides the saved editor",
            Style::default().fg(t.warning),
        )));
    } else {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(""));
    let workers = lines.len();
    lines.push(cycle_line(
        SettingsField::Workers,
        "Workers",
        &app.settings_workers.to_string(),
    ));
    let hide_line = lines.len();
    lines.push(cycle_line(
        SettingsField::HideDotfiles,
        "Hide dotfiles",
        hide,
    ));
    let overwrite = lines.len();
    lines.push(cycle_line(
        SettingsField::Overwrite,
        "Overwrite",
        app.settings_overwrite.settings_label(),
    ));
    let auto_line = lines.len();
    lines.push(cycle_line(SettingsField::AutoConnect, "Auto-connect", auto));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Last local path (restored at launch)",
        Style::default().fg(t.modal_labels),
    )));
    let last_local = lines.len();
    lines.push(Line::from(last_spans));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Saved to ~/.config/ldnddev/dd_ftp.toml (not the theme file)",
        Style::default().fg(t.text_secondary),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab cycle | Left/Right/Space change | Enter save | Esc cancel",
        Style::default().fg(t.warning),
    )));
    let selected = match app.settings_field {
        SettingsField::Editor => editor,
        SettingsField::Workers => workers,
        SettingsField::HideDotfiles => hide_line,
        SettingsField::Overwrite => overwrite,
        SettingsField::AutoConnect => auto_line,
        SettingsField::LastLocalCwd => last_local,
    };
    (
        lines,
        SessionMarks {
            editor,
            workers,
            hide: hide_line,
            overwrite,
            auto: auto_line,
            last_local,
            selected,
        },
    )
}

fn record_session_hits(map: &mut LayoutMap, content: Rect, scroll: usize, marks: &SessionMarks) {
    let push_field = |map: &mut LayoutMap, id: FieldId, line: usize| {
        let Some(area) = visible_line_rect(content, scroll, line) else {
            return;
        };
        map.fields.push(FieldRegion {
            id,
            area,
            text_x: content.x.saturating_add(2),
        });
    };
    let push_control = |map: &mut LayoutMap, id: ControlId, line: usize| {
        let Some(area) = visible_line_rect(content, scroll, line) else {
            return;
        };
        map.controls.push(ControlRegion { id, area });
    };
    push_field(map, FieldId::SettingsEditor, marks.editor);
    push_field(map, FieldId::SettingsLastLocal, marks.last_local);
    push_control(map, ControlId::SettingsWorkers, marks.workers);
    push_control(map, ControlId::SettingsHideDotfiles, marks.hide);
    push_control(map, ControlId::SettingsOverwrite, marks.overwrite);
    push_control(map, ControlId::SettingsAutoConnect, marks.auto);
}

fn visible_line_rect(content: Rect, scroll: usize, line: usize) -> Option<Rect> {
    if content.width == 0 || content.height == 0 || line < scroll {
        return None;
    }
    let rel = line - scroll;
    if rel >= content.height as usize {
        return None;
    }
    Some(Rect {
        x: content.x,
        y: content.y.saturating_add(rel as u16),
        width: content.width,
        height: 1,
    })
}

fn help_lines(t: &Theme, width: usize) -> Vec<Line<'static>> {
    let header = Style::default()
        .fg(t.modal_header)
        .add_modifier(Modifier::BOLD);
    let body = Style::default().fg(t.modal_text);
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled("Controls", header)));
    lines.push(Line::from(""));
    let mut last_group: Option<KeyGroup> = None;
    for kb in KEYMAP {
        if last_group != Some(kb.group) {
            if last_group.is_some() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(kb.group.title(), header)));
            last_group = Some(kb.group);
        }
        push_wrapped(
            &mut lines,
            &format!("  {}  {}", kb.keys, kb.action),
            width,
            body,
        );
    }
    lines.push(Line::from(""));
    push_wrapped(
        &mut lines,
        "Press F1 or Esc to close",
        width,
        Style::default().fg(t.warning),
    );
    lines.push(Line::from(Span::styled(
        "j/k scroll",
        Style::default().fg(t.text_secondary),
    )));
    lines
}

fn push_wrapped(lines: &mut Vec<Line<'static>>, text: &str, width: usize, style: Style) {
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        lines.push(Line::from(""));
        return;
    }
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + width).min(chars.len());
        if end < chars.len() {
            if let Some(rel) = chars[start..end].iter().rposition(|c| c.is_whitespace()) {
                if rel > 0 {
                    end = start + rel;
                }
            }
        }
        let slice: String = chars[start..end].iter().collect();
        lines.push(Line::from(Span::styled(
            slice.trim_end().to_string(),
            style,
        )));
        start = end;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod column_tests {
    use super::*;
    use dd_ftp_core::{EntryKind, FileEntry};

    fn entry(name: &str, size: u64, perms: Option<&str>) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: name.into(),
            kind: EntryKind::File,
            size,
            modified: None,
            permissions: perms.map(str::to_string),
        }
    }

    #[test]
    fn size_and_date_columns_align_with_and_without_perms() {
        let cols = MetaColumns {
            size: true,
            date: true,
            perms: true,
        };
        let with = entry_meta(&entry("a.bin", 1234, Some("644")), cols);
        let without = entry_meta(&entry(".", 0, None), cols);
        assert_eq!(with.chars().count(), without.chars().count());
        assert_eq!(&with[..7], "   1.2K");
        assert_eq!(&without[..7], "      0");
        assert_eq!(&with[7..25], &without[7..25]);
    }

    #[test]
    fn busy_label_pulses_through_the_four_frames() {
        assert_eq!(busy_label(0), "▃ d_d ▃");
        assert_eq!(busy_label(149), "▃ d_d ▃");
        assert_eq!(busy_label(150), "▅ d_d ▅");
        assert_eq!(busy_label(300), "█ d_d █");
        assert_eq!(busy_label(450), "▅ d_d ▅");
        assert_eq!(busy_label(600), "▃ d_d ▃");
    }

    #[test]
    fn meta_columns_drop_suffix_from_pane_width() {
        assert!(meta_columns(31).perms);
        assert!(meta_columns(25).date && !meta_columns(25).perms);
        assert!(meta_columns(7).size && !meta_columns(7).date);
        assert!(!meta_columns(6).size);
    }

    #[test]
    fn queue_panel_height_tracks_visible_jobs() {
        assert_eq!(queue_panel_height(0, false), 3);
        assert_eq!(queue_panel_height(0, true), 3);
        assert_eq!(queue_panel_height(1, false), 4);
        assert_eq!(queue_panel_height(5, false), 8);
        assert_eq!(queue_panel_height(5, true), 8);
        assert_eq!(queue_panel_height(9, true), 12);
        assert_eq!(queue_panel_height(40, false), 8);
        assert_eq!(queue_panel_height(40, true), 12);
    }

    #[test]
    fn idle_queue_strip_is_three_rows_and_omits_the_empty_message() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = AppState::default();
        app.focus = FocusPane::Queue;
        let mut map = LayoutMap::default();
        terminal
            .draw(|frame| render(frame, &mut app, &mut map))
            .unwrap();
        assert_eq!(map.queue.height, 3);
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("[3] Queue"));
        assert!(text.contains("Worker:"));
        assert!(!text.contains("No jobs in queue"));
    }

    #[test]
    fn queue_panel_grows_with_jobs_and_caps_when_focused() {
        use dd_ftp_core::TransferDirection;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut idle_focus = AppState::default();
        for n in 0..5 {
            idle_focus.queue.enqueue(TransferJob::new(
                format!("/tmp/{n}"),
                format!("/pub/{n}"),
                TransferDirection::Upload,
            ));
        }
        let mut map = LayoutMap::default();
        terminal
            .draw(|frame| render(frame, &mut idle_focus, &mut map))
            .unwrap();
        assert_eq!(map.queue.height, 8);

        idle_focus.focus = FocusPane::Queue;
        terminal
            .draw(|frame| render(frame, &mut idle_focus, &mut map))
            .unwrap();
        assert_eq!(map.queue.height, 8);

        for n in 5..12 {
            idle_focus.queue.enqueue(TransferJob::new(
                format!("/tmp/{n}"),
                format!("/pub/{n}"),
                TransferDirection::Upload,
            ));
        }
        terminal
            .draw(|frame| render(frame, &mut idle_focus, &mut map))
            .unwrap();
        assert_eq!(map.queue.height, 12);
        assert!(map.queue_list.height >= 1);
    }

    #[test]
    fn format_job_summary_uses_queue_helper_not_job_field() {
        use dd_ftp_core::{TransferDirection, TransferJob};
        use std::time::{Duration, Instant};

        let mut queue = TransferQueue::default();
        let job = TransferJob::new("/tmp/a", "/pub/a", TransferDirection::Upload);
        queue.enqueue(job);
        let started = queue.start_next().unwrap();
        let t0 = Instant::now();
        queue.update_active_progress_at(started.id, 0, None, t0);
        queue.update_active_progress_at(started.id, 1_000, None, t0 + Duration::from_millis(150));
        let summary = format_job_summary(&queue.active[0], &queue);
        assert!(
            summary.contains("ETA --:--"),
            "missing size must render ETA --:--; got {summary}"
        );
        // Speed lives on the queue, not TransferJob.
        let _ = queue.speed_and_eta(started.id);
    }

    #[test]
    fn footer_hints_match_the_settings_sheet() {
        let mut app = AppState::default();
        let narrow = footer_hint(&app, 79);
        assert!(
            narrow.starts_with("F1:Settings"),
            "narrow footer was {narrow}"
        );
        assert!(narrow.contains("^Q:Quit"), "narrow footer was {narrow}");
        assert!(!narrow.contains("F1:Help"));
        assert!(!narrow.contains("F3:Set"));
        let wide = footer_hint(&app, 80);
        assert!(wide.starts_with("F1: Settings"), "wide footer was {wide}");
        assert!(wide.contains("Ctrl+Q: Quit"), "wide footer was {wide}");
        assert!(!wide.contains("F1:Help"));
        assert!(!wide.contains("F3:Set"));
        app.show_settings = true;
        assert_eq!(footer_hint(&app, 79), "F1:Settings  Esc:Close  ^Q:Quit");
        assert_eq!(
            footer_hint(&app, 80),
            "F1: Settings  Esc: Close  Ctrl+Q: Quit"
        );
    }

    #[test]
    fn settings_sheet_paints_session_on_the_right_half() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = AppState::default();
        app.show_settings = true;
        app.settings_page = SETTINGS_SESSION;
        let mut map = LayoutMap::default();
        terminal
            .draw(|frame| render(frame, &mut app, &mut map))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Settings"));
        assert!(text.contains("Themes"));
        assert!(text.contains("Session"));
        assert!(text.contains("Help"));
        assert!(text.contains("Workers"));
        assert!(text.contains("Hide dotfiles"));
        assert!(text.contains("Auto-connect"));
        assert!(text.contains("Saved to"));
        assert!(text.contains("F1: Settings"));
        assert!(text.contains("Ctrl+Q: Quit"));
        assert!(!text.contains("F1:Help"));
        let nav = map.settings_nav.expect("settings nav");
        assert!(nav.x >= 50, "nav x was {}", nav.x);
        assert!(map
            .fields
            .iter()
            .any(|field| field.id == FieldId::SettingsEditor));
        assert!(map
            .controls
            .iter()
            .any(|control| control.id == ControlId::SettingsWorkers));
    }
}
