use std::collections::HashMap;

use dd_ftp_core::FileEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareBadge {
    LocalOnly,
    RemoteOnly,
    Equal,
    Differ,
}

impl CompareBadge {
    pub fn label(self) -> &'static str {
        match self {
            CompareBadge::LocalOnly => "[L]",
            CompareBadge::RemoteOnly => "[R]",
            CompareBadge::Equal => "[=]",
            CompareBadge::Differ => "[≠]",
        }
    }
}

/// Lowercased name → entry, skipping `.` / `..`. Later duplicates win.
pub fn index_by_name(entries: &[FileEntry]) -> HashMap<String, &FileEntry> {
    let mut map = HashMap::with_capacity(entries.len());
    for e in entries {
        if e.name != "." && e.name != ".." {
            map.insert(e.name.to_ascii_lowercase(), e);
        }
    }
    map
}

pub fn classify_compare_indexed(
    name: &str,
    local: &HashMap<String, &FileEntry>,
    remote: &HashMap<String, &FileEntry>,
) -> Option<CompareBadge> {
    if name == "." || name == ".." {
        return None;
    }
    let key = name.to_ascii_lowercase();
    match (local.get(&key), remote.get(&key)) {
        (Some(a), Some(b)) => Some(if same_meta(a, b) {
            CompareBadge::Equal
        } else {
            CompareBadge::Differ
        }),
        (Some(_), None) => Some(CompareBadge::LocalOnly),
        (None, Some(_)) => Some(CompareBadge::RemoteOnly),
        (None, None) => None,
    }
}

fn same_meta(a: &FileEntry, b: &FileEntry) -> bool {
    if a.size != b.size {
        return false;
    }
    match (a.modified, b.modified) {
        (None, None) => true,
        // SFTP mtimes are whole seconds; local listings keep subseconds.
        (Some(x), Some(y)) => x.timestamp() == y.timestamp(),
        _ => false,
    }
}

/// Classify `name` against both listings. Skips `.` / `..`.
pub fn classify_compare(
    name: &str,
    local: &[FileEntry],
    remote: &[FileEntry],
) -> Option<CompareBadge> {
    if name == "." || name == ".." {
        return None;
    }
    classify_compare_indexed(name, &index_by_name(local), &index_by_name(remote))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use dd_ftp_core::{EntryKind, FileEntry};

    fn fe(name: &str, size: u64, mtime: Option<i64>) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: name.to_string(),
            kind: EntryKind::File,
            size,
            modified: mtime.map(|s| Utc.timestamp_opt(s, 0).unwrap()),
            permissions: None,
        }
    }

    #[test]
    fn same_name_size_mtime_is_equal() {
        let local = vec![fe("a.txt", 10, Some(100))];
        let remote = vec![fe("a.txt", 10, Some(100))];
        assert_eq!(
            classify_compare("a.txt", &local, &remote),
            Some(CompareBadge::Equal)
        );
    }

    #[test]
    fn mtime_compared_at_second_resolution() {
        let mut local = fe("a.txt", 10, Some(100));
        local.modified = Some(Utc.timestamp_opt(100, 250_000_000).unwrap());
        let mut remote = fe("a.txt", 10, Some(100));
        remote.modified = Some(Utc.timestamp_opt(100, 0).unwrap());
        assert_eq!(
            classify_compare("a.txt", &[local.clone()], &[remote]),
            Some(CompareBadge::Equal)
        );
        let later = fe("a.txt", 10, Some(101));
        assert_eq!(
            classify_compare("a.txt", &[local], &[later]),
            Some(CompareBadge::Differ)
        );
    }

    #[test]
    fn missing_mtime_both_equal_size_is_equal() {
        let local = vec![fe("a.txt", 10, None)];
        let remote = vec![fe("a.txt", 10, None)];
        assert_eq!(
            classify_compare("a.txt", &local, &remote),
            Some(CompareBadge::Equal)
        );
    }

    #[test]
    fn same_name_different_size_is_differ() {
        let local = vec![fe("a.txt", 10, Some(100))];
        let remote = vec![fe("a.txt", 20, Some(100))];
        assert_eq!(
            classify_compare("a.txt", &local, &remote),
            Some(CompareBadge::Differ)
        );
    }

    #[test]
    fn local_only_and_remote_only() {
        let local = vec![fe("only-l", 1, None)];
        let remote = vec![fe("only-r", 1, None)];
        assert_eq!(
            classify_compare("only-l", &local, &remote),
            Some(CompareBadge::LocalOnly)
        );
        assert_eq!(
            classify_compare("only-r", &local, &remote),
            Some(CompareBadge::RemoteOnly)
        );
    }

    #[test]
    fn name_match_is_case_insensitive() {
        let local = vec![fe("Foo.TXT", 4, Some(1))];
        let remote = vec![fe("foo.txt", 4, Some(1))];
        assert_eq!(
            classify_compare("Foo.TXT", &local, &remote),
            Some(CompareBadge::Equal)
        );
        assert_eq!(
            classify_compare("foo.txt", &local, &remote),
            Some(CompareBadge::Equal)
        );
    }

    #[test]
    fn skips_dot_and_dotdot() {
        let local = vec![fe(".", 0, None), fe("..", 0, None)];
        let remote = vec![fe(".", 0, None)];
        assert_eq!(classify_compare(".", &local, &remote), None);
        assert_eq!(classify_compare("..", &local, &remote), None);
    }

    #[test]
    fn index_by_name_skips_dots_and_matches_classify() {
        let local = vec![
            fe(".", 0, None),
            fe("a.txt", 10, Some(100)),
            fe("OnlyL", 1, None),
        ];
        let remote = vec![
            fe("..", 0, None),
            fe("A.TXT", 10, Some(100)),
            fe("only-r", 1, None),
        ];
        let lmap = index_by_name(&local);
        let rmap = index_by_name(&remote);
        assert!(!lmap.contains_key("."));
        assert!(!rmap.contains_key(".."));
        assert_eq!(
            classify_compare_indexed("a.txt", &lmap, &rmap),
            classify_compare("a.txt", &local, &remote)
        );
        assert_eq!(
            classify_compare_indexed("OnlyL", &lmap, &rmap),
            Some(CompareBadge::LocalOnly)
        );
    }
}
