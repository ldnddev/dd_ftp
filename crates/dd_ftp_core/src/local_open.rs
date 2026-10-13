//! Create local transfer targets without following a final-path symlink.
//!
//! `File::create` follows symlinks. A destination planted as a symlink in a
//! shared directory (for example `/tmp`) would truncate the link target.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Append to `path`, creating it if needed. Fails if the last component is a symlink.
pub fn open_append_no_follow(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(nofollow_flag());
    }
    opts.open(path)
}

/// Create a new owner-only file. Fails if `path` already exists, including as a symlink.
pub fn create_private_new(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
        opts.custom_flags(nofollow_flag());
    }
    opts.open(path)
}

/// Create or truncate `path`. Fails if the last component is a symlink.
pub fn create_no_follow(path: &Path) -> io::Result<File> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("refusing to write through symlink {}", path.display()),
            ));
        }
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(nofollow_flag());
    }
    opts.open(path)
}

/// Extra `open(2)` flags so the last path component is not followed.
#[cfg(unix)]
pub fn nofollow_flag() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        0x20000
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos"
    ))]
    {
        0x100
    }
    #[cfg(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    {
        0x200
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    )))]
    {
        0
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dd_ftp_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_no_follow_does_not_truncate_symlink_target() {
        let dir = scratch("nofollow");
        let target = dir.join("target");
        std::fs::write(&target, b"secret").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = create_no_follow(&link).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).unwrap(), b"secret");

        let created = dir.join("plain");
        let mut file = create_no_follow(&created).unwrap();
        use std::io::Write;
        file.write_all(b"ok").unwrap();
        assert_eq!(std::fs::read(&created).unwrap(), b"ok");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
