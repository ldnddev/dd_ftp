use anyhow::Result;
use async_trait::async_trait;

use crate::{connection::ConnectionInfo, filesystem::FileEntry, transfer::TransferJob};

#[async_trait]
pub trait RemoteSession: Send + Sync {
    async fn connect(&mut self, info: ConnectionInfo) -> Result<()>;
    async fn disconnect(&mut self) -> Result<()>;

    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>>;
    async fn upload(&self, job: &TransferJob) -> Result<()>;
    async fn download(&self, job: &TransferJob) -> Result<()>;
}
