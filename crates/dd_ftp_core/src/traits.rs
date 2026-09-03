use std::sync::{atomic::AtomicBool, Arc};

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::{connection::ConnectionInfo, filesystem::FileEntry, transfer::TransferJob};

/// Progress callback boxed so FTP (`FtpStream` is not `Sync`) can impl this trait.
pub type ProgressCb = Box<dyn Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static>;

#[async_trait]
pub trait RemoteSession: Send {
    async fn connect(&mut self, info: ConnectionInfo) -> Result<()>;
    async fn disconnect(&mut self) -> Result<()>;

    async fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>>;
    async fn upload(&mut self, job: &TransferJob) -> Result<()>;
    async fn download(&mut self, job: &TransferJob) -> Result<()>;
    async fn upload_with_progress(
        &mut self,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        on_progress: ProgressCb,
    ) -> Result<()>;
    async fn download_with_progress(
        &mut self,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        on_progress: ProgressCb,
    ) -> Result<()>;

    async fn rename(&mut self, from: &str, to: &str) -> Result<()>;
    async fn remove_file(&mut self, path: &str) -> Result<()>;
    async fn remove_dir(&mut self, path: &str) -> Result<()>;
    async fn create_dir(&mut self, path: &str) -> Result<()>;
    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<()>;
}
