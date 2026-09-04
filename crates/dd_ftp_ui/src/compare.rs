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

fn find_by_name<'a>(entries: &'a [FileEntry], name: &str) -> Option<&'a FileEntry> {
    entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(name) && e.name != "." && e.name != "..")
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
    let l = find_by_name(local, name);
    let r = find_by_name(remote, name);
    match (l, r) {
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
}
