use crate::fields::{ColorField, COLOR_FIELDS};
use crate::omarchy::OmarchyChoice;
use crate::palette::{Palette, ThemeSource};
use crate::rgb::{parse_hex_color, Rgb};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Theme files are small. A larger file is rejected before YAML parse so a
/// cloned repo cannot feed the parser a nested bomb.
const MAX_THEME_BYTES: u64 = 256 * 1024;
const MAX_THEME_DEPTH: usize = 16;
const MAX_THEME_INDENT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseMode {
    Strict,
    Lenient,
}

#[derive(Debug, Deserialize)]
struct RawThemeFile {
    version: Option<serde_yaml::Value>,
    #[serde(default)]
    omarchy: Option<String>,
    #[serde(default)]
    header_quotes: Vec<String>,
    colors: Option<HashMap<String, String>>,
}

pub fn load_from_str(content: &str, mode: ParseMode, source: ThemeSource) -> Result<Palette> {
    if content.len() > MAX_THEME_BYTES as usize {
        bail!("theme YAML exceeds {MAX_THEME_BYTES} bytes");
    }
    validate_theme_yaml(content)?;
    let file: RawThemeFile = serde_yaml::from_str(content).context("Failed to parse theme YAML")?;
    let version = parse_version(&file.version)?;
    if version != crate::SUPPORTED_THEME_VERSION {
        return Err(anyhow!(
            "Unsupported theme schema version `{version}`; expected `{}`",
            crate::SUPPORTED_THEME_VERSION
        ));
    }
    let omarchy = match file.omarchy.as_deref() {
        Some(raw) => Some(OmarchyChoice::parse(raw)?),
        None => None,
    };
    let raw = file.colors.unwrap_or_default();
    let mut parsed: HashMap<String, Rgb> = HashMap::new();
    for (key, value) in raw {
        if value.trim().is_empty() {
            return Err(anyhow!("Missing color value for theme key `{key}`"));
        }
        parsed.insert(key.clone(), parse_hex_color(&key, &value)?);
    }

    // An Omarchy link supplies the canonical colors. Listed keys are overrides.
    if mode == ParseMode::Strict && omarchy.is_none() {
        for field in COLOR_FIELDS {
            if !parsed.contains_key(field.key) {
                return Err(anyhow!("Missing required theme color `{}`", field.key));
            }
        }
    }

    let mut palette = Palette::builtin();
    for (key, rgb) in &parsed {
        palette.set(key, *rgb);
    }
    if omarchy.is_some() {
        palette.overrides = parsed.keys().cloned().collect();
    }
    palette.header_quotes = file
        .header_quotes
        .into_iter()
        .map(|quote| {
            quote
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
        })
        .filter(|quote| !quote.trim().is_empty())
        .collect();
    palette.version = version;
    palette.source = source;
    palette.omarchy = omarchy;
    Ok(palette)
}

