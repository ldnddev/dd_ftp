use dd_ftp_app::{AppState, FocusPane};
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

    let title = Paragraph::new("dd_ftp  |  F1: controls")
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

    let queue = Paragraph::new(format!(
        "Pending: {} | Active: {} | Complete: {} | Failed: {}",
        app.queue.pending.len(),
        app.queue.active.len(),
        app.queue.completed.len(),
        app.queue.failed.len()
    ))
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
            Line::from("  c -> connect SFTP"),
            Line::from("  r -> refresh listing(s)"),
            Line::from("  u -> queue upload"),
            Line::from("  d -> queue download"),
            Line::from("  x -> force process one queued transfer"),
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
