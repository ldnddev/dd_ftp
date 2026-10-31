use ratatui::style::Color;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::{fs, sync::RwLock};

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
    pub omarchy: Option<ldnddev_theme::OmarchyChoice>,
    pub overrides: BTreeSet<String>,
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
            omarchy: None,
            overrides: BTreeSet::new(),
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

static THEME: RwLock<Option<LoadedTheme>> = RwLock::new(None);

pub fn cached_theme() -> LoadedTheme {
    match THEME.read() {
        Ok(guard) => {
            if let Some(t) = guard.as_ref() {
                return t.clone();
            }
        }
        Err(poisoned) => {
            if let Some(t) = poisoned.get_ref().as_ref() {
                return t.clone();
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

pub fn reload_theme() -> LoadedTheme {
    let loaded = load_theme_with_source();
    *THEME.write().unwrap() = Some(loaded.clone());
    loaded
}

pub fn load_theme() -> Theme {
    load_theme_with_source().theme
}

pub fn load_theme_with_source() -> LoadedTheme {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    load_resolved(&cwd, Some(&home), &ldnddev_theme::OmarchyPaths::system())
}

fn load_resolved(
    project_root: &Path,
    home: Option<&Path>,
    omarchy: &ldnddev_theme::OmarchyPaths,
) -> LoadedTheme {
    let mut candidates = vec![(ThemeSource::Local, project_root.join("dd_ftp_theme.yml"))];
    if let Some(home) = home {
        candidates.push((
            ThemeSource::Global,
            home.join(".config/ldnddev/dd_ftp_theme.yml"),
        ));
    }

    for (source, path) in candidates {
        if !path.exists() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(loaded) = interpret_theme(&content, source, &path, omarchy) {
            return loaded;
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

fn interpret_theme(
    content: &str,
    source: ThemeSource,
    path: &Path,
    omarchy: &ldnddev_theme::OmarchyPaths,
) -> Option<LoadedTheme> {
    match ldnddev_theme::load_from_str(content, ldnddev_theme::ParseMode::Strict, to_ldnd(source)) {
        Ok(palette) => Some(finish_palette(palette, source, path, omarchy, None)),
        Err(err) => {
            let msg = err.to_string();
            if let Some(version) = unsupported_version(&msg) {
                let shown = path.display();
                return Some(LoadedTheme {
                    theme: Theme::default(),
                    source,
                    path: Some(path.to_path_buf()),
                    version: Some(version),
                    warning: Some(format!(
                        "theme {shown}: unsupported version {version} (expected {THEME_SCHEMA_VERSION}); using built-in defaults"
                    )),
                    header_quotes: probe_quotes(content),
                });
            }
            let theme_file = parse_theme(content).ok()?;
            let version = theme_file.version;
            let header_quotes = theme_file.header_quotes.clone().unwrap_or_default();
            let shown = path.display().to_string();
            match version {
                Some(v) if v == THEME_SCHEMA_VERSION => {
                    let theme = map_theme(theme_file);
                    let mut palette = palette_from_theme(&theme, header_quotes);
                    palette.source = to_ldnd(source);
                    palette.version = u64::from(v);
                    Some(finish_palette(palette, source, path, omarchy, None))
                }
                Some(v) => Some(LoadedTheme {
                    theme: Theme::default(),
                    source,
                    path: Some(path.to_path_buf()),
                    version: Some(v),
                    warning: Some(format!(
                        "theme {shown}: unsupported version {v} (expected {THEME_SCHEMA_VERSION}); using built-in defaults"
                    )),
                    header_quotes,
                }),
                None => Some(LoadedTheme {
                    theme: map_theme(theme_file),
                    source,
                    path: Some(path.to_path_buf()),
                    version: None,
                    warning: Some(format!(
                        "theme {shown}: missing 'version: {THEME_SCHEMA_VERSION}' field; loaded anyway"
                    )),
                    header_quotes,
                }),
            }
        }
    }
}

fn finish_palette(
    palette: ldnddev_theme::Palette,
    source: ThemeSource,
    path: &Path,
    omarchy: &ldnddev_theme::OmarchyPaths,
    warning: Option<String>,
) -> LoadedTheme {
    let quotes = palette.header_quotes.clone();
    let version = u32::try_from(palette.version).ok();
    match ldnddev_theme::apply_omarchy(&palette, omarchy) {
        Ok(resolved) => LoadedTheme {
            theme: theme_from_palette(&resolved),
            source,
            path: Some(path.to_path_buf()),
            version,
            warning,
            header_quotes: resolved.header_quotes,
        },
        Err(err) => LoadedTheme {
            theme: Theme::default(),
            source: ThemeSource::Default,
            path: Some(path.to_path_buf()),
            version,
            warning: Some(format!(
                "omarchy theme failed ({err}); using built-in defaults"
            )),
            header_quotes: quotes,
        },
    }
}

fn to_ldnd(source: ThemeSource) -> ldnddev_theme::ThemeSource {
    match source {
        ThemeSource::Local => ldnddev_theme::ThemeSource::Local,
        ThemeSource::Global => ldnddev_theme::ThemeSource::Global,
        ThemeSource::Default => ldnddev_theme::ThemeSource::Default,
    }
}

fn unsupported_version(msg: &str) -> Option<u32> {
    let marker = "Unsupported theme schema version `";
    let start = msg.find(marker)? + marker.len();
    let rest = msg.get(start..)?;
    let end = rest.find('`')?;
    rest[..end].parse().ok()
}

fn probe_quotes(content: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct Probe {
        #[serde(default)]
        header_quotes: Vec<String>,
    }
    serde_yaml::from_str::<Probe>(content)
        .map(|probe| probe.header_quotes)
        .unwrap_or_default()
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
        omarchy: None,
        overrides: BTreeSet::new(),
    }
}

fn theme_from_palette(palette: &ldnddev_theme::Palette) -> Theme {
    let get = |key: &str| palette.get(key).map(rgb_to_color);
    let fallback = Theme::default();
    let text_active_focus = get("text_active_focus").unwrap_or(fallback.text_active_focus);
    Theme {
        base_background: get("base_background").unwrap_or(fallback.base_background),
        body_background: get("body_background").unwrap_or(fallback.body_background),
        modal_background: get("modal_background").unwrap_or(fallback.modal_background),
        text_primary: get("text_primary").unwrap_or(fallback.text_primary),
        text_secondary: get("text_secondary").unwrap_or(fallback.text_secondary),
        text_labels: get("text_labels").unwrap_or(fallback.text_labels),
        text_labels_active: get("text_labels_active").unwrap_or(text_active_focus),
        text_active_focus,
        modal_labels: get("modal_labels").unwrap_or(fallback.modal_labels),
        modal_text: get("modal_text").unwrap_or(fallback.modal_text),
        selected_background: get("selected_background").unwrap_or(fallback.selected_background),
        border_default: get("border_default").unwrap_or(fallback.border_default),
        border_active: get("border_active").unwrap_or(fallback.border_active),
        input_border_default: get("input_border_default").unwrap_or(fallback.input_border_default),
        input_border_focus: get("input_border_focus").unwrap_or(fallback.input_border_focus),
        input_text_default: get("input_text_default").unwrap_or(fallback.input_text_default),
        input_text_focus: get("input_text_focus").unwrap_or(fallback.input_text_focus),
        cursor: get("cursor").unwrap_or(fallback.cursor),
        scrollbar: get("scrollbar").unwrap_or(fallback.scrollbar),
        scrollbar_hover: get("scrollbar_hover").unwrap_or(fallback.scrollbar_hover),
        selection: get("selection").unwrap_or(fallback.selection),
        success: get("success").unwrap_or(fallback.success),
        warning: get("warning").unwrap_or(fallback.warning),
        error: get("error").unwrap_or(fallback.error),
        info: get("info").unwrap_or(fallback.info),
        folder: get("folders").unwrap_or(fallback.folder),
        file: get("files").unwrap_or(fallback.file),
        link: get("links").unwrap_or(fallback.link),
        omarchy: palette.omarchy.clone(),
        overrides: palette.overrides.clone(),
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

pub fn extra_theme_fields() -> &'static [ldnddev_theme::ColorField] {
    &[ldnddev_theme::EXTRA_SELECTION]
}

fn color_to_rgb(color: Color) -> Option<ldnddev_theme::Rgb> {
    match color {
        Color::Rgb(r, g, b) => Some(ldnddev_theme::Rgb { r, g, b }),
        _ => None,
    }
}

fn rgb_to_color(rgb: ldnddev_theme::Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}

pub fn palette_from_theme(theme: &Theme, quotes: Vec<String>) -> ldnddev_theme::Palette {
    let mut palette = ldnddev_theme::Palette::builtin();
    palette.header_quotes = quotes;
    let pairs = [
        ("base_background", theme.base_background),
        ("body_background", theme.body_background),
        ("modal_background", theme.modal_background),
        ("text_primary", theme.text_primary),
        ("text_secondary", theme.text_secondary),
        ("text_labels", theme.text_labels),
        ("text_active_focus", theme.text_active_focus),
        ("modal_labels", theme.modal_labels),
        ("modal_text", theme.modal_text),
        ("selected_background", theme.selected_background),
        ("border_default", theme.border_default),
        ("border_active", theme.border_active),
        ("scrollbar", theme.scrollbar),
        ("scrollbar_hover", theme.scrollbar_hover),
        ("input_border_default", theme.input_border_default),
        ("input_border_focus", theme.input_border_focus),
        ("input_text_default", theme.input_text_default),
        ("input_text_focus", theme.input_text_focus),
        ("cursor", theme.cursor),
        ("success", theme.success),
        ("warning", theme.warning),
        ("error", theme.error),
        ("info", theme.info),
        ("folders", theme.folder),
        ("files", theme.file),
        ("links", theme.link),
        ("selection", theme.selection),
    ];
    for (key, color) in pairs {
        if let Some(rgb) = color_to_rgb(color) {
            palette.set(key, rgb);
        }
    }
    palette.omarchy = theme.omarchy.clone();
    palette.overrides = theme.overrides.clone();
    palette
}

pub fn apply_palette(theme: &mut Theme, palette: &ldnddev_theme::Palette) {
    let get = |k: &str| palette.get(k).map(rgb_to_color);
    if let Some(c) = get("base_background") {
        theme.base_background = c;
    }
    if let Some(c) = get("body_background") {
        theme.body_background = c;
    }
    if let Some(c) = get("modal_background") {
        theme.modal_background = c;
    }
    if let Some(c) = get("text_primary") {
        theme.text_primary = c;
    }
    if let Some(c) = get("text_secondary") {
        theme.text_secondary = c;
    }
    if let Some(c) = get("text_labels") {
        theme.text_labels = c;
    }
    if let Some(c) = get("text_active_focus") {
        theme.text_active_focus = c;
        theme.text_labels_active = c;
    }
    if let Some(c) = get("modal_labels") {
        theme.modal_labels = c;
    }
    if let Some(c) = get("modal_text") {
        theme.modal_text = c;
    }
    if let Some(c) = get("selected_background") {
        theme.selected_background = c;
    }
    if let Some(c) = get("border_default") {
        theme.border_default = c;
    }
    if let Some(c) = get("border_active") {
        theme.border_active = c;
    }
    if let Some(c) = get("scrollbar") {
        theme.scrollbar = c;
    }
    if let Some(c) = get("scrollbar_hover") {
        theme.scrollbar_hover = c;
    }
    if let Some(c) = get("input_border_default") {
        theme.input_border_default = c;
    }
    if let Some(c) = get("input_border_focus") {
        theme.input_border_focus = c;
    }
    if let Some(c) = get("input_text_default") {
        theme.input_text_default = c;
    }
    if let Some(c) = get("input_text_focus") {
        theme.input_text_focus = c;
    }
    if let Some(c) = get("cursor") {
        theme.cursor = c;
    }
    if let Some(c) = get("success") {
        theme.success = c;
    }
    if let Some(c) = get("warning") {
        theme.warning = c;
    }
    if let Some(c) = get("error") {
        theme.error = c;
    }
    if let Some(c) = get("info") {
        theme.info = c;
    }
    if let Some(c) = get("folders") {
        theme.folder = c;
    }
    if let Some(c) = get("files") {
        theme.file = c;
    }
    if let Some(c) = get("links") {
        theme.link = c;
    }
    if let Some(c) = get("selection") {
        theme.selection = c;
    }
    theme.omarchy = palette.omarchy.clone();
    theme.overrides = palette.overrides.clone();
}

pub fn apply_live_theme(palette: &ldnddev_theme::Palette) {
    let mut loaded = cached_theme();
    apply_palette(&mut loaded.theme, palette);
    loaded.header_quotes = palette.header_quotes.clone();
    *THEME.write().unwrap() = Some(loaded);
}

pub fn apply_saved_theme(palette: &ldnddev_theme::Palette, source: ThemeSource, path: PathBuf) {
    apply_live_theme(palette);
    let mut loaded = cached_theme();
    loaded.source = source;
    loaded.path = Some(path);
    loaded.version = Some(u32::try_from(palette.version).unwrap_or(THEME_SCHEMA_VERSION));
    loaded.warning = None;
    *THEME.write().unwrap() = Some(loaded);
}

pub fn open_theme_ui(app: &mut dd_ftp_app::AppState) {
    let loaded = cached_theme();
    let mut palette = palette_from_theme(&loaded.theme, loaded.header_quotes.clone());
    palette.source = to_ldnd(loaded.source);
    if let Some(version) = loaded.version {
        palette.version = u64::from(version);
    }
    let picker = ldnddev_theme::ThemePicker::new(
        palette.clone(),
        extra_theme_fields(),
        app.omarchy_paths.clone(),
    );
    if picker.lists_omarchy() {
        app.theme_picker = Some(picker);
        app.theme_editor = None;
    } else {
        app.theme_picker = None;
        app.theme_editor = Some(ldnddev_theme::ThemeEditor::new(
            palette,
            extra_theme_fields(),
        ));
    }
}

pub fn close_theme(app: &mut dd_ftp_app::AppState, revert: bool) {
    if revert {
        if let Some(picker) = app.theme_picker.as_ref() {
            let palette = picker.revert_palette();
            apply_live_theme(&palette);
        } else if let Some(mut editor) = app.theme_editor.take() {
            editor.revert();
            apply_live_theme(&editor.palette);
        }
    }
    app.theme_picker = None;
    app.theme_editor = None;
}

/// Re-read `omarchy: follow` when the desktop theme file changes.
/// Does not touch the header tagline.
pub fn refresh_followed_theme(app: &mut dd_ftp_app::AppState) -> bool {
    if app.show_theme_debug {
        return false;
    }
    if !matches!(
        cached_theme().theme.omarchy,
        Some(ldnddev_theme::OmarchyChoice::Follow)
    ) {
        return false;
    }
    let stamp = ldnddev_theme::follow_stamp(&app.omarchy_paths);
    if stamp == app.follow_stamp {
        return false;
    }
    let loaded = reload_at(&app.omarchy_paths);
    app.follow_stamp = stamp;
    if let Some(msg) = loaded.warning {
        app.toast = Some(dd_ftp_app::Toast::warning(msg));
    }
    true
}

fn reload_at(omarchy: &ldnddev_theme::OmarchyPaths) -> LoadedTheme {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let loaded = load_resolved(&cwd, Some(&home), omarchy);
    *THEME.write().unwrap() = Some(loaded.clone());
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_THEME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn cached_theme_twice_succeeds() {
        let _guard = TEST_THEME_LOCK.lock().unwrap();
        let first = cached_theme();
        let second = cached_theme();
        assert_eq!(first.source.label(), second.source.label());
        assert_eq!(first.version, second.version);
    }

    #[test]
    fn reload_theme_replaces_the_cache() {
        let _guard = TEST_THEME_LOCK.lock().unwrap();
        {
            let mut theme_guard = THEME.write().unwrap();
            *theme_guard = Some(LoadedTheme {
                theme: Theme::default(),
                source: ThemeSource::Default,
                path: None,
                version: Some(99),
                warning: Some("sentinel".into()),
                header_quotes: vec!["x".into()],
            });
        }
        let loaded = reload_theme();
        let cached = cached_theme();
        assert_ne!(cached.version, Some(99));
        assert_ne!(cached.warning.as_deref(), Some("sentinel"));
        assert_eq!(cached.version, loaded.version);
        assert_eq!(cached.warning, loaded.warning);
        assert_eq!(cached.source.label(), loaded.source.label());
    }

    fn hex(color: Color) -> String {
        match color {
            Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
            _ => "?".into(),
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dd_ftp_theme_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("tmpdir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_stays_builtin() {
        let dir = TempDir::new();
        let loaded = load_resolved(
            dir.path(),
            None,
            &ldnddev_theme::OmarchyPaths::from_root(dir.path()),
        );
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.source.label(), "default");
        assert!(loaded.theme.omarchy.is_none());
        assert_eq!(hex(loaded.theme.base_background), "#0F1114");
    }

    #[test]
    fn omarchy_link_maps_colors_and_keeps_override() {
        let project = TempDir::new();
        let root = TempDir::new();
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../ldnddev_theme/tests/fixtures/omarchy/tokyo-night/colors.toml");
        fs::create_dir_all(root.path().join("stock/tokyo-night")).expect("mkdir");
        fs::copy(src, root.path().join("stock/tokyo-night/colors.toml")).expect("copy");
        fs::write(
            project.path().join("dd_ftp_theme.yml"),
            "version: 1\nomarchy: tokyo-night\ncolors:\n  text_labels: \"#ABCDEF\"\n",
        )
        .expect("yaml");
        let loaded = load_resolved(
            project.path(),
            None,
            &ldnddev_theme::OmarchyPaths::from_root(root.path()),
        );
        assert!(loaded.warning.is_none(), "{:?}", loaded.warning);
        assert_eq!(hex(loaded.theme.body_background), "#1A1B26");
        assert_eq!(hex(loaded.theme.text_labels), "#ABCDEF");
        assert_eq!(
            loaded.theme.omarchy,
            Some(ldnddev_theme::OmarchyChoice::Theme("tokyo-night".into()))
        );
        assert!(loaded.theme.overrides.contains("text_labels"));
        let palette = palette_from_theme(&loaded.theme, loaded.header_quotes.clone());
        assert_eq!(palette.omarchy, loaded.theme.omarchy);
        assert!(palette.overrides.contains("text_labels"));
        let mut round = Theme::default();
        apply_palette(&mut round, &palette);
        assert_eq!(round.omarchy, loaded.theme.omarchy);
        assert!(round.overrides.contains("text_labels"));
        assert_eq!(hex(round.text_labels), "#ABCDEF");
        assert_eq!(hex(round.body_background), "#1A1B26");
    }

    #[test]
    fn missing_omarchy_theme_uses_builtin_with_warning() {
        let project = TempDir::new();
        let root = TempDir::new();
        fs::write(
            project.path().join("dd_ftp_theme.yml"),
            "version: 1\nomarchy: missing-theme\n",
        )
        .expect("yaml");
        let loaded = load_resolved(
            project.path(),
            None,
            &ldnddev_theme::OmarchyPaths::from_root(root.path()),
        );
        let warning = loaded.warning.unwrap_or_default();
        assert!(warning.contains("omarchy theme failed"), "{warning}");
        assert_eq!(loaded.source.label(), "default");
        assert_eq!(hex(loaded.theme.base_background), "#0F1114");
        assert!(loaded.theme.omarchy.is_none());
    }
}
