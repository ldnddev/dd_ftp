use std::{
    convert::TryFrom,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, TransferJob};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_rustls::rustls::{client::ServerName, ClientConfig, OwnedTrustAnchor, RootCertStore};
use uuid::Uuid;

const FTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const TRANSFER_CHUNK: usize = 64 * 1024;

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
        let mut stream = tokio::time::timeout(
            FTP_CONNECT_TIMEOUT,
            async_ftp::FtpStream::connect((info.host.as_str(), info.port)),
        )
        .await
        .with_context(|| format!("FTP connect timed out: {}:{}", info.host, info.port))?
        .with_context(|| format!("FTP connect failed: {}:{}", info.host, info.port))?;

        let password = info.password.clone().unwrap_or_default();
        stream
            .login(info.username.as_str(), password.as_str())
            .await
            .with_context(|| format!("FTP login failed for {}", info.username))?;

        Ok(stream)
    }

    async fn login_secure_stream(info: &ConnectionInfo) -> Result<async_ftp::FtpStream> {
        let stream = tokio::time::timeout(
            FTP_CONNECT_TIMEOUT,
            async_ftp::FtpStream::connect((info.host.as_str(), info.port)),
        )
        .await
        .with_context(|| format!("FTPS connect timed out: {}:{}", info.host, info.port))?
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

        stream
            .cwd(path)
            .await
            .with_context(|| format!("FTP cwd failed: {path}"))?;

        let entries = stream
            .list(None)
            .await
            .with_context(|| format!("FTP list failed for path: {path}"))?;

        Ok(parse_list_entries(path, entries))
    }

    pub async fn rename(&mut self, _variant: FtpVariant, from: &str, to: &str) -> Result<()> {
        let stream = self.stream.as_mut().context("not connected")?;
        stream
            .rename(from, to)
            .await
            .with_context(|| format!("FTP rename failed: {from} -> {to}"))
    }

    pub async fn remove_file(&mut self, _variant: FtpVariant, path: &str) -> Result<()> {
        let stream = self.stream.as_mut().context("not connected")?;
        stream
            .rm(path)
            .await
            .with_context(|| format!("FTP delete file failed: {path}"))
    }

    pub async fn remove_dir(&mut self, _variant: FtpVariant, path: &str) -> Result<()> {
        let stream = self.stream.as_mut().context("not connected")?;
        stream
            .rmdir(path)
            .await
            .with_context(|| format!("FTP remove directory failed: {path}"))
    }

    pub async fn create_dir(&mut self, _variant: FtpVariant, path: &str) -> Result<()> {
        let stream = self.stream.as_mut().context("not connected")?;
        stream
            .mkdir(path)
            .await
            .with_context(|| format!("FTP create directory failed: {path}"))
    }

    pub async fn upload(&mut self, _variant: FtpVariant, job: &TransferJob) -> Result<()> {
        self.upload_with_progress(job, Arc::new(AtomicBool::new(false)), |_id, _tx, _size| {})
            .await
    }

    pub async fn download(&mut self, _variant: FtpVariant, job: &TransferJob) -> Result<()> {
        self.download_with_progress(job, Arc::new(AtomicBool::new(false)), |_id, _tx, _size| {})
            .await
    }

    pub async fn upload_with_progress<F>(
        &mut self,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        on_progress: F,
    ) -> Result<()>
    where
        F: Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static,
    {
        let stream = self.stream.as_mut().context("not connected")?;
        cwd_to_remote_parent(stream, job).await;

        let remote_name = remote_name_from_job(job);
        let size = transfer_size(stream, job, &remote_name).await;
        let local_file = tokio::fs::File::open(&job.local_path)
            .await
            .with_context(|| format!("FTP upload open failed: {}", job.local_path))?;

        let mut reader = ProgressReader {
            inner: local_file,
            transferred: 0,
            size,
            job_id: job.id,
            cancel,
            on_progress: Arc::new(on_progress),
        };

        match stream.put(&remote_name, &mut reader).await {
            Ok(()) => Ok(()),
            Err(_) if reader.cancel.load(Ordering::Relaxed) => bail!("cancelled"),
            Err(err) => {
                Err(err).with_context(|| format!("FTP upload failed to {}", job.remote_path))
            }
        }
    }

    pub async fn download_with_progress<F>(
        &mut self,
        job: &TransferJob,
        cancel: Arc<AtomicBool>,
        on_progress: F,
    ) -> Result<()>
    where
        F: Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static,
    {
        let stream = self.stream.as_mut().context("not connected")?;
        cwd_to_remote_parent(stream, job).await;

        let remote_name = remote_name_from_job(job);
        let size = transfer_size(stream, job, &remote_name).await;
        let local_path = job.local_path.clone();
        let job_id = job.id;
        let remote_path = job.remote_path.clone();
        let on_progress = Arc::new(on_progress);

        stream
            .retr(&remote_name, move |mut reader| {
                let cancel = Arc::clone(&cancel);
                let on_progress = Arc::clone(&on_progress);
                let local_path = local_path.clone();
                async move {
                    if let Some(parent) = std::path::Path::new(&local_path).parent() {
                        tokio::fs::create_dir_all(parent).await.with_context(|| {
                            format!("Cannot create local parent dir: {}", parent.display())
                        })?;
                    }
                    let mut file = tokio::fs::File::create(&local_path)
                        .await
                        .with_context(|| format!("Cannot write local file: {local_path}"))?;
                    copy_with_progress(
                        &mut reader,
                        &mut file,
                        &cancel,
                        size,
                        job_id,
                        on_progress.as_ref(),
                    )
                    .await?;
                    Ok::<(), anyhow::Error>(())
                }
            })
            .await
            .with_context(|| format!("FTP download failed from {remote_path}"))
    }
}

