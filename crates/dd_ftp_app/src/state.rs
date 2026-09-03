use std::time::{SystemTime, UNIX_EPOCH};

use dd_ftp_core::{ConnectionInfo, FileEntry, TransferDirection};
use dd_ftp_transfer::TransferQueue;

use crate::{TextField, Toast};

/// How to place `selected_*` after a listing or filter change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectPolicy {
    /// Keep the previously selected *name* if it is still visible; else clamp.
    PreserveName,
    /// Keep the numeric index, clamp to `visible_len.saturating_sub(1)`.
    Clamp,
    /// Select 0 (first visible row: `.` locally, or the first real entry remotely).
    Reset,
}

/// Built-in header taglines used when the theme supplies no `header_quotes`.
pub const DEFAULT_HEADER_QUOTES: [&str; 5] = [
    "Moving bytes so you don't have to.",
    "Remote files, local comfort.",
    "Drag, drop, transfer, repeat.",
    "Your packets are in good hands.",
    "Now serving: the whole filesystem.",
];

/// Time-seeded (XOR PID) index: stable within a run, varies per launch.
fn header_seed() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize)
        .unwrap_or(0)
        ^ std::process::id() as usize
}

/// Pick a random built-in header tagline once per launch (LDNDDEV TUI standard).
pub fn random_header_copy() -> String {
    DEFAULT_HEADER_QUOTES[header_seed() % DEFAULT_HEADER_QUOTES.len()].to_string()
}

