use anyhow::Result;
use dd_ftp_core::ConnectionInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteConfig {
    pub sites: Vec<ConnectionInfo>,
}

pub struct SiteManager;

impl SiteManager {
    pub fn load_from_toml(content: &str) -> Result<SiteConfig> {
        Ok(toml::from_str(content)?)
    }

    pub fn save_to_toml(config: &SiteConfig) -> Result<String> {
        Ok(toml::to_string_pretty(config)?)
    }
}
