use dd_ftp_app::{AppState, FocusPane};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
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

    let title = Paragraph::new("dd_ftp  |  [Tab] pane  [↑/↓] move  [c] connect  [r] refresh  [q] quit")
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
        .block(Block::default().title(" Local ").borders(Borders::ALL).border_style(local_style))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

    let remote = List::new(remote_items)
        .block(Block::default().title(" Remote ").borders(Borders::ALL).border_style(remote_style))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

    let mut local_state = ListState::default();
    local_state.select(Some(app.selected_local));

    let mut remote_state = ListState::default();
    remote_state.select(Some(app.selected_remote));

    frame.render_stateful_widget(local, panes[0], &mut local_state);
    frame.render_stateful_widget(remote, panes[1], &mut remote_state);

    let queue = Paragraph::new(format!(
        "Pending: {} | Active: {} | Complete: {} | Failed: {}",
        app.queue.pending.len(),
        app.queue.active.len(),
        app.queue.completed.len(),
        app.queue.failed.len()
    ))
    .block(Block::default().title(" Queue ").borders(Borders::ALL));
    frame.render_widget(queue, vertical[2]);

    let status = Paragraph::new(app.status.clone())
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(status, vertical[3]);
}
