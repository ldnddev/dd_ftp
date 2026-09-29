use std::{fs, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// User settings for dd_ftp. Not the theme — colors stay in `dd_ftp_theme.yml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// `$VISUAL` / `$EDITOR` still win when set. Empty / omitted = use env then `vi`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// Parallel transfer workers. Omitted = 2. Clamped to 1..=8 when applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_max_concurrency: Option<u32>,
    /// Restored as the local pane cwd at launch when the path is still a directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_local_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_dotfiles: bool,
    /// `ask`, `skip_all`, or `overwrite_all`. Omitted = ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite_policy: Option<String>,
    /// Connect the default bookmark when the app starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_connect_default: bool,
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
            ..Self::default()
        }
    }

    pub fn worker_max_or_default(&self) -> usize {
        self.worker_max_concurrency.unwrap_or(2).clamp(1, 8) as usize
    }

    pub fn overwrite_policy_or_default(&self) -> &str {
        self.overwrite_policy.as_deref().unwrap_or("ask")
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

    #[test]
    fn missing_keys_default_and_round_trip_new_fields() {
        let parsed = AppConfig::from_toml("editor = \"hx\"\n").unwrap();
        assert_eq!(parsed.editor.as_deref(), Some("hx"));
        assert_eq!(parsed.worker_max_or_default(), 2);
        assert!(!parsed.hide_dotfiles);
        assert_eq!(parsed.overwrite_policy_or_default(), "ask");
        assert!(!parsed.auto_connect_default);
        assert!(parsed.last_local_cwd.is_none());

        let cfg = AppConfig {
            editor: Some("nvim".into()),
            worker_max_concurrency: Some(4),
            last_local_cwd: Some("/tmp".into()),
            hide_dotfiles: true,
            overwrite_policy: Some("skip_all".into()),
            auto_connect_default: true,
        };
        let parsed = AppConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(parsed, cfg);
        assert_eq!(parsed.worker_max_or_default(), 4);
    }

    #[test]
    fn worker_max_clamps_to_1_through_8() {
        let mut cfg = AppConfig {
            worker_max_concurrency: Some(0),
            ..Default::default()
        };
        assert_eq!(cfg.worker_max_or_default(), 1);
        cfg.worker_max_concurrency = Some(99);
        assert_eq!(cfg.worker_max_or_default(), 8);
    }
}
