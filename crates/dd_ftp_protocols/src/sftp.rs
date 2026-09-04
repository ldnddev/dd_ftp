use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, RemoteSession, TransferJob};
use ssh2::{CheckResult, HashType, HostKeyType, KnownHostFileKind, Session};
use uuid::Uuid;

const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Display-only host-key prompt payload (fingerprint is unpadded SHA256 base64).
pub struct HostKeyOffer {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub changed: bool,
}

#[derive(Default)]
pub struct SftpSession {
    connected: bool,
    info: Option<ConnectionInfo>,
}

/// TCP connect with an explicit timeout after DNS resolution.
pub fn tcp_connect(host: &str, port: u16, d: Duration) -> Result<TcpStream> {
    let addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("tcp resolve failed: {host}:{port}"))?;
    let mut last_err: Option<anyhow::Error> = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, d) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_err = Some(err.into()),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("tcp connect failed: {host}:{port}")))
}

pub fn known_hosts_path() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    PathBuf::from(home.unwrap_or_else(|| ".".into())).join(".ssh/known_hosts")
}

pub fn host_spec(host: &str, port: u16) -> String {
    if port != 22 {
        format!("[{host}]:{port}")
    } else {
        host.to_string()
    }
}

/// SHA256 fingerprint matching `ssh-keygen -lf` (no padding).
pub fn sha256_fingerprint(hash: &[u8]) -> String {
    format!("SHA256:{}", base64_nopad(hash))
}

fn base64_nopad(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut chunks = data.chunks_exact(3);
    for chunk in chunks.by_ref() {
        let n = u32::from(chunk[0]) << 16 | u32::from(chunk[1]) << 8 | u32::from(chunk[2]);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
    }
    let rem = chunks.remainder();
    if rem.len() == 1 {
        let n = u32::from(rem[0]) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
    } else if rem.len() == 2 {
        let n = u32::from(rem[0]) << 16 | u32::from(rem[1]) << 8;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
    }
    out
}

/// Load `known_hosts` (missing file = NotFound) and `check_port`.
pub fn check_host_key(file: &Path, host: &str, port: u16, key: &[u8]) -> CheckResult {
    let Ok(sess) = Session::new() else {
        return CheckResult::Failure;
    };
    let Ok(mut known) = sess.known_hosts() else {
        return CheckResult::Failure;
    };
    if file.exists() {
        if !file.is_file() {
            return CheckResult::Failure;
        }
        if known.read_file(file, KnownHostFileKind::OpenSSH).is_err() {
            return CheckResult::Failure;
        }
    }
    known.check_port(host, port, key)
}

/// Append one OpenSSH known_hosts line. Never `write_file`.
pub fn append_known_host(
    file: &Path,
    host: &str,
    port: u16,
    key: &[u8],
    key_type: HostKeyType,
) -> Result<()> {
    let sess = Session::new().context("failed to create SSH session for known_hosts")?;
    let mut known = sess
        .known_hosts()
        .context("failed to create known_hosts set")?;
    let spec = host_spec(host, port);
    known
        .add(&spec, key, "", key_type.into())
        .with_context(|| format!("failed to add host key for {spec}"))?;
    let hosts = known.hosts().context("failed to read added host key")?;
    let added = hosts.last().context("known_hosts add produced no entry")?;
    let line = known
        .write_string(added, KnownHostFileKind::OpenSSH)
        .context("failed to format known_hosts line")?;

    if let Some(parent) = file.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
    }
    if !file.exists() {
        create_known_hosts_0600(file)?;
    }
    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .with_context(|| format!("cannot append {}", file.display()))?;
    out.write_all(line.as_bytes())?;
    if !line.ends_with('\n') {
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn create_known_hosts_0600(file: &Path) -> Result<()> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(file)
        .with_context(|| format!("cannot create {}", file.display()))?;
    Ok(())
}

