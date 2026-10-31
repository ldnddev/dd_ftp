use crate::fields::{extra_default, is_canonical_key, CANONICAL_DEFAULTS, COLOR_FIELDS};
use crate::omarchy::OmarchyChoice;
use crate::rgb::Rgb;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSource {
    Local,
    Global,
    Default,
}

impl ThemeSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Global => "global",
            Self::Default => "default",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    colors: BTreeMap<String, Rgb>,
    pub header_quotes: Vec<String>,
    pub version: u64,
    pub source: ThemeSource,
    /// Linked Omarchy theme. `None` is the hand-authored YAML palette.
    pub omarchy: Option<OmarchyChoice>,
    /// Keys in `colors` that the user edited on top of an Omarchy theme.
    pub overrides: BTreeSet<String>,
}

impl Palette {
    pub fn builtin() -> Self {
        let mut colors = BTreeMap::new();
        for (key, rgb) in CANONICAL_DEFAULTS {
            colors.insert((*key).to_string(), *rgb);
        }
        Self {
            colors,
            header_quotes: Vec::new(),
            version: crate::SUPPORTED_THEME_VERSION,
            source: ThemeSource::Default,
            omarchy: None,
            overrides: BTreeSet::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<Rgb> {
        self.colors.get(key).copied()
    }

    pub fn set(&mut self, key: &str, rgb: Rgb) {
        self.colors.insert(key.to_string(), rgb);
    }

    pub fn ensure_extras(&mut self, extras: &[crate::fields::ColorField]) {
        for field in extras {
            if self.colors.contains_key(field.key) {
                continue;
            }
            if let Some(rgb) = extra_default(field.key) {
                self.colors.insert(field.key.to_string(), rgb);
            }
        }
    }

    pub fn overlay(&mut self, other: &BTreeMap<String, Rgb>) {
        for (k, v) in other {
            self.colors.insert(k.clone(), *v);
        }
    }

    pub fn missing_canonical(&self) -> Vec<&'static str> {
        COLOR_FIELDS
            .iter()
            .filter(|f| !self.colors.contains_key(f.key))
            .map(|f| f.key)
            .collect()
    }

    pub fn extra_keys(&self) -> Vec<String> {
        self.colors
            .keys()
            .filter(|k| !is_canonical_key(k))
            .cloned()
            .collect()
    }

    pub fn iter_ordered<'a>(
        &'a self,
        fields: &'a [crate::fields::ColorField],
    ) -> impl Iterator<Item = (&'static str, Option<Rgb>)> + 'a {
        fields.iter().map(|f| (f.key, self.get(f.key)))
    }

    pub fn colors(&self) -> &BTreeMap<String, Rgb> {
        &self.colors
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::builtin()
    }
}
