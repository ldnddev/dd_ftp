use anyhow::Result;
use async_trait::async_trait;
use dd_ftp_core::{ConnectionInfo, FileEntry, RemoteSession, TransferJob};

#[derive(Default)]
pub struct FtpsSession;

#[async_trait]
impl RemoteSession for FtpsSession {
    async fn connect(&mut self, _info: ConnectionInfo) -> Result<()> {
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    async fn list_dir(&self, _path: &str) -> Result<Vec<FileEntry>> {
        Ok(vec![])
    }

    async fn upload(&self, _job: &TransferJob) -> Result<()> {
        anyhow::bail!("FTPS session not implemented yet")
    }

    async fn download(&self, _job: &TransferJob) -> Result<()> {
        anyhow::bail!("FTPS session not implemented yet")
    }
}
