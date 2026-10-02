//! Bounded UTF-8 scalar editing for menus, independent of platform key events.
pub const MAX_LINE_BYTES: usize = 4096;

/// Absolute UTF-8 byte ranges in a visual preedit, never a committed selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Composition {
    pub range: (usize, usize),
    pub selection: Option<(usize, usize)>,
}
/// Borrowed field window with scalar-column decorations clipped to its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleLine<'a> {
    pub value: &'a str,
    pub caret: usize,
    /// Native preedit with no cursor range hides the caret even when its text is clipped.
    pub caret_visible: bool,
    pub composition: Option<(usize, usize)>,
    pub selection: Option<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineEditor {
    value: String,
    cursor: usize,
    max_bytes: usize,
    composition: Option<Composition>,
    anchor: Option<usize>,
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
            composition: None,
            anchor: None,
        })
    }
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Byte offset, always on a UTF-8 scalar boundary.
    pub const fn cursor(&self) -> usize {
        self.cursor
    }
    pub const fn composition(&self) -> Option<Composition> {
        self.composition
    }
    /// Positive, sorted UTF-8 byte range for ordinary committed-text selection.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|&anchor| anchor != self.cursor)
            .map(|anchor| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }
    pub fn select_all(&mut self) {
        self.composition = None;
        self.cursor = self.value.len();
        self.anchor = (!self.value.is_empty()).then_some(0);
    }
    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.composition = None;
    }
    /// Validation failure preserves content, cursor, selection and composition.
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        if text.chars().any(invalid_character) {
            return Err("text input cannot contain control characters".into());
        }
        let (begin, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        if (self.value.len() - (end - begin))
            .checked_add(text.len())
            .is_none_or(|len| len > self.max_bytes)
        {
            return Err("text input exceeds its byte limit".into());
        }
        self.value.replace_range(begin..end, text);
        self.cursor = begin + text.len();
        self.anchor = None;
        self.composition = None;
        Ok(())
    }
    /// Clones a visual replacement of the committed selection, preserving this base.
    /// Native cursor endpoints are UTF-8 byte offsets relative to the replacement.
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
        preview.composition = None;
        if text.is_empty() {
            return Ok(preview); // Cancellation never deletes a committed selection.
        }
        let begin = self.selection().map_or(self.cursor, |range| range.0);
        preview.insert(text)?;
        if let Some((start, _)) = cursor {
            preview.cursor = begin + start;
        }
        preview.composition = Some(Composition {
            range: (begin, begin + text.len()),
            selection: cursor.map(|(start, end)| (begin + start, begin + end)),
        });
        Ok(preview)
    }
    fn move_to(&mut self, cursor: usize, extend: bool) {
        let anchor = extend.then(|| self.anchor.unwrap_or(self.cursor));
        self.cursor = cursor;
        self.anchor = anchor.filter(|&anchor| anchor != cursor);
        self.composition = None;
    }
    pub fn move_left(&mut self, extend: bool) {
        let cursor = if !extend && self.selection().is_some() {
            self.selection().unwrap().0
        } else {
            self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(at, _)| at)
        };
        self.move_to(cursor, extend);
    }
    pub fn move_right(&mut self, extend: bool) {
        let cursor = if !extend && self.selection().is_some() {
            self.selection().unwrap().1
        } else {
            self.value[self.cursor..]
                .chars()
                .next()
                .map_or(self.cursor, |character| self.cursor + character.len_utf8())
        };
        self.move_to(cursor, extend);
    }
    pub fn move_home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }
    pub fn move_end(&mut self, extend: bool) {
        self.move_to(self.value.len(), extend);
    }
    pub fn left(&mut self) {
        self.move_left(false);
    }
    pub fn right(&mut self) {
        self.move_right(false);
    }
    pub fn home(&mut self) {
        self.move_home(false);
    }
    pub fn end(&mut self) {
        self.move_end(false);
    }
    fn remove_selection(&mut self) -> bool {
        let Some((begin, end)) = self.selection() else {
            return false;
        };
        self.value.replace_range(begin..end, "");
        self.cursor = begin;
        self.anchor = None;
        true
    }
    pub fn backspace(&mut self) {
        self.composition = None;
        if self.remove_selection() {
            return;
        }
        let previous = self.cursor;
        self.left();
        self.value.replace_range(self.cursor..previous, "");
    }
    pub fn delete(&mut self) {
        self.composition = None;
        if self.remove_selection() {
            return;
        }
        if let Some(character) = self.value[self.cursor..].chars().next() {
            self.value
                .replace_range(self.cursor..self.cursor + character.len_utf8(), "");
        }
    }
    /// A bounded scalar window and caret column using the bitmap font metrics.
    pub fn visible(&self, max_chars: usize) -> (&str, usize) {
        let line = self.visible_line(max_chars);
        (line.value, line.caret)
    }
    /// Projects preedit decorations without allocating or splitting UTF-8 scalars.
    pub fn visible_line(&self, max_chars: usize) -> VisibleLine<'_> {
        let caret_visible = self
            .composition
            .is_none_or(|composition| composition.selection.is_some());
        if max_chars == 0 {
            return VisibleLine {
                value: &self.value[self.cursor..self.cursor],
                caret: 0,
                caret_visible,
                composition: None,
                selection: None,
            };
        }
        let column = self.value[..self.cursor].chars().count();
        let first = self.composition.map_or_else(
            || column.saturating_sub(max_chars),
            |composition| {
                self.value[..composition.range.1]
                    .chars()
                    .count()
                    .saturating_sub(max_chars)
                    .min(column)
            },
        );
        let start = self
            .value
            .char_indices()
            .nth(first)
            .map_or(self.value.len(), |(at, _)| at);
        let end = self.value[start..]
            .char_indices()
            .nth(max_chars)
            .map_or(self.value.len(), |(at, _)| start + at);
        let value = &self.value[start..end];
        let last = first + value.chars().count();
        let clip = |(begin, end): (usize, usize)| {
            let begin = self.value[..begin].chars().count().max(first);
            let end = self.value[..end].chars().count().min(last);
            (begin < end).then(|| (begin - first, end - first))
        };
        VisibleLine {
            value,
            caret: column - first,
            caret_visible,
            composition: self
                .composition
                .and_then(|composition| clip(composition.range)),
            selection: self
                .composition
                .map_or_else(|| self.selection(), |composition| composition.selection)
                .and_then(clip),
        }
    }
}
fn invalid_character(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn committed_selection_tracks_scalar_boundaries_reversal_and_plain_collapse() {
        let mut line = LineEditor::new("a별b", 16).unwrap();
        line.home();
        line.move_right(true);
        assert_eq!(line.selection(), Some((0, 1)));
        line.move_right(true);
        assert_eq!(line.selection(), Some((0, 4)));
        line.move_left(true);
        assert_eq!(line.selection(), Some((0, 1)));
        line.move_left(true);
        assert_eq!(line.selection(), None);
        assert_eq!(line.anchor, None);
        line.right();
        line.move_right(true);
        assert_eq!(line.selection(), Some((1, 4)));
        line.move_left(true);
        line.move_left(true);
        assert_eq!(line.selection(), Some((0, 1)));
        line.right();
        assert_eq!(line.cursor(), 1);
        assert!(line.selection().is_none());
        line.move_end(true);
        assert_eq!(line.selection(), Some((1, 5)));
        line.left();
        assert_eq!(line.cursor(), 1);
        assert!(line.selection().is_none());
        line.move_home(true);
        assert_eq!(line.selection(), Some((0, 1)));
        line.end();
        assert_eq!(line.cursor(), 5);
        assert!(line.selection().is_none());
        line.select_all();
        assert_eq!(line.selection(), Some((0, 5)));
        line.clear_selection();
        assert_eq!(line.cursor(), 5);
        assert!(line.selection().is_none());
        let mut empty = LineEditor::new("", 1).unwrap();
        empty.select_all();
        assert_eq!(empty.selection(), None);
        empty.move_left(true);
        empty.move_end(true);
        assert_eq!(empty.anchor, None);
    }
    #[test]
    fn full_capacity_replacement_and_both_deletions_are_atomic() {
        let mut line = LineEditor::new("a별b", 5).unwrap();
        line.home();
        line.right();
        line.move_right(true);
        let original = line.clone();
        for text in ["éé", "\n", "\u{2028}"] {
            assert!(line.insert(text).is_err());
            assert_eq!(line, original);
        }
        line.insert("音").unwrap();
        assert_eq!(line.value(), "a音b");
        assert_eq!(line.cursor(), 4);
        assert!(line.selection().is_none());
        for deletion in [LineEditor::delete, LineEditor::backspace] {
            let mut selected = original.clone();
            deletion(&mut selected);
            assert_eq!((selected.value(), selected.cursor()), ("ab", 1));
            assert_eq!(selected.selection(), None);
        }
        let mut all = original;
        all.select_all();
        all.insert("éé").unwrap();
        assert_eq!(all.value(), "éé");
        assert_eq!(all.cursor(), 4);
        all.select_all();
        all.insert("").unwrap();
        assert_eq!(all.value(), "");
    }
    #[test]
    fn committed_selection_is_clipped_without_becoming_composition() {
        let mut line = LineEditor::new("a별cd音f", 32).unwrap();
        line.home();
        line.move_end(true);
        assert_eq!(
            line.visible_line(3),
            VisibleLine {
                value: "d音f",
                caret: 3,
                caret_visible: true,
                composition: None,
                selection: Some((0, 3))
            }
        );
        assert_eq!(
            line.visible_line(0),
            VisibleLine {
                value: "",
                caret: 0,
                caret_visible: true,
                composition: None,
                selection: None
            }
        );
        let before = line.clone();
        line.move_left(true);
        assert_ne!(line, before);
        assert_eq!(line.value(), before.value());
        assert!(line.value().is_char_boundary(line.selection().unwrap().1));
    }
    #[test]
    fn ime_preview_replaces_selected_base_once_and_empty_preview_cancels() {
        let mut base = LineEditor::new("a別bc", 8).unwrap();
        base.home();
        base.right();
        base.move_right(true);
        let original = base.clone();
        let preview = base.preedit("音é", Some((3, 5))).unwrap();
        assert_eq!(preview.value(), "a音ébc");
        assert_eq!(preview.cursor(), 4);
        assert_eq!(preview.selection(), None);
        assert_eq!(
            preview.composition(),
            Some(Composition {
                range: (1, 6),
                selection: Some((4, 6))
            })
        );
        assert_eq!(preview.visible_line(8).selection, Some((2, 3)));
        assert_eq!(base, original);
        assert_eq!(base.preedit("", None).unwrap(), base);
        assert_eq!(base.preedit("", Some((0, 0))).unwrap(), base);
        assert!(base.preedit("音音", None).is_err());
        assert_eq!(base, original);
        assert!(base.preedit("音", Some((1, 3))).is_err());
        assert_eq!(base, original);
        assert!(
            !base
                .preedit("音é", None)
                .unwrap()
                .visible_line(8)
                .caret_visible
        );
        base.insert("音é").unwrap();
        assert_eq!(base.value(), preview.value());
        assert_eq!(base.cursor(), 6);
        assert!(base.selection().is_none());
        assert!(base.composition().is_none());
        let mut reversed = original;
        reversed.move_left(true); // collapsed at selection's original anchor.
        reversed.move_right(true);
        reversed.move_left(true);
        reversed.move_home(true);
        assert_eq!(reversed.selection(), Some((0, 1)));
        assert_eq!(
            reversed
                .preedit("音", None)
                .unwrap()
                .composition()
                .unwrap()
                .range,
            (0, 3)
        );
    }
    #[test]
    fn native_missing_cursor_hides_preedit_caret_until_composition_is_cleared() {
        let base = LineEditor::new("ab", 16).unwrap();
        let hidden = base.preedit("音", None).unwrap();
        assert!(!hidden.visible_line(8).caret_visible);
        assert!(!hidden.visible_line(0).caret_visible);
        assert_eq!(hidden.visible(8), ("ab音", 3));
        for selection in [Some((0, 0)), Some((0, 3)), Some((3, 3))] {
            let shown = base.preedit("音", selection).unwrap();
            assert!(shown.visible_line(8).caret_visible);
            assert!(shown.visible_line(0).caret_visible);
        }
        assert!(base.visible_line(0).caret_visible);
        assert!(
            base.preedit("", None)
                .unwrap()
                .visible_line(8)
                .caret_visible
        );
        assert!(
            hidden
                .preedit("", None)
                .unwrap()
                .visible_line(0)
                .caret_visible
        );
    }
    #[test]
    fn absolute_multibyte_composition_and_native_selection_project_scalar_columns() {
        let mut base = LineEditor::new("a별b", 32).unwrap();
        base.left();
        let original = base.clone();
        let preview = base.preedit("音é", Some((3, 5))).unwrap();
        assert_eq!(
            preview.composition(),
            Some(Composition {
                range: (4, 9),
                selection: Some((7, 9))
            })
        );
        assert_eq!(
            preview.visible_line(8),
            VisibleLine {
                value: "a별音éb",
                caret: 3,
                caret_visible: true,
                composition: Some((2, 4)),
                selection: Some((3, 4))
            }
        );
        assert_eq!(
            preview.visible_line(2),
            VisibleLine {
                value: "音é",
                caret: 1,
                caret_visible: true,
                composition: Some((0, 2)),
                selection: Some((1, 2))
            }
        );
        assert_eq!(preview.visible(2), ("音é", 1));
        assert_eq!(base, original);
    }
    #[test]
    fn selected_end_changes_equality_without_changing_value_caret_or_full_range() {
        let base = LineEditor::new("a", 16).unwrap();
        let collapsed = base.preedit("音é", Some((0, 0))).unwrap();
        let selected = base.preedit("音é", Some((0, 3))).unwrap();
        assert_eq!(collapsed.value(), selected.value());
        assert_eq!(collapsed.cursor(), selected.cursor());
        assert_eq!(
            collapsed.composition().unwrap().range,
            selected.composition().unwrap().range
        );
        assert_ne!(collapsed, selected);
        assert_eq!(collapsed.visible_line(8).selection, None);
        assert_eq!(selected.visible_line(8).selection, Some((1, 2)));
        assert_eq!(
            base.preedit("音é", None)
                .unwrap()
                .composition()
                .unwrap()
                .selection,
            None
        );
    }
    #[test]
    fn long_composition_scrolls_toward_its_end_without_hiding_the_caret() {
        let base = LineEditor::new("ab", 64).unwrap();
        let start = base.preedit("音별éxyz", Some((0, 8))).unwrap();
        assert_eq!(
            start.visible_line(3),
            VisibleLine {
                value: "音별é",
                caret: 0,
                caret_visible: true,
                composition: Some((0, 3)),
                selection: Some((0, 3))
            }
        );
        let middle = base.preedit("音별éxyz", Some((8, 11))).unwrap();
        assert_eq!(
            middle.visible_line(3),
            VisibleLine {
                value: "xyz",
                caret: 0,
                caret_visible: true,
                composition: Some((0, 3)),
                selection: Some((0, 3))
            }
        );
        let end = base.preedit("音별éxyz", None).unwrap();
        assert_eq!(
            end.visible_line(3),
            VisibleLine {
                value: "xyz",
                caret: 3,
                caret_visible: false,
                composition: Some((0, 3)),
                selection: None
            }
        );
        assert_eq!(
            end.visible_line(0),
            VisibleLine {
                value: "",
                caret: 0,
                caret_visible: false,
                composition: None,
                selection: None
            }
        );
        assert_eq!(start.visible_line(1).value, "音");
        assert_eq!(end.visible_line(usize::MAX).value, "ab音별éxyz");
    }
    #[test]
    fn rejected_edits_retain_preview_and_successful_mutations_clear_only_metadata() {
        let base = LineEditor::new("ab", 8).unwrap();
        let preview = base.preedit("音", Some((0, 3))).unwrap();
        let mut rejected = preview.clone();
        for text in ["\n", "別別"] {
            assert!(rejected.insert(text).is_err());
            assert_eq!(rejected, preview);
        }
        assert!(rejected.preedit("音", Some((1, 3))).is_err());
        assert_eq!(rejected, preview);
        let mut insert = preview.clone();
        insert.insert("").unwrap();
        assert!(insert.composition().is_none());
        assert_eq!(insert.value(), preview.value());
        for edit in [
            LineEditor::left,
            LineEditor::right,
            LineEditor::home,
            LineEditor::end,
            LineEditor::backspace,
            LineEditor::delete,
        ] {
            let mut edited = preview.clone();
            edit(&mut edited);
            assert!(edited.composition().is_none());
            assert!(edited.value().is_char_boundary(edited.cursor()));
        }
        let empty = preview.preedit("", None).unwrap();
        assert!(empty.composition().is_none());
        assert_eq!(empty.value(), preview.value());
        assert_eq!(base.composition(), None);
    }
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
