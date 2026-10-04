//! Deferred borrowed bitmap windows with literal whole-cluster boundaries.
use super::text_input::LineEditor;

#[test]
fn bitmap_scalar_budgets_omit_whole_clusters_and_keep_full_string_flag_pairing() {
    for (text, scalars) in [
        ("e\u{301}", 2),
        ("\u{1100}\u{1161}\u{11a8}", 3),
        ("👩\u{200d}💻", 3),
        ("👍🏽", 2),
        ("❤\u{fe0f}", 2),
        ("\u{301}", 1),
    ] {
        let mut editor = LineEditor::new(text, 64).unwrap();
        let retained = editor.clone();
        let full = editor.visible_line(scalars);
        assert_eq!((full.value, full.caret), (text, scalars));
        assert_eq!(full.value.as_ptr(), editor.value().as_ptr());
        for budget in 0..scalars {
            let empty = editor.visible_line(budget);
            assert_eq!(
                (empty.value, empty.caret, empty.composition, empty.selection),
                ("", 0, None, None)
            );
            assert_eq!(empty.value.as_ptr(), editor.value()[text.len()..].as_ptr());
        }
        assert_eq!(editor, retained);
        editor.home();
        assert_eq!(editor.visible_line(scalars).value, text);
        assert_eq!(editor.visible_line(scalars).caret, 0);
        let empty = editor.visible_line(scalars - 1);
        assert_eq!(empty.value, "");
        assert_eq!(empty.value.as_ptr(), editor.value().as_ptr());
    }
    let mut line = LineEditor::new("Ae\u{301}B", 16).unwrap();
    assert_eq!(line.visible(2), ("B", 1));
    line.left();
    assert_eq!(line.cursor(), 4);
    assert_eq!(line.visible(2), ("e\u{301}", 2));
    assert_eq!(
        line.visible_line(2).value.as_ptr(),
        line.value()[1..].as_ptr()
    );
    line.home();
    assert_eq!(line.visible(2), ("A", 0));
    assert_eq!(line.visible(3), ("Ae\u{301}", 0));
    line.select_all();
    assert_eq!(line.visible_line(2).selection, Some((0, 1)));

    let mut flags = LineEditor::new("🇦🇧🇨🇩🇪", 32).unwrap();
    assert_eq!(flags.visible(3), ("🇨🇩🇪", 3));
    assert_eq!(
        flags.visible_line(3).value.as_ptr(),
        flags.value()[8..].as_ptr()
    );
    assert_eq!(flags.visible(2), ("🇪", 1));
    flags.home();
    assert_eq!(flags.visible(3), ("🇦🇧", 0));
    flags.right();
    assert_eq!(flags.cursor(), 8);
    assert_eq!(flags.visible(1), ("", 0));
    assert_eq!(
        flags.visible_line(1).value.as_ptr(),
        flags.value()[8..].as_ptr()
    );

    let mut ordinary = LineEditor::new("a별cd音f", 32).unwrap();
    assert_eq!(ordinary.visible(3), ("d音f", 3));
    ordinary.left();
    ordinary.left();
    assert_eq!(ordinary.visible(3), ("별cd", 3));
    ordinary.home();
    assert_eq!(ordinary.visible(3), ("a별c", 0));
}

#[test]
fn bitmap_composition_priority_retains_native_internal_scalar_decorations_and_missing_cursor() {
    let mut base = LineEditor::new("AZ", 32).unwrap();
    base.home();
    base.right();
    let original = base.clone();
    let preview = base.preedit("e\u{301}x", Some((1, 3))).unwrap();
    let full = preview.visible_line(3);
    assert_eq!(
        (full.value, full.caret, full.composition, full.selection),
        ("e\u{301}x", 1, Some((0, 3)), Some((1, 2)))
    );
    assert_eq!(full.value.as_ptr(), preview.value()[1..].as_ptr());
    let caret_cluster = preview.visible_line(2);
    assert_eq!(
        (
            caret_cluster.value,
            caret_cluster.caret,
            caret_cluster.composition,
            caret_cluster.selection
        ),
        ("e\u{301}", 1, Some((0, 2)), Some((1, 2)))
    );
    for budget in [0, 1] {
        let empty = preview.visible_line(budget);
        assert_eq!(
            (empty.value, empty.caret, empty.composition, empty.selection),
            ("", 0, None, None)
        );
        assert_eq!(empty.value.as_ptr(), preview.value()[2..].as_ptr());
        assert!(empty.caret_visible);
    }
    let hidden = base.preedit("e\u{301}x", None).unwrap();
    let full = hidden.visible_line(3);
    assert_eq!(
        (full.value, full.caret, full.composition, full.selection),
        ("e\u{301}x", 3, Some((0, 3)), None)
    );
    assert!(!full.caret_visible);
    assert!(!hidden.visible_line(0).caret_visible);
    assert_eq!(base, original);

    // The native replacement is just ZWJ, but its enclosing displayed cluster
    // includes the original glyphs on both sides of the composition range.
    let mut joined = LineEditor::new("👩💻", 32).unwrap();
    joined.home();
    joined.right();
    let preview = joined.preedit("\u{200d}", Some((0, 3))).unwrap();
    let full = preview.visible_line(3);
    assert_eq!(
        (full.value, full.caret, full.composition, full.selection),
        ("👩\u{200d}💻", 1, Some((1, 2)), Some((1, 2)))
    );
    assert_eq!(full.value.as_ptr(), preview.value().as_ptr());
    let short = preview.visible_line(2);
    assert_eq!((short.value, short.caret), ("", 0));
    assert_eq!(short.value.as_ptr(), preview.value()[4..].as_ptr());
    assert_eq!((joined.value(), joined.cursor()), ("👩💻", 4));
}
