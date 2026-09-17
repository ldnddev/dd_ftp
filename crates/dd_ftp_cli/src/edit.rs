use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::SystemTime,
};

pub const EDIT_SIZE_WARN_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFingerprint {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

pub fn resolve_editor_with(
    visual: Option<&str>,
    editor_env: Option<&str>,
    configured: &str,
) -> String {
    for candidate in [visual, editor_env] {
        if let Some(s) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    "vi".to_string()
}

pub fn resolve_editor(configured: &str) -> String {
    resolve_editor_with(
        std::env::var("VISUAL").ok().as_deref(),
        std::env::var("EDITOR").ok().as_deref(),
        configured,
    )
}

pub fn run_editor(path: &Path, configured: &str) -> std::io::Result<ExitStatus> {
    let spec = resolve_editor(configured);
    let mut parts = spec.split_whitespace();
    let prog = parts.next().unwrap_or("vi");
    let mut cmd = Command::new(prog);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd.arg(path);
    cmd.status()
}

pub fn looks_binary(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 8192];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    buf[..n].contains(&0)
}

pub fn fingerprint(path: &Path) -> Option<FileFingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileFingerprint {
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}

pub fn sanitize_component(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
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

pub fn cache_root() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("dd_ftp").join("edit")
}

pub fn edit_cache_path(host: &str, port: u16, remote_path: &str) -> PathBuf {
    let mut path = cache_root().join(format!("{}_{port}", sanitize_component(host)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_drops_dotdot_and_separators() {
        let p = edit_cache_path("ex.com", 22, "/etc/../passwd");
        let s = p.to_string_lossy();
        assert!(s.contains("ex.com_22"));
        assert!(s.ends_with("passwd") || s.contains("passwd"));
        assert!(!s.contains(".."));
    }

    #[test]
    fn sanitize_component_replaces_unsafe() {
        assert_eq!(sanitize_component("a b/c"), "a_b_c");
        assert_eq!(sanitize_component(".."), "_");
        assert_eq!(sanitize_component(""), "_");
    }

    #[test]
    fn looks_binary_detects_nul() {
        let dir = std::env::temp_dir().join(format!("dd_ftp_bin_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("a.bin");
        let txt = dir.join("a.txt");
        std::fs::write(&bin, b"hello\0world").unwrap();
        std::fs::write(&txt, b"hello world").unwrap();
        assert!(looks_binary(&bin));
        assert!(!looks_binary(&txt));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_changes_with_len() {
        let dir = std::env::temp_dir().join(format!("dd_ftp_fp_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x");
        std::fs::write(&f, b"abc").unwrap();
        let a = fingerprint(&f).unwrap();
        std::fs::write(&f, b"abcd").unwrap();
        let b = fingerprint(&f).unwrap();
        assert_ne!(a.len, b.len);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_editor_precedence() {
        assert_eq!(
            resolve_editor_with(Some("code -w"), Some("nano"), "hx"),
            "code -w"
        );
        assert_eq!(resolve_editor_with(None, Some("nano"), "hx"), "nano");
        assert_eq!(resolve_editor_with(None, None, "hx"), "hx");
        assert_eq!(resolve_editor_with(None, Some("  "), "  hx  "), "hx");
        assert_eq!(resolve_editor_with(None, None, ""), "vi");
    }
}
