//! F2 theme list. Apps draw the rows and translate keys into [`EditorKey`].

use crate::editor::{EditorKey, ThemeSaveTarget};
use crate::fields::ColorField;
use crate::omarchy::{
    discover, display_name, load_colors, map_palette, OmarchyChoice, OmarchyPaths, OmarchyTheme,
};
use crate::palette::Palette;
use std::fs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerRow {
    Follow { name: String },
    Theme(OmarchyTheme),
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerOutcome {
    None,
    Applied,
    EditColors,
    RequestSave,
    Error(String),
    Closed { reverted: bool },
}

#[derive(Clone, Debug)]
pub struct ThemePicker {
    pub rows: Vec<PickerRow>,
    pub selected: usize,
    pub scroll: usize,
    pub palette: Palette,
    snapshot: Palette,
    pub paths: OmarchyPaths,
    extras: Vec<ColorField>,
    pub save_target: ThemeSaveTarget,
}

impl ThemePicker {
    pub fn new(palette: Palette, extras: &[ColorField], paths: OmarchyPaths) -> Self {
        let mut rows = Vec::new();
        if let Some(name) = current_name(&paths) {
            rows.push(PickerRow::Follow { name });
        }
        rows.extend(discover(&paths).into_iter().map(PickerRow::Theme));
        rows.push(PickerRow::Custom);
        let selected = selected_for(&rows, palette.omarchy.as_ref());
        let scroll = selected.saturating_sub(12);
        let snapshot = palette.clone();
        Self {
            rows,
            selected,
            scroll,
            palette,
            snapshot,
            paths,
            extras: extras.to_vec(),
            save_target: ThemeSaveTarget::Global,
        }
    }

    /// True when Follow or a named theme is available. Otherwise F2 opens the hex editor.
    pub fn lists_omarchy(&self) -> bool {
        self.rows
            .iter()
            .any(|row| !matches!(row, PickerRow::Custom))
    }

    pub fn revert_palette(&self) -> Palette {
        self.snapshot.clone()
    }

    pub fn select(&mut self, selected: usize) {
        let last = self.rows.len().saturating_sub(1);
        self.selected = selected.min(last);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected > self.scroll + 12 {
            self.scroll = self.selected.saturating_sub(12);
        }
    }

    pub fn handle(&mut self, key: EditorKey) -> PickerOutcome {
        match key {
            EditorKey::Esc => {
                self.palette = self.snapshot.clone();
                PickerOutcome::Closed { reverted: true }
            }
            EditorKey::Char('y') | EditorKey::Char('Y') | EditorKey::Char('s') => {
                PickerOutcome::RequestSave
            }
            EditorKey::Char('e') | EditorKey::Char('E') => PickerOutcome::EditColors,
            EditorKey::Tab => {
                self.save_target = self.save_target.toggled();
                PickerOutcome::None
            }
            EditorKey::Enter => self.apply_selected(),
            EditorKey::Down | EditorKey::Char('j') => {
                self.select(self.selected.saturating_add(1));
                PickerOutcome::None
            }
            EditorKey::Up | EditorKey::Char('k') => {
                self.select(self.selected.saturating_sub(1));
                PickerOutcome::None
            }
            EditorKey::Char('G') => {
                self.select(self.rows.len().saturating_sub(1));
                PickerOutcome::None
            }
            EditorKey::Char('g') => {
                self.select(0);
                PickerOutcome::None
            }
            _ => PickerOutcome::None,
        }
    }

    fn apply_selected(&mut self) -> PickerOutcome {
        let Some(row) = self.rows.get(self.selected).cloned() else {
            return PickerOutcome::None;
        };
        let choice = match row {
            PickerRow::Follow { .. } => Some(OmarchyChoice::Follow),
            PickerRow::Theme(theme) => Some(OmarchyChoice::Theme(theme.slug)),
            PickerRow::Custom => None,
        };
        match self.palette_for(choice) {
            Ok(palette) => {
                self.palette = palette;
                PickerOutcome::Applied
            }
            Err(err) => PickerOutcome::Error(err.to_string()),
        }
    }

    fn palette_for(&self, choice: Option<OmarchyChoice>) -> anyhow::Result<Palette> {
        let mut next = match &choice {
            None => {
                if self.snapshot.omarchy.is_none() {
                    let mut custom = self.snapshot.clone();
                    custom.overrides.clear();
                    custom
                } else {
                    let mut builtin = Palette::builtin();
                    builtin.header_quotes = self.snapshot.header_quotes.clone();
                    builtin.source = self.snapshot.source;
                    builtin.version = self.snapshot.version;
                    builtin
                }
            }
            Some(choice) => {
                let colors = load_colors(&self.paths, choice)?;
                let mut mapped = map_palette(&colors);
                mapped.header_quotes = self.snapshot.header_quotes.clone();
                mapped.source = self.snapshot.source;
                mapped.version = self.snapshot.version;
                mapped.omarchy = Some(choice.clone());
                mapped.overrides.clear();
                mapped
            }
        };
        next.ensure_extras(&self.extras);
        Ok(next)
    }
}

fn current_name(paths: &OmarchyPaths) -> Option<String> {
    if !paths.current_colors.is_file() {
        return None;
    }
    let name = fs::read_to_string(&paths.current_name).ok()?;
    let slug = name.trim();
    if slug.is_empty() {
        return None;
    }
    Some(display_name(slug))
}

fn selected_for(rows: &[PickerRow], choice: Option<&OmarchyChoice>) -> usize {
    let found = rows.iter().position(|row| match (row, choice) {
        (PickerRow::Follow { .. }, Some(OmarchyChoice::Follow)) => true,
        (PickerRow::Theme(theme), Some(OmarchyChoice::Theme(slug))) => theme.slug == *slug,
        (PickerRow::Custom, None) => true,
        _ => false,
    });
    found.unwrap_or(0)
}
