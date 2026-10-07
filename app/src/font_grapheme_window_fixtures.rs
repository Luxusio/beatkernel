//! Deferred actual prepared font metrics; no graphics device or native IME.
use crate::{
    font_atlas::FontAtlas, font_fixture::font_bytes, font_text::FontText, texture::TextureId,
    ui::text_input::LineEditor,
};
use std::sync::Arc;

fn font(prepared: &str, advance_units: u16) -> (FontText, Arc<FontAtlas>) {
    let mut bytes = font_bytes();
    let table = |tag: &[u8; 4]| {
        (0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize)
            .map(|index| 12 + index * 16)
            .find(|&row| &bytes[row..row + 4] == tag)
            .unwrap()
    };
    let hmtx_row = table(b"hmtx");
    let head_row = table(b"head");
    let hmtx = u32::from_be_bytes(bytes[hmtx_row + 8..hmtx_row + 12].try_into().unwrap()) as usize;
    let head = u32::from_be_bytes(bytes[head_row + 8..head_row + 12].try_into().unwrap()) as usize;
    // Vary only real synthetic font metrics, never a projected-window oracle.
    for index in 0..3 {
        bytes[hmtx + index * 4..hmtx + index * 4 + 2].copy_from_slice(&advance_units.to_be_bytes());
    }
    fn checksum(bytes: &[u8]) -> u32 {
        bytes.chunks(4).fold(0u32, |sum, part| {
            let mut word = [0; 4];
            word[..part.len()].copy_from_slice(part);
            sum.wrapping_add(u32::from_be_bytes(word))
        })
    }
    let sum = checksum(&bytes[hmtx..hmtx + 12]);
    bytes[hmtx_row + 4..hmtx_row + 8].copy_from_slice(&sum.to_be_bytes());
    bytes[head + 8..head + 12].fill(0);
    let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&bytes));
    bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    let mut atlas = FontAtlas::new(bytes, 10.0, 128, 128, 64).unwrap();
    for character in prepared.chars() {
        atlas.prepare(character).unwrap();
    }
    let atlas = Arc::new(atlas);
    (
        FontText::new(atlas.clone(), TextureId::allocate().unwrap()).unwrap(),
        atlas,
    )
}

#[test]
fn prepared_font_windows_borrow_whole_clusters_and_crop_pixels_without_rewriting_native_decorations()
 {
    let (font, atlas) = font("AZe\u{301}x👩\u{200d}💻🇦🇧🇨🇩🇪", 600);
    assert_eq!(atlas.get('A').unwrap().advance, 6.0);
    assert!(
        atlas.get('\u{301}').unwrap().missing,
        "cached tofu is not a fallback shaping engine"
    );
    let pixels = atlas.image().pixels().to_vec();
    let glyph_count = atlas.len();
    let mut base = LineEditor::new("AZ", 32).unwrap();
    base.home();
    base.right();
    let preview = base.preedit("e\u{301}x", Some((1, 3))).unwrap();
    let full = font.field_line(&preview, 18).unwrap();
    assert_eq!(
        (full.value, full.caret_x, full.composition, full.selection),
        ("e\u{301}x", 6, Some((0, 18)), Some((6, 12)))
    );
    assert_eq!(full.value.as_ptr(), preview.value()[1..].as_ptr());
    let narrow = font.field_line(&preview, 9).unwrap();
    assert_eq!(
        (
            narrow.value,
            narrow.caret_x,
            narrow.composition,
            narrow.selection
        ),
        ("e\u{301}", 6, Some((0, 9)), Some((6, 9)))
    );
    let tiny = font.field_line(&preview, 5).unwrap();
    assert_eq!(
        (tiny.value, tiny.caret_x, tiny.composition, tiny.selection),
        ("e\u{301}", 5, Some((0, 5)), None)
    );
    let hidden = base.preedit("e\u{301}x", None).unwrap();
    assert!(!font.field_line(&hidden, 18).unwrap().caret_visible);
    let zero = font.field_line(&hidden, 0).unwrap();
    assert_eq!(
        (zero.value, zero.caret_x, zero.composition, zero.selection),
        ("", 0, None, None)
    );
    assert_eq!(zero.value.as_ptr(), hidden.value()[5..].as_ptr());
    assert!(!zero.caret_visible);

    let mut joined = LineEditor::new("👩💻", 32).unwrap();
    joined.home();
    joined.right();
    let preview = joined.preedit("\u{200d}", Some((0, 3))).unwrap();
    let whole = font.field_line(&preview, 18).unwrap();
    assert_eq!(
        (
            whole.value,
            whole.caret_x,
            whole.composition,
            whole.selection
        ),
        ("👩\u{200d}💻", 6, Some((6, 12)), Some((6, 12)))
    );
    assert_eq!(font.field_line(&preview, 1).unwrap().value, "👩\u{200d}💻");
    assert_eq!(font.field_line(&preview, 1).unwrap().caret_x, 1);
    let mut flags = LineEditor::new("🇦🇧🇨🇩🇪", 32).unwrap();
    assert_eq!(font.field_line(&flags, 18).unwrap().value, "🇨🇩🇪");
    flags.home();
    assert_eq!(font.field_line(&flags, 12).unwrap().value, "🇦🇧");
    assert_eq!(atlas.len(), glyph_count);
    assert_eq!(atlas.image().pixels(), pixels);
    assert_eq!((base.value(), base.cursor()), ("AZ", 1));
}

