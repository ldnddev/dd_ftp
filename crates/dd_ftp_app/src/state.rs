use dd_ftp_core::{ConnectionInfo, FileEntry};
use dd_ftp_transfer::TransferQueue;

use crate::{TextField, Toast};

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
    pub prompt_value: TextField,
    pub prompt_target: Option<String>,
    pub filter_pattern: String,
    pub mouse_pos: Option<(u16, u16)>,
    pub quick_connect: ConnectionInfo,
    pub quick_connect_field: QuickConnectField,
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
    pub ftp_session: Option<dd_ftp_ftp::UnifiedFtpSession>,
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
            ftp_session: None,
        }
    }
}
