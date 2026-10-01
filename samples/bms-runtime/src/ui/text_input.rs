//! Bounded UTF-8 scalar editing for menus, independent of platform key events.
pub const MAX_LINE_BYTES: usize = 4096;

#[derive(Clone, Debug)]
pub struct LineEditor {
    value: String,
    cursor: usize,
    max_bytes: usize,
}
impl LineEditor {
    pub fn new(value: &str, max_bytes: usize) -> Result<Self, String> {
        if max_bytes == 0 || max_bytes > MAX_LINE_BYTES || value.len() > max_bytes {
            return Err("text input exceeds its byte limit".into());
        }
        if value.chars().any(invalid_character) {
            return Err("text input cannot contain control characters".into());
        }
        let mut text = String::new();
        text.try_reserve_exact(max_bytes)
            .map_err(|_| "text input allocation failed")?;
        text.push_str(value);
        Ok(Self {
            value: text,
            cursor: value.len(),
            max_bytes,
        })
    }
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Byte offset, always on a UTF-8 scalar boundary.
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
    /// Validation failure preserves both content and cursor.
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        if text.chars().any(invalid_character) {
            return Err("text input cannot contain control characters".into());
        }
        if self
            .value
            .len()
            .checked_add(text.len())
            .is_none_or(|len| len > self.max_bytes)
        {
            return Err("text input exceeds its byte limit".into());
        }
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
        Ok(())
    }
    pub fn left(&mut self) {
        self.cursor = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at);
    }
    pub fn right(&mut self) {
        if let Some(character) = self.value[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.value.len();
    }
    pub fn backspace(&mut self) {
        let previous = self.cursor;
        self.left();
        self.value.replace_range(self.cursor..previous, "");
    }
    pub fn delete(&mut self) {
        if let Some(character) = self.value[self.cursor..].chars().next() {
            self.value
                .replace_range(self.cursor..self.cursor + character.len_utf8(), "");
        }
    }
    /// A bounded scalar window and caret column using the bitmap font metrics.
    pub fn visible(&self, max_chars: usize) -> (&str, usize) {
        if max_chars == 0 {
            return (&self.value[self.cursor..self.cursor], 0);
        }
        let column = self.value[..self.cursor].chars().count();
        let first = column.saturating_sub(max_chars);
        let start = self
            .value
            .char_indices()
            .nth(first)
            .map_or(self.value.len(), |(at, _)| at);
        let end = self.value[start..]
            .char_indices()
            .nth(max_chars)
            .map_or(self.value.len(), |(at, _)| start + at);
        (&self.value[start..end], column - first)
    }
}
fn invalid_character(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_cursor_and_rejected_edits_preserve_boundaries() {
        let mut line = LineEditor::new("a별b", 8).unwrap();
        line.left();
        line.backspace();
        assert_eq!((line.value(), line.cursor()), ("ab", 1));
        line.insert("音").unwrap();
        assert_eq!(line.value(), "a音b");
        let before = (line.value().to_owned(), line.cursor());
        assert!(line.insert("\n").is_err());
        assert!(line.insert("별별").is_err());
        assert_eq!((line.value().to_owned(), line.cursor()), before);
        line.home();
        line.delete();
        line.right();
        line.delete();
        assert_eq!(line.value(), "音");
        line.backspace();
        assert_eq!(line.value(), "");
    }
    #[test]
    fn visible_window_tracks_end_and_middle_without_splitting_utf8() {
        let mut line = LineEditor::new("a별cd音f", 32).unwrap();
        assert_eq!(line.visible(3), ("d音f", 3));
        line.left();
        line.left();
        assert_eq!(line.visible(3), ("별cd", 3));
        line.home();
        assert_eq!(line.visible(3), ("a별c", 0));
        assert_eq!(line.visible(0), ("", 0));
    }
}
