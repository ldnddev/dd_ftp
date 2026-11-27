//! SSH private-key picker. Opens in the parent of an existing key path, or in
//! the window home directory.

use std::path::{Path, PathBuf};

use dd_ftp_app::{SortKey, cmp_file_entries};
use dd_ftp_core::{EntryKind, FileEntry};

pub fn start_dir(home: Option<&Path>, private_key: &str) -> PathBuf {
    let raw = private_key.trim();
    if !raw.is_empty() {
        let path = expand_tilde(home, raw);
        if path.is_dir() {
            return path;
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && parent.is_dir()
        {
            return parent.to_path_buf();
        }
    }
    home.filter(|path| path.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn prefer_name(private_key: &str, cwd: &Path) -> Option<String> {
    let raw = private_key.trim();
    if raw.is_empty() {
        return None;
    }
    let path = expand_tilde(None, raw);
    let name = path.file_name()?.to_string_lossy().into_owned();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())?;
    if parent == cwd { Some(name) } else { None }
}

pub fn list_dir(path: &Path) -> Vec<FileEntry> {
    let cwd = path.display().to_string();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.display().to_string())
        .unwrap_or_else(|| cwd.clone());
    let mut entries = vec![file_entry(".", &cwd, true), file_entry("..", &parent, true)];
    if let Ok(read) = std::fs::read_dir(path) {
        for item in read.flatten() {
            let Ok(meta) = item.metadata() else {
                continue;
            };
            let name = item.file_name().to_string_lossy().into_owned();
            entries.push(file_entry(
                &name,
                &item.path().display().to_string(),
                meta.is_dir(),
            ));
        }
    }
    entries.sort_by(|a, b| cmp_file_entries(a, b, SortKey::Name, true));
    entries
}

fn expand_tilde(home: Option<&Path>, raw: &str) -> PathBuf {
    if raw == "~" {
        return home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest);
    }
    PathBuf::from(raw)
}

fn file_entry(name: &str, path: &str, directory: bool) -> FileEntry {
    FileEntry {
        name: name.to_string(),
        path: path.to_string(),
        kind: if directory {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        size: 0,
        modified: None,
        permissions: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_dir_uses_the_parent_of_an_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("id_ed25519");
        std::fs::write(&key, b"key").unwrap();
        let start = start_dir(Some(dir.path()), key.to_str().unwrap());
        assert_eq!(start, dir.path());
        assert_eq!(
            prefer_name(key.to_str().unwrap(), &start).as_deref(),
            Some("id_ed25519")
        );
        let names: Vec<_> = list_dir(&start)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert!(names.contains(&"id_ed25519".to_string()));
    }
}