#[test]
fn zero_and_tiny_real_advances_apply_the_scalar_cap_only_between_whole_clusters() {
    for units in [0, 1] {
        let (font, atlas) = font("Ae\u{301}B", units);
        assert_eq!(
            atlas.get('A').unwrap().advance,
            if units == 0 { 0.0 } else { 0.01 }
        );
        let value = format!("{}e\u{301}B", "A".repeat(1023));
        let mut editor = LineEditor::new(&value, 4096).unwrap();
        let end = font.field_line(&editor, 100).unwrap();
        assert_eq!(end.value, format!("{}e\u{301}B", "A".repeat(1021)));
        assert_eq!(end.value.as_ptr(), editor.value()[2..].as_ptr());
        assert_eq!(end.value.chars().count(), 1024);
        assert_eq!(end.caret_x, if units == 0 { 0 } else { 10 });
        editor.home();
        let beginning = font.field_line(&editor, 100).unwrap();
        assert_eq!(beginning.value, "A".repeat(1023));
        assert_eq!(beginning.value.as_ptr(), editor.value().as_ptr());
        assert_eq!(beginning.caret_x, 0);

        let exact = format!("A{}", "\u{301}".repeat(1023));
        let editor = LineEditor::new(&exact, 4096).unwrap();
        assert_eq!(font.field_line(&editor, 100).unwrap().value, exact);
        let oversized = format!("A{}", "\u{301}".repeat(1024));
        let mut editor = LineEditor::new(&oversized, 4096).unwrap();
        let retained = editor.clone();
        assert!(font.field_line(&editor, 100).is_err());
        assert_eq!(editor, retained);
        assert_eq!(font.field_line(&editor, 0).unwrap().value, "");
        editor.home();
        assert!(font.field_line(&editor, 100).is_err());
        let tail = LineEditor::new(&format!("{oversized}B"), 4096).unwrap();
        assert_eq!(font.field_line(&tail, 100).unwrap().value, "B");
    }
}

#[test]
fn font_projection_validates_all_cached_text_before_empty_or_offscreen_windows_and_preserves_scalar_compatibility()
 {
    let (font, atlas) = font("A가 ", 600);
    let before = atlas.image().pixels().to_vec();
    let glyphs = atlas.len();
    let value = format!("{}Ω", "A".repeat(1025));
    let mut editor = LineEditor::new(&value, 4096).unwrap();
    editor.home();
    let retained = editor.clone();
    for width in [0, 1, 12, u32::MAX] {
        assert!(font.field_line(&editor, width).is_err());
    }
    assert_eq!(editor, retained);
    assert_eq!(atlas.get('Ω'), None);
    assert_eq!(atlas.len(), glyphs);
    assert_eq!(atlas.image().pixels(), before);
    let mut ordinary = LineEditor::new("AA", 16).unwrap();
    let end = font.field_line(&ordinary, 1).unwrap();
    assert_eq!((end.value, end.caret_x), ("", 0));
    ordinary.move_home(true);
    let home = font.field_line(&ordinary, 1).unwrap();
    assert_eq!(
        (home.value, home.caret_x, home.selection),
        ("A", 0, Some((0, 1)))
    );
    let editor = LineEditor::new("A가 A", 32).unwrap();
    let all = font.field_line(&editor, 100).unwrap();
    assert_eq!((all.value, all.caret_x), ("A가 A", 24));
    assert_eq!(font.field_line(&editor, 12).unwrap().value, " A");
}
