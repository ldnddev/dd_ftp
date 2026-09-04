use std::path::{Component, Path, PathBuf};

use anyhow::bail;
use dd_ftp_core::FileEntry;

fn is_safe_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    if Path::new(name).is_absolute() {
        return false;
    }
    if name.contains('/') || name.contains('\\') {
        return false;
    }
    true
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Join `name` onto `cwd` for a *local* create/rename/download destination.
/// Rejects empty, `.`, `..`, absolute names, and any name containing a
/// path separator. After join, the normalized result must stay under `cwd`
/// (also reject `name` values like `foo/../../etc`).
pub fn safe_local_child(cwd: &Path, name: &str) -> anyhow::Result<PathBuf> {
    if !is_safe_component(name) {
        bail!("path escapes directory");
    }
    let joined = cwd.join(name);
    let cwd_n = normalize_lexical(cwd);
    let joined_n = normalize_lexical(&joined);
    if !joined_n.starts_with(&cwd_n) {
        bail!("path escapes directory");
    }
    Ok(joined)
}

/// Join `name` onto remote `cwd`. Rejects empty, `.`, `..`, leading `/`,
/// and any name containing `/`. Recursive builders call this once per
/// path component.
pub fn safe_remote_child(cwd: &str, name: &str) -> anyhow::Result<String> {
    if !is_safe_component(name) {
        bail!("path escapes directory");
    }
    let joined = join_remote_path(cwd, name);
    let cwd_n = if cwd.is_empty() { "/" } else { cwd };
    let cwd_prefix = cwd_n.trim_end_matches('/');
    if cwd_prefix.is_empty() || cwd_prefix == "/" {
        if !joined.starts_with('/') {
            bail!("path escapes directory");
        }
        return Ok(joined);
    }
    if joined == cwd_prefix || joined.starts_with(&format!("{cwd_prefix}/")) {
        Ok(joined)
    } else {
        bail!("path escapes directory");
    }
}

pub(crate) fn join_remote_path(base: &str, child: &str) -> String {
    if child.starts_with('/') {
        return child.to_string();
    }
    let base = if base.is_empty() { "/" } else { base };
    if base == "/" {
        format!("/{}", child.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            child.trim_start_matches('/')
        )
    }
}

pub(crate) fn parent_remote_path(path: &str) -> String {
    let p = if path.is_empty() { "/" } else { path };
    if p == "/" {
        return "/".to_string();
    }
    let trimmed = p.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
    }
}

pub(crate) fn local_list(path: &str) -> Vec<FileEntry> {
    let mut out = Vec::new();

    let current_path = if path.is_empty() { "." } else { path };
    let parent_path = Path::new(current_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| current_path.to_string());

    out.push(FileEntry {
        name: ".".to_string(),
        path: current_path.to_string(),
        kind: dd_ftp_core::EntryKind::Directory,
        size: 0,
        modified: None,
        permissions: None,
    });

    out.push(FileEntry {
        name: "..".to_string(),
        path: parent_path,
        kind: dd_ftp_core::EntryKind::Directory,
        size: 0,
        modified: None,
        permissions: None,
    });

    if let Ok(entries) = std::fs::read_dir(current_path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let kind = if meta.is_dir() {
                    dd_ftp_core::EntryKind::Directory
                } else {
                    dd_ftp_core::EntryKind::File
                };

                let modified = meta.modified().ok().map(Into::into);
                #[cfg(unix)]
                let permissions = {
                    use std::os::unix::fs::PermissionsExt;
                    Some(format!("{:o}", meta.permissions().mode() & 0o7777))
                };
                #[cfg(not(unix))]
                let permissions = None;

                out.push(FileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().to_string_lossy().to_string(),
                    kind,
                    size: meta.len(),
                    modified,
                    permissions,
                });
            }
        }
    }

    let (special, mut regular): (Vec<_>, Vec<_>) = out
        .into_iter()
        .partition(|e| e.name == "." || e.name == "..");

    regular.sort_by_key(|a| a.name.to_lowercase());

    let mut result = special;
    result.extend(regular);
    result
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn join_remote_path_joins_base_and_name() {
        assert_eq!(join_remote_path("/pub", "file.bin"), "/pub/file.bin");
        assert_eq!(join_remote_path("/", "file.bin"), "/file.bin");
    }

    #[test]
    fn safe_local_child_table() {
        let cwd = Path::new("/tmp/pane");
        struct Case {
            name: &'static str,
            ok: bool,
        }
        let cases = [
            Case {
                name: "foo",
                ok: true,
            },
            Case {
                name: ".",
                ok: false,
            },
            Case {
                name: "..",
                ok: false,
            },
            Case {
                name: "/etc/passwd",
                ok: false,
            },
            Case {
                name: "foo/bar",
                ok: false,
            },
            Case {
                name: "foo/../../etc",
                ok: false,
            },
        ];
        for case in cases {
            let got = safe_local_child(cwd, case.name);
            assert_eq!(got.is_ok(), case.ok, "local name {:?}", case.name);
            if case.ok {
                assert_eq!(got.unwrap(), cwd.join("foo"));
            }
        }
    }

    #[test]
    fn safe_remote_child_table() {
        struct Case {
            name: &'static str,
            ok: bool,
        }
        let cases = [
            Case {
                name: "foo",
                ok: true,
            },
            Case {
                name: ".",
                ok: false,
            },
            Case {
                name: "..",
                ok: false,
            },
            Case {
                name: "/etc/passwd",
                ok: false,
            },
            Case {
                name: "foo/bar",
                ok: false,
            },
            Case {
                name: "foo/../../etc",
                ok: false,
            },
            Case {
                name: "/leading",
                ok: false,
            },
        ];
        for case in cases {
            let got = safe_remote_child("/pub", case.name);
            assert_eq!(got.is_ok(), case.ok, "remote name {:?}", case.name);
            if case.ok {
                assert_eq!(got.unwrap(), "/pub/foo");
            }
        }
    }

    #[test]
    fn local_list_fills_size_and_modified() {
        let dir = std::env::temp_dir().join(format!(
            "dd_ftp_local_list_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("hello.txt");
        std::fs::write(&file, b"hello world").expect("write file");

        let entries = local_list(dir.to_str().expect("utf8 path"));
        let hello = entries
            .iter()
            .find(|e| e.name == "hello.txt")
            .expect("listed file");
        assert!(hello.size > 0);
        assert!(hello.modified.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
