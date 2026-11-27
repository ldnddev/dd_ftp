use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use dd_ftp_core::ConnectionInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteConfig {
    pub sites: Vec<ConnectionInfo>,
    pub default_site: Option<usize>,
}

pub struct SiteManager;

impl SiteManager {
    pub fn load_from_toml(content: &str) -> Result<SiteConfig> {
        Ok(toml::from_str(content)?)
    }

    pub fn save_to_toml(config: &SiteConfig) -> Result<String> {
        Ok(toml::to_string_pretty(config)?)
    }

    pub fn default_config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/dd_ftp/sites.toml")
    }

    pub fn load_or_default() -> Result<SiteConfig> {
        Self::load_path(&Self::default_config_path())
    }

    /// Load `path`, or an empty config when the file is missing. Passwords in
    /// the file are dropped. `sites.toml` is not a secret store.
    pub fn load_path(path: &Path) -> Result<SiteConfig> {
        if !path.exists() {
            return Ok(SiteConfig::default());
        }

        let content = fs::read_to_string(path)?;
        let mut cfg = Self::load_from_toml(&content)?;
        for site in cfg.sites.iter_mut() {
            site.password = None;
        }
        Ok(cfg)
    }

    pub fn save_to_default_path(config: &SiteConfig) -> Result<()> {
        Self::save_path(&Self::default_config_path(), config)
    }

    /// Write `config` to `path` as mode `0600`. The password field is cleared
    /// before the write.
    pub fn save_path(path: &Path, config: &SiteConfig) -> Result<()> {
        if let Some(parent) = path.parent() {
            crate::persist::ensure_private_dir(parent)?;
        }

        let mut sanitized = config.clone();
        for site in sanitized.sites.iter_mut() {
            site.password = None;
        }

        let content = Self::save_to_toml(&sanitized)?;
        crate::persist::write_private(path, &content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_ftp_core::Protocol;

    #[test]
    fn save_to_toml_omits_password_even_if_the_caller_left_it_set() {
        let site = ConnectionInfo {
            protocol: Protocol::Ftp,
            password: Some("s3cret-value".into()),
            ..ConnectionInfo::default()
        };
        let cfg = SiteConfig {
            sites: vec![site],
            default_site: None,
        };
        let text = SiteManager::save_to_toml(&cfg).unwrap();
        assert!(!text.contains("s3cret-value"));
        assert!(!text.contains("password"));
    }
}