fn verify_host_key(
    session: &Session,
    info: &ConnectionInfo,
    on_challenge: &mut Option<Box<dyn FnOnce(HostKeyOffer) -> bool + Send>>,
) -> Result<()> {
    let (key, key_type) = session
        .host_key()
        .context("ssh handshake produced no host key")?;
    let key = key.to_vec();
    let hash = session
        .host_key_hash(HashType::Sha256)
        .context("ssh handshake produced no host key hash")?;
    let full_fp = sha256_fingerprint(hash);
    let fingerprint = full_fp
        .strip_prefix("SHA256:")
        .unwrap_or(full_fp.as_str())
        .to_string();

    let path = known_hosts_path();
    let outcome = check_host_key(&path, &info.host, info.port, &key);
    match outcome {
        CheckResult::Match => Ok(()),
        CheckResult::Failure => {
            bail!("host key check failed for {}:{}", info.host, info.port)
        }
        CheckResult::NotFound | CheckResult::Mismatch => {
            let changed = matches!(outcome, CheckResult::Mismatch);
            let offer = HostKeyOffer {
                host: info.host.clone(),
                port: info.port,
                fingerprint,
                changed,
            };
            let accepted = if let Some(cb) = on_challenge.take() {
                cb(offer)
            } else {
                false
            };
            if !accepted {
                bail!("host key rejected for {}:{}", info.host, info.port);
            }
            append_known_host(&path, &info.host, info.port, &key, key_type)?;
            Ok(())
        }
    }
}

impl SftpSession {
    /// Info-only handle. PR 4 reconnects per call; PR 6 fills a persistent inner.
    pub fn with_info(info: ConnectionInfo) -> Self {
        Self {
            connected: true,
            info: Some(info),
        }
    }

    fn open_authenticated_session(info: &ConnectionInfo) -> Result<Session> {
        Self::open_authenticated_session_with_handler(info, None)
    }

    fn open_authenticated_session_with_handler(
        info: &ConnectionInfo,
        mut on_challenge: Option<Box<dyn FnOnce(HostKeyOffer) -> bool + Send>>,
    ) -> Result<Session> {
        let tcp = tcp_connect(info.host.as_str(), info.port, TCP_CONNECT_TIMEOUT)
            .with_context(|| format!("tcp connect failed: {}:{}", info.host, info.port))?;

        let mut session = Session::new().context("failed to create SSH session")?;
        session.set_tcp_stream(tcp);
        session.handshake().context("ssh handshake failed")?;

        verify_host_key(&session, info, &mut on_challenge)?;

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
            let identities = agent
                .identities()
                .context("failed to read ssh-agent identities")?;

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
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;

        let mut out = Vec::new();
        for (full_path, stat) in sftp
            .readdir(Path::new(path))
            .with_context(|| format!("failed reading remote path: {path}"))?
        {
            let Some(name) = full_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
            else {
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
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(out)
    }

    fn rename_sync(info: &ConnectionInfo, from: &str, to: &str) -> Result<()> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;
        sftp.rename(Path::new(from), Path::new(to), None)
            .with_context(|| format!("failed to rename remote path: {from} -> {to}"))
    }

    fn remove_file_sync(info: &ConnectionInfo, path: &str) -> Result<()> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;
        sftp.unlink(Path::new(path))
            .with_context(|| format!("failed to delete remote file: {path}"))
    }

