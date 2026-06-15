use ratatui::style::Color;
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Theme {
    pub base_background: Color,
    pub body_background: Color,
    pub modal_background: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_labels: Color,
    pub text_labels_active: Color,
    pub text_active_focus: Color,
    pub modal_labels: Color,
    pub modal_text: Color,
    pub selected_background: Color,
    pub border_default: Color,
    pub border_active: Color,
    pub input_border_default: Color,
    pub input_border_focus: Color,
    pub input_text_default: Color,
    pub input_text_focus: Color,
    pub cursor: Color,
    pub scrollbar: Color,
    pub scrollbar_hover: Color,
    pub selection: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub folder: Color,
    pub file: Color,
    pub link: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Canonical LDNDDEV TUI Visual Standard v1 tokens.
            base_background: Color::Rgb(0x0f, 0x11, 0x14),
            body_background: Color::Rgb(0x2a, 0x2d, 0x31),
            modal_background: Color::Rgb(0x1c, 0x1e, 0x21),
            text_primary: Color::Rgb(0xf5, 0xf6, 0xf7),
            text_secondary: Color::Rgb(0x9e, 0xa3, 0xaa),
            text_labels: Color::Rgb(0xff, 0xaf, 0x46),
            text_labels_active: Color::Rgb(0x64, 0xb4, 0xf5),
            text_active_focus: Color::Rgb(0x64, 0xb4, 0xf5),
            modal_labels: Color::Rgb(0x64, 0xb4, 0xf5),
            modal_text: Color::Rgb(0xf5, 0xf6, 0xf7),
            selected_background: Color::Rgb(0x0f, 0x11, 0x14),
            border_default: Color::Rgb(0xf5, 0xf6, 0xf7),
            border_active: Color::Rgb(0x64, 0xb4, 0xf5),
            input_border_default: Color::Rgb(0xf5, 0xf6, 0xf7),
            input_border_focus: Color::Rgb(0x64, 0xb4, 0xf5),
            input_text_default: Color::Rgb(0xf5, 0xf6, 0xf7),
            input_text_focus: Color::Rgb(0x64, 0xb4, 0xf5),
            cursor: Color::Rgb(0x64, 0xb4, 0xf5),
            scrollbar: Color::Rgb(0xff, 0xa0, 0x87),
            scrollbar_hover: Color::Rgb(0x64, 0xb4, 0xf5),
            // App-specific extra: distinct background for in-field text selection
            // (drag-select). Not part of the canonical token set.
            selection: Color::Rgb(0x20, 0x60, 0xa0),
            success: Color::Rgb(0x82, 0xe0, 0xaa),
            warning: Color::Rgb(0xf5, 0xc4, 0x69),
            error: Color::Rgb(0xe5, 0x73, 0x73),
            info: Color::Rgb(0x5d, 0xad, 0xe2),
            folder: Color::Rgb(0x64, 0xb4, 0xf5),
            file: Color::Rgb(0xff, 0xaf, 0x46),
            link: Color::Rgb(0xff, 0xa0, 0x87),
        }
    }
}

/// Theme schema version this build understands (LDNDDEV TUI Visual Standard).
pub const THEME_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct ThemeFile {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    header_quotes: Option<Vec<String>>,
    colors: ThemeColors,
}

#[derive(Debug, Deserialize)]
struct ThemeColors {
    base_background: String,
    body_background: String,
    modal_background: String,
    text_primary: String,
    text_secondary: String,
    text_labels: String,
    text_labels_active: Option<String>,
    text_active_focus: Option<String>,
    modal_labels: String,
    modal_text: String,
    selected_background: String,
    border_default: String,
    border_active: String,
    input_border_default: String,
    input_border_focus: String,
    input_text_default: String,
    input_text_focus: String,
    cursor: Option<String>,
    scrollbar: Option<String>,
    scrollbar_hover: Option<String>,
    selection: Option<String>,
    success: String,
    warning: String,
    error: String,
    info: String,
    folders: Option<String>,
    files: Option<String>,
    links: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum ThemeSource {
    Local,
    Global,
    Default,
}

impl ThemeSource {
    pub fn label(self) -> &'static str {
        match self {
            ThemeSource::Local => "local",
            ThemeSource::Global => "global",
            ThemeSource::Default => "default",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedTheme {
    pub theme: Theme,
    pub source: ThemeSource,
    pub path: Option<PathBuf>,
    /// Schema version declared by the loaded file (None for built-in defaults
    /// or files missing the field).
    pub version: Option<u32>,
    /// Non-fatal load warning (bad version, parse fallback). Surfaced at
    /// startup as a toast and in the F2 Credits modal.
    pub warning: Option<String>,
    /// Optional per-theme header tagline override. Empty = use built-in list.
    pub header_quotes: Vec<String>,
}

pub fn load_theme() -> Theme {
    load_theme_with_source().theme
}

pub fn load_theme_with_source() -> LoadedTheme {
    let candidates = [
        (ThemeSource::Local, PathBuf::from("dd_ftp_theme.yml")),
        (
            ThemeSource::Global,
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".config/ldnddev/dd_ftp_theme.yml"),
        ),
    ];

    for (source, path) in candidates {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(theme_file) = parse_theme(&content) {
                    let version = theme_file.version;
                    let header_quotes = theme_file.header_quotes.clone().unwrap_or_default();
                    let shown = path.display().to_string();
                    return match version {
                        Some(v) if v == THEME_SCHEMA_VERSION => LoadedTheme {
                            theme: map_theme(theme_file),
                            source,
                            path: Some(path),
                            version,
                            warning: None,
                            header_quotes,
                        },
                        Some(v) => LoadedTheme {
                            // Unsupported schema version: fall back to defaults.
                            theme: Theme::default(),
                            source,
                            path: Some(path),
                            version,
                            warning: Some(format!(
                                "theme {shown}: unsupported version {v} (expected {THEME_SCHEMA_VERSION}); using built-in defaults"
                            )),
                            header_quotes,
                        },
                        None => LoadedTheme {
                            // Missing version: load leniently but warn.
                            theme: map_theme(theme_file),
                            source,
                            path: Some(path),
                            version,
                            warning: Some(format!(
                                "theme {shown}: missing 'version: {THEME_SCHEMA_VERSION}' field; loaded anyway"
                            )),
                            header_quotes,
                        },
                    };
                }
            }
        }
    }

