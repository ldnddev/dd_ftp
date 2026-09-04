use dd_ftp_app::{
    AppState, ChoicePromptKind, FocusPane, PromptKind, QuickConnectField, TextPromptKind,
    ToastLevel,
};
use dd_ftp_core::{Protocol, TransferJob};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

use crate::layout::{ControlId, ControlRegion, FieldId, FieldRegion, LayoutMap};
use crate::theme::{cached_theme, Theme};

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

pub fn render(frame: &mut Frame, app: &AppState, map: &mut LayoutMap) {
    *map = LayoutMap::default();
    let loaded = cached_theme();
    let t = loaded.theme;

    // Full app background (header + footer included)
    frame.render_widget(
        Block::default().style(Style::default().bg(t.base_background)),
        frame.area(),
    );

    let queue_height = if app.focus == FocusPane::Queue { 12 } else { 8 };
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

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_active))
        .style(Style::default().bg(t.base_background))
        .title(Line::from(vec![Span::styled(
            " dd_ftp ",
            Style::default()
                .fg(t.text_active_focus)
                .add_modifier(Modifier::BOLD),
        )]))
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
    let local_items: Vec<ListItem> = local_visible
        .into_iter()
        .map(|e| file_list_item(e, local_row_width, local_cols, &t))
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
        .map(|e| file_list_item(e, remote_row_width, remote_cols, &t))
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

    let local_path = Paragraph::new(shorten_middle(&app.local_cwd, 70))
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    " Local Path ",
                    Style::default().fg(t.text_labels),
                )]))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_default)),
        );

    let remote_path = Paragraph::new(shorten_middle(&app.remote_cwd, 70))
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    " Remote Path ",
                    Style::default().fg(t.text_labels),
                )]))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_default)),
        );

    frame.render_widget(local_path, path_panes[0]);
    frame.render_widget(remote_path, path_panes[1]);

    let local = List::new(local_items)
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    " [1] Local ",
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

    let remote = List::new(remote_items)
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    " [2] Remote ",
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
            Paragraph::new("Not connected — o Quick Connect · m Bookmarks · F1 Help")
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

    if app.show_compare {
        render_compare_view(frame, content_area, app, &t);
    }

    let queue_style = if app.focus == FocusPane::Queue {
        Style::default().fg(t.border_active)
    } else {
        Style::default().fg(t.border_default)
    };

    let worker_state = if app.worker_running {
        "running"
    } else {
        "idle"
    };

    let mut queue_text = vec![Line::from(vec![
        Span::styled("Worker: ", Style::default().fg(t.text_secondary)),
        Span::styled(worker_state, Style::default().fg(t.info)),
        Span::raw(format!(
            " ({}/{}) | Pending: {} | Active: {} | Complete: {} | Failed: {} | Cancelled: {}",
            app.worker_active_count,
            app.worker_max_concurrency,
            app.queue.pending.len(),
            app.queue.active.len(),
            app.queue.completed.len(),
            app.queue.failed.len(),
            app.queue.cancelled.len()
        )),
    ])];

    if app.queue.active.is_empty() && app.queue.pending.is_empty() && app.queue.failed.is_empty() {
        queue_text.push(Line::from("No jobs in queue"));
    } else {
        let row_cap = if app.focus == FocusPane::Queue { 5 } else { 2 };

        for job in app.queue.active.iter().take(row_cap) {
            queue_text.push(Line::from(vec![
                Span::styled(
                    "A ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format_job_summary(job)),
            ]));
        }
        for job in app.queue.pending.iter().take(row_cap) {
            queue_text.push(Line::from(vec![
                Span::styled(
                    "P ",
                    Style::default()
                        .fg(t.text_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format_job_summary(job)),
            ]));
        }
        for job in app.queue.failed.iter().rev().take(row_cap) {
            queue_text.push(Line::from(vec![
                Span::styled(
                    "F ",
                    Style::default().fg(t.error).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format_job_summary(job)),
            ]));
        }
    }

    let queue_title_style = if app.focus == FocusPane::Queue {
        Style::default()
            .fg(t.text_active_focus)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.text_labels)
    };

    let queue = Paragraph::new(queue_text.clone())
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .scroll((app.queue_scroll as u16, 0))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    " [3] Queue ",
                    queue_title_style,
                )]))
                .borders(Borders::ALL)
                .border_style(queue_style),
        );
    frame.render_widget(queue, queue_area);
    render_scrollbar(
        frame,
        queue_area,
        app.queue_scroll,
        queue_text.len(),
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );

    // Footer: width-adaptive key hints (terse on narrow, full on wide).
    // Always leads with F1:Help. Connection state lives in the header; the
    // transient status string is shown right-aligned only when there is room.
    let keys = if status_area.width < 75 {
        "F1:Help  Tab:Pane  j/k:Nav  /:Filter  u/d:Xfer  m:Sites  q:Quit"
    } else if status_area.width < 110 {
        "F1: Help   Tab: Pane   j/k: Nav   h/l: Dir   /: Filter   u/d: Up/Down   m: Bookmarks   o: Connect   q: Quit"
    } else {
        "F1: Help   Tab: Pane   j/k: Nav   h/l: Dir   /: Filter   u: Upload   d: Download   m: Bookmarks   o: Connect   r: Refresh   q: Quit   (mouse: click/scroll/drag)"
    };
    let footer_keys =
        Paragraph::new(keys).style(Style::default().fg(t.text_secondary).bg(t.base_background));
    frame.render_widget(footer_keys, status_area);

    // Right-aligned transient status, only when the terminal is wide enough
    // that it won't collide with the key hints.
    if status_area.width >= 90 && !app.status.is_empty() {
        let s = app.status.to_lowercase();
        let status_color = if s.contains("failed") || s.contains("error") {
            t.error
        } else if s.contains("saved") || s.contains("connected") || s.contains("complete") {
            t.success
        } else if s.contains("loading") || s.contains("refresh") || s.contains("processing") {
            t.info
        } else {
            t.text_secondary
        };
        let status = Paragraph::new(Line::from(Span::styled(
            format!("{} ", app.status),
            Style::default().fg(status_color),
        )))
        .alignment(Alignment::Right)
        .style(Style::default().bg(t.base_background));
        frame.render_widget(status, status_area);
    }

    if app.show_help {
        let backdrop = Block::default().style(
            Style::default()
                .bg(t.base_background)
                .add_modifier(Modifier::DIM),
        );
        frame.render_widget(backdrop, frame.area());

        let area = centered_rect(70, 70, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let lines = vec![
            Line::from(vec![Span::styled(
                "Controls",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("Global"),
            Line::from("  F1 -> toggle this help (opening closes theme debug)"),
            Line::from("  Esc -> close current modal; if compare is on, close compare"),
            Line::from("  F2 -> toggle theme debug overlay (opening closes help)"),
            Line::from("  q -> quit (confirms if transfers are active)"),
            Line::from("  Ctrl+C -> cancel in-flight transfers"),
            Line::from("  C -> toggle directory compare"),
            Line::from(""),
            Line::from("Navigation"),
            Line::from("  1 -> focus Local pane"),
            Line::from("  2 -> focus Remote pane"),
            Line::from("  3 -> focus Queue pane"),
            Line::from("  Tab -> cycle pane focus"),
            Line::from("  j / Down -> move down"),
            Line::from("  k / Up -> move up"),
            Line::from("  l -> enter selected directory"),
            Line::from("  h -> go to parent directory"),
            Line::from("  r -> refresh listing(s)"),
            Line::from("  Enter -> enter directory, or queue upload/download of a file"),
            Line::from(""),
            Line::from("Connection / bookmarks"),
            Line::from("  o -> open quick connect"),
            Line::from("  m -> open bookmarks modal"),
            Line::from("  b -> cycle bookmarks"),
            Line::from("  c -> connect using the quick-connect form / disconnect"),
            Line::from("  c (bookmarks) -> connect highlighted bookmark (disconnect first)"),
            Line::from("  d (bookmarks) -> delete bookmark with confirm"),
            Line::from("  e (bookmarks) -> edit bookmark"),
            Line::from("  D (bookmarks) -> set default bookmark"),
            Line::from("  B -> save current quick-connect as bookmark"),
            Line::from("  Ctrl+K -> keyring health check"),
            Line::from(""),
            Line::from("Transfers"),
            Line::from("  u -> queue upload (directories recurse)"),
            Line::from("  d -> queue download (directories recurse)"),
            Line::from("  R -> retry last failed transfer"),
            Line::from("  X -> clear pending queue"),
            Line::from("  Ctrl+C -> cancel in-flight transfers"),
            Line::from("  Enter (file) -> queue upload (local) or download (remote)"),
            Line::from("  Overwrite: Enter/s skip  o overwrite  a overwrite-all  n skip-all  r rename  Esc abort"),
            Line::from(""),
            Line::from("Filters / compare"),
            Line::from("  / -> toggle filter (Esc closes and clears the pattern)"),
            Line::from("  C -> toggle directory compare"),
            Line::from("  Esc -> close compare when no modal is open"),
            Line::from(""),
            Line::from("File operations"),
            Line::from("  n / Ctrl+n -> new file/folder (Tab toggles)"),
            Line::from("  e / Ctrl+Alt+e -> rename selected item"),
            Line::from("  Delete / Ctrl+Delete -> delete selected item with confirm"),
            Line::from(""),
            Line::from("Selection / sort"),
            Line::from("  Space -> toggle multi-select mark on the focused row"),
            Line::from("  p -> SFTP chmod prompt (remote pane)"),
            Line::from("  s -> cycle sort key: name -> size -> date -> name"),
            Line::from("  S -> toggle sort direction"),
            Line::from("  . -> toggle hide-dotfiles"),
            Line::from(""),
            Line::from("Mouse"),
            Line::from("  Wheel -> scroll list / queue / help (also over the scrollbar rail)"),
            Line::from("  Click row -> focus pane, select visible row"),
            Line::from("  Double-click directory -> enter it"),
            Line::from("  Double-click file -> queue transfer (same as Enter)"),
            Line::from("  Drag scrollbar -> scroll"),
            Line::from("  QC / prompt field click-drag -> cursor / selection"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press F1 or Esc to close",
                Style::default().fg(t.warning),
            )]),
        ];

        let help = Paragraph::new(lines.clone())
            .style(Style::default().bg(t.modal_background).fg(t.modal_text))
            .scroll((app.help_scroll as u16, 0))
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Help ",
                        Style::default()
                            .fg(t.modal_labels)
                            .add_modifier(Modifier::BOLD),
                    )]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.border_active)),
            );

        frame.render_widget(help, area);
        let viewport_height = area.height.saturating_sub(2) as usize;
        let content_height = lines.len();
        if content_height > viewport_height {
            render_scrollbar(
                frame,
                area,
                app.help_scroll,
                content_height,
                t.scrollbar,
                t.scrollbar_hover,
                app.mouse_pos,
            );
        }

        map.help = Some(area);
        map.help_scrollbar = Some(Rect {
            x: area.x + area.width.saturating_sub(1),
            y: area.y,
            width: 1,
            height: area.height,
        });
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

                frame.render_widget(input, cols[col_idx]);

                let cell = cols[col_idx];
                match field {
                    QuickConnectField::Protocol => {
                        map.controls.push(ControlRegion {
                            id: ControlId::QcProtocol,
                            area: cell,
                        });
                    }
                    other => {
                        let fid = match other {
                            QuickConnectField::Name => FieldId::QcName,
                            QuickConnectField::Host => FieldId::QcHost,
                            QuickConnectField::Port => FieldId::QcPort,
                            QuickConnectField::Username => FieldId::QcUsername,
                            QuickConnectField::Password => FieldId::QcPassword,
                            QuickConnectField::PrivateKey => FieldId::QcPrivateKey,
                            QuickConnectField::Path => FieldId::QcPath,
                            QuickConnectField::Protocol => unreachable!(),
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

        let footer = Paragraph::new(vec![
            Line::from("Tab/Shift+Tab move field | ←/→ protocol | Enter connect"),
            Line::from("Ctrl+S save bookmark | Esc close"),
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

    if app.show_theme_debug {
        let area = centered_rect(60, 70, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let p = loaded
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(built-in defaults)".to_string());

        let version_str = loaded
            .version
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(none)".to_string());

        let mut lines = vec![
            Line::from(vec![Span::styled(
                "Theme Debug",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!("source:  {}", loaded.source.label())),
            Line::from(format!("path:    {}", p)),
            Line::from(format!("version: {}", version_str)),
        ];
        if let Some(w) = loaded.warning.as_ref() {
            lines.push(Line::from(vec![Span::styled(
                format!("warning: {}", w),
                Style::default().fg(t.warning),
            )]));
        }
        lines.extend([
            Line::from(""),
            Line::from("color tokens:"),
            Line::from("base_background / body_background / modal_background"),
            Line::from("border_default / border_active / scrollbar / scrollbar_hover"),
            Line::from("text_primary / text_secondary / text_labels"),
            Line::from("text_active_focus / text_labels_active"),
            Line::from("modal_labels / modal_text / selected_background"),
            Line::from("input_border_default / input_border_focus"),
            Line::from("input_text_default / input_text_focus / cursor"),
            Line::from("success / warning / error / info"),
            Line::from("folder / file / link"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press F2 to close",
                Style::default().fg(t.warning),
            )]),
        ]);

        let modal = Paragraph::new(lines)
            .style(Style::default().bg(t.modal_background).fg(t.modal_text))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Theme ",
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
            0,
            12,
            t.scrollbar,
            t.scrollbar_hover,
            app.mouse_pos,
        );
    }

    if app.show_prompt {
        match app.prompt_kind {
            Some(PromptKind::Choice(kind)) => {
                let area = centered_rect(60, 24, frame.area());
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
                        (
                            " Overwrite ",
                            vec![
                                Line::from(vec![Span::styled(
                                    "Destination exists",
                                    Style::default().fg(t.modal_labels),
                                )]),
                                Line::from(vec![Span::styled(
                                    format!("local: {local}"),
                                    Style::default().fg(t.modal_text),
                                )]),
                                Line::from(vec![Span::styled(
                                    format!("remote: {remote}"),
                                    Style::default().fg(t.modal_text),
                                )]),
                                Line::from(""),
                                Line::from(vec![Span::styled(
                                    "Enter/s skip  o overwrite  a overwrite-all  n skip-all  r rename  Esc abort",
                                    Style::default().fg(t.warning),
                                )]),
                            ],
                        )
                    }
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
                    TextPromptKind::OverwriteRename => (" Rename destination ", "Enter new name:"),
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
    let width = area.width.min(56);
    let height = area.height.min(5);

    if width == 0 || height == 0 {
        return;
    }

    let x = area.x + area.width.saturating_sub(width);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height.saturating_add(1)));
    let toast_area = Rect {
        x,
        y,
        width,
        height,
    };

    let (title, border_color) = match toast.level {
        ToastLevel::Info => (" Info ", t.info),
        ToastLevel::Success => (" Success ", t.success),
        ToastLevel::Warning => (" Warning ", t.warning),
        ToastLevel::Error => (" Error ", t.error),
    };

    frame.render_widget(Clear, toast_area);

    let toast_widget = Paragraph::new(toast.message.clone())
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
                .border_style(Style::default().fg(border_color)),
        );

    frame.render_widget(toast_widget, toast_area);
}

fn format_job_summary(job: &TransferJob) -> String {
    let direction = match job.direction {
        dd_ftp_core::TransferDirection::Upload => "up",
        dd_ftp_core::TransferDirection::Download => "down",
    };

    let progress = format_progress(job);

    let local = shorten_middle(&job.local_path, 22);
    let remote = shorten_middle(&job.remote_path, 22);

    format!(
        "{} {} -> {} [{}] r{}",
        direction, local, remote, progress, job.retries
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
    let meta = entry_meta(entry, cols);
    let meta_len = meta.chars().count();
    let name_max = width
        .saturating_sub(prefix.chars().count() + meta_len)
        .max(1);
    let name = shorten_middle(&entry.name, name_max);
    let pad = width.saturating_sub(prefix.chars().count() + name.chars().count() + meta_len);
    let padding = " ".repeat(pad);

    ListItem::new(Line::from(vec![
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

fn render_compare_view(frame: &mut Frame, area: Rect, app: &AppState, t: &Theme) {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CompareStatus {
        LocalOnly,
        RemoteOnly,
        Same,
    }

    let local_names: std::collections::HashSet<_> = app
        .local_entries
        .iter()
        .map(|e| e.name.to_lowercase())
        .collect();
    let remote_names: std::collections::HashSet<_> = app
        .remote_entries
        .iter()
        .map(|e| e.name.to_lowercase())
        .collect();

    let mut compare_items: Vec<(String, CompareStatus)> = Vec::new();

    for entry in &app.local_entries {
        let status = if remote_names.contains(&entry.name.to_lowercase()) {
            CompareStatus::Same
        } else {
            CompareStatus::LocalOnly
        };
        compare_items.push((entry.name.clone(), status));
    }

    for entry in &app.remote_entries {
        if !local_names.contains(&entry.name.to_lowercase()) {
            compare_items.push((entry.name.clone(), CompareStatus::RemoteOnly));
        }
    }

    compare_items.sort_by(|a, b| {
        let a_dir = app
            .local_entries
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(&a.0) && e.is_dir());
        let b_dir = app
            .local_entries
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(&b.0) && e.is_dir());
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        }
    });

    let compare_items: Vec<ListItem> = compare_items
        .iter()
        .map(|(name, status)| {
            let (prefix, color) = match status {
                CompareStatus::LocalOnly => ("[L] ", t.warning),
                CompareStatus::RemoteOnly => ("[R] ", t.info),
                CompareStatus::Same => ("[=] ", t.success),
            };
            ListItem::new(Span::styled(
                format!("{}{}", prefix, name),
                Style::default().fg(color),
            ))
        })
        .collect();

    let compare_block = List::new(compare_items)
        .style(Style::default().bg(t.body_background).fg(t.text_primary))
        .block(
            Block::default()
                .title(Line::from(vec![Span::styled(
                    " Compare | [L] local only | [R] remote only | [=] same ",
                    Style::default()
                        .fg(t.text_labels)
                        .add_modifier(Modifier::BOLD),
                )]))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_active)),
        );

    frame.render_widget(compare_block, area);
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
    fn meta_columns_drop_suffix_from_pane_width() {
        assert!(meta_columns(31).perms);
        assert!(meta_columns(25).date && !meta_columns(25).perms);
        assert!(meta_columns(7).size && !meta_columns(7).date);
        assert!(!meta_columns(6).size);
    }
}
