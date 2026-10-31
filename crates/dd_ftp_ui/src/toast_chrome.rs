//! Bottom-right toast chrome shared by the ldnddev TUI apps.
//!
//! Keep this file identical across apps. The border, glyph, and message use
//! the semantic color, shifted toward black or white until the text clears
//! 4.5:1 against the modal background.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub const TOAST_WIDTH: u16 = 60;
const TOAST_HEIGHT: u16 = 3;
const TOAST_GAP: u16 = 1;
const TEXT_CONTRAST: f64 = 4.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ToastKind {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct ToastPaint {
    pub modal_background: Color,
    pub text: Color,
    pub success: Color,
    pub info: Color,
    pub warning: Color,
    pub error: Color,
}

#[derive(Clone, Copy, Debug)]
pub struct ToastLine<'a> {
    pub kind: ToastKind,
    pub message: &'a str,
}

/// Draw `toasts` above the footer. The last item sits nearest the footer.
/// Returned rects are newest first.
pub fn render_toast_stack(
    frame: &mut ratatui::Frame,
    area: Rect,
    toasts: &[ToastLine<'_>],
    paint: ToastPaint,
    lift: u16,
) -> Vec<Rect> {
    if toasts.is_empty() {
        return Vec::new();
    }
    let max_width = area.width.saturating_sub(2);
    let width = TOAST_WIDTH.min(max_width);
    if width < 10 {
        return Vec::new();
    }
    let right_x = area.x + area.width.saturating_sub(width + 1);
    let mut y = area
        .y
        .saturating_add(area.height.saturating_sub(TOAST_HEIGHT + 1 + lift));
    let mut drawn = Vec::new();
    for toast in toasts.iter().rev() {
        if y + TOAST_HEIGHT > area.y + area.height || y < area.y {
            break;
        }
        let rect = Rect {
            x: right_x,
            y,
            width,
            height: TOAST_HEIGHT,
        };
        paint_one(frame, rect, toast, &paint);
        drawn.push(rect);
        if y < area.y + TOAST_HEIGHT + TOAST_GAP {
            break;
        }
        y = y.saturating_sub(TOAST_HEIGHT + TOAST_GAP);
    }
    drawn
}

fn paint_one(frame: &mut ratatui::Frame, rect: Rect, toast: &ToastLine<'_>, paint: &ToastPaint) {
    let (glyph, raw) = match toast.kind {
        ToastKind::Success => ("✓", paint.success),
        ToastKind::Info => ("ℹ", paint.info),
        ToastKind::Warning => ("⚠", paint.warning),
        ToastKind::Error => ("✕", paint.error),
    };
    let bg = paint.modal_background;
    // Keep the semantic hue. Shift it toward black or white until the
    // glyph, border, and message clear 4.5:1 on the modal background.
    let accent = readable(raw, paint.text, bg);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(bg))
        .border_style(Style::default().fg(accent));
    frame.render_widget(block, rect);
    let line = Line::from(vec![
        Span::styled(format!("{glyph} "), Style::default().fg(accent).bg(bg)),
        Span::styled(toast.message, Style::default().fg(accent).bg(bg)),
    ]);
    frame.render_widget(
        Paragraph::new(line),
        Rect {
            x: rect.x + 2,
            y: rect.y + 1,
            width: rect.width.saturating_sub(4),
            height: 1,
        },
    );
}

fn readable(color: Color, fallback: Color, background: Color) -> Color {
    let (Some(start), Some(background)) = (rgb_of(color), rgb_of(background)) else {
        return fallback;
    };
    let repaired = repair(start, background, TEXT_CONTRAST);
    if ldnddev_theme::contrast_ratio(repaired, background) >= TEXT_CONTRAST {
        Color::Rgb(repaired.r, repaired.g, repaired.b)
    } else {
        fallback
    }
}

/// Blend `start` toward whichever of black or white contrasts more with
/// `background`, in the same 0.05 steps the theme mapper uses.
fn repair(
    start: ldnddev_theme::Rgb,
    background: ldnddev_theme::Rgb,
    floor: f64,
) -> ldnddev_theme::Rgb {
    if ldnddev_theme::contrast_ratio(start, background) >= floor {
        return start;
    }
    let white = ldnddev_theme::Rgb::new(255, 255, 255);
    let black = ldnddev_theme::Rgb::new(0, 0, 0);
    let target = if ldnddev_theme::contrast_ratio(white, background)
        >= ldnddev_theme::contrast_ratio(black, background)
    {
        white
    } else {
        black
    };
    let mut current = start;
    for step in 1..=20 {
        let amount = f64::from(step) * 0.05;
        current = ldnddev_theme::mix(start, target, amount);
        if ldnddev_theme::contrast_ratio(current, background) >= floor {
            return current;
        }
    }
    current
}

fn rgb_of(color: Color) -> Option<ldnddev_theme::Rgb> {
    match color {
        Color::Rgb(r, g, b) => Some(ldnddev_theme::Rgb { r, g, b }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_contrast_accent_is_shifted_until_readable() {
        let background = Color::Rgb(0x1c, 0x1e, 0x21);
        let accent = Color::Rgb(0x1d, 0x1f, 0x22);
        let text = Color::Rgb(0xf5, 0xf6, 0xf7);
        let painted = readable(accent, text, background);
        let Color::Rgb(r, g, b) = painted else {
            panic!("expected an rgb color");
        };
        let ratio = ldnddev_theme::contrast_ratio(
            ldnddev_theme::Rgb { r, g, b },
            ldnddev_theme::Rgb {
                r: 0x1c,
                g: 0x1e,
                b: 0x21,
            },
        );
        assert!(ratio >= TEXT_CONTRAST, "{ratio}");
        assert_ne!(painted, accent);
    }

    #[test]
    fn readable_accent_stays_on_the_message() {
        let background = Color::Rgb(0x1c, 0x1e, 0x21);
        let accent = Color::Rgb(0xf5, 0xc4, 0x69);
        let text = Color::Rgb(0xf5, 0xf6, 0xf7);
        assert_eq!(readable(accent, text, background), accent);
    }
}
