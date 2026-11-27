//! Transfer workers for the window. The fake backend copies bytes in process.
//! A live backend uses its own SFTP or FTP sessions and reuses them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use dd_ftp_app::{OverwritePolicy, PendingFile};
use dd_ftp_core::{
    ConnectionInfo, EntryKind, FileEntry, Protocol, RemoteSession, TransferDirection, TransferJob,
    create_no_follow,
};
use dd_ftp_ftp::UnifiedFtpSession;
use dd_ftp_protocols::SftpSession;
use uuid::Uuid;

use crate::session::{self, Backend, FakeTree};

const CHUNK: usize = 64 * 1024;
const PROGRESS_MIN: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrainStep {
    Enqueue,
    Skip,
    Prompt,
    RenameAuto,
}

pub fn should_spawn(
    connected: bool,
    cancel_requested: bool,
    active: usize,
    max: usize,
    pending_len: usize,
) -> bool {
    connected && !cancel_requested && active < max && pending_len > 0
}

pub fn should_emit_progress(last: Option<Instant>, now: Instant, complete: bool) -> bool {
    if complete {
        return true;
    }
    match last {
        None => true,
        Some(then) => now.saturating_duration_since(then) >= PROGRESS_MIN,
    }
}

pub fn session_identity_eq(left: &ConnectionInfo, right: &ConnectionInfo) -> bool {
    left.protocol == right.protocol
        && left.host == right.host
        && left.port == right.port
        && left.username == right.username
        && left.private_key == right.private_key
}

pub fn resolve_conflict(dest_exists: bool, dest_newer: bool, policy: OverwritePolicy) -> DrainStep {
    if !dest_exists {
        return DrainStep::Enqueue;
    }
    match policy {
        OverwritePolicy::Ask => DrainStep::Prompt,
        OverwritePolicy::OverwriteAll => DrainStep::Enqueue,
        OverwritePolicy::SkipAll => DrainStep::Skip,
        OverwritePolicy::OverwriteNewer => {
            if dest_newer {
                DrainStep::Enqueue
            } else {
                DrainStep::Prompt
            }
        }
        OverwritePolicy::SkipNewer => {
            if dest_newer {
                DrainStep::Skip
            } else {
                DrainStep::Prompt
            }
        }
        OverwritePolicy::RenameNewer => {
            if dest_newer {
                DrainStep::RenameAuto
            } else {
                DrainStep::Prompt
            }
        }
    }
}

pub fn uniquify_filename(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && !ext.contains('/') => {
            (stem.to_string(), format!(".{ext}"))
        }
        _ => (name.to_string(), String::new()),
    };
    for index in 1..10_000 {
        let candidate = format!("{stem}-{index}{ext}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("{stem}-copy{ext}")
}

pub fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn replace_base(path: &str, name: &str) -> String {
    match path.rfind(['/', '\\']) {
        Some(index) => format!("{}{name}", &path[..=index]),
        None => name.to_string(),
    }
}

pub fn safe_local_child(parent: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        anyhow::bail!("path escapes directory");
    }
    Ok(parent.join(name))
}

pub fn dest_is_newer(source: Option<DateTime<Utc>>, dest: Option<DateTime<Utc>>) -> bool {
    match (source, dest) {
        (Some(source), Some(dest)) => dest > source,
        _ => false,
    }
}

/// Local recursive walk. A directory symlink is skipped. A file symlink is
/// followed only when the target stays inside this folder.
pub fn walk_local_files(root: &Path, remote_root: &str) -> Result<Vec<PendingFile>> {
    let mut out = Vec::new();
    let root_canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut stack = vec![(root.to_path_buf(), remote_root.to_string())];
    while let Some((local_dir, remote_dir)) = stack.pop() {
        let read = std::fs::read_dir(&local_dir)
            .with_context(|| format!("read_dir {}", local_dir.display()))?;
        for entry in read.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "." || name_str == ".." {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let kind = meta.file_type();
            if kind.is_symlink() {
                let Ok(target) = std::fs::canonicalize(entry.path()) else {
                    continue;
                };
                if !target.starts_with(&root_canon) || target.is_dir() {
                    continue;
                }
            }
            if kind.is_dir() {
                let child_remote = session::join_remote(&remote_dir, name_str.as_ref())?;
                stack.push((entry.path(), child_remote));
            } else {
                let remote_path = session::join_remote(&remote_dir, name_str.as_ref())?;
                out.push(PendingFile {
                    local_path: entry.path().to_string_lossy().into_owned(),
                    remote_path,
                    direction: TransferDirection::Upload,
                    size_bytes: Some(meta.len()),
                    source_modified: meta.modified().ok().map(DateTime::<Utc>::from),
                    dest_modified: None,
                    delete_source: false,
                });
            }
        }
    }
    Ok(out)
}

