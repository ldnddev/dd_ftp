use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Active,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: Uuid,
    pub local_path: String,
    pub remote_path: String,
    pub direction: TransferDirection,
    pub size_bytes: Option<u64>,
    pub transferred_bytes: u64,
    pub status: TransferStatus,
    pub retries: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
}

impl TransferJob {
    pub fn new(local_path: impl Into<String>, remote_path: impl Into<String>, direction: TransferDirection) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            local_path: local_path.into(),
            remote_path: remote_path.into(),
            direction,
            size_bytes: None,
            transferred_bytes: 0,
            status: TransferStatus::Pending,
            retries: 0,
            created_at: now,
            updated_at: now,
            last_error: None,
        }
    }
}
