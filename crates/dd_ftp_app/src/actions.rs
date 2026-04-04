use dd_ftp_core::{ConnectionInfo, FileEntry, TransferJob};
use uuid::Uuid;

#[derive(Debug)]
pub enum Action {
    Connect(ConnectionInfo),
    Disconnect,
    SetConnected(bool),
    SetLocalEntries(Vec<FileEntry>),
    SetRemoteEntries(Vec<FileEntry>),
    SetBookmarks(Vec<ConnectionInfo>),
    SelectNextBookmark,
    SelectPrevBookmark,
    ToggleQuickConnect,
    ToggleBookmarks,
    QuickConnectNextField,
    QuickConnectPrevField,
    QuickConnectInput(char),
    QuickConnectBackspace,
    QuickConnectSetProtocolNext,
    QuickConnectSetProtocolPrev,
    QuickConnectSetFromBookmark(ConnectionInfo),
    QueueTransfer(TransferJob),
    StartNextTransfer,
    MarkTransferCompleted(TransferJob),
    MarkTransferFailed(TransferJob),
    MarkTransferCancelled(TransferJob),
    RetryLastFailed,
    UpdateTransferProgress {
        job_id: Uuid,
        transferred_bytes: u64,
        size_bytes: Option<u64>,
    },
    ClearPendingTransfers,
    SetStatus(String),
    FocusNextPane,
    ToggleHelp,
    SelectUp,
    SelectDown,
}

#[derive(Debug)]
pub enum AppEvent {
    Ui(Action),
    EffectCompleted(Action),
}
