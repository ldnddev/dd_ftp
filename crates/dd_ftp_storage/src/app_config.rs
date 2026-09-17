use std::{fs, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// User settings for dd_ftp. Not the theme — colors stay in `dd_ftp_theme.yml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// `$VISUAL` / `$EDITOR` still win when set. Empty / omitted = use env then `vi`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
}

impl AppConfig {
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/ldnddev/dd_ftp.toml")
    }

    pub fn from_toml(content: &str) -> Result<Self> {
        Ok(toml::from_str(content)?)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn load_or_default() -> Result<Self> {
        let path = Self::default_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        Self::from_toml(&content)
    }

    pub fn save_to_default_path(&self) -> Result<PathBuf> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, self.to_toml()?)?;
        Ok(path)
    }

    pub fn editor_or_empty(&self) -> String {
        self.editor.clone().unwrap_or_default()
    }

    pub fn with_editor(editor: &str) -> Self {
        let trimmed = editor.trim();
        Self {
            editor: if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_serializes_without_editor_key() {
        let toml = AppConfig::default().to_toml().unwrap();
        assert!(!toml.contains("editor"));
    }

    #[test]
    fn round_trip_editor() {
        let cfg = AppConfig::with_editor("  hx  ");
        assert_eq!(cfg.editor.as_deref(), Some("hx"));
        let parsed = AppConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn blank_editor_is_unset() {
        let cfg = AppConfig::with_editor("   ");
        assert_eq!(cfg.editor, None);
    }

    #[test]
    fn default_path_is_under_ldnddev() {
        let p = AppConfig::default_path();
        let s = p.to_string_lossy();
        assert!(s.ends_with("ldnddev/dd_ftp.toml") || s.ends_with("ldnddev\\dd_ftp.toml"));
    }
}
