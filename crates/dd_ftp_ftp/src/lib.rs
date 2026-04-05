use std::{convert::TryFrom, io::Cursor};

use anyhow::{Context, Result};
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, TransferJob};
use tokio::io::{AsyncReadExt, BufReader};
use tokio_rustls::rustls::{
    client::ServerName, ClientConfig, OwnedTrustAnchor, RootCertStore,
};

#[derive(Debug, Clone, Copy)]
pub enum FtpVariant {
    Ftp,
    Ftps,
}

#[derive(Default)]
pub struct UnifiedFtpSession {
    connected: bool,
    info: Option<ConnectionInfo>,
}

impl UnifiedFtpSession {
    pub fn new() -> Self {
        Self::default()
    }

    async fn login_stream(info: &ConnectionInfo) -> Result<async_ftp::FtpStream> {
        let mut stream = async_ftp::FtpStream::connect((info.host.as_str(), info.port))
            .await
            .with_context(|| format!("FTP connect failed: {}:{}", info.host, info.port))?;

        let password = info.password.clone().unwrap_or_default();
        stream
            .login(info.username.as_str(), password.as_str())
            .await
            .with_context(|| format!("FTP login failed for {}", info.username))?;

        Ok(stream)
    }

    async fn login_secure_stream(info: &ConnectionInfo) -> Result<async_ftp::FtpStream> {
        let stream = async_ftp::FtpStream::connect((info.host.as_str(), info.port))
            .await
            .with_context(|| format!("FTPS connect failed: {}:{}", info.host, info.port))?;

        let mut root_store = RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));

        let config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let domain = ServerName::try_from(info.host.as_str())
            .with_context(|| format!("Invalid FTPS server name: {}", info.host))?;

        let mut secure = stream
            .into_secure(config, domain)
            .await
            .with_context(|| format!("FTPS TLS upgrade failed for {}", info.host))?;

        let password = info.password.clone().unwrap_or_default();
        secure
            .login(info.username.as_str(), password.as_str())
            .await
            .with_context(|| format!("FTPS login failed for {}", info.username))?;

        Ok(secure)
    }

    pub async fn connect(&mut self, variant: FtpVariant, info: ConnectionInfo) -> Result<()> {
        match variant {
            FtpVariant::Ftp => {
                let mut stream = Self::login_stream(&info).await?;
                stream.quit().await.ok();
                self.connected = true;
                self.info = Some(info);
                Ok(())
            }
            FtpVariant::Ftps => {
                let mut stream = Self::login_secure_stream(&info).await?;
                stream.quit().await.ok();
                self.connected = true;
                self.info = Some(info);
                Ok(())
            }
        }
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        self.info = None;
        Ok(())
    }

    pub async fn list_dir(&self, variant: FtpVariant, path: &str) -> Result<Vec<FileEntry>> {
        let info = self.info.as_ref().context("not connected")?;
        match variant {
            FtpVariant::Ftp => {
                let mut stream = Self::login_stream(info).await?;
                let entries = stream
                    .nlst(Some(path))
                    .await
                    .with_context(|| format!("FTP nlst failed for path: {path}"))?;
                stream.quit().await.ok();

                Ok(Self::map_nlst_entries(entries))
            }
            FtpVariant::Ftps => {
                let mut stream = Self::login_secure_stream(info).await?;
                let entries = stream
                    .nlst(Some(path))
                    .await
                    .with_context(|| format!("FTPS nlst failed for path: {path}"))?;
                stream.quit().await.ok();

                Ok(Self::map_nlst_entries(entries))
            }
        }
    }

    pub async fn upload(&self, variant: FtpVariant, job: &TransferJob) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?;
        match variant {
            FtpVariant::Ftp => {
                let mut stream = Self::login_stream(info).await?;
                let remote_name = Self::remote_name_from_job(job);

                let mut local_file = tokio::fs::File::open(&job.local_path)
                    .await
                    .with_context(|| format!("FTP upload open failed: {}", job.local_path))?;

                stream
                    .put(&remote_name, &mut local_file)
                    .await
                    .with_context(|| format!("FTP upload failed to {}", job.remote_path))?;

                stream.quit().await.ok();
                Ok(())
            }
            FtpVariant::Ftps => {
                let mut stream = Self::login_secure_stream(info).await?;
                let remote_name = Self::remote_name_from_job(job);

                let bytes = tokio::fs::read(&job.local_path)
                    .await
                    .with_context(|| format!("FTPS upload read failed: {}", job.local_path))?;
                let mut reader = Cursor::new(bytes);

                stream
                    .put(&remote_name, &mut reader)
                    .await
                    .with_context(|| format!("FTPS upload failed to {}", job.remote_path))?;

                stream.quit().await.ok();
                Ok(())
            }
        }
    }

    pub async fn download(&self, variant: FtpVariant, job: &TransferJob) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?;
        match variant {
            FtpVariant::Ftp => {
                let mut stream = Self::login_stream(info).await?;
                let remote_name = Self::remote_name_from_job(job);

                let bytes = stream
                    .simple_retr(&remote_name)
                    .await
                    .with_context(|| format!("FTP download failed from {}", job.remote_path))?
                    .into_inner();

                Self::write_download_bytes(job, bytes).await?;
                stream.quit().await.ok();
                Ok(())
            }
            FtpVariant::Ftps => {
                let mut stream = Self::login_secure_stream(info).await?;
                let remote_name = Self::remote_name_from_job(job);

                let bytes = stream
                    .retr(
                        &remote_name,
                        |mut reader: BufReader<async_ftp::DataStream>| async move {
                            let mut buffer = Vec::new();
                            reader
                                .read_to_end(&mut buffer)
                                .await
                                .map_err(async_ftp::FtpError::ConnectionError)?;
                            Ok::<Vec<u8>, anyhow::Error>(buffer)
                        },
                    )
                    .await
                    .with_context(|| format!("FTPS download failed from {}", job.remote_path))?;

                Self::write_download_bytes(job, bytes).await?;
                stream.quit().await.ok();
                Ok(())
            }
        }
    }

    fn map_nlst_entries(entries: Vec<String>) -> Vec<FileEntry> {
        entries
            .into_iter()
            .map(|p| FileEntry {
                name: p.rsplit('/').next().unwrap_or(&p).to_string(),
                path: p,
                kind: EntryKind::Other,
                size: 0,
                modified: None,
                permissions: None,
            })
            .collect()
    }

    fn remote_name_from_job(job: &TransferJob) -> String {
        let remote_path = std::path::Path::new(&job.remote_path);
        remote_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| job.remote_path.clone())
    }

    async fn write_download_bytes(job: &TransferJob, bytes: Vec<u8>) -> Result<()> {
        if let Some(parent) = std::path::Path::new(&job.local_path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Cannot create local parent dir: {}", parent.display()))?;
        }

        tokio::fs::write(&job.local_path, &bytes)
            .await
            .with_context(|| format!("Cannot write local file: {}", job.local_path))?;

        Ok(())
    }
}