pub fn walk_remote_tree(
    tree: &FakeTree,
    remote_root: &str,
    local_root: &Path,
) -> Result<Vec<PendingFile>> {
    let mut out = Vec::new();
    let mut stack = vec![(remote_root.to_string(), local_root.to_path_buf())];
    while let Some((remote_dir, local_dir)) = stack.pop() {
        let Some(entries) = tree.dirs.get(&remote_dir) else {
            anyhow::bail!("failed reading remote path: {remote_dir}");
        };
        for entry in entries {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            if entry.kind == EntryKind::Directory {
                let child_remote = session::join_remote(&remote_dir, &entry.name)?;
                let child_local = safe_local_child(&local_dir, &entry.name)?;
                stack.push((child_remote, child_local));
            } else {
                let remote_path = if entry.path.starts_with('/') {
                    entry.path.clone()
                } else {
                    session::join_remote(&remote_dir, &entry.name)?
                };
                let local_path = safe_local_child(&local_dir, &entry.name)?;
                out.push(PendingFile {
                    local_path: local_path.to_string_lossy().into_owned(),
                    remote_path,
                    direction: TransferDirection::Download,
                    size_bytes: Some(entry.size),
                    source_modified: entry.modified,
                    dest_modified: None,
                    delete_source: false,
                });
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct DestStat {
    pub exists: bool,
    pub modified: Option<DateTime<Utc>>,
    pub names: Vec<String>,
}

pub fn stat_fake_upload(tree: &FakeTree, remote_path: &str) -> DestStat {
    let parent = session::parent_remote(remote_path);
    let name = base_name(remote_path).to_string();
    let names = tree.dir_names(&parent);
    let modified = tree
        .dir_entry(&parent, &name)
        .and_then(|entry| entry.modified);
    DestStat {
        exists: tree.dir_entry(&parent, &name).is_some(),
        modified,
        names,
    }
}

pub enum XferEvent {
    Progress {
        generation: u64,
        id: Uuid,
        transferred: u64,
        size: Option<u64>,
    },
    Finished {
        generation: u64,
        job: TransferJob,
        error: Option<String>,
        cancelled: bool,
    },
    Listed {
        generation: u64,
        token: u64,
        entries: Vec<FileEntry>,
        error: Option<String>,
    },
}

struct RunJob {
    generation: u64,
    job: TransferJob,
    info: ConnectionInfo,
    cancel: Arc<AtomicBool>,
    pool_max: usize,
}

struct ListJob {
    generation: u64,
    token: u64,
    info: ConnectionInfo,
    path: String,
}

enum Cmd {
    Run(RunJob),
    List(ListJob),
    DropPool,
    Shutdown,
}

pub struct Engine {
    cmd_tx: Sender<Cmd>,
    evt_rx: Receiver<XferEvent>,
    fake: Option<Arc<Mutex<FakeTree>>>,
}

impl Engine {
    pub fn spawn(backend: Backend) -> Self {
        let fake = match &backend {
            Backend::Fake(tree) => Some(Arc::clone(tree)),
            Backend::Live => None,
        };
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        let worker_fake = fake.clone();
        thread::Builder::new()
            .name("dd-ftp-xfer".into())
            .spawn(move || engine_loop(worker_fake, cmd_rx, evt_tx))
            .expect("spawn transfer engine");
        Self {
            cmd_tx,
            evt_rx,
            fake,
        }
    }

    pub fn fake(&self) -> Option<Arc<Mutex<FakeTree>>> {
        self.fake.clone()
    }

    pub fn run(
        &self,
        generation: u64,
        job: TransferJob,
        info: ConnectionInfo,
        cancel: Arc<AtomicBool>,
        pool_max: usize,
    ) {
        let _ = self.cmd_tx.send(Cmd::Run(RunJob {
            generation,
            job,
            info,
            cancel,
            pool_max,
        }));
    }

    pub fn list_remote(&self, generation: u64, token: u64, info: ConnectionInfo, path: String) {
        let _ = self.cmd_tx.send(Cmd::List(ListJob {
            generation,
            token,
            info,
            path,
        }));
    }

    pub fn drop_pool(&self) {
        let _ = self.cmd_tx.send(Cmd::DropPool);
    }

    pub fn try_recv(&self) -> Result<XferEvent, TryRecvError> {
        self.evt_rx.try_recv()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Shutdown);
    }
}

fn engine_loop(
    fake: Option<Arc<Mutex<FakeTree>>>,
    cmd_rx: Receiver<Cmd>,
    evt_tx: Sender<XferEvent>,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .thread_name("dd-ftp-xfer-net")
        .build()
        .expect("transfer runtime");
    let pool = Arc::new(Mutex::new(Vec::<Pooled>::new()));
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Shutdown => {
                pool.lock().unwrap_or_else(|err| err.into_inner()).clear();
                break;
            }
            Cmd::DropPool => {
                pool.lock().unwrap_or_else(|err| err.into_inner()).clear();
            }
            Cmd::List(job) => {
                let evt_tx = evt_tx.clone();
                let fake = fake.clone();
                let handle = runtime.handle().clone();
                let pool = Arc::clone(&pool);
                thread::spawn(move || {
                    let outcome = if let Some(tree) = fake {
                        let tree = tree.lock().unwrap_or_else(|err| err.into_inner());
                        tree.dirs
                            .get(&job.path)
                            .cloned()
                            .ok_or_else(|| anyhow!("failed reading remote path: {}", job.path))
                    } else {
                        handle.block_on(live_list(&pool, job.info, &job.path))
                    };
                    let event = match outcome {
                        Ok(entries) => XferEvent::Listed {
                            generation: job.generation,
                            token: job.token,
                            entries,
                            error: None,
                        },
                        Err(err) => XferEvent::Listed {
                            generation: job.generation,
                            token: job.token,
                            entries: Vec::new(),
                            error: Some(err.to_string()),
                        },
                    };
                    let _ = evt_tx.send(event);
                });
            }
            Cmd::Run(job) => {
                let evt_tx = evt_tx.clone();
                let fake = fake.clone();
                let handle = runtime.handle().clone();
                let pool = Arc::clone(&pool);
                thread::spawn(move || run_job(fake, handle, pool, job, evt_tx));
            }
        }
    }
}

struct ProgressSink {
    tx: Sender<XferEvent>,
    generation: u64,
    id: Uuid,
    last: Mutex<Option<Instant>>,
}

impl ProgressSink {
    fn emit(&self, transferred: u64, size: Option<u64>) {
        let now = Instant::now();
        let complete = size.is_some_and(|total| transferred >= total);
        let mut last = self.last.lock().unwrap_or_else(|err| err.into_inner());
        if !should_emit_progress(*last, now, complete) {
            return;
        }
        *last = Some(now);
        let _ = self.tx.send(XferEvent::Progress {
            generation: self.generation,
            id: self.id,
            transferred,
            size,
        });
    }
}

fn run_job(
    fake: Option<Arc<Mutex<FakeTree>>>,
    handle: tokio::runtime::Handle,
    pool: Arc<Mutex<Vec<Pooled>>>,
    job: RunJob,
    evt_tx: Sender<XferEvent>,
) {
    let generation = job.generation;
    let cancel = Arc::clone(&job.cancel);
    let progress = ProgressSink {
        tx: evt_tx.clone(),
        generation,
        id: job.job.id,
        last: Mutex::new(None),
    };
    let outcome = if let Some(tree) = fake {
        fake_transfer(&tree, &job.job, &cancel, &progress)
    } else {
        handle.block_on(live_transfer(
            &pool,
            job.info,
            &job.job,
            Arc::clone(&cancel),
            job.pool_max,
            &progress,
        ))
    };
    let cancelled = cancel.load(Ordering::Relaxed);
    let error = match outcome {
        Ok(()) => None,
        Err(err) => Some(err.to_string()),
    };
    let _ = evt_tx.send(XferEvent::Finished {
        generation,
        job: job.job,
        error,
        cancelled,
    });
}

fn fake_transfer(
    tree: &Arc<Mutex<FakeTree>>,
    job: &TransferJob,
    cancel: &AtomicBool,
    progress: &ProgressSink,
) -> Result<()> {
    loop {
        let hold = tree
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .hold_transfers;
        if !hold {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        thread::sleep(Duration::from_millis(10));
    }
    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }
    match job.direction {
        TransferDirection::Upload => {
            let bytes = std::fs::read(&job.local_path)
                .with_context(|| format!("cannot open local file: {}", job.local_path))?;
            let size = Some(bytes.len() as u64);
            let mut transferred = 0_u64;
            while transferred < bytes.len() as u64 {
                if cancel.load(Ordering::Relaxed) {
                    anyhow::bail!("cancelled");
                }
                transferred = transferred
                    .saturating_add(CHUNK as u64)
                    .min(bytes.len() as u64);
                progress.emit(transferred, size);
            }
            progress.emit(bytes.len() as u64, size);
            tree.lock()
                .unwrap_or_else(|err| err.into_inner())
                .put_bytes(&job.remote_path, &bytes);
            if job.delete_source {
                let _ = std::fs::remove_file(&job.local_path);
            }
            Ok(())
        }
        TransferDirection::Download => {
            let bytes = {
                let guard = tree.lock().unwrap_or_else(|err| err.into_inner());
                guard
                    .files
                    .get(&job.remote_path)
                    .cloned()
                    .with_context(|| format!("remote file is missing: {}", job.remote_path))?
            };
            let path = Path::new(&job.local_path);
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            if let Ok(meta) = std::fs::symlink_metadata(path)
                && meta.file_type().is_symlink()
            {
                anyhow::bail!("refusing to write through symlink {}", path.display());
            }
            let mut file = create_no_follow(path)?;
            let size = Some(bytes.len() as u64);
            let mut transferred = 0_u64;
            while transferred < bytes.len() as u64 {
                if cancel.load(Ordering::Relaxed) {
                    anyhow::bail!("cancelled");
                }
                let next = transferred
                    .saturating_add(CHUNK as u64)
                    .min(bytes.len() as u64);
                use std::io::Write;
                file.write_all(&bytes[transferred as usize..next as usize])?;
                transferred = next;
                progress.emit(transferred, size);
            }
            if job.delete_source {
                tree.lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .remove_bytes(&job.remote_path);
            }
            Ok(())
        }
    }
}

enum Pooled {
    Sftp(SftpSession),
    Ftp(Box<UnifiedFtpSession>),
}

impl Pooled {
    fn info(&self) -> Option<ConnectionInfo> {
        match self {
            Self::Sftp(session) => session.connection_info().cloned(),
            Self::Ftp(session) => session.connection_info().cloned(),
        }
    }
}

fn take_pooled(pool: &Mutex<Vec<Pooled>>, info: &ConnectionInfo) -> Option<Pooled> {
    let mut pool = pool.lock().unwrap_or_else(|err| err.into_inner());
    let index = pool.iter().position(|slot| {
        slot.info()
            .is_some_and(|current| session_identity_eq(&current, info))
    })?;
    Some(pool.swap_remove(index))
}

fn return_pooled(pool: &Mutex<Vec<Pooled>>, session: Pooled, info: &ConnectionInfo, max: usize) {
    if session
        .info()
        .is_none_or(|current| !session_identity_eq(&current, info))
    {
        return;
    }
    let mut pool = pool.lock().unwrap_or_else(|err| err.into_inner());
    if pool.len() >= max.max(1) {
        return;
    }
    pool.push(session);
}

async fn connect_pooled(info: ConnectionInfo) -> Result<Pooled> {
    match info.protocol {
        Protocol::Sftp => {
            let mut session = SftpSession::default();
            session.connect(info).await?;
            Ok(Pooled::Sftp(session))
        }
        Protocol::Ftp | Protocol::Ftps => {
            let mut session = UnifiedFtpSession::new();
            session.connect(info).await?;
            Ok(Pooled::Ftp(Box::new(session)))
        }
    }
}

async fn live_list(
    pool: &Mutex<Vec<Pooled>>,
    info: ConnectionInfo,
    path: &str,
) -> Result<Vec<FileEntry>> {
    let mut session = match take_pooled(pool, &info) {
        Some(session) => session,
        None => connect_pooled(info.clone()).await?,
    };
    let listed = match &mut session {
        Pooled::Sftp(session) => session.list_dir(path).await,
        Pooled::Ftp(session) => session.list_dir(path).await,
    };
    return_pooled(pool, session, &info, 8);
    listed
}

async fn live_transfer(
    pool: &Mutex<Vec<Pooled>>,
    info: ConnectionInfo,
    job: &TransferJob,
    cancel: Arc<AtomicBool>,
    pool_max: usize,
    progress: &ProgressSink,
) -> Result<()> {
    let pooled = take_pooled(pool, &info);
    let reused = pooled.is_some();
    let mut session = match pooled {
        Some(session) => session,
        None => connect_pooled(info.clone()).await?,
    };
    let mut outcome = transfer_one(&mut session, job, Arc::clone(&cancel), progress).await;
    if outcome.is_err() && reused && !cancel.load(Ordering::Relaxed) {
        disconnect_pooled(&mut session).await;
        session = connect_pooled(info.clone()).await?;
        outcome = transfer_one(&mut session, job, cancel, progress).await;
    }
    if outcome.is_ok() {
        if job.delete_source && job.direction == TransferDirection::Download {
            let removed = match &mut session {
                Pooled::Sftp(session) => session.remove_file(&job.remote_path).await,
                Pooled::Ftp(session) => session.remove_file(&job.remote_path).await,
            };
            if let Err(err) = removed {
                disconnect_pooled(&mut session).await;
                return Err(err);
            }
        }
        return_pooled(pool, session, &info, pool_max);
        if job.delete_source && job.direction == TransferDirection::Upload {
            let _ = std::fs::remove_file(&job.local_path);
        }
        Ok(())
    } else {
        disconnect_pooled(&mut session).await;
        outcome
    }
}

fn reporter(progress: &ProgressSink) -> impl Fn(Uuid, u64, Option<u64>) + Send + Sync + 'static {
    let tx = progress.tx.clone();
    let generation = progress.generation;
    let id = progress.id;
    let last = Arc::new(Mutex::new(None));
    move |_job: Uuid, transferred: u64, size: Option<u64>| {
        let now = Instant::now();
        let complete = size.is_some_and(|total| transferred >= total);
        let mut last = last.lock().unwrap_or_else(|err| err.into_inner());
        if !should_emit_progress(*last, now, complete) {
            return;
        }
        *last = Some(now);
        let _ = tx.send(XferEvent::Progress {
            generation,
            id,
            transferred,
            size,
        });
    }
}

