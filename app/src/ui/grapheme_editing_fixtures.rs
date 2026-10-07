//! Deferred committed grapheme editing; expected byte boundaries are literal.
//! Native preedit and visible bitmap columns intentionally retain scalar metrics.
use super::text_input::LineEditor;

#[test]
fn navigation_and_each_deletion_keep_literal_extended_clusters_whole() {
    for (cluster, bytes) in [
        ("a", 1),
        ("é", 2),
        ("별", 3),
        ("e\u{301}", 3),
        ("\u{301}", 2),
        ("\u{1100}\u{1161}\u{11a8}", 9),
        ("👍🏽", 8),
        ("👩\u{200d}💻", 11),
        ("👨\u{200d}👩\u{200d}👧\u{200d}👦", 25),
        ("❤\u{fe0f}", 6),
        ("1\u{fe0f}\u{20e3}", 7),
    ] {
        assert_eq!(cluster.len(), bytes);
        let text = format!("{cluster}x");
        let mut line = LineEditor::new(&text, 64).unwrap();
        assert_eq!(line.cursor(), bytes + 1);
        line.left();
        assert_eq!(line.cursor(), bytes);
        line.left();
        assert_eq!(line.cursor(), 0);
        line.left();
        assert_eq!(line.cursor(), 0);
        line.right();
        assert_eq!(line.cursor(), bytes);
        line.right();
        assert_eq!(line.cursor(), bytes + 1);
        line.right();
        assert_eq!(line.cursor(), bytes + 1);
        line.home();
        line.delete();
        assert_eq!((line.value(), line.cursor()), ("x", 0));
        let mut line = LineEditor::new(&text, 64).unwrap();
        line.left();
        line.backspace();
        assert_eq!((line.value(), line.cursor()), ("x", 0));
        line.backspace();
        assert_eq!((line.value(), line.cursor()), ("x", 0));
        line.end();
        line.delete();
        assert_eq!((line.value(), line.cursor()), ("x", 1));
    }
}

#[test]
fn regional_indicator_pairs_use_full_string_parity_after_navigation_insertion_and_deletion() {
    let mut flags = LineEditor::new("🇦🇧🇨🇩🇪", 64).unwrap();
    assert_eq!(flags.value().len(), 20);
    for expected in [16, 8, 0, 0] {
        flags.left();
        assert_eq!(flags.cursor(), expected);
    }
    for expected in [8, 16, 20, 20] {
        flags.right();
        assert_eq!(flags.cursor(), expected);
    }
    flags.home();
    flags.insert("🇫").unwrap();
    assert_eq!(flags.value(), "🇫🇦🇧🇨🇩🇪");
    assert_eq!(
        flags.cursor(),
        8,
        "inserted RI joins its following RI before the caret is admitted"
    );
    flags.home();
    for expected in [8, 16, 24] {
        flags.move_right(true);
        assert_eq!(flags.selection(), Some((0, expected)));
    }
    flags.left();
    assert_eq!((flags.cursor(), flags.selection()), (0, None));
    flags.right();
    flags.delete();
    assert_eq!((flags.value(), flags.cursor()), ("🇫🇦🇩🇪", 8));
    flags.backspace();
    assert_eq!((flags.value(), flags.cursor()), ("🇩🇪", 0));

    for backward in [false, true] {
        let mut separated = LineEditor::new("🇦x🇧🇨", 64).unwrap();
        separated.home();
        separated.right();
        if backward {
            separated.right();
            separated.backspace();
        } else {
            separated.delete();
        }
        assert_eq!((separated.value(), separated.cursor()), ("🇦🇧🇨", 8));
        separated.backspace();
        assert_eq!((separated.value(), separated.cursor()), ("🇨", 0));
        separated.right();
        assert_eq!(separated.cursor(), 4);
    }
}

