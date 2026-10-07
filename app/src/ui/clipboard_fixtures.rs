//! Portable clipboard transactions exercise real bounded editor state without native I/O.
use super::{ClipboardAction, ClipboardEdit, ClipboardRequest};
use crate::ui::text_input::LineEditor;

fn selected_multibyte(reverse: bool, max_bytes: usize) -> LineEditor {
    let mut line = LineEditor::new("L별é🙂R", max_bytes).unwrap();
    if reverse {
        line.left();
        for _ in 0..3 {
            line.move_left(true);
        }
    } else {
        line.home();
        line.right();
        for _ in 0..3 {
            line.move_right(true);
        }
    }
    assert_eq!(line.selection(), Some((1, 10)));
    line
}

#[test]
fn copy_and_cut_preserve_exact_multibyte_selection_in_both_directions() {
    for reverse in [false, true] {
        let base = selected_multibyte(reverse, 16);
        let original = base.clone();
        let copy = ClipboardEdit::prepare(&base, ClipboardAction::Copy)
            .unwrap()
            .unwrap();
        assert_eq!(copy.request(), ClipboardRequest::Write("별é🙂".into()));
        assert!(copy.candidate().is_none());
        assert!(copy.complete(Ok(None)).unwrap().is_none());
        assert_eq!(base, original);

        let cut = ClipboardEdit::prepare(&base, ClipboardAction::Cut)
            .unwrap()
            .unwrap();
        assert_eq!(cut.request(), ClipboardRequest::Write("별é🙂".into()));
        let staged = cut.candidate().unwrap().clone();
        assert_eq!((staged.value(), staged.cursor()), ("LR", 1));
        assert_eq!(staged.selection(), None);
        assert_eq!(staged.composition(), None);
        assert!(cut.matches(&base));
        assert!(!cut.matches(&staged));
        assert_eq!(base, original);
        assert_eq!(cut.complete(Ok(None)).unwrap(), Some(staged));
        assert_eq!(base, original);
    }
}

#[test]
fn copy_and_cut_without_selection_do_not_create_backend_requests() {
    for value in ["", "별é🙂"] {
        let mut base = LineEditor::new(value, 16).unwrap();
        for after_collapsing_selection in [false, true] {
            if after_collapsing_selection {
                base.home();
                base.move_right(true);
                base.move_left(true);
            }
            assert_eq!(base.selection(), None);
            let original = base.clone();
            for action in [ClipboardAction::Copy, ClipboardAction::Cut] {
                assert!(ClipboardEdit::prepare(&base, action).unwrap().is_none());
            }
            let paste = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
                .unwrap()
                .unwrap();
            assert_eq!(paste.request(), ClipboardRequest::Read);
            assert!(paste.candidate().is_none());
            assert_eq!(base, original);
        }
    }
}

#[test]
fn backend_errors_and_wrong_reply_shapes_never_publish_a_cut_or_paste() {
    let base = selected_multibyte(false, 16);
    let original = base.clone();
    for action in [
        ClipboardAction::Copy,
        ClipboardAction::Cut,
        ClipboardAction::Paste,
    ] {
        let edit = ClipboardEdit::prepare(&base, action).unwrap().unwrap();
        if action == ClipboardAction::Cut {
            assert_eq!(edit.candidate().unwrap().value(), "LR");
        }
        assert!(
            edit.complete(Err("native clipboard unavailable".into()))
                .is_err()
        );
        assert_eq!(base, original);

        let edit = ClipboardEdit::prepare(&base, action).unwrap().unwrap();
        let wrong_reply = if action == ClipboardAction::Paste {
            None
        } else {
            Some("unexpected read payload".into())
        };
        assert!(edit.complete(Ok(wrong_reply)).is_err());
        assert_eq!(base, original);
    }
    // A failed write leaves the original selected bytes available for a retry.
    let retry = ClipboardEdit::prepare(&base, ClipboardAction::Cut)
        .unwrap()
        .unwrap();
    assert_eq!(retry.request(), ClipboardRequest::Write("별é🙂".into()));
    assert_eq!(retry.complete(Ok(None)).unwrap().unwrap().value(), "LR");
}

