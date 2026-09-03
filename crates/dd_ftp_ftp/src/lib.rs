use std::convert::TryFrom;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use dd_ftp_core::{ConnectionInfo, EntryKind, FileEntry, TransferJob};
use tokio::io::{AsyncReadExt, BufReader};
use tokio_rustls::rustls::{client::ServerName, ClientConfig, OwnedTrustAnchor, RootCertStore};

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

fn parse_list_entries(base_path: &str, lines: Vec<String>) -> Vec<FileEntry> {
    lines
        .into_iter()
        .filter_map(|line| parse_list_line(base_path, &line))
        .collect()
}

fn parse_list_line(base_path: &str, line: &str) -> Option<FileEntry> {
    let line = line.trim();
    if line.is_empty() {
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
