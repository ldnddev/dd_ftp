//! Reusable single-line text editor model: value + cursor + optional selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    pub value: String,
    pub cursor: usize,         // char index in 0..=len
    pub anchor: Option<usize>, // Some => active selection between anchor and cursor
}

impl TextField {
    // Inherent constructor, not the `FromStr` trait — callers use `TextField::from_str`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let len = s.chars().count();
        TextField {
            value: s.to_string(),
            cursor: len,
            anchor: None,
        }
    }

    pub fn len(&self) -> usize {
        self.value.chars().count()
    }
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Selection as a half-open char range [start, end), if active and non-empty.
    pub fn selected_range(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let (lo, hi) = if a <= self.cursor {
            (a, self.cursor)
        } else {
            (self.cursor, a)
        };
        if lo == hi {
            None
        } else {
            Some((lo, hi))
        }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.value.len())
    }

    fn delete_range(&mut self, lo: usize, hi: usize) {
        let blo = self.byte_at(lo);
        let bhi = self.byte_at(hi);
        self.value.replace_range(blo..bhi, "");
        self.cursor = lo;
        self.anchor = None;
    }

    fn delete_selection_if_any(&mut self) -> bool {
        if let Some((lo, hi)) = self.selected_range() {
            self.delete_range(lo, hi);
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.delete_selection_if_any();
        let b = self.byte_at(self.cursor);
        self.value.insert(b, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        if self.cursor > 0 {
            self.delete_range(self.cursor - 1, self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        if self.cursor < self.len() {
            self.delete_range(self.cursor, self.cursor + 1);
        }
    }

    /// dir: -1 left, +1 right. shift extends selection, else collapses it.
    pub fn move_cursor(&mut self, dir: i32, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        if dir < 0 {
            self.cursor = self.cursor.saturating_sub(1);
        } else if dir > 0 {
            self.cursor = (self.cursor + 1).min(self.len());
        }
    }

    pub fn move_home(&mut self, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = 0;
    }

    pub fn move_end(&mut self, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = self.len();
    }

    pub fn delete_word_left(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        let chars: Vec<char> = self.value.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        self.delete_range(i, self.cursor);
    }

    pub fn set_cursor(&mut self, idx: usize) {
        self.cursor = idx.min(self.len());
        self.anchor = None;
    }

    pub fn begin_drag(&mut self, idx: usize) {
        let i = idx.min(self.len());
        self.cursor = i;
        self.anchor = Some(i);
    }

    pub fn extend_drag(&mut self, idx: usize) {
        self.cursor = idx.min(self.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tf(s: &str, cursor: usize) -> TextField {
        TextField {
            value: s.to_string(),
            cursor,
            anchor: None,
        }
    }

    #[test]
    fn insert_at_cursor() {
        let mut f = tf("ac", 1);
        f.insert_char('b');
        assert_eq!(f.value, "abc");
        assert_eq!(f.cursor, 2);
    }

    #[test]
    fn backspace_removes_left_of_cursor() {
        let mut f = tf("abc", 2);
        f.backspace();
        assert_eq!(f.value, "ac");
        assert_eq!(f.cursor, 1);
    }

    #[test]
    fn typing_replaces_active_selection() {
        let mut f = TextField {
            value: "hello".into(),
            cursor: 4,
            anchor: Some(1),
        };
        f.insert_char('X'); // selection [1,4) = "ell" replaced
        assert_eq!(f.value, "hXo");
        assert_eq!(f.cursor, 2);
        assert_eq!(f.anchor, None);
    }

    #[test]
    fn backspace_deletes_active_selection() {
        let mut f = TextField {
            value: "hello".into(),
            cursor: 1,
            anchor: Some(4),
        };
        f.backspace();
        assert_eq!(f.value, "ho");
        assert_eq!(f.cursor, 1);
        assert_eq!(f.anchor, None);
    }

    #[test]
    fn move_right_with_shift_extends_selection() {
        let mut f = tf("abc", 0);
        f.move_cursor(1, true);
        assert_eq!(f.anchor, Some(0));
        assert_eq!(f.cursor, 1);
        f.move_cursor(1, false); // no shift collapses
        assert_eq!(f.anchor, None);
    }

    #[test]
    fn set_and_drag_cursor_clamp() {
        let mut f = tf("abc", 0);
        f.set_cursor(99);
        assert_eq!(f.cursor, 3);
        f.begin_drag(1);
        assert_eq!(f.anchor, Some(1));
        f.extend_drag(99);
        assert_eq!(f.cursor, 3);
        assert_eq!(f.anchor, Some(1));
    }

    #[test]
    fn delete_word_left() {
        let mut f = tf("foo bar", 7);
        f.delete_word_left();
        assert_eq!(f.value, "foo ");
        assert_eq!(f.cursor, 4);
    }

    #[test]
    fn home_end() {
        let mut f = tf("abc", 1);
        f.move_home(false);
        assert_eq!(f.cursor, 0);
        f.move_end(false);
        assert_eq!(f.cursor, 3);
    }
}