/// Pick a random tagline from a theme-supplied `header_quotes` list, falling
/// back to the built-in list when empty.
pub fn random_header_copy_from(quotes: &[String]) -> String {
    let pool: Vec<&str> = quotes
        .iter()
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .collect();
    if pool.is_empty() {
        return random_header_copy();
    }
    pool[header_seed() % pool.len()].to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Local,
    Remote,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextPromptKind {
    CreateFile,
    CreateFolder,
    Rename,
    OverwriteRename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoicePromptKind {
    ConfirmQuit,
    ConfirmDelete,
    ConfirmBookmarkDelete,
    HostKey,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwritePolicy {
    #[default]
    Ask,
    OverwriteAll,
    SkipAll,
}

/// One file discovered by a folder scan. Enqueued only after drain + overwrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFile {
    pub local_path: String,
    pub remote_path: String,
    pub direction: TransferDirection,
    pub size_bytes: Option<u64>,
}

/// Display snapshot of the overwrite ChoicePrompt. Remaining files live on the CLI run stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverwritePrompt {
    pub current: PendingFile,
    pub remaining: Vec<PendingFile>,
    pub apply_all: OverwritePolicy,
}

/// Display-only host-key prompt. The oneshot lives in CLI `in_flight`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyView {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Text(TextPromptKind),
    Choice(ChoicePromptKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuickConnectField {
    Name,
    Host,
    Port,
    Username,
    Password,
    PrivateKey,
    Protocol,
    Path,
}

impl QuickConnectField {
    pub fn next(self) -> Self {
        match self {
            QuickConnectField::Name => QuickConnectField::Host,
            QuickConnectField::Host => QuickConnectField::Port,
            QuickConnectField::Port => QuickConnectField::Username,
            QuickConnectField::Username => QuickConnectField::Password,
            QuickConnectField::Password => QuickConnectField::PrivateKey,
            QuickConnectField::PrivateKey => QuickConnectField::Protocol,
            QuickConnectField::Protocol => QuickConnectField::Path,
            QuickConnectField::Path => QuickConnectField::Name,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            QuickConnectField::Name => QuickConnectField::Path,
            QuickConnectField::Host => QuickConnectField::Name,
            QuickConnectField::Port => QuickConnectField::Host,
            QuickConnectField::Username => QuickConnectField::Port,
            QuickConnectField::Password => QuickConnectField::Username,
            QuickConnectField::PrivateKey => QuickConnectField::Password,
            QuickConnectField::Protocol => QuickConnectField::PrivateKey,
            QuickConnectField::Path => QuickConnectField::Protocol,
        }
    }
}

#[derive(Debug)]
pub struct AppState {
    pub connected: bool,
    pub local_cwd: String,
    pub remote_cwd: String,
    pub local_entries: Vec<FileEntry>,
    pub remote_entries: Vec<FileEntry>,
    pub selected_local: usize,
    pub selected_remote: usize,
    pub focus: FocusPane,
    /// Last focused file pane (`Local` or `Remote`). Filter-bar counts use this when Queue is focused.
    pub last_file_pane: FocusPane,
    pub header_copy: String,
    pub show_help: bool,
    pub show_theme_debug: bool,
    pub help_scroll: usize,
    pub show_quick_connect: bool,
    pub show_bookmarks: bool,
    pub show_filter: bool,
    pub show_compare: bool,
    pub show_prompt: bool,
    pub prompt_kind: Option<PromptKind>,
    pub prompt_value: TextField,
    pub prompt_target: Option<String>,
    pub filter_pattern: String,
    pub mouse_pos: Option<(u16, u16)>,
    pub quick_connect: ConnectionInfo,
    pub quick_connect_field: QuickConnectField,
    /// Derived editing view of the currently focused quick-connect field.
    /// Invariant: call `qc_hydrate()` before reading and `qc_flush()` after
    /// mutating, and re-hydrate whenever `quick_connect` or
    /// `quick_connect_field` is changed directly.
    pub qc_field: TextField,
    pub worker_running: bool,
    pub worker_active_count: usize,
    pub worker_max_concurrency: usize,
    pub worker_cancel_requested: bool,
    pub bookmarks: Vec<ConnectionInfo>,
    pub selected_bookmark: usize,
    pub active_connection: Option<ConnectionInfo>,
    pub status: String,
    pub toast: Option<Toast>,
    pub queue_scroll: usize,
    pub queue: TransferQueue,
    /// Host-key ChoicePrompt fields. Oneshot is not stored here.
    pub host_key: Option<HostKeyView>,
    /// Overwrite ChoicePrompt fields. pending_scan on the CLI run stack is source of truth.
    pub overwrite: Option<OverwritePrompt>,
}

impl AppState {
    pub fn expire_toast(&mut self) {
        if self.toast.as_ref().is_some_and(Toast::is_expired) {
            self.toast = None;
        }
    }

    pub fn qc_active_value(&self) -> String {
        use crate::QuickConnectField as F;
        match self.quick_connect_field {
            F::Name => self.quick_connect.name.clone(),
            F::Host => self.quick_connect.host.clone(),
            F::Port => self.quick_connect.port.to_string(),
            F::Username => self.quick_connect.username.clone(),
            F::Password => self.quick_connect.password.clone().unwrap_or_default(),
            F::PrivateKey => self.quick_connect.private_key.clone().unwrap_or_default(),
            F::Path => self.quick_connect.initial_path.clone(),
            F::Protocol => String::new(),
        }
    }

    pub fn qc_hydrate(&mut self) {
        self.qc_field = TextField::from_str(&self.qc_active_value());
    }

    pub fn qc_flush(&mut self) {
        use crate::QuickConnectField as F;
        let v = self.qc_field.value.clone();
        match self.quick_connect_field {
            F::Name => self.quick_connect.name = v,
            F::Host => self.quick_connect.host = v,
            F::Port => self.quick_connect.port = v.parse().unwrap_or(0),
            F::Username => self.quick_connect.username = v,
            F::Password => self.quick_connect.password = Some(v),
            F::PrivateKey => self.quick_connect.private_key = Some(v),
            F::Path => self.quick_connect.initial_path = v,
            F::Protocol => {}
        }
    }

    pub fn any_modal_open(&self) -> bool {
        self.show_help
            || self.show_filter
            || self.show_prompt
            || self.show_quick_connect
            || self.show_bookmarks
    }

    pub fn is_text_prompt(&self) -> bool {
        matches!(self.prompt_kind, Some(PromptKind::Text(_)))
    }

    pub fn is_choice_prompt(&self) -> bool {
        matches!(self.prompt_kind, Some(PromptKind::Choice(_)))
    }

    pub fn entry_visible(name: &str, pattern: &str) -> bool {
        pattern.is_empty() || name.to_lowercase().contains(&pattern.to_lowercase())
    }

    pub fn visible_local(&self) -> Vec<&FileEntry> {
        self.local_entries
            .iter()
            .filter(|e| Self::entry_visible(&e.name, &self.filter_pattern))
            .collect()
    }

    pub fn visible_remote(&self) -> Vec<&FileEntry> {
        self.remote_entries
            .iter()
            .filter(|e| Self::entry_visible(&e.name, &self.filter_pattern))
            .collect()
    }

    pub fn selected_local_entry(&self) -> Option<&FileEntry> {
        self.visible_local().get(self.selected_local).copied()
    }

    pub fn selected_remote_entry(&self) -> Option<&FileEntry> {
        self.visible_remote().get(self.selected_remote).copied()
    }

    pub fn set_focus(&mut self, pane: FocusPane) {
        if pane != FocusPane::Queue {
            self.last_file_pane = pane;
        }
        self.focus = pane;
    }

    pub fn filter_count_pane(&self) -> FocusPane {
        match self.focus {
            FocusPane::Local | FocusPane::Remote => self.focus,
            FocusPane::Queue => self.last_file_pane,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connected: false,
            local_cwd: ".".to_string(),
            remote_cwd: "/".to_string(),
            local_entries: vec![],
            remote_entries: vec![],
            selected_local: 0,
            selected_remote: 0,
            focus: FocusPane::Local,
            last_file_pane: FocusPane::Local,
            header_copy: random_header_copy(),
            show_help: false,
            show_theme_debug: false,
            help_scroll: 0,
            show_quick_connect: false,
            show_bookmarks: false,
            show_filter: false,
            show_compare: false,
            show_prompt: false,
            prompt_kind: None,
            prompt_value: TextField::default(),
            prompt_target: None,
            filter_pattern: String::new(),
            mouse_pos: None,
            quick_connect: ConnectionInfo::default(),
            quick_connect_field: QuickConnectField::Name,
            qc_field: TextField::default(),
            worker_running: false,
            worker_active_count: 0,
            worker_max_concurrency: 2,
            worker_cancel_requested: false,
            bookmarks: vec![],
            selected_bookmark: 0,
            active_connection: None,
            status: "Ready".to_string(),
            toast: None,
            queue_scroll: 0,
            queue: TransferQueue::default(),
            host_key: None,
            overwrite: None,
        }
    }
}

#[cfg(test)]
mod visible_tests {
    use super::*;
    use crate::{reduce, Action};
    use dd_ftp_core::{EntryKind, FileEntry};

    fn fe(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: name.to_string(),
            kind: EntryKind::File,
            size: 0,
            modified: None,
            permissions: None,
        }
    }

    #[test]
    fn visible_local_table() {
        struct Case {
            pattern: &'static str,
            want: &'static [&'static str],
        }
        let entries = vec![fe("a"), fe("Foo"), fe("afoo"), fe("bar")];
        let cases = [
            Case {
                pattern: "",
                want: &["a", "Foo", "afoo", "bar"],
            },
            Case {
                pattern: "foo",
                want: &["Foo", "afoo"],
            },
        ];
        for case in cases {
            let s = AppState {
                local_entries: entries.clone(),
                filter_pattern: case.pattern.to_string(),
                ..Default::default()
            };
            let names: Vec<&str> = s.visible_local().iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, case.want, "pattern {:?}", case.pattern);
        }
    }

    #[test]
    fn visible_local_selection_clamps_when_pattern_shrinks_list_to_one() {
        let mut s = AppState {
            local_entries: vec![fe("a"), fe("b"), fe("c")],
            selected_local: 2,
            focus: FocusPane::Local,
            ..Default::default()
        };
        reduce(&mut s, Action::FilterInput('a'));
        assert_eq!(s.visible_local().len(), 1);
        assert_eq!(s.selected_local, 0);
        assert_eq!(s.selected_local_entry().map(|e| e.name.as_str()), Some("a"));
    }

    #[test]
    fn filter_count_pane_uses_last_file_pane_when_queue_focused() {
        let mut s = AppState::default();
        assert_eq!(s.filter_count_pane(), FocusPane::Local);
        s.set_focus(FocusPane::Remote);
        assert_eq!(s.last_file_pane, FocusPane::Remote);
        s.set_focus(FocusPane::Queue);
        assert_eq!(s.focus, FocusPane::Queue);
        assert_eq!(s.last_file_pane, FocusPane::Remote);
        assert_eq!(s.filter_count_pane(), FocusPane::Remote);
        s.set_focus(FocusPane::Local);
        assert_eq!(s.filter_count_pane(), FocusPane::Local);
    }
}
