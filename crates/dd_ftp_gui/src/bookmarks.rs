//! Bookmark list edits. The file write and the keyring call stay in the shell.
//! Index rules match the terminal: identity is host + username + port, delete
//! matches the label, and the default site is moved to index 0.

use dd_ftp_core::ConnectionInfo;
use dd_ftp_storage::SiteConfig;

/// Insert or replace `info`. Returns `(index, updated)`.
pub fn upsert(config: &mut SiteConfig, info: ConnectionInfo) -> (usize, bool) {
    let existing = config.sites.iter().position(|site| {
        site.host == info.host && site.username == info.username && site.port == info.port
    });
    if let Some(index) = existing {
        config.sites[index] = info;
        if config.default_site.is_none() {
            config.default_site = Some(index);
        }
        (index, true)
    } else {
        config.sites.push(info);
        let index = config.sites.len().saturating_sub(1);
        if config.default_site.is_none() {
            config.default_site = Some(0);
        }
        (index, false)
    }
}

/// Remove the site whose label is `name`. Adjust `default_site` the way the
/// terminal does. Returns the removed site.
pub fn remove_named(config: &mut SiteConfig, name: &str) -> Option<ConnectionInfo> {
    let index = config.sites.iter().position(|site| site.name == name)?;
    let removed = config.sites.remove(index);
    if let Some(default_idx) = config.default_site {
        config.default_site = if config.sites.is_empty() {
            None
        } else if default_idx == index {
            Some(0)
        } else if default_idx > index {
            Some(default_idx - 1)
        } else {
            Some(default_idx)
        };
    }
    Some(removed)
}

/// Make `index` the default by swapping it to position 0.
pub fn make_default(config: &mut SiteConfig, index: usize) -> bool {
    if config.sites.is_empty() || index >= config.sites.len() {
        return false;
    }
    if index != 0 {
        config.sites.swap(0, index);
    }
    config.default_site = Some(0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_ftp_core::Protocol;

    fn site(name: &str, host: &str, port: u16) -> ConnectionInfo {
        ConnectionInfo {
            name: name.into(),
            host: host.into(),
            port,
            protocol: Protocol::Sftp,
            username: "ada".into(),
            password: Some("s3cret-value".into()),
            private_key: None,
            initial_path: "/".into(),
        }
    }

    #[test]
    fn first_save_becomes_default_zero_and_update_keeps_a_later_default() {
        let mut config = SiteConfig::default();
        let (index, updated) = upsert(&mut config, site("Desk", "files.example", 22));
        assert_eq!((index, updated), (0, false));
        assert_eq!(config.default_site, Some(0));
        let _ = upsert(&mut config, site("Other", "other.example", 22));
        config.default_site = Some(1);
        let (index, updated) = upsert(&mut config, site("Desk2", "files.example", 22));
        assert_eq!((index, updated), (0, true));
        assert_eq!(config.default_site, Some(1));
        assert_eq!(config.sites[0].name, "Desk2");
    }

    #[test]
    fn delete_shifts_the_default_index() {
        let mut config = SiteConfig {
            sites: vec![
                site("A", "a.example", 22),
                site("B", "b.example", 22),
                site("C", "c.example", 22),
            ],
            default_site: Some(2),
        };
        let removed = remove_named(&mut config, "B").unwrap();
        assert_eq!(removed.name, "B");
        assert_eq!(config.default_site, Some(1));
        assert!(make_default(&mut config, 1));
        assert_eq!(config.sites[0].name, "C");
        assert_eq!(config.default_site, Some(0));
    }
}
