//! Window prefs. Density and Vim keys live here, never in the theme YAML
//! and never in `dd_ftp.toml`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

impl Density {
    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Comfortable => Self::Compact,
            Self::Compact => Self::Comfortable,
        }
    }

    pub fn row(self) -> f32 {
        match self {
            Self::Comfortable => 28.0,
            Self::Compact => 24.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiSettings {
    #[serde(default)]
    pub density: Density,
    /// On by default, matching the terminal.
    #[serde(default = "default_true")]
    pub vim_keys: bool,
    #[serde(default)]
    pub reduced_motion: bool,
}

fn default_true() -> bool {
    true
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            density: Density::Comfortable,
            vim_keys: true,
            reduced_motion: false,
        }
    }
}

/// `$XDG_CONFIG_HOME/ldnddev/dd_ftp/gui.json`, or `~/.config/ldnddev/dd_ftp/gui.json`.
pub fn settings_path() -> PathBuf {
    let home = ldnddev_theme::default_config_home().unwrap_or_else(|| PathBuf::from(".config"));
    home.join("ldnddev").join("dd_ftp").join("gui.json")
}

pub fn load_from(path: &Path) -> GuiSettings {
    let Ok(bytes) = fs::read(path) else {
        return GuiSettings::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save_to(path: &Path, settings: &GuiSettings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(settings).unwrap_or_else(|_| b"{}".to_vec());
    fs::write(path, json)
}

/// Owner-only write, matching `dd_ftp_storage`'s session file.
pub fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(contents.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_keys_default_on_and_round_trip_skips_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let gui = dir.path().join("gui.json");
        let toml = dir.path().join("dd_ftp.toml");
        let before = b"editor = \"hx\"\n".to_vec();
        fs::write(&toml, &before).unwrap();
        let settings = GuiSettings {
            density: Density::Compact,
            vim_keys: false,
            reduced_motion: false,
        };
        save_to(&gui, &settings).unwrap();
        assert_eq!(load_from(&gui), settings);
        assert_eq!(fs::read(&toml).unwrap(), before);
        let text = fs::read_to_string(&gui).unwrap();
        assert!(text.contains("compact"));
        assert!(text.contains("vim_keys"));
        assert!(!text.contains("editor"));
        let missing = load_from(&dir.path().join("nope.json"));
        assert!(missing.vim_keys);
        assert_eq!(missing.density, Density::Comfortable);
    }
}