pub fn load_from_file(path: &Path, mode: ParseMode, source: ThemeSource) -> Result<Palette> {
    let content = read_no_follow(path, MAX_THEME_BYTES)
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
    if let Some(choice) = &palette.omarchy {
        out.push_str("omarchy: ");
        out.push_str(choice.token());
        out.push('\n');
    }
    if !palette.header_quotes.is_empty() {
        out.push_str("header_quotes:\n");
        for quote in &palette.header_quotes {
            out.push_str("  - ");
            out.push_str(&yaml_quote(quote));
            out.push('\n');
        }
    }
    if palette.omarchy.is_some() {
        let overrides: Vec<_> = fields
            .iter()
            .filter(|field| palette.overrides.contains(field.key))
            .collect();
        if overrides.is_empty() {
            return out;
        }
        out.push_str("colors:\n");
        for field in overrides {
            let hex = palette
                .get(field.key)
                .map(|c| c.to_hex())
                .unwrap_or_else(|| "#000000".to_string());
            out.push_str("  ");
            out.push_str(field.key);
            out.push_str(": \"");
            out.push_str(&hex);
            out.push_str("\"\n");
        }
        return out;
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
    write_no_follow(&path, &render_yaml(palette, fields))
        .with_context(|| format!("Failed to write theme file {}", path.display()))?;
    Ok(path)
}

fn read_no_follow(path: &Path, max_bytes: u64) -> Result<String> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!("refusing to follow symlink {}", path.display());
    }
    if !meta.is_file() {
        bail!("refusing to read non-file {}", path.display());
    }
    if meta.len() > max_bytes {
        bail!(
            "{} is {} bytes; refusing to read more than {max_bytes}",
            path.display(),
            meta.len()
        );
    }
    let file =
        open_no_follow(path).with_context(|| format!("Failed to read {}", path.display()))?;
    // Stat the fd, not the path. A symlink or fifo swapped in after the
    // path check must not be followed or block this read.
    let opened = file
        .metadata()
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if opened.file_type().is_symlink() || !opened.is_file() {
        bail!("refusing to read non-file {}", path.display());
    }
    if opened.len() > max_bytes {
        bail!(
            "{} is {} bytes; refusing to read more than {max_bytes}",
            path.display(),
            opened.len()
        );
    }
    let mut buf = String::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_string(&mut buf)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if buf.len() as u64 > max_bytes {
        bail!("{} exceeds {max_bytes} bytes", path.display());
    }
    Ok(buf)
}

/// Open the final path component without following a symlink.
///
/// `File::open` follows a symlink swapped in after `symlink_metadata`.
/// `O_NOFOLLOW` makes that open fail. `O_NONBLOCK` keeps a swapped-in fifo
/// from blocking. Linux values: `O_NOFOLLOW` is `0x20000`, `O_NONBLOCK` is `0x800`.
fn open_no_follow(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NOFOLLOW does not follow a final symlink. O_NONBLOCK so a fifo
        // open returns instead of waiting.
        const O_NOFOLLOW: i32 = 0x20000;
        const O_NONBLOCK: i32 = 0x800;
        options.custom_flags(O_NOFOLLOW | O_NONBLOCK);
    }
    let opened = options.open(path);
    #[cfg(target_os = "linux")]
    {
        // ELOOP. ErrorKind::FilesystemLoop is still unstable.
        const ELOOP: i32 = 40;
        if let Err(err) = &opened {
            if err.raw_os_error() == Some(ELOOP) {
                return Err(std::io::Error::new(
                    err.kind(),
                    format!("refusing to follow symlink {}", path.display()),
                ));
            }
        }
    }
    opened
}

fn write_no_follow(path: &Path, contents: &str) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            bail!("refusing to follow symlink {}", path.display());
        }
        if !meta.is_file() {
            bail!("refusing to overwrite non-file {}", path.display());
        }
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("theme");
    let tmp = parent.join(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Reject anchors, aliases, and deep documents before `serde_yaml` expands them.
fn validate_theme_yaml(content: &str) -> Result<()> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut comment = false;
    let mut prev_ws = true;
    let mut depth = 0usize;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if comment {
            if c == '\n' {
                comment = false;
                prev_ws = true;
            }
            continue;
        }
        if in_single {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_single = false;
                }
            }
            prev_ws = false;
            continue;
        }
        if in_double {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == '"' {
                in_double = false;
            }
            prev_ws = false;
            continue;
        }
        match c {
            '#' if prev_ws => comment = true,
            '\'' => {
                in_single = true;
                prev_ws = false;
            }
            '"' => {
                in_double = true;
                prev_ws = false;
            }
            '\n' => prev_ws = true,
            '&' | '*'
                if chars
                    .peek()
                    .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_') =>
            {
                bail!("theme YAML anchors and aliases are not allowed");
            }
            '{' | '[' => {
                depth += 1;
                if depth > MAX_THEME_DEPTH {
                    bail!("theme YAML is nested too deeply");
                }
                prev_ws = false;
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                prev_ws = false;
            }
            c if c.is_whitespace() => prev_ws = true,
            _ => prev_ws = false,
        }
    }

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.chars().take_while(|c| *c == ' ').count();
        if indent > MAX_THEME_INDENT {
            bail!("theme YAML is nested too deeply");
        }
    }
    Ok(())
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
