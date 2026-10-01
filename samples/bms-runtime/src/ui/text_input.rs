//! Bounded UTF-8 scalar editing for menus, independent of platform key events.
pub const MAX_LINE_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Creates visual composition text at the committed caret without changing
    /// this editor. Cursor endpoints are UTF-8 byte offsets within `text`; the
    /// first endpoint places the preview caret, without replacing a selection.
    pub fn preedit(&self, text: &str, cursor: Option<(usize, usize)>) -> Result<Self, String> {
        if let Some((start, end)) = cursor {
            if start > end
                || end > text.len()
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return Err(
                    "preedit cursor must be ordered UTF-8 scalar boundaries within its text".into(),
                );
            }
        }
        let mut preview = self.clone();
        preview.insert(text)?;
        if let Some((start, _)) = cursor {
            preview.cursor = self.cursor + start;
        }
        Ok(preview)
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
    fn preedit_unicode_middle_insertion_tracks_byte_cursor_and_preserves_base() {
        let mut base = LineEditor::new("a별b", 12).unwrap();
        base.left();
        let original = base.clone();
        let text = "音é";
        for (selection, caret) in [
            (Some((0, 0)), 4),
            (Some((0, 3)), 4),
            (Some((3, 5)), 7),
            (Some((5, 5)), 9),
            (None, 9),
        ] {
            let preview = base.preedit(text, selection).unwrap();
            assert_eq!(preview.value(), "a별音éb");
            assert_eq!(preview.value().len(), 10);
            assert_eq!(preview.cursor(), caret);
            assert!(preview.value().is_char_boundary(preview.cursor()));
            assert_eq!(base, original);
        }
        let exact = LineEditor::new("a별b", 10)
            .unwrap()
            .preedit(text, None)
            .unwrap();
        assert_eq!(exact.value(), "a별b音é");
        assert_eq!(exact.cursor(), 10);
        assert_eq!(exact.max_bytes, 10);
    }
    #[test]
    fn preedit_empty_text_and_zero_or_end_cursors_leave_committed_editor_intact() {
        let mut base = LineEditor::new("ab", 8).unwrap();
        base.home();
        base.right();
        for selection in [None, Some((0, 0))] {
            assert_eq!(base.preedit("", selection).unwrap(), base);
        }
        assert_eq!(base.preedit("音", Some((0, 3))).unwrap().cursor(), 1);
        assert_eq!(base.preedit("音", Some((3, 3))).unwrap().cursor(), 4);
        assert_eq!(base.preedit("音", None).unwrap().value(), "a音b");
        assert_eq!((base.value(), base.cursor()), ("ab", 1));
        let empty = LineEditor::new("", 1).unwrap();
        assert_eq!(empty.preedit("", None).unwrap(), empty);
        assert_eq!(empty.preedit("x", Some((1, 1))).unwrap().cursor(), 1);
        assert_eq!(empty.value(), "");
    }
    #[test]
    fn preedit_rejects_control_capacity_and_invalid_byte_ranges_without_base_mutation() {
        let mut base = LineEditor::new("a別b", 8).unwrap();
        base.left();
        let original = base.clone();
        for selection in [
            Some((3, 0)),
            Some((0, 4)),
            Some((1, 3)),
            Some((0, 2)),
            Some((usize::MAX, usize::MAX)),
        ] {
            assert!(base.preedit("音", selection).is_err());
            assert_eq!(base, original);
        }
        for text in ["\n", "\t", "\u{2028}", "\u{2029}", "音音", "abcdef"] {
            assert!(base.preedit(text, None).is_err());
            assert_eq!(base, original);
        }
        assert!(base.preedit("", Some((0, 1))).is_err());
        assert_eq!(base, original);
        let preview = base.preedit("音", Some((3, 3))).unwrap();
        assert_eq!(preview.value().len(), 8);
        assert_eq!(preview.cursor(), 7);
        assert_eq!(base, original);
    }
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
