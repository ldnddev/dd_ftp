//! Omarchy `colors.toml` → ldnddev theme tokens.
//!
//! Read the theme directories. Do not shell out to `omarchy`, and do not call
//! `omarchy theme set` (that retints the desktop).
//!
//! # Mapping
//!
//! Derive missing shades the way `omarchy-theme-color` does, then:
//!
//! | token | source |
//! |---|---|
//! | `base_background` | `darker_background` |
//! | `body_background` | `background` |
//! | `modal_background` | `dark_background` |
//! | `selected_background` / `selection` | `selection` |
//! | `text_primary` | `foreground`, or `bright_foreground` when that contrasts better |
//! | `text_secondary` | `dark_foreground` (blended to 3:1) |
//! | `text_disabled` | `muted` |
//! | `text_inverse` | `background` |
//! | `text_labels` / `scrollbar` | `orange` |
//! | `text_active_focus` / `modal_labels` / `modal_header` / `scrollbar_hover` / `input_border_focus` | `accent` |
//! | `modal_text` / `input_text_default` | `foreground` |
//! | `border_default` / `input_border_default` | `muted` |
//! | `border_active` | `active_border_color`, else `accent` |
//! | `input_text_focus` / `cursor` | `bright_foreground` |
//! | `success` / `warning` / `error` / `info` | `green` / `yellow` / `red` / `cyan` |
//! | `folders` / `files` / `links` | `blue` / `yellow` / `cyan` |
//!
//! `text_primary` is blended until it reaches 4.5:1 against the shell and the
//! body. `text_secondary` is blended toward that text until it reaches 3:1.
//! Accent, status, and tree colors stay as the theme authored them.

use crate::palette::{Palette, ThemeSource};
use crate::rgb::{parse_hex_color, Rgb};
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OmarchyChoice {
    Follow,
    Theme(String),
}