async fn transfer_one(
    session: &mut Pooled,
    job: &TransferJob,
    cancel: Arc<AtomicBool>,
    progress: &ProgressSink,
) -> Result<()> {
    match session {
        Pooled::Sftp(session) => match job.direction {
            TransferDirection::Upload => {
                SftpSession::upload_with_progress(session, job, cancel, reporter(progress)).await
            }
            TransferDirection::Download => {
                SftpSession::download_with_progress(session, job, cancel, reporter(progress)).await
            }
        },
        Pooled::Ftp(session) => match job.direction {
            TransferDirection::Upload => {
                UnifiedFtpSession::upload_with_progress(session, job, cancel, reporter(progress))
                    .await
            }
            TransferDirection::Download => {
                UnifiedFtpSession::download_with_progress(session, job, cancel, reporter(progress))
                    .await
            }
        },
    }
}

async fn disconnect_pooled(session: &mut Pooled) {
    match session {
        Pooled::Sftp(session) => {
            let _ = session.disconnect().await;
        }
        Pooled::Ftp(session) => {
            let _ = session.disconnect().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn spawn_gate_matches_the_worker_cap() {
        assert!(should_spawn(true, false, 1, 2, 3));
        assert!(!should_spawn(true, false, 2, 2, 3));
        assert!(!should_spawn(true, true, 0, 2, 3));
        assert!(!should_spawn(false, false, 0, 2, 3));
        assert!(!should_spawn(true, false, 0, 2, 0));
    }

    #[test]
    fn progress_emits_the_first_sample_then_waits() {
        let start = Instant::now();
        assert!(should_emit_progress(None, start, false));
        assert!(!should_emit_progress(
            Some(start),
            start + Duration::from_millis(50),
            false
        ));
        assert!(should_emit_progress(
            Some(start),
            start + Duration::from_millis(100),
            false
        ));
        assert!(should_emit_progress(
            Some(start),
            start + Duration::from_millis(10),
            true
        ));
    }

    #[test]
    fn conflict_policy_table() {
        assert_eq!(
            resolve_conflict(false, false, OverwritePolicy::Ask),
            DrainStep::Enqueue
        );
        assert_eq!(
            resolve_conflict(true, false, OverwritePolicy::Ask),
            DrainStep::Prompt
        );
        assert_eq!(
            resolve_conflict(true, true, OverwritePolicy::SkipAll),
            DrainStep::Skip
        );
        assert_eq!(
            resolve_conflict(true, true, OverwritePolicy::OverwriteAll),
            DrainStep::Enqueue
        );
        assert_eq!(
            resolve_conflict(true, true, OverwritePolicy::SkipNewer),
            DrainStep::Skip
        );
        assert_eq!(
            resolve_conflict(true, false, OverwritePolicy::SkipNewer),
            DrainStep::Prompt
        );
        assert_eq!(
            resolve_conflict(true, true, OverwritePolicy::OverwriteNewer),
            DrainStep::Enqueue
        );
        assert_eq!(
            resolve_conflict(true, true, OverwritePolicy::RenameNewer),
            DrainStep::RenameAuto
        );
    }

    #[test]
    fn identity_ignores_password_and_label() {
        let mut left = ConnectionInfo {
            name: "one".into(),
            host: "files.example".into(),
            port: 22,
            protocol: Protocol::Sftp,
            username: "ada".into(),
            password: Some("secret".into()),
            private_key: None,
            initial_path: "/".into(),
        };
        let right = ConnectionInfo {
            name: "other".into(),
            password: None,
            ..left.clone()
        };
        assert!(session_identity_eq(&left, &right));
        left.port = 21;
        assert!(!session_identity_eq(&left, &right));
    }

    #[test]
    fn folder_upload_skips_a_symlink_that_leaves_the_tree() {
        let root = temp_dir("dd_ftp_gui_walk");
        let tree = root.join("folder");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(root.join("secret.txt"), b"nope").unwrap();
        std::fs::write(tree.join("ok.txt"), b"yes").unwrap();
        std::os::unix::fs::symlink(root.join("secret.txt"), tree.join("leak")).unwrap();
        std::os::unix::fs::symlink(tree.join("ok.txt"), tree.join("inside")).unwrap();
        std::os::unix::fs::symlink("..", tree.join("loop")).unwrap();
        let files = walk_local_files(&tree, "/pub").unwrap();
        let remotes: Vec<&str> = files.iter().map(|file| file.remote_path.as_str()).collect();
        assert!(remotes.contains(&"/pub/ok.txt"), "{remotes:?}");
        assert!(remotes.contains(&"/pub/inside"), "{remotes:?}");
        assert!(
            !remotes
                .iter()
                .any(|path| path.contains("secret") || path.contains("leak"))
        );
        assert!(!remotes.iter().any(|path| path.contains("loop")));
    }
}
