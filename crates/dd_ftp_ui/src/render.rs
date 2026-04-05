use dd_ftp_app::{AppState, FocusPane, QuickConnectField};
use dd_ftp_core::{Protocol, TransferJob};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

pub fn render(frame: &mut Frame, app: &AppState) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let bookmark_label = app
        .bookmarks
        .get(app.selected_bookmark)
        .map(|b| {
            if b.name.trim().is_empty() {
                format!("Bookmark: {}", b.host)
            } else {
                format!("Bookmark: {}", b.name)
            }
        })
        .unwrap_or_else(|| "Bookmark: none".to_string());

    let connect_label = if app.connected { "disconnect" } else { "connect" };

    let title = Paragraph::new(format!(
        "dd_ftp  |  F1: controls  |  [b] cycle bookmarks  [c] {}  |  {}",
        connect_label, bookmark_label
    ))
    .style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, vertical[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(vertical[1]);

    let local_items: Vec<ListItem> = app
        .local_entries
        .iter()
        .map(|e| ListItem::new(e.name.clone()))
        .collect();
    let remote_items: Vec<ListItem> = app
        .remote_entries
        .iter()
        .map(|e| ListItem::new(e.name.clone()))
        .collect();

    let local_style = if app.focus == FocusPane::Local {
        Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let remote_style = if app.focus == FocusPane::Remote {
        Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let local = List::new(local_items)
        .block(Block::default().title(" [1] Local ").borders(Borders::ALL).border_style(local_style))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

    let remote = List::new(remote_items)
        .block(Block::default().title(" [2] Remote ").borders(Borders::ALL).border_style(remote_style))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

    let mut local_state = ListState::default();
    local_state.select(Some(app.selected_local));

    let mut remote_state = ListState::default();
    remote_state.select(Some(app.selected_remote));

    frame.render_stateful_widget(local, panes[0], &mut local_state);
    frame.render_stateful_widget(remote, panes[1], &mut remote_state);

    let queue_style = if app.focus == FocusPane::Queue {
        Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let active_label = app
        .queue
        .active
        .first()
        .map(format_job_summary)
        .unwrap_or_else(|| "none".to_string());

    let pending_label = app
        .queue
        .pending
        .first()
        .map(format_job_summary)
        .unwrap_or_else(|| "none".to_string());

    let failed_label = app
        .queue
        .failed
        .last()
        .map(format_job_summary)
        .unwrap_or_else(|| "none".to_string());

    let worker_state = if app.worker_running { "running" } else { "idle" };

    let queue_text = vec![
        Line::from(format!(
            "Worker: {} | Pending: {} | Active: {} | Complete: {} | Failed: {} | Cancelled: {}",
            worker_state,
            app.queue.pending.len(),
            app.queue.active.len(),
            app.queue.completed.len(),
            app.queue.failed.len(),
            app.queue.cancelled.len()
        )),
        Line::from(format!("Active:  {}", active_label)),
        Line::from(format!("Next:    {}", pending_label)),
        Line::from(format!("Failed:  {}", failed_label)),
    ];

    let queue = Paragraph::new(queue_text)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(" [3] Queue ")
                .borders(Borders::ALL)
                .border_style(queue_style),
        );
    frame.render_widget(queue, vertical[2]);

    let status = Paragraph::new(app.status.clone())
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(status, vertical[3]);

    if app.show_help {
        // Dim backdrop behind modal.
        let backdrop = Block::default().style(Style::default().bg(Color::Black).add_modifier(Modifier::DIM));
        frame.render_widget(backdrop, frame.area());

        let area = centered_rect(70, 70, frame.area());
        frame.render_widget(Clear, area);

        let lines = vec![
            Line::from(vec![Span::styled("Controls", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
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
            Line::from("  C -> cancel active transfer"),
            Line::from("  B -> save current quick-connect as bookmark"),
            Line::from(""),
            Line::from("Global"),
            Line::from("  F1 -> toggle this help"),
            Line::from("  q -> quit"),
            Line::from(""),
            Line::from(vec![Span::styled("Press F1 or Esc to close", Style::default().fg(Color::Yellow))]),
        ];

        let help = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left)
            .block(Block::default().title(" Help ").borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));

        frame.render_widget(help, area);
    }

    if app.show_quick_connect {
        let area = centered_rect(70, 65, frame.area());
        frame.render_widget(Clear, area);

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

        let rows = vec![
            (QuickConnectField::Name, format!("Name: {}", app.quick_connect.name)),
            (QuickConnectField::Host, format!("Host: {}", app.quick_connect.host)),
            (QuickConnectField::Port, format!("Port: {}", app.quick_connect.port)),
            (
                QuickConnectField::Username,
                format!("User: {}", app.quick_connect.username),
            ),
            (
                QuickConnectField::Password,
                format!("Pass: {}", password_mask),
            ),
            (
                QuickConnectField::PrivateKey,
                format!("SSH Key: {}", private_key),
            ),
            (
                QuickConnectField::Protocol,
                format!("Protocol: {}", protocol),
            ),
            (
                QuickConnectField::Path,
                format!("Path: {}", app.quick_connect.initial_path),
            ),
        ];

        let mut lines = vec![
            Line::from(vec![Span::styled(
                "Quick Connect",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
        ];

        for (field, value) in rows {
            if field == app.quick_connect_field {
                lines.push(Line::from(vec![Span::styled(
                    format!("> {}", value),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )]));
            } else {
                lines.push(Line::from(format!("  {}", value)));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Tab/Shift+Tab move field | ←/→ protocol | Enter connect"));
        lines.push(Line::from("Set either Password or SSH Key for SFTP auth"));
        lines.push(Line::from("Ctrl+S save bookmark | Esc close"));

        let modal = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(" Connection ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

        frame.render_widget(modal, area);
    }

    if app.show_bookmarks {
        let area = centered_rect(70, 65, frame.area());
        frame.render_widget(Clear, area);

        let mut lines = vec![
            Line::from(vec![Span::styled(
                "Bookmarks",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
        lines.push(Line::from("j/k move | Enter load into quick connect | c connect"));
        lines.push(Line::from("e edit | d delete | D set default | Esc close"));

        let modal = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(" Bookmarks ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

        frame.render_widget(modal, area);
    }
}

fn format_job_summary(job: &TransferJob) -> String {
    let direction = match job.direction {
        dd_ftp_core::TransferDirection::Upload => "up",
        dd_ftp_core::TransferDirection::Download => "down",
    };

    let progress = if let Some(size) = job.size_bytes {
        if size > 0 {
            let pct = (job.transferred_bytes as f64 / size as f64) * 100.0;
            format!("{:.0}%", pct)
        } else {
            "0%".to_string()
        }
    } else {
        format!("{}B", job.transferred_bytes)
    };

    let local = shorten_middle(&job.local_path, 28);
    let remote = shorten_middle(&job.remote_path, 28);

    format!(
        "{} {} -> {} ({}, retry {})",
        direction, local, remote, progress, job.retries
    )
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
