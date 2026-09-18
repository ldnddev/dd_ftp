use crate::fields::{ColorField, COLOR_FIELDS};
use crate::palette::{Palette, ThemeSource};
use crate::rgb::{parse_hex_color, Rgb};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseMode {
    Strict,
    Lenient,
}

#[derive(Debug, Deserialize)]
struct RawThemeFile {
    version: Option<serde_yaml::Value>,
    #[serde(default)]
    header_quotes: Vec<String>,
    colors: Option<HashMap<String, String>>,
}

pub fn load_from_str(content: &str, mode: ParseMode, source: ThemeSource) -> Result<Palette> {
    let file: RawThemeFile = serde_yaml::from_str(content).context("Failed to parse theme YAML")?;
    let version = parse_version(&file.version)?;
    if version != crate::SUPPORTED_THEME_VERSION {
        return Err(anyhow!(
            "Unsupported theme schema version `{version}`; expected `{}`",
            crate::SUPPORTED_THEME_VERSION
        ));
    }
    let raw = file.colors.unwrap_or_default();
    let mut parsed: HashMap<String, Rgb> = HashMap::new();
    for (key, value) in raw {
        if value.trim().is_empty() {
            return Err(anyhow!("Missing color value for theme key `{key}`"));
        }
        parsed.insert(key.clone(), parse_hex_color(&key, &value)?);
    }

    if mode == ParseMode::Strict {
        for field in COLOR_FIELDS {
            if !parsed.contains_key(field.key) {
                return Err(anyhow!("Missing required theme color `{}`", field.key));
            }
        }
    }

    let mut palette = Palette::builtin();
    for (key, rgb) in parsed {
        palette.set(&key, rgb);
    }
    palette.header_quotes = file.header_quotes;
    palette.version = version;
    palette.source = source;
    Ok(palette)
}

pub fn load_from_file(path: &Path, mode: ParseMode, source: ThemeSource) -> Result<Palette> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read theme file: {}", path.display()))?;
    load_from_str(&content, mode, source)
        .with_context(|| format!("Failed to parse theme file: {}", path.display()))
}

pub fn load_lookup(
    project_root: &Path,
    filename: &str,
    config_home: Option<&Path>,
    mode: ParseMode,
) -> Result<Palette> {
    let local = crate::paths::local_theme_path(project_root, filename);
    if local.exists() {
        return load_from_file(&local, mode, ThemeSource::Local);
    }
    if let Some(home) = config_home {
        let global = crate::paths::global_theme_path(home, filename);
        if global.exists() {
            return load_from_file(&global, mode, ThemeSource::Global);
        }
    }
    Ok(Palette::builtin())
}

pub fn render_yaml(palette: &Palette, fields: &[ColorField]) -> String {
    let mut out = String::from("version: 1\n");
    if !palette.header_quotes.is_empty() {
        out.push_str("header_quotes:\n");
        for quote in &palette.header_quotes {
            out.push_str("  - ");
            out.push_str(&yaml_quote(quote));
            out.push('\n');
        }
    }
    out.push_str("colors:\n");
    let mut last_group = "";
    let mut written = std::collections::HashSet::new();
    for field in fields {
        if field.group != last_group {
            out.push_str("\n  # ");
            out.push_str(field.group);
            out.push('\n');
            last_group = field.group;
        }
        let hex = palette
            .get(field.key)
            .map(|c| c.to_hex())
            .unwrap_or_else(|| "#000000".to_string());
        out.push_str("  ");
        out.push_str(field.key);
        out.push_str(": \"");
        out.push_str(&hex);
        out.push_str("\"\n");
        written.insert(field.key);
    }
    let extras: Vec<_> = palette
        .extra_keys()
        .into_iter()
        .filter(|k| !written.contains(k.as_str()))
        .collect();
    if !extras.is_empty() {
        out.push_str("\n  # Extra\n");
        for key in extras {
            if let Some(rgb) = palette.get(&key) {
                out.push_str("  ");
                out.push_str(&key);
                out.push_str(": \"");
                out.push_str(&rgb.to_hex());
                out.push_str("\"\n");
            }
        }
    }
    out
}

pub fn save_theme(
    palette: &Palette,
    project_root: &Path,
    filename: &str,
    target: crate::editor::ThemeSaveTarget,
    config_home: Option<&Path>,
    fields: &[ColorField],
) -> Result<PathBuf> {
    let path = match target {
        crate::editor::ThemeSaveTarget::Local => {
            crate::paths::local_theme_path(project_root, filename)
        }
        crate::editor::ThemeSaveTarget::Global => {
            let home = config_home
                .map(Path::to_path_buf)
                .or_else(crate::paths::default_config_home)
                .ok_or_else(|| {
                    anyhow!("Cannot save global theme: XDG_CONFIG_HOME and HOME are unset")
                })?;
            crate::paths::global_theme_path(&home, filename)
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create theme directory {}", parent.display()))?;
    }
    fs::write(&path, render_yaml(palette, fields))
        .with_context(|| format!("Failed to write theme file {}", path.display()))?;
    Ok(path)
}

fn parse_version(version: &Option<serde_yaml::Value>) -> Result<u64> {
    let Some(value) = version else {
        return Err(anyhow!(
            "Missing required theme key `version`; expected `{}`",
            crate::SUPPORTED_THEME_VERSION
        ));
    };
    match value {
        serde_yaml::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| anyhow!("Theme key `version` must be an integer")),
        serde_yaml::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Err(anyhow!("Missing value for theme key `version`"));
            }
            s.parse::<u64>()
                .context("Theme key `version` must be an integer")
        }
        serde_yaml::Value::Null => Err(anyhow!("Missing value for theme key `version`")),
        _ => Err(anyhow!("Theme key `version` must be an integer")),
    }
}

fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