impl OmarchyChoice {
    pub fn token(&self) -> &str {
        match self {
            Self::Follow => "follow",
            Self::Theme(slug) => slug,
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        let token = normalize_slug(raw);
        if token.is_empty() {
            return Err(anyhow!("Omarchy theme name is empty"));
        }
        if token == "follow" {
            return Ok(Self::Follow);
        }
        if token.contains('/') || token.contains("..") || token.starts_with('.') {
            return Err(anyhow!("Invalid Omarchy theme name `{raw}`"));
        }
        Ok(Self::Theme(token))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Directories and files that identify installed Omarchy themes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmarchyPaths {
    pub stock: PathBuf,
    pub user: PathBuf,
    pub current_name: PathBuf,
    pub current_colors: PathBuf,
}

impl OmarchyPaths {
    pub fn system() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let omarchy = std::env::var_os("OMARCHY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
        let config =
            crate::paths::default_config_home().unwrap_or_else(|| PathBuf::from(".config"));
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local/state")))
            .unwrap_or_else(|| PathBuf::from(".local/state"));
        Self {
            stock: omarchy.join("themes"),
            user: config.join("omarchy").join("themes"),
            current_name: state.join("omarchy").join("current").join("theme.name"),
            current_colors: state
                .join("omarchy")
                .join("current")
                .join("theme")
                .join("colors.toml"),
        }
    }

    /// `root/stock`, `root/user`, and `root/state` stand in for the system paths.
    pub fn from_root(root: &Path) -> Self {
        Self {
            stock: root.join("stock"),
            user: root.join("user"),
            current_name: root.join("state").join("theme.name"),
            current_colors: root.join("state").join("theme").join("colors.toml"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmarchyColors {
    pub mode: ThemeMode,
    values: BTreeMap<String, Rgb>,
}

impl OmarchyColors {
    pub fn get(&self, key: &str) -> Option<Rgb> {
        self.values.get(key).copied()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeOrigin {
    Stock,
    User,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmarchyTheme {
    pub slug: String,
    pub name: String,
    pub mode: ThemeMode,
    pub origin: ThemeOrigin,
}

pub fn display_name(slug: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for c in slug.chars() {
        if c == '-' || c == ' ' {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            cap = true;
        } else if cap {
            for up in c.to_uppercase() {
                out.push(up);
            }
            cap = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn normalize_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in raw.trim().chars() {
        if c.is_ascii_whitespace() || c == '-' || c == '_' {
            dash = !out.is_empty();
            continue;
        }
        if dash {
            out.push('-');
            dash = false;
        }
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Name file contents plus the colors file mtime. `None` when follow cannot resolve.
pub fn follow_stamp(paths: &OmarchyPaths) -> Option<String> {
    let name = fs::read_to_string(&paths.current_name).ok()?;
    let modified = fs::metadata(&paths.current_colors).ok()?.modified().ok()?;
    Some(format!("{}\n{}", name.trim(), stamp_time(modified)))
}

fn stamp_time(time: SystemTime) -> String {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub fn discover(paths: &OmarchyPaths) -> Vec<OmarchyTheme> {
    let mut slugs = BTreeMap::<String, ThemeOrigin>::new();
    for slug in list_slugs(&paths.stock) {
        if paths.stock.join(&slug).join("colors.toml").is_file() {
            slugs.insert(slug, ThemeOrigin::Stock);
        }
    }
    for slug in list_slugs(&paths.user) {
        if !paths.user.join(&slug).join("colors.toml").is_file() {
            continue;
        }
        slugs
            .entry(slug)
            .and_modify(|origin| *origin = ThemeOrigin::Both)
            .or_insert(ThemeOrigin::User);
    }
    let mut themes = Vec::new();
    for (slug, origin) in slugs {
        let Some(colors) = load_slug(paths, &slug).ok() else {
            continue;
        };
        themes.push(OmarchyTheme {
            name: display_name(&slug),
            slug,
            mode: colors.mode,
            origin,
        });
    }
    themes
}

pub fn load_colors(paths: &OmarchyPaths, choice: &OmarchyChoice) -> Result<OmarchyColors> {
    match choice {
        OmarchyChoice::Follow => {
            let text = fs::read_to_string(&paths.current_colors).with_context(|| {
                format!(
                    "Current Omarchy theme colors are missing ({})",
                    paths.current_colors.display()
                )
            })?;
            colors_from_str(&text).context("Current Omarchy theme colors could not be parsed")
        }
        OmarchyChoice::Theme(slug) => load_slug(paths, slug),
    }
}

pub fn colors_from_str(text: &str) -> Result<OmarchyColors> {
    derive_colors(parse_raw(text)?, false)
}

pub fn map_palette(colors: &OmarchyColors) -> Palette {
    let background = colors.get("background").unwrap_or(Rgb::new(0, 0, 0));
    let foreground = colors.get("foreground").unwrap_or(Rgb::new(255, 255, 255));
    let bright = colors.get("bright_foreground").unwrap_or(foreground);
    let darker = colors.get("darker_background").unwrap_or(background);
    let dark_bg = colors.get("dark_background").unwrap_or(background);
    let accent = colors
        .get("accent")
        .or_else(|| colors.get("blue"))
        .unwrap_or(bright);
    let border_active = colors.get("active_border_color").unwrap_or(accent);
    let orange = colors
        .get("orange")
        .unwrap_or(colors.get("yellow").unwrap_or(accent));
    let yellow = colors.get("yellow").unwrap_or(orange);
    let muted = colors
        .get("muted")
        .unwrap_or(colors.get("dark_foreground").unwrap_or(foreground));
    let selection = colors.get("selection").unwrap_or(dark_bg);
    let surfaces = [darker, background];
    let extreme = if colors.mode == ThemeMode::Light {
        Rgb::new(0, 0, 0)
    } else {
        Rgb::new(255, 255, 255)
    };
    let mut text = foreground;
    if worst(text, &surfaces) < 4.5 && worst(bright, &surfaces) > worst(text, &surfaces) {
        text = bright;
    }
    text = ensure(text, &surfaces, 4.5, extreme);
    let secondary_src = colors.get("dark_foreground").unwrap_or(muted);
    let secondary = ensure(secondary_src, &surfaces, 3.0, text);

    let mut palette = Palette::builtin();
    palette.source = ThemeSource::Default;
    let set = [
        ("base_background", darker),
        ("body_background", background),
        ("modal_background", dark_bg),
        ("selected_background", selection),
        ("selection", selection),
        ("text_primary", text),
        ("text_secondary", secondary),
        ("text_disabled", muted),
        ("text_inverse", background),
        ("text_labels", orange),
        ("text_active_focus", accent),
        ("modal_labels", accent),
        ("modal_text", foreground),
        ("modal_header", accent),
        ("border_default", muted),
        ("border_active", border_active),
        ("scrollbar", orange),
        ("scrollbar_hover", accent),
        ("input_border_default", muted),
        ("input_border_focus", accent),
        ("input_text_default", foreground),
        ("input_text_focus", bright),
        ("cursor", bright),
        (
            "success",
            colors.get("green").unwrap_or(Rgb::new(0x82, 0xe0, 0xaa)),
        ),
        ("warning", yellow),
        (
            "error",
            colors.get("red").unwrap_or(Rgb::new(0xe5, 0x73, 0x73)),
        ),
        ("info", colors.get("cyan").unwrap_or(accent)),
        ("folders", colors.get("blue").unwrap_or(accent)),
        ("files", yellow),
        ("links", colors.get("cyan").unwrap_or(accent)),
    ];
    for (key, rgb) in set {
        palette.set(key, rgb);
    }
    palette
}

/// Replace an Omarchy-linked palette with the live mapped colors, then reapply overrides.
pub fn apply_omarchy(palette: &Palette, paths: &OmarchyPaths) -> Result<Palette> {
    let Some(choice) = palette.omarchy.clone() else {
        return Ok(palette.clone());
    };
    let colors = load_colors(paths, &choice)?;
    let mut mapped = map_palette(&colors);
    for key in &palette.overrides {
        if let Some(rgb) = palette.get(key) {
            mapped.set(key, rgb);
        }
    }
    mapped.header_quotes = palette.header_quotes.clone();
    mapped.source = palette.source;
    mapped.version = palette.version;
    mapped.omarchy = Some(choice);
    mapped.overrides = palette.overrides.clone();
    Ok(mapped)
}

pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (hi, lo) = if luminance(a) >= luminance(b) {
        (luminance(a), luminance(b))
    } else {
        (luminance(b), luminance(a))
    };
    (hi + 0.05) / (lo + 0.05)
}

fn load_slug(paths: &OmarchyPaths, slug: &str) -> Result<OmarchyColors> {
    if slug.is_empty() || slug.contains('/') || slug.contains("..") {
        return Err(anyhow!("Invalid Omarchy theme name `{slug}`"));
    }
    let stock = paths.stock.join(slug).join("colors.toml");
    let user = paths.user.join(slug).join("colors.toml");
    let mut raw = BTreeMap::new();
    let mut found = false;
    if stock.is_file() {
        raw.extend(parse_raw(
            &fs::read_to_string(&stock)
                .with_context(|| format!("Failed to read {}", stock.display()))?,
        )?);
        found = true;
    }
    if user.is_file() {
        raw.extend(parse_raw(
            &fs::read_to_string(&user)
                .with_context(|| format!("Failed to read {}", user.display()))?,
        )?);
        found = true;
    }
    if !found {
        return Err(anyhow!("Omarchy theme `{slug}` has no colors.toml"));
    }
    let light_mode = paths.stock.join(slug).join("light.mode").is_file()
        || paths.user.join(slug).join("light.mode").is_file();
    derive_colors(raw, light_mode)
}

fn list_slugs(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut slugs = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() && !file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') || name.contains('/') || name.contains("..") {
            continue;
        }
        slugs.push(name.to_string());
    }
    slugs
}

fn parse_raw(text: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (idx, line) in text.lines().enumerate() {
        let Some((key, value)) = parse_assignment(line) else {
            continue;
        };
        if value.is_empty() {
            return Err(anyhow!(
                "Missing color value for `{key}` on line {}",
                idx + 1
            ));
        }
        out.insert(key, value);
    }
    Ok(out)
}

fn parse_assignment(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, rest) = line.split_once('=')?;
    let key = key.trim().trim_matches(['"', '\'']);
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let rest = rest.trim();
    let value = if let Some(rest) = rest.strip_prefix('"') {
        rest.split('"').next().unwrap_or("").trim().to_string()
    } else if let Some(rest) = rest.strip_prefix('\'') {
        rest.split('\'').next().unwrap_or("").trim().to_string()
    } else {
        rest.split_whitespace().next().unwrap_or("").to_string()
    };
    Some((key.to_string(), value))
}

fn derive_colors(
    mut raw: BTreeMap<String, String>,
    light_mode_file: bool,
) -> Result<OmarchyColors> {
    alias(&mut raw, "background", "bg");
    alias(&mut raw, "dark_background", "dark_bg");
    alias(&mut raw, "darker_background", "darker_bg");
    alias(&mut raw, "lighter_background", "lighter_bg");
    alias(&mut raw, "foreground", "fg");
    alias(&mut raw, "dark_foreground", "dark_fg");
    alias(&mut raw, "light_foreground", "light_fg");
    alias(&mut raw, "bright_foreground", "bright_fg");

    if !raw.contains_key("background") {
        if let Some(c0) = raw.get("color0").cloned() {
            raw.insert("background".to_string(), c0);
        }
    }
    if !raw.contains_key("foreground") {
        if let Some(c7) = raw.get("color7").cloned() {
            raw.insert("foreground".to_string(), c7);
        }
    }
    if let Some(bg) = raw.get("background").cloned() {
        raw.insert("color0".to_string(), bg);
    }
    if let Some(fg) = raw.get("foreground").cloned() {
        raw.insert("color7".to_string(), fg);
    }

    for (key, fallback) in [
        ("red", "color1"),
        ("green", "color2"),
        ("yellow", "color3"),
        ("blue", "color4"),
        ("magenta", "color5"),
        ("cyan", "color6"),
        ("bright_red", "color9"),
        ("bright_green", "color10"),
        ("bright_yellow", "color11"),
        ("bright_blue", "color12"),
        ("bright_magenta", "color13"),
        ("bright_cyan", "color14"),
    ] {
        alias(&mut raw, key, fallback);
    }
    alias(&mut raw, "magenta", "purple");
    alias(&mut raw, "bright_magenta", "bright_purple");

    alias(&mut raw, "light_foreground", "color7");
    alias(&mut raw, "light_foreground", "foreground");
    alias(&mut raw, "bright_foreground", "color15");
    alias(&mut raw, "bright_foreground", "foreground");
    if let Some(cursor) = raw.get("bright_foreground").cloned() {
        raw.insert("cursor".to_string(), cursor);
    }
    alias(&mut raw, "lighter_background", "color0");
    alias(&mut raw, "lighter_background", "background");
    alias(&mut raw, "dark_foreground", "color8");
    alias(&mut raw, "dark_foreground", "foreground");
    alias(&mut raw, "muted", "color8");
    alias(&mut raw, "muted", "dark_foreground");
    alias(&mut raw, "selection", "selection_background");
    alias(&mut raw, "selection", "color8");
    alias(&mut raw, "selection", "color0");
    alias(&mut raw, "selection", "background");
    if let Some(selection) = raw.get("selection").cloned() {
        raw.entry("selection_background".to_string())
            .or_insert(selection);
    }
    alias(&mut raw, "selection_foreground", "bright_foreground");
    alias(&mut raw, "orange", "yellow");

    let background = required_hex(&raw, "background")?;
    let foreground = required_hex(&raw, "foreground")?;
    if !raw.contains_key("brown") {
        if let Some(orange) = optional_hex(&raw, "orange") {
            raw.insert(
                "brown".to_string(),
                mix(orange, Rgb::new(0, 0, 0), 0.5).to_hex(),
            );
        }
    }
    if !raw.contains_key("dark_background") {
        raw.insert(
            "dark_background".to_string(),
            mix(background, Rgb::new(0, 0, 0), 0.25).to_hex(),
        );
    }
    if !raw.contains_key("darker_background") {
        raw.insert(
            "darker_background".to_string(),
            mix(background, Rgb::new(0, 0, 0), 0.5).to_hex(),
        );
    }
    for (bright, base) in [
        ("bright_red", "red"),
        ("bright_yellow", "yellow"),
        ("bright_green", "green"),
        ("bright_cyan", "cyan"),
        ("bright_blue", "blue"),
        ("bright_magenta", "magenta"),
    ] {
        if raw.contains_key(bright) {
            continue;
        }
        if let Some(color) = optional_hex(&raw, base) {
            raw.insert(
                bright.to_string(),
                mix(color, Rgb::new(255, 255, 255), 0.2).to_hex(),
            );
        }
    }
    alias(&mut raw, "accent", "blue");
    alias(&mut raw, "accent", "bright_blue");
    alias(&mut raw, "accent", "foreground");

    let mode = resolve_mode(&raw, light_mode_file, background);
    let mut values = BTreeMap::new();
    for (key, value) in &raw {
        if let Ok(rgb) = parse_hex_color(key, value) {
            values.insert(key.clone(), rgb);
        }
    }
    values.entry("background".to_string()).or_insert(background);
    values.entry("foreground".to_string()).or_insert(foreground);
    Ok(OmarchyColors { mode, values })
}

fn resolve_mode(
    raw: &BTreeMap<String, String>,
    light_mode_file: bool,
    background: Rgb,
) -> ThemeMode {
    let written = raw
        .get("mode")
        .or_else(|| raw.get("theme_type"))
        .map(|s| s.to_ascii_lowercase());
    match written.as_deref() {
        Some("light") => ThemeMode::Light,
        Some("dark") => ThemeMode::Dark,
        _ if light_mode_file => ThemeMode::Light,
        _ => {
            let lum = u16::from(background.r) + u16::from(background.g) + u16::from(background.b);
            if lum > 382 {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            }
        }
    }
}

fn alias(raw: &mut BTreeMap<String, String>, key: &str, fallback: &str) {
    if raw.contains_key(key) {
        return;
    }
    if let Some(value) = raw.get(fallback).cloned() {
        raw.insert(key.to_string(), value);
    }
}

fn required_hex(raw: &BTreeMap<String, String>, key: &str) -> Result<Rgb> {
    let value = raw
        .get(key)
        .ok_or_else(|| anyhow!("Omarchy colors.toml is missing `{key}`"))?;
    parse_hex_color(key, value)
}

fn optional_hex(raw: &BTreeMap<String, String>, key: &str) -> Option<Rgb> {
    raw.get(key)
        .and_then(|value| parse_hex_color(key, value).ok())
}

/// `start * (1 - t) + end * t`, rounded half away from zero, clamped.
pub fn mix(start: Rgb, end: Rgb, amount: f64) -> Rgb {
    let t = amount.clamp(0.0, 1.0);
    let ch = |a: u8, b: u8| {
        let v = f64::from(a) * (1.0 - t) + f64::from(b) * t;
        v.round().clamp(0.0, 255.0) as u8
    };
    Rgb::new(ch(start.r, end.r), ch(start.g, end.g), ch(start.b, end.b))
}

fn luminance(color: Rgb) -> f64 {
    fn ch(v: u8) -> f64 {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * ch(color.r) + 0.7152 * ch(color.g) + 0.0722 * ch(color.b)
}

fn worst(color: Rgb, surfaces: &[Rgb]) -> f64 {
    surfaces
        .iter()
        .map(|surface| contrast_ratio(color, *surface))
        .fold(f64::MAX, f64::min)
}

fn ensure(original: Rgb, surfaces: &[Rgb], floor: f64, target: Rgb) -> Rgb {
    let mut t = 0.0;
    while worst(original, surfaces) < floor && t < 1.0 {
        t += 0.05;
        let next = mix(original, target, t);
        if worst(next, surfaces) >= floor {
            return next;
        }
    }
    original
}
