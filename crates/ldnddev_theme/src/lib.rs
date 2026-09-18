//! Shared YAML theme tokens and live color editor for ldnddev TUI apps.
//!
//! The crate is independent of ratatui/crossterm so apps on 0.29 and 0.30 can
//! both depend on it. Convert [`Rgb`] with `ratatui::style::Color::Rgb(r, g, b)`.

mod editor;
mod fields;
mod palette;
mod paths;
mod rgb;
mod yaml;

pub use editor::{
    theme_editor_rows, EditorKey, EditorOutcome, ThemeEditor, ThemeEditorRow, ThemeSaveTarget,
};
pub use fields::{
    extra_default, is_canonical_key, ColorField, COLOR_FIELDS, EXTRA_MODAL_HEADER, EXTRA_SELECTION,
    EXTRA_TEXT_DISABLED, EXTRA_TEXT_INVERSE,
};
pub use palette::{Palette, ThemeSource};
pub use paths::{default_config_home, global_theme_path, local_theme_path};
pub use rgb::{parse_hex_color, parse_hex_input, Rgb};
pub use yaml::{load_from_file, load_from_str, load_lookup, render_yaml, save_theme, ParseMode};

pub const SUPPORTED_THEME_VERSION: u64 = 1;
