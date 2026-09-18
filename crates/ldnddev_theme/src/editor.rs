use crate::fields::{ColorField, COLOR_FIELDS};
use crate::palette::Palette;
use crate::rgb::parse_hex_input;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSaveTarget {
    Global,
    Local,
}

impl ThemeSaveTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Local => "local",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Global => Self::Local,
            Self::Local => Self::Global,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeEditorRow {
    Header(&'static str),
    Color(usize),
}

pub fn theme_editor_rows(fields: &[ColorField]) -> Vec<ThemeEditorRow> {
    let mut rows = Vec::new();
    let mut last_group = "";
    for (i, field) in fields.iter().enumerate() {
        if field.group != last_group {
            rows.push(ThemeEditorRow::Header(field.group));
            last_group = field.group;
        }
        rows.push(ThemeEditorRow::Color(i));
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKey {
    Up,
    Down,
    Left,
    Right,
    Tab,
    Enter,
    Esc,
    Backspace,
    Char(char),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorOutcome {
    None,
    PaletteChanged,
    RequestSave,
    Closed { reverted: bool },
    HexError(String),
}

#[derive(Clone, Debug)]
pub struct ThemeEditor {
    pub palette: Palette,
    snapshot: Palette,
    pub fields: Vec<ColorField>,
    pub selected: usize,
    pub scroll: usize,
    pub channel: usize,
    pub hex_draft: String,
    pub editing_hex: bool,
    pub save_target: ThemeSaveTarget,
}

impl ThemeEditor {
    pub fn new(mut palette: Palette, extras: &[ColorField]) -> Self {
        let mut fields = COLOR_FIELDS.to_vec();
        for extra in extras {
            if !fields.iter().any(|f| f.key == extra.key) {
                fields.push(*extra);
            }
        }
        palette.ensure_extras(extras);
        let hex_draft = palette
            .get(fields[0].key)
            .map(|c| c.to_hex())
            .unwrap_or_else(|| "#000000".to_string());
        Self {
            snapshot: palette.clone(),
            palette,
            fields,
            selected: 0,
            scroll: 0,
            channel: 0,
            hex_draft,
            editing_hex: false,
            save_target: ThemeSaveTarget::Global,
        }
    }

    pub fn selected_key(&self) -> &'static str {
        self.fields[self.selected.min(self.fields.len() - 1)].key
    }

    pub fn revert(&mut self) {
        self.palette = self.snapshot.clone();
        self.sync_hex_from_selected();
        self.editing_hex = false;
    }

    pub fn reset_builtin(&mut self) {
        let quotes = self.palette.header_quotes.clone();
        let source = self.palette.source;
        self.palette = Palette::builtin();
        self.palette.header_quotes = quotes;
        self.palette.source = source;
        self.palette.ensure_extras(&self.fields);
        self.sync_hex_from_selected();
        self.editing_hex = false;
    }

    pub fn select(&mut self, selected: usize) {
        let last = self.fields.len().saturating_sub(1);
        self.selected = selected.min(last);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected > self.scroll + 12 {
            self.scroll = self.selected.saturating_sub(12);
        }
        if !self.editing_hex {
            self.sync_hex_from_selected();
        }
    }

    fn sync_hex_from_selected(&mut self) {
        self.hex_draft = self
            .palette
            .get(self.selected_key())
            .map(|c| c.to_hex())
            .unwrap_or_else(|| "#000000".to_string());
    }

    pub fn handle(&mut self, key: EditorKey, shift: bool) -> EditorOutcome {
        if self.editing_hex {
            return self.handle_hex(key);
        }
        match key {
            EditorKey::Esc => {
                self.revert();
                EditorOutcome::Closed { reverted: true }
            }
            EditorKey::Char('y') | EditorKey::Char('Y') | EditorKey::Char('s') => {
                EditorOutcome::RequestSave
            }
            EditorKey::Char('R') => {
                self.reset_builtin();
                EditorOutcome::PaletteChanged
            }
            EditorKey::Tab => {
                self.save_target = self.save_target.toggled();
                EditorOutcome::None
            }
            EditorKey::Enter => {
                self.editing_hex = true;
                EditorOutcome::None
            }
            EditorKey::Down | EditorKey::Char('j') => {
                self.select(self.selected.saturating_add(1));
                EditorOutcome::None
            }
            EditorKey::Up | EditorKey::Char('k') => {
                self.select(self.selected.saturating_sub(1));
                EditorOutcome::None
            }
            EditorKey::Char('[') => {
                self.channel = self.channel.saturating_sub(1);
                EditorOutcome::None
            }
            EditorKey::Char(']') => {
                self.channel = (self.channel + 1).min(2);
                EditorOutcome::None
            }
            EditorKey::Char('+') | EditorKey::Char('=') => {
                let step = if shift { 1 } else { 8 };
                self.nudge(step)
            }
            EditorKey::Char('-') | EditorKey::Char('_') => {
                let step = if shift { -1 } else { -8 };
                self.nudge(step)
            }
            EditorKey::Left | EditorKey::Char('h') => self.nudge(-8),
            EditorKey::Right | EditorKey::Char('l') => self.nudge(8),
            EditorKey::Char('G') => {
                self.select(self.fields.len().saturating_sub(1));
                EditorOutcome::None
            }
            _ => EditorOutcome::None,
        }
    }

    fn nudge(&mut self, delta: i16) -> EditorOutcome {
        let key = self.selected_key();
        let current = self
            .palette
            .get(key)
            .unwrap_or(crate::rgb::Rgb::new(0, 0, 0));
        let next = current.nudge_channel(self.channel, delta);
        self.palette.set(key, next);
        self.hex_draft = next.to_hex();
        self.editing_hex = false;
        EditorOutcome::PaletteChanged
    }

    fn handle_hex(&mut self, key: EditorKey) -> EditorOutcome {
        match key {
            EditorKey::Esc => {
                self.editing_hex = false;
                self.sync_hex_from_selected();
                EditorOutcome::None
            }
            EditorKey::Enter => match parse_hex_input(&self.hex_draft) {
                Ok(rgb) => {
                    let key = self.selected_key();
                    self.palette.set(key, rgb);
                    self.hex_draft = rgb.to_hex();
                    self.editing_hex = false;
                    EditorOutcome::PaletteChanged
                }
                Err(err) => EditorOutcome::HexError(err.to_string()),
            },
            EditorKey::Backspace => {
                let _ = self.hex_draft.pop();
                EditorOutcome::None
            }
            EditorKey::Char(c) if c.is_ascii_hexdigit() || c == '#' => {
                if self.hex_draft.len() < 7 {
                    self.hex_draft.push(c.to_ascii_uppercase());
                }
                EditorOutcome::None
            }
            _ => EditorOutcome::None,
        }
    }
}