    LoadedTheme {
        theme: Theme::default(),
        source: ThemeSource::Default,
        path: None,
        version: None,
        warning: None,
        header_quotes: Vec::new(),
    }
}

fn parse_theme(content: &str) -> anyhow::Result<ThemeFile> {
    // Native YAML parser first (your provided format).
    if let Ok(v) = serde_yaml::from_str::<ThemeFile>(content) {
        return Ok(v);
    }

    // TOML compatibility fallback for older files.
    if let Ok(v) = toml::from_str::<ThemeFile>(content) {
        return Ok(v);
    }

    // Last-resort simple parser.
    let mut map = std::collections::HashMap::<String, String>::new();
    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l == "colors:" {
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }

    let get = |k: &str| -> anyhow::Result<String> {
        map.get(k)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing theme key: {k}"))
    };

    Ok(ThemeFile {
        version: map
            .get("version")
            .and_then(|v| v.trim().parse::<u32>().ok()),
        // The line-based fallback parser does not handle YAML lists.
        header_quotes: None,
        colors: ThemeColors {
            base_background: get("base_background")?,
            body_background: get("body_background")?,
            modal_background: get("modal_background")?,
            text_primary: get("text_primary")?,
            text_secondary: get("text_secondary")?,
            text_labels: get("text_labels")?,
            text_labels_active: map.get("text_labels_active").cloned(),
            text_active_focus: map.get("text_active_focus").cloned(),
            modal_labels: get("modal_labels")?,
            modal_text: get("modal_text")?,
            selected_background: get("selected_background")?,
            border_default: get("border_default")?,
            border_active: get("border_active")?,
            input_border_default: get("input_border_default")?,
            input_border_focus: get("input_border_focus")?,
            input_text_default: get("input_text_default")?,
            input_text_focus: get("input_text_focus")?,
            cursor: map.get("cursor").cloned(),
            scrollbar: map.get("scrollbar").cloned(),
            scrollbar_hover: map.get("scrollbar_hover").cloned(),
            selection: map.get("selection").cloned(),
            success: get("success")?,
            warning: get("warning")?,
            error: get("error")?,
            info: get("info")?,
            folders: map.get("folders").cloned(),
            files: map.get("files").cloned(),
            links: map.get("links").cloned(),
        },
    })
}

fn map_theme(tf: ThemeFile) -> Theme {
    let c = tf.colors;
    Theme {
        base_background: parse_hex(&c.base_background).unwrap_or(Theme::default().base_background),
        body_background: parse_hex(&c.body_background).unwrap_or(Theme::default().body_background),
        modal_background: parse_hex(&c.modal_background)
            .unwrap_or(Theme::default().modal_background),
        text_primary: parse_hex(&c.text_primary).unwrap_or(Theme::default().text_primary),
        text_secondary: parse_hex(&c.text_secondary).unwrap_or(Theme::default().text_secondary),
        text_labels: parse_hex(&c.text_labels).unwrap_or(Theme::default().text_labels),
        text_labels_active: c
            .text_labels_active
            .as_deref()
            .and_then(parse_hex)
            .or_else(|| c.text_active_focus.as_deref().and_then(parse_hex))
            .unwrap_or(Theme::default().text_labels_active),
        text_active_focus: c
            .text_active_focus
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().text_active_focus),
        modal_labels: parse_hex(&c.modal_labels).unwrap_or(Theme::default().modal_labels),
        modal_text: parse_hex(&c.modal_text).unwrap_or(Theme::default().modal_text),
        selected_background: parse_hex(&c.selected_background)
            .unwrap_or(Theme::default().selected_background),
        border_default: parse_hex(&c.border_default).unwrap_or(Theme::default().border_default),
        border_active: parse_hex(&c.border_active).unwrap_or(Theme::default().border_active),
        input_border_default: parse_hex(&c.input_border_default)
            .unwrap_or(Theme::default().input_border_default),
        input_border_focus: parse_hex(&c.input_border_focus)
            .unwrap_or(Theme::default().input_border_focus),
        input_text_default: parse_hex(&c.input_text_default)
            .unwrap_or(Theme::default().input_text_default),
        input_text_focus: parse_hex(&c.input_text_focus)
            .unwrap_or(Theme::default().input_text_focus),
        cursor: c
            .cursor
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().cursor),
        scrollbar: c
            .scrollbar
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().scrollbar),
        scrollbar_hover: c
            .scrollbar_hover
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().scrollbar_hover),
        selection: c
            .selection
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().selection),
        success: parse_hex(&c.success).unwrap_or(Theme::default().success),
        warning: parse_hex(&c.warning).unwrap_or(Theme::default().warning),
        error: parse_hex(&c.error).unwrap_or(Theme::default().error),
        info: parse_hex(&c.info).unwrap_or(Theme::default().info),
        folder: c
            .folders
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().folder),
        file: c
            .files
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().file),
        link: c
            .links
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(Theme::default().link),
    }
}

fn parse_hex(input: &str) -> Option<Color> {
    let s = input.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
