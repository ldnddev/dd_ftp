use std::{
    fs::File,
    io,
    net::TcpStream,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, RemoteSession, TransferJob};
use ssh2::Session;

#[derive(Default)]
pub struct SftpSession {
    connected: bool,
    info: Option<ConnectionInfo>,
}

impl SftpSession {
    fn open_authenticated_session(info: &ConnectionInfo) -> Result<Session> {
        let tcp = TcpStream::connect((info.host.as_str(), info.port))
            .with_context(|| format!("tcp connect failed: {}:{}", info.host, info.port))?;

        let mut session = Session::new().context("failed to create SSH session")?;
        session.set_tcp_stream(tcp);
        session.handshake().context("ssh handshake failed")?;

        if let Some(key_path) = info.private_key.as_deref() {
            session
                .userauth_pubkey_file(
                    info.username.as_str(),
                    None,
                    Path::new(key_path),
                    info.password.as_deref(),
                )
                .with_context(|| format!("public key auth failed for user {}", info.username))?;
        } else if let Some(password) = info.password.as_deref() {
            session
                .userauth_password(info.username.as_str(), password)
                .with_context(|| format!("password auth failed for user {}", info.username))?;
        } else {
            let mut agent = session.agent().context("failed to open ssh-agent")?;
            agent.connect().context("failed to connect to ssh-agent")?;
            agent
                .list_identities()
                .context("failed to list ssh-agent identities")?;
            let identities = agent.identities().context("failed to read ssh-agent identities")?;

            let mut authed = false;
            for identity in identities {
                if agent.userauth(info.username.as_str(), &identity).is_ok() {
                    authed = true;
                    break;
                }
            }

            if !authed {
                bail!(
                    "ssh-agent auth failed for user {} (set password or private_key)",
                    info.username
                );
            }
        }

        if !session.authenticated() {
            bail!("authentication failed for {}", info.username);
        }

        Ok(session)
    }

    fn map_kind(perm: Option<u32>) -> EntryKind {
        match perm.map(|p| p & 0o170000) {
            Some(0o040000) => EntryKind::Directory,
            Some(0o100000) => EntryKind::File,
            Some(0o120000) => EntryKind::Symlink,
            _ => EntryKind::Other,
        }
    }

    fn list_dir_sync(info: &ConnectionInfo, path: &str) -> Result<Vec<FileEntry>> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session.sftp().context("failed to initialize sftp subsystem")?;

        let mut out = Vec::new();
        for (full_path, stat) in sftp
            .readdir(Path::new(path))
            .with_context(|| format!("failed reading remote path: {path}"))?
        {
            let Some(name) = full_path.file_name().map(|s| s.to_string_lossy().to_string()) else {
                continue;
            };

            if name == "." || name == ".." {
                continue;
            }

            let modified = stat
                .mtime
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts as i64, 0));

            out.push(FileEntry {
                name,
                path: full_path.to_string_lossy().to_string(),
                kind: Self::map_kind(stat.perm),
                size: stat.size.unwrap_or(0),
                modified,
                permissions: stat.perm.map(|p| format!("{:o}", p & 0o7777)),
            });
        }

        out.sort_by(|a, b| {
            // Directories first, then name.
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(out)
    }

    fn upload_sync(info: &ConnectionInfo, job: &TransferJob) -> Result<()> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session.sftp().context("failed to initialize sftp subsystem")?;

        let mut local_file = File::open(&job.local_path)
            .with_context(|| format!("cannot open local file: {}", job.local_path))?;

        let remote_path = Path::new(&job.remote_path);
        let mut remote_file = sftp
            .create(remote_path)
            .with_context(|| format!("cannot create remote file: {}", job.remote_path))?;

        io::copy(&mut local_file, &mut remote_file)
            .with_context(|| format!("upload failed to {}", job.remote_path))?;

        Ok(())
    }

    fn download_sync(info: &ConnectionInfo, job: &TransferJob) -> Result<()> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session.sftp().context("failed to initialize sftp subsystem")?;

        let remote_path = Path::new(&job.remote_path);
        let mut remote_file = sftp
            .open(remote_path)
            .with_context(|| format!("cannot open remote file: {}", job.remote_path))?;

        let local_path = PathBuf::from(&job.local_path);
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create local parent dir: {}", parent.display()))?;
        }

        let mut local_file = File::create(&local_path)
            .with_context(|| format!("cannot create local file: {}", local_path.display()))?;

        io::copy(&mut remote_file, &mut local_file)
            .with_context(|| format!("download failed from {}", job.remote_path))?;

        Ok(())
    }
}

#[async_trait]
impl RemoteSession for SftpSession {
    async fn connect(&mut self, info: ConnectionInfo) -> Result<()> {
        let probe_info = info.clone();

        tokio::task::spawn_blocking(move || Self::open_authenticated_session(&probe_info))
            .await
            .map_err(|e| anyhow!("join error during connect: {e}"))??;

        self.connected = true;
        self.info = Some(info);

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        self.info = None;
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || Self::list_dir_sync(&info, &path))
            .await
            .map_err(|e| anyhow!("join error during list_dir: {e}"))?
    }

    async fn upload(&self, job: &TransferJob) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let job = job.clone();

        tokio::task::spawn_blocking(move || Self::upload_sync(&info, &job))
            .await
            .map_err(|e| anyhow!("join error during upload: {e}"))?
    }

    async fn download(&self, job: &TransferJob) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let job = job.clone();

        tokio::task::spawn_blocking(move || Self::download_sync(&info, &job))
            .await
            .map_err(|e| anyhow!("join error during download: {e}"))?
    }
}