    fn remove_dir_sync(info: &ConnectionInfo, path: &str) -> Result<()> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;
        sftp.rmdir(Path::new(path))
            .with_context(|| format!("failed to remove remote directory: {path}"))
    }

    fn create_dir_sync(info: &ConnectionInfo, path: &str) -> Result<()> {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;
        sftp.mkdir(Path::new(path), 0o755)
            .with_context(|| format!("failed to create remote directory: {path}"))
    }

    fn upload_sync<F>(
        info: &ConnectionInfo,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(u64, Option<u64>) + Send + 'static,
    {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;

        let mut local_file = File::open(&job.local_path)
            .with_context(|| format!("cannot open local file: {}", job.local_path))?;
        let size = local_file.metadata().ok().map(|m| m.len());

        let remote_path = Path::new(&job.remote_path);
        let mut remote_file = sftp
            .create(remote_path)
            .with_context(|| format!("cannot create remote file: {}", job.remote_path))?;

        let mut transferred = 0_u64;
        let mut buf = [0_u8; 64 * 1024];

        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }

            let read = local_file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            remote_file.write_all(&buf[..read])?;
            transferred = transferred.saturating_add(read as u64);
            on_progress(transferred, size);
        }

        Ok(())
    }

    fn download_sync<F>(
        info: &ConnectionInfo,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(u64, Option<u64>) + Send + 'static,
    {
        let session = Self::open_authenticated_session(info)?;
        let sftp = session
            .sftp()
            .context("failed to initialize sftp subsystem")?;

        let remote_path = Path::new(&job.remote_path);
        let mut remote_file = sftp
            .open(remote_path)
            .with_context(|| format!("cannot open remote file: {}", job.remote_path))?;
        let size = sftp.stat(remote_path).ok().and_then(|s| s.size);

        let local_path = PathBuf::from(&job.local_path);
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create local parent dir: {}", parent.display()))?;
        }

        let mut local_file = File::create(&local_path)
            .with_context(|| format!("cannot create local file: {}", local_path.display()))?;

        let mut transferred = 0_u64;
        let mut buf = [0_u8; 64 * 1024];

        loop {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }

            let read = remote_file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            local_file.write_all(&buf[..read])?;
            transferred = transferred.saturating_add(read as u64);
            on_progress(transferred, size);
        }

        Ok(())
    }

    pub async fn upload_with_progress<F>(
        &self,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        on_progress: F,
    ) -> Result<()>
    where
        F: Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static,
    {
        let info = self.info.as_ref().context("not connected")?.clone();
        let job = job.clone();
        let on_progress = Arc::new(on_progress);

        tokio::task::spawn_blocking(move || {
            let on_progress_closure = {
                let on_progress = Arc::clone(&on_progress);
                let job_id = job.id;
                move |transferred: u64, size: Option<u64>| {
                    on_progress(job_id, transferred, size);
                }
            };
            Self::upload_sync(&info, &job, cancel, on_progress_closure)
        })
        .await
        .map_err(|e| anyhow!("join error during upload_with_progress: {e}"))?
    }

    pub async fn download_with_progress<F>(
        &self,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        on_progress: F,
    ) -> Result<()>
    where
        F: Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static,
    {
        let info = self.info.as_ref().context("not connected")?.clone();
        let job = job.clone();
        let on_progress = Arc::new(on_progress);

        tokio::task::spawn_blocking(move || {
            let on_progress_closure = {
                let on_progress = Arc::clone(&on_progress);
                let job_id = job.id;
                move |transferred: u64, size: Option<u64>| {
                    on_progress(job_id, transferred, size);
                }
            };
            Self::download_sync(&info, &job, cancel, on_progress_closure)
        })
        .await
        .map_err(|e| anyhow!("join error during download_with_progress: {e}"))?
    }
}

impl SftpSession {
    pub async fn connect_with_host_key_handler<F>(
        &mut self,
        info: ConnectionInfo,
        handler: F,
    ) -> Result<()>
    where
        F: FnOnce(HostKeyOffer) -> bool + Send + 'static,
    {
        let probe_info = info.clone();
        tokio::task::spawn_blocking(move || {
            Self::open_authenticated_session_with_handler(&probe_info, Some(Box::new(handler)))
        })
        .await
        .map_err(|e| anyhow!("join error during connect: {e}"))??;

        self.connected = true;
        self.info = Some(info);
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
        self.upload_with_progress(job, Arc::new(AtomicBool::new(false)), |_id, _tx, _size| {})
            .await
    }

    async fn download(&self, job: &TransferJob) -> Result<()> {
        self.download_with_progress(job, Arc::new(AtomicBool::new(false)), |_id, _tx, _size| {})
            .await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let from = from.to_string();
        let to = to.to_string();

        tokio::task::spawn_blocking(move || Self::rename_sync(&info, &from, &to))
            .await
            .map_err(|e| anyhow!("join error during rename: {e}"))?
    }

