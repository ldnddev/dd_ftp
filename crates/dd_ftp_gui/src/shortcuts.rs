//! Shortcut sheet. Every terminal `KEYMAP` row is `action · gui · tui`.

use dd_ftp_app::keys::KEYMAP;

pub fn lines() -> Vec<String> {
    let mut out = Vec::new();
    let mut section = "";
    for row in KEYMAP {
        let title = row.group.title();
        if title != section {
            section = title;
            out.push(section.to_string());
        }
        out.push(format!("{} · {} · {}", row.action, row.gui, row.keys));
    }
    out
}
