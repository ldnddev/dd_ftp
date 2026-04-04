use crate::{Action, AppState, FocusPane};

pub fn reduce(state: &mut AppState, action: Action) {
    match action {
        Action::Connect(info) => {
            state.status = format!("Connecting to {}...", info.host);
        }
        Action::Disconnect => {
            state.connected = false;
            state.status = "Disconnected".to_string();
        }
        Action::SetConnected(value) => {
            state.connected = value;
            state.status = if value {
                "Connected".to_string()
            } else {
                "Disconnected".to_string()
            };
        }
        Action::SetLocalEntries(entries) => {
            state.local_entries = entries;
            state.selected_local = 0;
        }
        Action::SetRemoteEntries(entries) => {
            state.remote_entries = entries;
            state.selected_remote = 0;
        }
        Action::QueueTransfer(job) => {
            state.queue.enqueue(job);
            state.status = format!("Queue: {} pending", state.queue.pending.len());
        }
        Action::SetStatus(msg) => {
            state.status = msg;
        }
        Action::FocusNextPane => {
            state.focus = match state.focus {
                FocusPane::Local => FocusPane::Remote,
                FocusPane::Remote => FocusPane::Queue,
                FocusPane::Queue => FocusPane::Local,
            };
        }
        Action::SelectUp => match state.focus {
            FocusPane::Local => {
                if state.selected_local > 0 {
                    state.selected_local -= 1;
                }
            }
            FocusPane::Remote => {
                if state.selected_remote > 0 {
                    state.selected_remote -= 1;
                }
            }
            FocusPane::Queue => {}
        },
        Action::SelectDown => match state.focus {
            FocusPane::Local => {
                if state.selected_local < state.local_entries.len().saturating_sub(1) {
                    state.selected_local += 1;
                }
            }
            FocusPane::Remote => {
                if state.selected_remote < state.remote_entries.len().saturating_sub(1) {
                    state.selected_remote += 1;
                }
            }
            FocusPane::Queue => {}
        },
    }
}
