use dd_ftp_app::{AppState, FocusPane, QuickConnectField};
use dd_ftp_core::{Protocol, TransferJob};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

use crate::theme::{load_theme_with_source, Theme};

pub fn render(frame: &mut Frame, app: &AppState) {
    let loaded = load_theme_with_source();
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
            Constraint::Length(1),
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
        let filter = Paragraph::new(Line::from(vec![
            Span::styled(" Filter: ", Style::default().fg(t.text_labels)),
            Span::styled(&app.filter_pattern, Style::default().fg(t.input_text_focus)),
            Span::styled(
                "█",
                Style::default()
                    .fg(t.cursor)
                    .add_modifier(Modifier::RAPID_BLINK),
            ),
        ]))
        .style(Style::default().bg(filter_bg));
        frame.render_widget(filter, vertical[1]);
    }

    let title_text = match app.focus {
        FocusPane::Local => "dd_ftp | F1: help | [/] filter | [m] bookmarks | [u] upload",
        FocusPane::Remote => "dd_ftp | F1: help | [/] filter | [m] bookmarks | [d] download",
        FocusPane::Queue => "dd_ftp | F1: help | [R] retry [C] cancel [X] clear",
    };

    let title = Paragraph::new(title_text).style(
        Style::default()
            .fg(t.text_active_focus)
            .bg(t.base_background),
    );
    frame.render_widget(title, vertical[0]);

    // Main content background (local/remote/queue region)
    let content_area = vertical[2];
    let queue_area = vertical[3];
    let status_area = vertical[4];

    frame.render_widget(
        Block::default().style(Style::default().bg(t.body_background)),
        content_area,
    );
    frame.render_widget(
        Block::default().style(Style::default().bg(t.body_background)),
        queue_area,
    );

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(content_area);

    let filter_match = |name: &str| {
        if app.filter_pattern.is_empty() {
            true
        } else {
            name.to_lowercase()
                .contains(&app.filter_pattern.to_lowercase())
        }
    };

    let local_items: Vec<ListItem> = app
        .local_entries
        .iter()
        .filter(|e| filter_match(&e.name))
        .map(|e| {
            let color = match e.kind {
                dd_ftp_core::EntryKind::Directory => t.folder,
                dd_ftp_core::EntryKind::Symlink => t.link,
                _ => t.file,
            };
            let prefix = if e.kind == dd_ftp_core::EntryKind::Directory {
                "> "
            } else {
                "  "
            };
            ListItem::new(Span::styled(
                format!("{}{}", prefix, e.name),
                Style::default().fg(color),
            ))
        })
        .collect();
    let remote_items: Vec<ListItem> = app
        .remote_entries
        .iter()
        .filter(|e| filter_match(&e.name))
        .map(|e| {
            let color = match e.kind {
                dd_ftp_core::EntryKind::Directory => t.folder,
                dd_ftp_core::EntryKind::Symlink => t.link,
                _ => t.file,
            };
            let prefix = if e.kind == dd_ftp_core::EntryKind::Directory {
                "> "
            } else {
                "  "
            };
            ListItem::new(Span::styled(
                format!("{}{}", prefix, e.name),
                Style::default().fg(color),
            ))
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
    frame.render_stateful_widget(remote, panes[1], &mut remote_state);

    render_scrollbar(
        frame,
        panes[0],
        app.selected_local,
        app.local_entries.len(),
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );
    render_scrollbar(
        frame,
        panes[1],
        app.selected_remote,
        app.remote_entries.len(),
        t.scrollbar,
        t.scrollbar_hover,
        app.mouse_pos,
    );

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

    let connected_label = app
        .active_connection
        .as_ref()
        .map(|c| {
            if c.name.trim().is_empty() {
                format!("site:{}@{}:{}", c.username, c.host, c.port)
            } else {
                format!("site:{}", c.name)
            }
        })
        .unwrap_or_else(|| "site:none".to_string());

    let status_text = if app.connected {
        format!("{} | {}", app.status, connected_label)
    } else {
        format!("{} | site:none", app.status)
    };

    let status_color = if app.error_modal.is_some()
        || app.status.to_lowercase().contains("failed")
        || app.status.to_lowercase().contains("error")
    {
        t.error
    } else if app.status.to_lowercase().contains("saved")
        || app.status.to_lowercase().contains("connected")
        || app.status.to_lowercase().contains("complete")
    {
        t.success
    } else if app.status.to_lowercase().contains("loading")
        || app.status.to_lowercase().contains("refresh")
        || app.status.to_lowercase().contains("processing")
    {
        t.info
    } else {
        t.warning
    };

    let status =
        Paragraph::new(status_text).style(Style::default().fg(status_color).bg(t.base_background));
    frame.render_widget(status, status_area);

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
            Line::from("Navigation"),
            Line::from("  1 -> focus Local pane"),
            Line::from("  2 -> focus Remote pane"),
            Line::from("  3 -> focus Queue pane"),
            Line::from("  Tab -> cycle pane focus"),
            Line::from("  j / Down -> move down"),
            Line::from("  k / Up -> move up"),
            Line::from("  l -> enter selected directory"),
            Line::from("  h -> go to parent directory"),
            Line::from(""),
            Line::from("Actions"),
            Line::from("  / -> toggle filter"),
            Line::from("  C -> toggle directory compare"),
            Line::from("  b -> cycle bookmarks"),
            Line::from("  m -> open bookmarks modal"),
            Line::from("  o -> open quick connect"),
            Line::from("  c -> connect/disconnect (SFTP+FTP connect path)"),
            Line::from("  r -> refresh listing(s)"),
            Line::from("  u -> queue upload"),
            Line::from("  d -> queue download"),
            Line::from("  x -> worker status hint"),
            Line::from("  X -> clear pending queue"),
            Line::from("  R -> retry last failed transfer"),
            Line::from("  Ctrl+K -> keyring health check"),
            Line::from("  B -> save current quick-connect as bookmark"),
            Line::from(""),
            Line::from("File Operations"),
            Line::from("  Ctrl+n -> create new file"),
            Line::from("  Ctrl+Shift+n -> create new folder"),
            Line::from("  Ctrl+Alt+e -> rename selected item"),
            Line::from("  Ctrl+Delete -> delete selected item"),
            Line::from(""),
            Line::from("Global"),
            Line::from("  F1 -> toggle this help"),
            Line::from("  F2 -> toggle theme debug overlay"),
            Line::from("  q -> quit"),
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

                let input_content = if focused {
                    Line::from(vec![
                        Span::styled(value.clone(), Style::default().fg(text_color)),
                        Span::styled(
                            "█",
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
        lines.push(Line::from("e edit | d delete | D set default | Esc close"));

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

        let lines = vec![
            Line::from(vec![Span::styled(
                "Theme Debug",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!("source: {}", loaded.source.label())),
            Line::from(format!("path:   {}", p)),
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
        ];

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

    if let Some(err) = &app.error_modal {
        let area = centered_rect(65, 35, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let lines = vec![
            Line::from(vec![Span::styled(
                "Error",
                Style::default()
                    .fg(t.modal_labels)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(err.clone()),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press Esc or Enter to close",
                Style::default().fg(t.warning),
            )]),
        ];

        let modal = Paragraph::new(lines.clone())
            .style(Style::default().bg(t.modal_background).fg(t.modal_text))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Alert ",
                        Style::default()
                            .fg(t.modal_labels)
                            .add_modifier(Modifier::BOLD),
                    )]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.error)),
            );

        frame.render_widget(modal, area);
        render_scrollbar(
            frame,
            area,
            0,
            lines.len(),
            t.scrollbar,
            t.scrollbar_hover,
            app.mouse_pos,
        );
    }

    if app.show_prompt {
        let area = centered_rect(60, 30, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.modal_background)),
            area,
        );

        let (title, message) = match app.prompt_type {
            Some(dd_ftp_app::PromptType::CreateFile) => (" Create File ", "Enter file name:"),
            Some(dd_ftp_app::PromptType::CreateFolder) => (" Create Folder ", "Enter folder name:"),
            Some(dd_ftp_app::PromptType::Rename) => (" Rename ", "Enter new name:"),
            Some(dd_ftp_app::PromptType::Delete) => (" Delete ", "Confirm delete (y/n):"),
            None => (" Prompt ", ""),
        };

        let mut lines = vec![Line::from(vec![Span::styled(
            message,
            Style::default().fg(t.modal_labels),
        )])];

        // Show target file for delete/rename
        if let Some(ref target) = app.prompt_target {
            if app.prompt_type == Some(dd_ftp_app::PromptType::Delete) {
                lines.push(Line::from(vec![Span::styled(
                    format!("File: {}", target),
                    Style::default().fg(t.text_secondary),
                )]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(t.input_text_focus)),
            Span::styled(&app.prompt_value, Style::default().fg(t.input_text_focus)),
            Span::styled(
                "█",
                Style::default()
                    .fg(t.cursor)
                    .add_modifier(Modifier::RAPID_BLINK),
            ),
        ]));
        lines.push(Line::from(""));
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
    }
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
            .any(|e| e.name == a.0 || e.kind == dd_ftp_core::EntryKind::Directory);
        let b_dir = app
            .local_entries
            .iter()
            .any(|e| e.name == b.0 || e.kind == dd_ftp_core::EntryKind::Directory);
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