async fn cwd_to_remote_parent(stream: &mut async_ftp::FtpStream, job: &TransferJob) {
    let remote_path = std::path::Path::new(&job.remote_path);
    if let Some(parent) = remote_path.parent() {
        if parent.as_os_str() != "" {
            stream.cwd(parent.to_string_lossy().as_ref()).await.ok();
        }
    }
}

fn remote_name_from_job(job: &TransferJob) -> String {
    let remote_path = std::path::Path::new(&job.remote_path);
    remote_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| job.remote_path.clone())
}

async fn transfer_size(
    stream: &mut async_ftp::FtpStream,
    job: &TransferJob,
    remote_name: &str,
) -> Option<u64> {
    if let Some(size) = job.size_bytes {
        return Some(size);
    }
    stream
        .size(remote_name)
        .await
        .ok()
        .flatten()
        .map(|n| n as u64)
}

struct ProgressReader<R, F> {
    inner: R,
    transferred: u64,
    size: Option<u64>,
    job_id: Uuid,
    cancel: Arc<AtomicBool>,
    on_progress: Arc<F>,
}

impl<R: AsyncRead + Unpin, F: Fn(Uuid, u64, Option<u64>)> AsyncRead for ProgressReader<R, F> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.cancel.load(Ordering::Relaxed) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")));
        }
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let n = buf.filled().len().saturating_sub(before);
                if n > 0 {
                    self.transferred = self.transferred.saturating_add(n as u64);
                    let transferred = self.transferred;
                    let size = self.size;
                    let job_id = self.job_id;
                    (self.on_progress)(job_id, transferred, size);
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

async fn copy_with_progress<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    cancel: &AtomicBool,
    size: Option<u64>,
    job_id: Uuid,
    on_progress: &F,
) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: Fn(Uuid, u64, Option<u64>) + Send + Sync,
{
    let mut buf = vec![0_u8; TRANSFER_CHUNK];
    let mut transferred = 0_u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        transferred = transferred.saturating_add(n as u64);
        on_progress(job_id, transferred, size);
    }
    Ok(transferred)
}

fn parse_list_entries(base_path: &str, lines: Vec<String>) -> Vec<FileEntry> {
    lines
        .into_iter()
        .filter_map(|line| parse_list_line(base_path, &line))
        .collect()
}

fn parse_list_line(base_path: &str, line: &str) -> Option<FileEntry> {
    let line = line.trim();
    if line.is_empty() || is_total_header(line) {
        return None;
    }

    let entry = parse_unix_list_line(base_path, line)
        .unwrap_or_else(|| parse_unparsed_list_line(base_path, line));
    if entry.name == "." || entry.name == ".." {
        return None;
    }
    Some(entry)
}

fn parse_unix_list_line(base_path: &str, line: &str) -> Option<FileEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9 || !is_unix_perm(parts[0]) {
        return None;
    }
    let size: u64 = parts[4].parse().ok()?;
    let modified = parse_ls_date(parts[5], parts[6], parts[7]);
    let mut name = parts[8..].join(" ");
    if let Some((before, _)) = name.split_once(" -> ") {
        if !before.is_empty() {
            name = before.to_string();
        }
    }
    if name.is_empty() {
        return None;
    }
    let kind = match parts[0].as_bytes().first() {
        Some(b'd') => EntryKind::Directory,
        Some(b'l') => EntryKind::Symlink,
        _ => EntryKind::File,
    };
    Some(FileEntry {
        name: name.clone(),
        path: join_ftp_path(base_path, &name),
        kind,
        size,
        modified,
        permissions: unix_mode_octal(parts[0]),
    })
}

