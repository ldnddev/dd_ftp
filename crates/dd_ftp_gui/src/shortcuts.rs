//! Shortcut sheet. The first Global row is the shared Settings chord.

#[derive(Clone, Copy, Debug)]
pub struct SheetRow {
    pub section: &'static str,
    pub action: &'static str,
    pub gui: &'static str,
    pub tui: &'static str,
}

pub const ROWS: &[SheetRow] = &[
    SheetRow {
        section: "Global",
        action: "Settings",
        gui: "F1",
        tui: "F1",
    },
    SheetRow {
        section: "Global",
        action: "Session",
        gui: "F3",
        tui: "F3",
    },
    SheetRow {
        section: "Global",
        action: "Close",
        gui: "Esc",
        tui: "Esc",
    },
    SheetRow {
        section: "Global",
        action: "Quit",
        gui: "Ctrl+Q",
        tui: "Ctrl+Q",
    },
    SheetRow {
        section: "Navigation",
        action: "Move selection",
        gui: "Up/Down",
        tui: "j/k",
    },
    SheetRow {
        section: "Navigation",
        action: "Enter directory",
        gui: "Enter",
        tui: "l",
    },
    SheetRow {
        section: "Connection",
        action: "Connect",
        gui: "Connect",
        tui: "o",
    },
];

pub fn lines() -> Vec<String> {
    let mut out = Vec::new();
    let mut section = "";
    for row in ROWS {
        if row.section != section {
            section = row.section;
            out.push(section.to_string());
        }
        out.push(format!("{} · {} · {}", row.action, row.gui, row.tui));
    }
    out
}
