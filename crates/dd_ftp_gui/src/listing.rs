//! Local directory listing. Sort and filter match the terminal pane.

use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use dd_ftp_app::{AppState, SortKey, cmp_file_entries, is_dot_or_dotdot};
use dd_ftp_core::{EntryKind, FileEntry};

#[derive(Clone, Debug)]
pub struct LocalEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

impl LocalEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }

    pub fn marker(&self) -> &'static str {
        if self.is_dir() { "dir" } else { "file" }
    }

    pub fn path_key(&self) -> String {
        self.path.display().to_string()
    }

    pub fn to_file_entry(&self) -> FileEntry {
        FileEntry {
            name: self.name.clone(),
            path: self.path_key(),
            kind: self.kind,
            size: self.size,
            modified: self.modified,
            permissions: None,
        }
    }
}

/// `.` and `..`, then every directory entry. Dotfiles stay in the list;
/// the view hides them.
pub fn list_dir(path: &Path) -> Vec<LocalEntry> {
    let current = path.to_path_buf();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| current.clone());
    let mut out = vec![
        LocalEntry {
            name: ".".to_string(),
            path: current.clone(),
            kind: EntryKind::Directory,
            size: 0,
            modified: None,
        },
        LocalEntry {
            name: "..".to_string(),
            path: parent,
            kind: EntryKind::Directory,
            size: 0,
            modified: None,
        },
    ];
    let Ok(entries) = std::fs::read_dir(path) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let kind = if meta.is_dir() {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        let modified = meta.modified().ok().map(DateTime::<Utc>::from);
        out.push(LocalEntry {
            name,
            path: entry.path(),
            kind,
            size: meta.len(),
            modified,
        });
    }
    out
}

pub fn visible(
    entries: &[LocalEntry],
    pattern: &str,
    hide_dotfiles: bool,
    sort_key: SortKey,
    sort_asc: bool,
) -> Vec<LocalEntry> {
    let mut indexed: Vec<(usize, &LocalEntry)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| AppState::entry_visible(&entry.name, pattern))
        .filter(|(_, entry)| {
            !(hide_dotfiles && entry.name.starts_with('.') && !is_dot_or_dotdot(&entry.name))
        })
        .collect();
    indexed.sort_by(|(_, a), (_, b)| {
        cmp_file_entries(&a.to_file_entry(), &b.to_file_entry(), sort_key, sort_asc)
    });
    indexed
        .into_iter()
        .map(|(_, entry)| entry.clone())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathSegment {
    pub label: String,
    pub target: String,
}

pub fn split_local_path(path: &Path) -> Vec<PathSegment> {
    let mut acc = PathBuf::new();
    let mut segs = Vec::new();
    for component in path.components() {
        acc.push(component);
        let label = match component {
            Component::RootDir => "/".to_string(),
            Component::Prefix(prefix) => prefix.as_os_str().to_string_lossy().into_owned(),
            Component::CurDir => ".".to_string(),
            Component::ParentDir => "..".to_string(),
            Component::Normal(name) => name.to_string_lossy().into_owned(),
        };
        segs.push(PathSegment {
            label,
            target: acc.to_string_lossy().into_owned(),
        });
    }
    if segs.is_empty() {
        let text = path.display().to_string();
        segs.push(PathSegment {
            label: text.clone(),
            target: text,
        });
    }
    segs
}

/// Remote path prefixes. `/var/www` is `/`, `/var`, `/var/www`.
pub fn split_remote_path(path: &str) -> Vec<PathSegment> {
    if path.is_empty() || path == "/" {
        return vec![PathSegment {
            label: "/".into(),
            target: "/".into(),
        }];
    }
    let mut segs = Vec::new();
    let mut acc = String::new();
    if path.starts_with('/') {
        acc = "/".into();
        segs.push(PathSegment {
            label: "/".into(),
            target: "/".into(),
        });
    }
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        acc = if acc == "/" {
            format!("/{part}")
        } else if acc.is_empty() {
            part.to_string()
        } else {
            format!("{acc}/{part}")
        };
        segs.push(PathSegment {
            label: part.to_string(),
            target: acc.clone(),
        });
    }
    if segs.is_empty() {
        segs.push(PathSegment {
            label: path.to_string(),
            target: path.to_string(),
        });
    }
    segs
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SegmentAction {
    Parent,
    Goto(String),
}