fn parse_unparsed_list_line(base_path: &str, line: &str) -> FileEntry {
    let name = extract_filename_from_list_line(line);
    let kind = if line.starts_with('d') {
        EntryKind::Directory
    } else if line.starts_with('l') {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    FileEntry {
        name: name.clone(),
        path: join_ftp_path(base_path, &name),
        kind,
        size: 0,
        modified: None,
        permissions: None,
    }
}

fn is_total_header(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(second) = parts.next() else {
        return false;
    };
    first.eq_ignore_ascii_case("total") && second.parse::<u64>().is_ok()
}

fn extract_filename_from_list_line(line: &str) -> String {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let name = if parts.len() >= 9 {
        parts[8..].join(" ")
    } else if parts.len() >= 8 {
        parts[7..].join(" ")
    } else {
        line.to_string()
    };
    match name.split_once(" -> ") {
        Some((before, _)) if !before.is_empty() => before.to_string(),
        _ => name,
    }
}

fn join_ftp_path(base: &str, name: &str) -> String {
    let base = if base.is_empty() { "/" } else { base };
    if base == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", base.trim_end_matches('/'))
    }
}

fn is_unix_perm(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10 && matches!(b[0], b'd' | b'-' | b'l' | b'b' | b'c' | b'p' | b's')
}

fn unix_mode_octal(perm: &str) -> Option<String> {
    let c: Vec<char> = perm.chars().collect();
    if c.len() < 10 {
        return None;
    }
    let mut mode = 0u32;
    if c[1] == 'r' {
        mode |= 0o400;
    }
    if c[2] == 'w' {
        mode |= 0o200;
    }
    match c[3] {
        'x' => mode |= 0o100,
        's' => mode |= 0o4100,
        'S' => mode |= 0o4000,
        _ => {}
    }
    if c[4] == 'r' {
        mode |= 0o040;
    }
    if c[5] == 'w' {
        mode |= 0o020;
    }
    match c[6] {
        'x' => mode |= 0o010,
        's' => mode |= 0o2010,
        'S' => mode |= 0o2000,
        _ => {}
    }
    if c[7] == 'r' {
        mode |= 0o004;
    }
    if c[8] == 'w' {
        mode |= 0o002;
    }
    match c[9] {
        'x' => mode |= 0o001,
        't' => mode |= 0o1001,
        'T' => mode |= 0o1000,
        _ => {}
    }
    Some(format!("{mode:o}"))
}

fn parse_ls_date(month: &str, day: &str, year_or_time: &str) -> Option<DateTime<Utc>> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month_num = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(month))? as u32 + 1;
    let day: u32 = day.parse().ok()?;
    if let Some((h, rest)) = year_or_time.split_once(':') {
        let hour: u32 = h.parse().ok()?;
        let minute: u32 = rest.split(':').next().and_then(|m| m.parse().ok())?;
        let now = Utc::now();
        let mut year = now.year();
        let dt = naive_utc(year, month_num, day, hour, minute)?;
        if dt > now {
            year -= 1;
            naive_utc(year, month_num, day, hour, minute)
        } else {
            Some(dt)
        }
    } else {
        let year: i32 = year_or_time.parse().ok()?;
        naive_utc(year, month_num, day, 0, 0)
    }
}

fn naive_utc(year: i32, month: u32, day: u32, hour: u32, min: u32) -> Option<DateTime<Utc>> {
    let naive = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, min, 0)?;
    Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

