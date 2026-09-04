/// Locked key table (§1 of the design doc), including PR 8 rows and mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyGroup {
    Global,
    Navigation,
    Connection,
    Transfers,
    Filter,
    FileOps,
    Mouse,
}

impl KeyGroup {
    pub fn title(self) -> &'static str {
        match self {
            KeyGroup::Global => "Global",
            KeyGroup::Navigation => "Navigation",
            KeyGroup::Connection => "Connection / bookmarks",
            KeyGroup::Transfers => "Transfers",
            KeyGroup::Filter => "Filters / compare",
            KeyGroup::FileOps => "File operations",
            KeyGroup::Mouse => "Mouse",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KeyBinding {
    pub keys: &'static str,
    pub action: &'static str,
    pub group: KeyGroup,
}

pub const KEYMAP: &[KeyBinding] = &[
    KeyBinding {
        keys: "F1",
        action: "Toggle help (opening closes theme debug)",
        group: KeyGroup::Global,
    },
    KeyBinding {
        keys: "Esc",
        action: "Close current modal; when compare is on and no modal is open, close compare",
        group: KeyGroup::Global,
    },
    KeyBinding {
        keys: "F2",
        action: "Toggle theme debug (opening closes help)",
        group: KeyGroup::Global,
    },
    KeyBinding {
        keys: "Ctrl+q",
        action: "Quit (confirms if transfers are active)",
        group: KeyGroup::Global,
    },
    KeyBinding {
        keys: "Ctrl+C",
        action: "Cancel in-flight transfers (ignored while help, filter, prompt, or quick-connect is open)",
        group: KeyGroup::Global,
    },
    KeyBinding {
        keys: "C",
        action: "Toggle directory compare",
        group: KeyGroup::Global,
    },
    KeyBinding {
        keys: "1",
        action: "Local pane",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "2",
        action: "Remote pane",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "3",
        action: "Queue pane",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "Tab",
        action: "Cycle focus",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "j/k",
        action: "Move selection",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "l",
        action: "Enter directory",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "h",
        action: "Parent directory",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "r",
        action: "Refresh",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "Enter",
        action: "Enter directory, or queue upload (local file) / download (remote file). Queue pane: no-op.",
        group: KeyGroup::Navigation,
    },
    KeyBinding {
        keys: "o",
        action: "Quick connect",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "m",
        action: "Bookmarks modal",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "b",
        action: "Cycle bookmarks",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "c",
        action: "Connect using the quick-connect form / disconnect if already connected",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "c (bookmarks)",
        action: "Connect the highlighted bookmark (disconnects first if already connected)",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "d (bookmarks)",
        action: "Delete bookmark with confirm",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "e (bookmarks)",
        action: "Edit bookmark",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "D",
        action: "Set default bookmark",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "B",
        action: "Save current quick-connect as bookmark",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "Ctrl+K",
        action: "Keyring health check",
        group: KeyGroup::Connection,
    },
    KeyBinding {
        keys: "u",
        action: "Queue upload (directories recurse; marked set or focused row)",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "d",
        action: "Queue download (directories recurse; marked set or focused row)",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "R",
        action: "Retry last failed",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "X",
        action: "Clear pending queue",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "Ctrl+C",
        action: "Cancel in-flight transfers",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "Enter",
        action: "On a file, queue upload/download of the marked set (or focused row)",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "Enter/s skip  o overwrite  a overwrite-all  n skip-all  r rename  Esc abort",
        action: "Overwrite prompt (default skip)",
        group: KeyGroup::Transfers,
    },
    KeyBinding {
        keys: "/",
        action: "Toggle filter (Esc closes and clears the pattern)",
        group: KeyGroup::Filter,
    },
    KeyBinding {
        keys: "C",
        action: "Toggle directory compare",
        group: KeyGroup::Filter,
    },
    KeyBinding {
        keys: "Esc",
        action: "Close compare when no modal is open",
        group: KeyGroup::Filter,
    },
    KeyBinding {
        keys: "n",
        action: "Create (alias of Ctrl+n)",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "Ctrl+n",
        action: "Create file/folder prompt (Tab toggles file/folder)",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "e",
        action: "Rename selected item",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "Ctrl+Alt+e",
        action: "Rename (alias)",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "Delete",
        action: "Delete selected item with confirm",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "Ctrl+Delete",
        action: "Delete with confirm (alias)",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "Space",
        action: "Toggle multi-select mark on the visible focused row (not . / ..)",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "p",
        action: "SFTP chmod prompt (remote pane)",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "s",
        action: "Cycle sort key: name → size → date → name",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "S",
        action: "Toggle sort direction",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: ".",
        action: "Toggle hide-dotfiles",
        group: KeyGroup::FileOps,
    },
    KeyBinding {
        keys: "Mouse: wheel",
        action: "Scroll list / queue / help (also over the scrollbar rail)",
        group: KeyGroup::Mouse,
    },
    KeyBinding {
        keys: "Mouse: click row",
        action: "Focus pane, select visible row",
        group: KeyGroup::Mouse,
    },
    KeyBinding {
        keys: "Mouse: double-click dir",
        action: "Enter directory",
        group: KeyGroup::Mouse,
    },
    KeyBinding {
        keys: "Mouse: double-click file",
        action: "Queue transfer (same as Enter)",
        group: KeyGroup::Mouse,
    },
    KeyBinding {
        keys: "Mouse: drag scrollbar",
        action: "Scroll",
        group: KeyGroup::Mouse,
    },
    KeyBinding {
        keys: "Mouse: QC / prompt field click-drag",
        action: "Cursor / selection",
        group: KeyGroup::Mouse,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_keys_appear_in_readme_controls() {
        let readme = include_str!("../../../README.md");
        let start = readme
            .find("## Controls")
            .expect("README.md must have a Controls section");
        let rest = &readme[start..];
        let end = rest[1..].find("\n## ").map(|i| i + 1).unwrap_or(rest.len());
        let controls = &rest[..end];
        for kb in KEYMAP {
            assert!(
                controls.contains(kb.keys),
                "KEYMAP keys {:?} not found in README Controls section",
                kb.keys
            );
        }
    }
}