#[test]
fn shifted_selection_reverses_and_collapses_at_cluster_edges_and_cut_or_replace_consumes_it_once() {
    let mut line = LineEditor::new("ae\u{301}👩\u{200d}💻Z", 32).unwrap();
    assert_eq!(line.value().len(), 16);
    line.home();
    line.right(); // Anchor will begin at byte 1.
    line.move_right(true);
    assert_eq!(line.selection(), Some((1, 4)));
    line.move_right(true);
    assert_eq!(line.selection(), Some((1, 15)));
    line.move_left(true);
    assert_eq!(line.selection(), Some((1, 4)));
    line.move_left(true);
    assert_eq!(line.selection(), None);
    line.move_left(true);
    assert_eq!(line.selection(), Some((0, 1)));
    line.right();
    assert_eq!((line.cursor(), line.selection()), (1, None));
    line.move_end(true);
    assert_eq!(line.selection(), Some((1, 16)));
    line.left();
    assert_eq!((line.cursor(), line.selection()), (1, None));
    line.right();
    line.move_right(true);
    assert_eq!(line.selection(), Some((4, 15)));
    assert_eq!(
        &line.value()[4..15],
        "👩\u{200d}💻",
        "the clipboard's selected bytes are the complete cluster"
    );
    let selected = line.clone();
    for remove in [LineEditor::delete, LineEditor::backspace] {
        let mut cut = selected.clone();
        remove(&mut cut);
        assert_eq!(
            (cut.value(), cut.cursor(), cut.selection()),
            ("ae\u{301}Z", 4, None)
        );
    }
    line.insert("音").unwrap();
    assert_eq!(
        (line.value(), line.cursor(), line.selection()),
        ("ae\u{301}音Z", 7, None)
    );
    line.select_all();
    assert_eq!(line.selection(), Some((0, 8)));
    line.insert("").unwrap();
    assert_eq!(
        (line.value(), line.cursor(), line.selection()),
        ("", 0, None)
    );
}

#[test]
fn successful_mutations_that_join_neighbors_snap_forward_in_the_resulting_full_text() {
    let mut accent = LineEditor::new("\u{301}x", 16).unwrap();
    accent.home();
    accent.insert("e").unwrap();
    assert_eq!(accent.value().as_bytes(), &[0x65, 0xcc, 0x81, 0x78]);
    assert_eq!(accent.cursor(), 3);
    accent.backspace();
    assert_eq!((accent.value(), accent.cursor()), ("x", 0));

    let mut emoji = LineEditor::new("👩💻", 32).unwrap();
    emoji.home();
    emoji.right();
    emoji.insert("\u{200d}").unwrap();
    assert_eq!((emoji.value(), emoji.cursor()), ("👩\u{200d}💻", 11));
    emoji.backspace();
    assert_eq!((emoji.value(), emoji.cursor()), ("", 0));

    for remove in [LineEditor::delete, LineEditor::backspace] {
        let mut hangul = LineEditor::new("\u{1100}x\u{1161}\u{11a8}!", 32).unwrap();
        hangul.home();
        hangul.right();
        hangul.move_right(true);
        assert_eq!(hangul.selection(), Some((3, 4)));
        remove(&mut hangul);
        assert_eq!(
            (hangul.value(), hangul.cursor(), hangul.selection()),
            ("\u{1100}\u{1161}\u{11a8}!", 9, None)
        );
        hangul.backspace();
        assert_eq!((hangul.value(), hangul.cursor()), ("!", 0));
    }
    let mut replaced = LineEditor::new("\u{1100}x\u{1161}", 16).unwrap();
    replaced.home();
    replaced.right();
    replaced.move_right(true);
    replaced.insert("").unwrap();
    assert_eq!(
        (replaced.value(), replaced.cursor()),
        ("\u{1100}\u{1161}", 6)
    );
}

#[test]
fn invalid_controls_and_byte_capacity_preserve_the_entire_selected_or_native_preview_editor() {
    let mut selected = LineEditor::new("ae\u{301}b", 5).unwrap();
    selected.home();
    selected.right();
    selected.move_right(true);
    let original = selected.clone();
    for text in ["éé", "\n", "\t", "\0", "\u{7f}", "\u{2028}", "\u{2029}"] {
        assert!(selected.insert(text).is_err());
        assert_eq!(selected, original);
    }
    selected.insert("音").unwrap();
    assert_eq!((selected.value(), selected.cursor()), ("a音b", 4));
    let mut base = LineEditor::new("AZ", 5).unwrap();
    base.home();
    base.right();
    let preview = base.preedit("e\u{301}", Some((1, 3))).unwrap();
    let mut rejected = preview.clone();
    for text in ["Q", "\n", "\u{2029}"] {
        assert!(rejected.insert(text).is_err());
        assert_eq!(
            rejected, preview,
            "rejection must not normalize or clear native composition"
        );
    }
    for range in [
        Some((2, 3)),
        Some((3, 1)),
        Some((0, 4)),
        Some((usize::MAX, usize::MAX)),
    ] {
        assert!(base.preedit("e\u{301}", range).is_err());
        assert_eq!(
            (base.value(), base.cursor(), base.composition()),
            ("AZ", 1, None)
        );
    }
    assert!(LineEditor::new("\u{301}", 1).is_err());
    let bound = "a".repeat(4096);
    let mut full = LineEditor::new(&bound, 4096).unwrap();
    let before = full.clone();
    assert!(full.insert("b").is_err());
    assert_eq!(full, before);
    assert!(LineEditor::new("", 4097).is_err());
}