#[cfg(test)]
mod parse_list_tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn parse_list_entries_unix_file() {
        let entries = parse_list_entries(
            "/pub",
            vec!["-rw-r--r-- 1 user group 1234 Jan  2 12:00 file.bin".into()],
        );
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.name, "file.bin");
        assert_eq!(e.path, "/pub/file.bin");
        assert!(
            !e.path.contains("-rw-"),
            "path must be join(base, name), not the LIST line"
        );
        assert_eq!(e.size, 1234);
        assert_eq!(e.kind, EntryKind::File);
        let dt = e.modified.expect("unix LIST mtime");
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 2);
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 0);
        assert_eq!(e.permissions.as_deref(), Some("644"));
    }

    #[test]
    fn parse_list_entries_unix_dir() {
        let entries = parse_list_entries(
            "/pub",
            vec!["drwxr-xr-x 2 user group 4096 Jan  2  2025 dirname".into()],
        );
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.name, "dirname");
        assert_eq!(e.path, "/pub/dirname");
        assert_eq!(e.kind, EntryKind::Directory);
        assert_eq!(e.size, 4096);
        let dt = e.modified.expect("unix LIST mtime");
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 2);
        assert_eq!(e.permissions.as_deref(), Some("755"));
    }

    #[test]
    fn parse_list_entries_symlink_uses_name_before_arrow() {
        let entries = parse_list_entries(
            "/pub",
            vec!["lrwxrwxrwx 1 user group 8 Jan  2 12:00 linkname -> target".into()],
        );
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.name, "linkname");
        assert_eq!(e.path, "/pub/linkname");
        assert!(!e.name.contains("->"));
        assert_eq!(e.kind, EntryKind::Symlink);
    }

    #[test]
    fn parse_list_entries_garbage_line() {
        let line = "this is not a listing";
        let entries = parse_list_entries("/pub", vec![line.into()]);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.name, extract_filename_from_list_line(line));
        assert_eq!(e.path, join_ftp_path("/pub", &e.name));
        assert_ne!(e.path, line);
        assert_eq!(e.size, 0);
        assert!(e.modified.is_none());
        assert!(e.permissions.is_none());
    }

    #[test]
    fn parse_list_entries_skips_total_header() {
        let entries = parse_list_entries(
            "/pub",
            vec![
                "total 64".into(),
                "Total 128".into(),
                "-rw-r--r-- 1 user group 1 Jan  2 12:00 a.txt".into(),
            ],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.txt");
        assert!(entries
            .iter()
            .all(|e| !e.name.to_lowercase().starts_with("total")));
    }

    #[test]
    fn parse_list_entries_skips_dot_and_dotdot() {
        let entries = parse_list_entries(
            "/pub",
            vec![
                "drwxr-xr-x 2 user group 4096 Jan  2 12:00 .".into(),
                "drwxr-xr-x 2 user group 4096 Jan  2 12:00 ..".into(),
                "-rw-r--r-- 1 user group 1 Jan  2 12:00 a.txt".into(),
            ],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.txt");
        assert_eq!(entries[0].path, "/pub/a.txt");
    }

    #[test]
    fn parse_list_entries_dos_stays_name_only() {
        let entries = parse_list_entries(
            "/pub",
            vec!["01-02-26  12:00PM               1234 file.bin".into()],
        );
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.size, 0);
        assert!(e.modified.is_none());
        assert!(e.permissions.is_none());
        assert_eq!(e.path, join_ftp_path("/pub", &e.name));
        assert!(!e.path.contains("12:00PM") || e.name.contains("12:00PM"));
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex;

    struct CancelAfter<R> {
        inner: R,
        cancel: Arc<AtomicBool>,
        reads: u32,
    }

    impl<R: AsyncRead + Unpin> AsyncRead for CancelAfter<R> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            self.reads += 1;
            if self.reads > 1 {
                self.cancel.store(true, Ordering::Relaxed);
            }
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    #[tokio::test]
    async fn copy_with_progress_reports_monotonic_transferred() {
        let data = vec![7_u8; 200 * 1024];
        let mut reader = Cursor::new(data.clone());
        let mut writer = Vec::new();
        let cancel = AtomicBool::new(false);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_cb = Arc::clone(&seen);
        let n = copy_with_progress(
            &mut reader,
            &mut writer,
            &cancel,
            Some(data.len() as u64),
            Uuid::nil(),
            &move |_id, transferred, _size| {
                seen_cb.lock().unwrap().push(transferred);
            },
        )
        .await
        .expect("copy");
        assert_eq!(n, data.len() as u64);
        assert_eq!(writer.len(), data.len());
        let prog = seen.lock().unwrap();
        assert!(!prog.is_empty());
        assert!(
            prog.windows(2).all(|w| w[1] >= w[0]),
            "transferred must be monotonic: {prog:?}"
        );
        assert_eq!(*prog.last().unwrap(), data.len() as u64);
    }

    #[tokio::test]
    async fn copy_with_progress_cancel_mid_stream_errors_cancelled() {
        let data = vec![1_u8; 200 * 1024];
        let cancel = Arc::new(AtomicBool::new(false));
        let mut reader = CancelAfter {
            inner: Cursor::new(data),
            cancel: Arc::clone(&cancel),
            reads: 0,
        };
        let mut writer = Vec::new();
        let err = copy_with_progress(
            &mut reader,
            &mut writer,
            &cancel,
            Some(200 * 1024),
            Uuid::nil(),
            &|_id, _t, _s| {},
        )
        .await
        .expect_err("cancel mid-stream");
        assert!(
            err.to_string().to_lowercase().contains("cancelled"),
            "expected cancelled error, got {err}"
        );
        assert!(writer.len() < 200 * 1024);
    }
}