#[test]
fn paste_replaces_selection_at_the_final_byte_limit_without_normalizing_text() {
    let mut base = LineEditor::new("L별R", 7).unwrap();
    base.left();
    base.move_left(true);
    assert_eq!(base.selection(), Some((1, 4)));
    let original = base.clone();
    let paste = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
        .unwrap()
        .unwrap();
    assert!(paste.candidate().is_none());
    // Leading/trailing spaces and a decomposed accent must survive byte-for-byte.
    let payload = " e\u{301} ";
    let result = paste.complete(Ok(Some(payload.into()))).unwrap().unwrap();
    assert_eq!(result.value(), "L e\u{301} R");
    assert_eq!(result.value().len(), 7);
    assert_eq!(result.cursor(), 6);
    assert_eq!(result.selection(), None);
    assert_eq!(result.composition(), None);
    assert_eq!(base, original);

    let oversized_for_field = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
        .unwrap()
        .unwrap();
    assert!(
        oversized_for_field
            .complete(Ok(Some("🙂é".into())))
            .is_err()
    );
    assert_eq!(base, original);

    let empty = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
        .unwrap()
        .unwrap()
        .complete(Ok(Some(String::new())))
        .unwrap()
        .unwrap();
    assert_eq!((empty.value(), empty.cursor()), ("LR", 1));
    assert_eq!(empty.selection(), None);
    let unchanged = ClipboardEdit::prepare(&empty, ClipboardAction::Paste)
        .unwrap()
        .unwrap()
        .complete(Ok(Some(String::new())))
        .unwrap()
        .unwrap();
    assert_eq!(unchanged, empty);
    assert_eq!(base, original);
}

#[test]
fn paste_rejects_controls_and_oversized_utf8_payload_without_losing_selection() {
    let mut base = LineEditor::new("keep", 4096).unwrap();
    base.select_all();
    let original = base.clone();
    let exact = "🙂".repeat(1024);
    assert_eq!(exact.len(), 4096);
    let admitted = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
        .unwrap()
        .unwrap()
        .complete(Ok(Some(exact.clone())))
        .unwrap()
        .unwrap();
    assert_eq!(admitted.value(), exact);
    assert_eq!(admitted.cursor(), 4096);
    assert_eq!(admitted.selection(), None);
    assert_eq!(base, original);

    let oversized = exact + "a";
    assert_eq!(oversized.len(), 4097);
    for payload in [
        oversized.as_str(),
        "prefix\n",
        "\r\n",
        "a\tb",
        "\0",
        "\u{7f}",
        "\u{85}",
        "\u{2028}",
        "\u{2029}",
    ] {
        let paste = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
            .unwrap()
            .unwrap();
        assert!(paste.complete(Ok(Some(payload.into()))).is_err());
        assert_eq!(base, original);
    }
}

#[test]
fn pending_edit_matches_the_entire_editor_baseline_and_rejects_composition() {
    let base = selected_multibyte(false, 16);
    let pending = ClipboardEdit::prepare(&base, ClipboardAction::Paste)
        .unwrap()
        .unwrap();
    assert!(pending.matches(&base));
    assert!(pending.matches(&base.clone()));

    let mut without_selection = base.clone();
    without_selection.clear_selection();
    assert_eq!(without_selection.value(), base.value());
    assert_eq!(without_selection.cursor(), base.cursor());
    assert!(!pending.matches(&without_selection));
    let mut moved = without_selection.clone();
    moved.left();
    assert_eq!(moved.value(), base.value());
    assert!(!pending.matches(&moved));
    assert!(!pending.matches(&selected_multibyte(true, 16)));
    // A matching value, caret and selected range with a different field budget
    // cannot authorize a result prepared for the previous editor instance.
    assert!(!pending.matches(&selected_multibyte(false, 32)));

    for native_range in [None, Some((0, 0)), Some((0, 3))] {
        let composing = base.preedit("音", native_range).unwrap();
        let original = composing.clone();
        assert!(!pending.matches(&composing));
        for action in [
            ClipboardAction::Copy,
            ClipboardAction::Cut,
            ClipboardAction::Paste,
        ] {
            assert!(ClipboardEdit::prepare(&composing, action).is_err());
            assert_eq!(composing, original);
        }
    }
}
