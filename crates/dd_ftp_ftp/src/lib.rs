use std::convert::TryFrom;

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

pub struct UnifiedFtpSession {
    stream: Option<async_ftp::FtpStream>,
    info: Option<ConnectionInfo>,
}

impl std::fmt::Debug for UnifiedFtpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnifiedFtpSession")
            .field("info", &self.info)
            .finish()
    }
}

impl Default for UnifiedFtpSession {
    fn default() -> Self {
        Self::new()
    }
}

impl UnifiedFtpSession {
    pub fn new() -> Self {
        Self {
            stream: None,
            info: None,
        }
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
        let mut stream = match variant {
            FtpVariant::Ftp => Self::login_stream(&info).await?,
            FtpVariant::Ftps => Self::login_secure_stream(&info).await?,
        };

        let path = info.initial_path.trim();
        if !path.is_empty() && path != "/" {
            stream.cwd(path).await.ok();
        }

        self.stream = Some(stream);
        self.info = Some(info);
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            stream.quit().await.ok();
        }
        self.info = None;
        Ok(())
    }

    pub async fn list_dir(&mut self, _variant: FtpVariant, path: &str) -> Result<Vec<FileEntry>> {
        let stream = self.stream.as_mut().context("not connected")?;

        stream.cwd(path).await.with_context(|| format!("FTP cwd failed: {path}"))?;

        let entries = stream
            .list(None)
            .await
            .with_context(|| format!("FTP list failed for path: {path}"))?;

        Ok(Self::parse_list_entries(entries))
    }

    fn parse_list_entries(lines: Vec<String>) -> Vec<FileEntry> {
        lines
            .into_iter()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let kind = if line.starts_with('d') {
                    EntryKind::Directory
                } else if line.starts_with('l') {
                    EntryKind::Symlink
                } else {
                    EntryKind::File
                };
                let name = Self::extract_filename_from_list_line(line);
                Some(FileEntry {
                    name,
                    path: line.to_string(),
                    kind,
                    size: 0,
                    modified: None,
                    permissions: None,
                })
            })
            .collect()
    }

    fn extract_filename_from_list_line(line: &str) -> String {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 9 {
            parts[8..].join(" ")
        } else if parts.len() >= 8 {
            parts[7..].join(" ")
        } else {
            line.to_string()
        }
    }

    pub async fn upload(&mut self, _variant: FtpVariant, job: &TransferJob) -> Result<()> {
        let stream = self.stream.as_mut().context("not connected")?;

        let remote_path = std::path::Path::new(&job.remote_path);
        if let Some(parent) = remote_path.parent() {
            if parent.as_os_str() != "" {
                stream.cwd(parent.to_string_lossy().as_ref()).await.ok();
            }
        }

        let remote_name = Self::remote_name_from_job(job);
        let mut local_file = tokio::fs::File::open(&job.local_path)
            .await
            .with_context(|| format!("FTP upload open failed: {}", job.local_path))?;

        stream
            .put(&remote_name, &mut local_file)
            .await
            .with_context(|| format!("FTP upload failed to {}", job.remote_path))?;

        Ok(())
    }

    pub async fn download(&mut self, variant: FtpVariant, job: &TransferJob) -> Result<()> {
        let stream = self.stream.as_mut().context("not connected")?;

        let remote_path = std::path::Path::new(&job.remote_path);
        if let Some(parent) = remote_path.parent() {
            if parent.as_os_str() != "" {
                stream.cwd(parent.to_string_lossy().as_ref()).await.ok();
            }
        }

        let remote_name = Self::remote_name_from_job(job);
        let bytes = match variant {
            FtpVariant::Ftp => stream
                .simple_retr(&remote_name)
                .await
                .with_context(|| format!("FTP download failed from {}", job.remote_path))?
                .into_inner(),
            FtpVariant::Ftps => stream
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
                .with_context(|| format!("FTPS download failed from {}", job.remote_path))?,
        };

        Self::write_download_bytes(job, bytes).await?;
        Ok(())
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