/// A click cds to that prefix. A double-click on the current segment goes up.
pub fn segment_action(
    segments: &[PathSegment],
    index: usize,
    double: bool,
) -> Option<SegmentAction> {
    let segment = segments.get(index)?;
    if double && index + 1 == segments.len() {
        Some(SegmentAction::Parent)
    } else {
        Some(SegmentAction::Goto(segment.target.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind, size: u64, modified: Option<i64>) -> LocalEntry {
        LocalEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}")),
            kind,
            size,
            modified: modified.and_then(|secs| DateTime::from_timestamp(secs, 0)),
        }
    }

    fn names(rows: &[LocalEntry]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    #[test]
    fn name_sort_pins_dots_and_directories() {
        let rows = vec![
            entry("b.txt", EntryKind::File, 1, None),
            entry("..", EntryKind::Directory, 0, None),
            entry("a-dir", EntryKind::Directory, 9, None),
            entry(".", EntryKind::Directory, 0, None),
            entry("a.txt", EntryKind::File, 3, None),
        ];
        let shown = visible(&rows, "", false, SortKey::Name, true);
        assert_eq!(names(&shown), vec![".", "..", "a-dir", "a.txt", "b.txt"]);
        let flipped = visible(&rows, "", false, SortKey::Name, false);
        assert_eq!(names(&flipped), vec![".", "..", "a-dir", "b.txt", "a.txt"]);
    }

    #[test]
    fn size_and_date_sort_after_the_dot_rows() {
        let rows = vec![
            entry(".", EntryKind::Directory, 0, None),
            entry("..", EntryKind::Directory, 0, None),
            entry("big", EntryKind::File, 30, Some(10)),
            entry("small", EntryKind::File, 2, Some(50)),
        ];
        assert_eq!(
            names(&visible(&rows, "", false, SortKey::Size, true)),
            vec![".", "..", "small", "big"]
        );
        assert_eq!(
            names(&visible(&rows, "", false, SortKey::Size, false)),
            vec![".", "..", "big", "small"]
        );
        assert_eq!(
            names(&visible(&rows, "", false, SortKey::Date, true)),
            vec![".", "..", "big", "small"]
        );
        assert_eq!(
            names(&visible(&rows, "", false, SortKey::Date, false)),
            vec![".", "..", "small", "big"]
        );
    }

    #[test]
    fn filter_and_hide_keep_dot_and_dotdot() {
        let rows = vec![
            entry(".", EntryKind::Directory, 0, None),
            entry("..", EntryKind::Directory, 0, None),
            entry(".secret", EntryKind::File, 1, None),
            entry("alpha.txt", EntryKind::File, 1, None),
        ];
        let hidden = visible(&rows, "", true, SortKey::Name, true);
        assert_eq!(names(&hidden), vec![".", "..", "alpha.txt"]);
        let filtered = visible(&rows, "ALP", false, SortKey::Name, true);
        assert_eq!(names(&filtered), vec!["alpha.txt"]);
    }

    #[test]
    fn path_segments_match_the_terminal_bar() {
        let segs = split_local_path(Path::new("/home/demo/www"));
        assert_eq!(segs.first().map(|seg| seg.label.as_str()), Some("/"));
        assert_eq!(
            segs.last().map(|seg| seg.target.as_str()),
            Some("/home/demo/www")
        );
        assert_eq!(segs.last().map(|seg| seg.label.as_str()), Some("www"));
        assert_eq!(
            segment_action(&segs, 1, false),
            Some(SegmentAction::Goto("/home".into()))
        );
        assert_eq!(
            segment_action(&segs, segs.len() - 1, true),
            Some(SegmentAction::Parent)
        );
        assert_eq!(
            segment_action(&segs, segs.len() - 1, false),
            Some(SegmentAction::Goto("/home/demo/www".into()))
        );
        assert!(segment_action(&segs, 9, false).is_none());
    }

    #[test]
    fn list_dir_reads_dotfiles_and_pins_dot_rows() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.txt"), b"a").unwrap();
        std::fs::write(dir.path().join(".secret"), b"ss").unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        let rows = list_dir(dir.path());
        let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names[0], ".");
        assert_eq!(names[1], "..");
        assert!(names.contains(&".secret"));
        assert!(names.contains(&"alpha.txt"));
        assert!(names.contains(&"nested"));
        let secret = rows.iter().find(|row| row.name == ".secret").unwrap();
        assert_eq!(secret.size, 2);
    }
}
