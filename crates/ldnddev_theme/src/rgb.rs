use anyhow::{anyhow, Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn nudge_channel(self, channel: usize, delta: i16) -> Self {
        let mut rgb = [self.r, self.g, self.b];
        let idx = channel % 3;
        let next = i16::from(rgb[idx]) + delta;
        rgb[idx] = next.clamp(0, 255) as u8;
        Self {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
        }
    }
}

pub fn parse_hex_input(value: &str) -> Result<Rgb> {
    let trimmed = value.trim();
    let with_hash = if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}")
    };
    parse_hex_color("color", &with_hash)
}

pub fn parse_hex_color(key: &str, value: &str) -> Result<Rgb> {
    let hex = value
        .strip_prefix('#')
        .ok_or_else(|| anyhow!("Theme color `{key}` must start with #"))?;
    if hex.len() != 6 {
        return Err(anyhow!("Theme color `{key}` must be #RRGGBB"));
    }
    let r = u8::from_str_radix(&hex[0..2], 16)
        .with_context(|| format!("Invalid red channel for theme color `{key}`"))?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .with_context(|| format!("Invalid green channel for theme color `{key}`"))?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .with_context(|| format!("Invalid blue channel for theme color `{key}`"))?;
    Ok(Rgb { r, g, b })
}
