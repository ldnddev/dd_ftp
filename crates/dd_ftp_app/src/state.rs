use std::collections::HashSet;

use dd_ftp_core::{ConnectionInfo, FileEntry};
use dd_ftp_transfer::TransferQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Local,
    Remote,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptType {
    CreateFile,
    CreateFolder,
    Rename,
    Delete,
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
    pub show_help: bool,
    pub show_theme_debug: bool,
    pub help_scroll: usize,
    pub show_quick_connect: bool,
    pub show_bookmarks: bool,
    pub show_filter: bool,
    pub show_compare: bool,
    pub show_prompt: bool,
    pub prompt_type: Option<PromptType>,
    pub prompt_value: String,
    pub prompt_target: Option<String>,
    pub filter_pattern: String,
    pub mouse_pos: Option<(u16, u16)>,
    pub quick_connect: ConnectionInfo,
    pub quick_connect_field: QuickConnectField,
    pub quick_connect_dirty_fields: HashSet<QuickConnectField>,
    pub worker_running: bool,
    pub worker_active_count: usize,
    pub worker_max_concurrency: usize,
    pub worker_cancel_requested: bool,
    pub bookmarks: Vec<ConnectionInfo>,
    pub selected_bookmark: usize,
    pub active_connection: Option<ConnectionInfo>,
    pub status: String,
    pub error_modal: Option<String>,
    pub queue_scroll: usize,
    pub queue: TransferQueue,
    pub ftp_session: Option<dd_ftp_ftp::UnifiedFtpSession>,
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
            show_help: false,
            show_theme_debug: false,
            help_scroll: 0,
            show_quick_connect: false,
            show_bookmarks: false,
            show_filter: false,
            show_compare: false,
            show_prompt: false,
            prompt_type: None,
            prompt_value: String::new(),
            prompt_target: None,
            filter_pattern: String::new(),
            mouse_pos: None,
            quick_connect: ConnectionInfo::default(),
            quick_connect_field: QuickConnectField::Name,
            quick_connect_dirty_fields: HashSet::new(),
            worker_running: false,
            worker_active_count: 0,
            worker_max_concurrency: 2,
            worker_cancel_requested: false,
            bookmarks: vec![],
            selected_bookmark: 0,
            active_connection: None,
            status: "Ready".to_string(),
            error_modal: None,
            queue_scroll: 0,
            queue: TransferQueue::default(),
            ftp_session: None,
        }
    }
}
