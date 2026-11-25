//! Same theme lookup as the terminal: `./dd_ftp_theme.yml`, then
//! `$HOME/.config/ldnddev/dd_ftp_theme.yml`, then the built-in palette.
//! An `omarchy` key is applied through the theme crate.

use std::path::{Path, PathBuf};

use ldnddev_theme::{
    apply_omarchy, load_from_str, Palette, ParseMode, ThemeSource,
};

pub const THEME_FILE: &str = "dd_ftp_theme.yml";

#[derive(Clone, Debug)]
pub struct LoadedTheme {
    pub palette: Palette,
    pub label: String,
    pub warning: Option<String>,
}

pub fn load_theme(project_root: &Path, home: Option<&Path>, omarchy: &ldnddev_theme::OmarchyPaths) -> LoadedTheme {
    let mut candidates = vec![(
        ThemeSource::Local,
        project_root.join(THEME_FILE),
    )];
    if let Some(home) = home {
        candidates.push((
            ThemeSource::Global,
            home.join(".config").join("ldnddev").join(THEME_FILE),
        ));
    }
    for (source, path) in candidates {
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match load_from_str(&text, ParseMode::Strict, source) {
            Ok(palette) => return finish(palette, path.display().to_string(), omarchy, None),
            Err(err) => {
                return LoadedTheme {
                    palette: Palette::builtin(),
                    label: path.display().to_string(),
                    warning: Some(format!("could not parse theme file: {err}")),
                };
            }
        }
    }
    LoadedTheme {
        palette: Palette::builtin(),
        label: "built-in".to_string(),
        warning: None,
    }
}

fn finish(
    palette: Palette,
    label: String,
    omarchy: &ldnddev_theme::OmarchyPaths,
    warning: Option<String>,
) -> LoadedTheme {
    match apply_omarchy(&palette, omarchy) {
        Ok(resolved) => LoadedTheme {
            palette: resolved,
            label,
            warning,
        },
        Err(err) => LoadedTheme {
            palette: Palette::builtin(),
            label,
            warning: Some(format!(
                "omarchy theme failed ({err}); using built-in defaults"
            )),
        },
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
