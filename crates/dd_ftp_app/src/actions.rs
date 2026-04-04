use dd_ftp_core::{ConnectionInfo, FileEntry, TransferJob};

#[derive(Debug)]
pub enum Action {
    Connect(ConnectionInfo),
    Disconnect,
    SetConnected(bool),
    SetLocalEntries(Vec<FileEntry>),
    SetRemoteEntries(Vec<FileEntry>),
    QueueTransfer(TransferJob),
    SetStatus(String),
    FocusNextPane,
    SelectUp,
    SelectDown,
}

#[derive(Debug)]
pub enum AppEvent {
    Ui(Action),
    EffectCompleted(Action),
}