#[test]
fn native_preedit_keeps_scalar_endpoints_and_base_cancellation_then_ordinary_edits_normalize_before_acting()
 {
    let mut base = LineEditor::new("AZ", 32).unwrap();
    base.home();
    base.right();
    let saved = base.clone();
    let preview = base.preedit("e\u{301}", Some((1, 3))).unwrap();
    assert_eq!(
        (preview.value(), preview.cursor(), preview.selection()),
        ("Ae\u{301}Z", 2, None)
    );
    let composition = preview.composition().unwrap();
    assert_eq!(composition.range, (1, 4));
    assert_eq!(composition.selection, Some((2, 4)));
    assert_eq!(base, saved);
    assert_eq!(base.preedit("", None).unwrap(), base);
    assert_eq!(base.preedit("", Some((0, 0))).unwrap(), base);
    let visible = preview.visible_line(8);
    assert_eq!(
        (
            visible.value,
            visible.caret,
            visible.composition,
            visible.selection
        ),
        ("Ae\u{301}Z", 2, Some((1, 3)), Some((2, 3)))
    );
    assert!(visible.caret_visible);
    base.insert("e\u{301}").unwrap();
    assert_eq!(
        (base.value(), base.cursor(), base.composition()),
        ("Ae\u{301}Z", 4, None)
    );

    for (edit, expected, cursor) in [
        (LineEditor::left as fn(&mut LineEditor), "Ae\u{301}Z", 1),
        (LineEditor::right, "Ae\u{301}Z", 5),
        (LineEditor::backspace, "AZ", 1),
        (LineEditor::delete, "Ae\u{301}", 4),
        (LineEditor::clear_selection, "Ae\u{301}Z", 4),
    ] {
        let mut line = preview.clone();
        edit(&mut line);
        assert_eq!(
            (line.value(), line.cursor(), line.composition()),
            (expected, cursor, None)
        );
    }
    let mut selected = preview.clone();
    selected.move_left(true);
    assert_eq!((selected.cursor(), selected.selection()), (1, Some((1, 4))));
    let mut inserted = preview;
    inserted.insert("Q").unwrap();
    assert_eq!((inserted.value(), inserted.cursor()), ("Ae\u{301}QZ", 5));

    let mut joining_base = LineEditor::new("\u{301}Z", 16).unwrap();
    joining_base.home();
    let hidden = joining_base.preedit("e", None).unwrap();
    assert_eq!((hidden.value(), hidden.cursor()), ("e\u{301}Z", 1));
    assert_eq!(hidden.composition().unwrap().range, (0, 1));
    assert_eq!(hidden.composition().unwrap().selection, None);
    assert!(!hidden.visible_line(8).caret_visible);
    for cursor in [None, Some((0, 0))] {
        let cancelled = hidden.preedit("", cursor).unwrap();
        assert_eq!(
            (
                cancelled.value(),
                cancelled.cursor(),
                cancelled.composition()
            ),
            ("e\u{301}Z", 3, None)
        );
        assert!(cancelled.visible_line(8).caret_visible);
        assert_eq!(
            hidden.cursor(),
            1,
            "cancelling a preview cannot mutate that original snapshot"
        );
    }
    let mut selected_base = LineEditor::new("ae\u{301}b", 16).unwrap();
    selected_base.home();
    selected_base.right();
    selected_base.move_right(true);
    assert_eq!(selected_base.selection(), Some((1, 4)));
    assert_eq!(selected_base.preedit("", None).unwrap(), selected_base);
    assert_eq!(
        selected_base.preedit("", Some((0, 0))).unwrap(),
        selected_base
    );
    let mut normalized = hidden;
    normalized.insert("").unwrap();
    assert_eq!(
        (
            normalized.value(),
            normalized.cursor(),
            normalized.composition()
        ),
        ("e\u{301}Z", 3, None)
    );
    assert_eq!(
        (joining_base.value(), joining_base.cursor()),
        ("\u{301}Z", 0)
    );
}