    async fn remove_file(&self, path: &str) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || Self::remove_file_sync(&info, &path))
            .await
            .map_err(|e| anyhow!("join error during remove_file: {e}"))?
    }

    async fn remove_dir(&self, path: &str) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || Self::remove_dir_sync(&info, &path))
            .await
            .map_err(|e| anyhow!("join error during remove_dir: {e}"))?
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let info = self.info.as_ref().context("not connected")?.clone();
        let path = path.to_string();

        tokio::task::spawn_blocking(move || Self::create_dir_sync(&info, &path))
            .await
            .map_err(|e| anyhow!("join error during create_dir: {e}"))?
    }
}

#[cfg(test)]
mod host_key_tests {
    use super::*;
    use ssh2::CheckResult;
    use std::time::Instant;

    fn temp_known_hosts(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dd_ftp_kh_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn ed25519_blob(raw32: &[u8; 32]) -> Vec<u8> {
        let mut v = Vec::new();
        let t = b"ssh-ed25519";
        v.extend_from_slice(&(t.len() as u32).to_be_bytes());
        v.extend_from_slice(t);
        v.extend_from_slice(&(32u32).to_be_bytes());
        v.extend_from_slice(raw32);
        v
    }

    fn check_kind(result: CheckResult) -> &'static str {
        match result {
            CheckResult::Match => "match",
            CheckResult::Mismatch => "mismatch",
            CheckResult::NotFound => "notfound",
            CheckResult::Failure => "failure",
        }
    }

    #[test]
    fn check_port_match_notfound_mismatch_failure() {
        let path = temp_known_hosts("check");
        let key = ed25519_blob(&[1u8; 32]);
        let other = ed25519_blob(&[2u8; 32]);

        assert_eq!(
            check_kind(check_host_key(&path, "example.com", 22, &key)),
            "notfound",
            "missing file is unknown, not accept"
        );

        append_known_host(&path, "example.com", 22, &key, HostKeyType::Ed25519)
            .expect("append port 22");
        assert_eq!(
            check_kind(check_host_key(&path, "example.com", 22, &key)),
            "match"
        );
        assert_eq!(
            check_kind(check_host_key(&path, "example.com", 22, &other)),
            "mismatch"
        );

        let path_alt = temp_known_hosts("port");
        append_known_host(&path_alt, "example.com", 2222, &key, HostKeyType::Ed25519)
            .expect("append port 2222");
        assert_eq!(
            check_kind(check_host_key(&path_alt, "example.com", 22, &key)),
            "notfound",
            "[host]:2222 must not match port 22"
        );
        assert_eq!(
            check_kind(check_host_key(&path_alt, "example.com", 2222, &key)),
            "match"
        );

        let fail_dir = std::env::temp_dir().join(format!(
            "dd_ftp_kh_dir_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&fail_dir).expect("dir");
        assert_eq!(
            check_kind(check_host_key(&fail_dir, "example.com", 22, &key)),
            "failure"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&path_alt);
        let _ = std::fs::remove_dir_all(&fail_dir);
    }

    #[test]
    fn append_does_not_drop_a_pre_existing_comment_line() {
        let path = temp_known_hosts("comment");
        std::fs::write(&path, "# keep this comment\n").expect("seed comment");
        let key = ed25519_blob(&[3u8; 32]);
        append_known_host(&path, "example.com", 22, &key, HostKeyType::Ed25519).expect("append");
        let contents = std::fs::read_to_string(&path).expect("read");
        assert!(
            contents.contains("# keep this comment"),
            "append must not rewrite/drop comments, got {contents:?}"
        );
        assert!(contents.contains("example.com"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tcp_connect_blackhole_returns_quickly() {
        let start = Instant::now();
        let _ = tcp_connect("192.0.2.1", 1, Duration::from_millis(200));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "connect_timeout should return within ~2s, took {:?}",
            start.elapsed()
        );
    }
}
