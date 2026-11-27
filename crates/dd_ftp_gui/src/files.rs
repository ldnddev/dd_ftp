//! Remote file helpers for the window. The decisions match the terminal:
//! editor order, private edit cache, binary sniff, and delete summary.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub const EDIT_SIZE_WARN_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFingerprint {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum TestEditor {
    Keep,
    Write(Vec<u8>),
    WriteNewer(Vec<u8>),
}

pub fn resolve_editor_with(
    visual: Option<&str>,
    editor_env: Option<&str>,
    configured: &str,
) -> String {
    for candidate in [visual, editor_env] {
        if let Some(value) = candidate.map(str::trim).filter(|value| !value.is_empty()) {
            return value.to_string();
        }
    }
    let configured = configured.trim();
    if configured.is_empty() {
        "vi".to_string()
    } else {
        configured.to_string()
    }
}

pub fn resolve_editor(configured: &str) -> String {
    resolve_editor_with(
        std::env::var("VISUAL").ok().as_deref(),
        std::env::var("EDITOR").ok().as_deref(),
        configured,
    )
}

pub fn run_editor(path: &Path, spec: &str) -> std::io::Result<std::process::ExitStatus> {
    let mut parts = spec.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let mut cmd = Command::new(program);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd.arg(path);
    cmd.status()
}

pub fn looks_binary(path: &Path) -> bool {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut buf = [0u8; 8192];
    let count = match file.read(&mut buf) {
        Ok(count) => count,
        Err(_) => return false,
    };
    buf[..count].contains(&0)
}

pub fn fingerprint(path: &Path) -> Option<FileFingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileFingerprint {
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}

pub fn cache_root(override_root: Option<&Path>) -> PathBuf {
    if let Some(root) = override_root {
        return root.to_path_buf();
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("dd_ftp").join("edit")
}

pub fn edit_cache_path(root: &Path, host: &str, port: u16, remote_path: &str) -> PathBuf {
    let mut path = root.join(format!("{}_{port}", sanitize_component(host)));
    let mut pushed = false;
    for part in remote_path.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        path.push(sanitize_component(part));
        pushed = true;
    }
    if !pushed {
        path.push("file");
    }
    path
}

pub fn sanitize_component(value: &str) -> String {
    let out: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out == "." || out == ".." {
        "_".to_string()
    } else {
        out
    }
}

pub fn prepare_edit_cache(file: &Path, root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    tighten_dir_0700(root)?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
        if parent.starts_with(root) {
            tighten_dir_0700(parent)?;
        }
    }
    Ok(())
}

pub fn tighten_edit_file(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "edit cache path is a symlink",
            ));
        }
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn tighten_dir_0700(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dir)?.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(dir, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}

pub fn delete_summary(names: &[String], dirs: usize) -> String {
    let count = names.len();
    let files = count.saturating_sub(dirs);
    match (count, files, dirs) {
        (0, _, _) => "nothing".to_string(),
        (1, _, _) => format!("'{}'", names[0]),
        (_, _, 0) => format!("{count} files"),
        (_, 0, _) => format!("{count} folders"),
        _ => format!("{count} items ({files} files, {dirs} folders)"),
    }
}

pub fn large_edit_label(size: u64) -> String {
    format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_order_is_visual_then_editor_then_session_then_vi() {
        assert_eq!(
            resolve_editor_with(Some("code"), Some("nano"), "emacs"),
            "code"
        );
        assert_eq!(
            resolve_editor_with(Some("  "), Some("nano"), "emacs"),
            "nano"
        );
        assert_eq!(resolve_editor_with(None, None, " emacs "), "emacs");
        assert_eq!(resolve_editor_with(None, None, " "), "vi");
    }

    #[test]
    fn cache_path_drops_dotdot_and_separators() {
        let path = edit_cache_path(Path::new("/tmp/cache"), "ex.com", 22, "/etc/../passwd");
        let text = path.to_string_lossy();
        assert!(text.contains("ex.com_22"), "{text}");
        assert!(text.contains("passwd"), "{text}");
        assert!(!text.contains(".."), "{text}");
    }

    #[test]
    fn binary_sniff_sees_a_nul_and_delete_summary_quotes_one_name() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("bin");
        let text = dir.path().join("text");
        std::fs::write(&binary, b"a\0b").unwrap();
        std::fs::write(&text, b"abc").unwrap();
        assert!(looks_binary(&binary));
        assert!(!looks_binary(&text));
        assert_eq!(delete_summary(&["notes.txt".to_string()], 0), "'notes.txt'");
        assert_eq!(
            delete_summary(&["a".to_string(), "b".to_string()], 1),
            "2 items (1 files, 1 folders)"
        );
    }

    #[test]
    fn edit_cache_file_is_owner_readable_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        prepare_edit_cache(&file, dir.path()).unwrap();
        std::fs::write(&file, b"abc").unwrap();
        tighten_edit_file(&file).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::symlink_metadata(&file)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            let dir_mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600);
            assert_eq!(dir_mode, 0o700);
        }
    }
}
