//! Local directory listing. Same shape as the terminal pane: name, size, dir marker.

use std::path::{Path, PathBuf};

use dd_ftp_core::{EntryKind, FileEntry};

#[derive(Clone, Debug)]
pub struct LocalEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
}

impl LocalEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }

    pub fn marker(&self) -> &'static str {
        if self.is_dir() { "dir" } else { "file" }
    }
}

pub fn list_dir(path: &Path, hide_dotfiles: bool) -> Vec<LocalEntry> {
    let mut out = Vec::new();
    if let Some(parent) = path.parent()
        && parent != path
    {
        out.push(LocalEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            kind: EntryKind::Directory,
            size: 0,
        });
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return out;
    };
    let mut regular = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if hide_dotfiles && name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let kind = if meta.is_dir() {
            EntryKind::Directory
        } else if meta.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::File
        };
        regular.push(LocalEntry {
            name,
            path: entry.path(),
            kind,
            size: meta.len(),
        });
    }
    regular.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out.extend(regular);
    out
}

/// Keep a `FileEntry` available for callers that already speak the core type.
#[allow(dead_code)]
pub fn to_file_entry(entry: &LocalEntry) -> FileEntry {
    FileEntry {
        name: entry.name.clone(),
        path: entry.path.display().to_string(),
        kind: entry.kind,
        size: entry.size,
        modified: None,
        permissions: None,
    }
}
