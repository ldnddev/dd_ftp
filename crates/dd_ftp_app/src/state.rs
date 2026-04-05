use dd_ftp_core::{ConnectionInfo, FileEntry};
use dd_ftp_transfer::TransferQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Local,
    Remote,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub show_quick_connect: bool,
    pub show_bookmarks: bool,
    pub quick_connect: ConnectionInfo,
    pub quick_connect_field: QuickConnectField,
    pub worker_running: bool,
    pub worker_active_count: usize,
    pub worker_max_concurrency: usize,
    pub worker_cancel_requested: bool,
    pub bookmarks: Vec<ConnectionInfo>,
    pub selected_bookmark: usize,
    pub active_connection: Option<ConnectionInfo>,
    pub status: String,
    pub error_modal: Option<String>,
    pub queue: TransferQueue,
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
            show_quick_connect: false,
            show_bookmarks: false,
            quick_connect: ConnectionInfo::default(),
            quick_connect_field: QuickConnectField::Name,
            worker_running: false,
            worker_active_count: 0,
            worker_max_concurrency: 2,
            worker_cancel_requested: false,
            bookmarks: vec![],
            selected_bookmark: 0,
            active_connection: None,
            status: "Ready".to_string(),
            error_modal: None,
            queue: TransferQueue::default(),
        }
    }
}
