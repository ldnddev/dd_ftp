pub mod connection;
pub mod error;
pub mod filesystem;
pub mod traits;
pub mod transfer;

pub use connection::{ConnectionInfo, Protocol};
pub use error::CoreError;
pub use filesystem::{EntryKind, FileEntry};
pub use traits::{ProgressCb, RemoteSession};
pub use transfer::{TransferDirection, TransferJob, TransferStatus};
