use dd_ftp_core::FileEntry;
use dd_ftp_transfer::TransferQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Local,
    Remote,
    Queue,
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
    pub status: String,
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
            status: "Ready".to_string(),
            queue: TransferQueue::default(),
        }
    }
}
