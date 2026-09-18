use std::path::{Path, PathBuf};

pub fn default_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

pub fn local_theme_path(project_root: &Path, filename: &str) -> PathBuf {
    project_root.join(filename)
}

pub fn global_theme_path(config_home: &Path, filename: &str) -> PathBuf {
    config_home.join("ldnddev").join(filename)
}
