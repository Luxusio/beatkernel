//! Original minimal TrueType data exercises the real parser and rasterizer.
use crate::font_atlas::FontAtlas;

// Three glyphs: an original triangle, another triangle and an empty space.
// The same triangle is mapped to Latin A and Hangul GA to exercise Unicode
// lookup without distributing any third-party font.
pub(crate) fn font_bytes() -> Vec<u8> {
    fn u16_at(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    fn checksum(bytes: &[u8]) -> u32 {
        bytes.chunks(4).fold(0u32, |sum, part| {
            let mut word = [0; 4];
            word[..part.len()].copy_from_slice(part);
            sum.wrapping_add(u32::from_be_bytes(word))
        })
    }
    let mut head = vec![0; 54];
    u32_at(&mut head, 0, 0x0001_0000);
    u32_at(&mut head, 4, 0x0001_0000);
    u32_at(&mut head, 12, 0x5f0f_3cf5);
    u16_at(&mut head, 18, 1000);
    u16_at(&mut head, 40, 500);
    u16_at(&mut head, 42, 700);
    u16_at(&mut head, 46, 8);
    u16_at(&mut head, 48, 2);
    u16_at(&mut head, 50, 1); // Long loca offsets.
    let mut hhea = vec![0; 36];
    u32_at(&mut hhea, 0, 0x0001_0000);
    u16_at(&mut hhea, 4, 800);
    u16_at(&mut hhea, 6, (-200i16) as u16);
    u16_at(&mut hhea, 10, 600);
    u16_at(&mut hhea, 16, 500);
    u16_at(&mut hhea, 18, 1);
    u16_at(&mut hhea, 34, 3);
    let mut maxp = vec![0; 32];
    u32_at(&mut maxp, 0, 0x0001_0000);
    u16_at(&mut maxp, 4, 3);
    u16_at(&mut maxp, 6, 3);
    u16_at(&mut maxp, 8, 1);
    u16_at(&mut maxp, 14, 1);
    let hmtx = [600u16, 0, 600, 0, 600, 0]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    let mut triangle = Vec::new();
    for value in [1i16, 0, 0, 500, 700, 2, 0] {
        triangle.extend_from_slice(&value.to_be_bytes());
    }
    triangle.extend_from_slice(&[1, 1, 1]); // On-curve, signed long coordinates.
    for value in [0i16, 500, -250, 0, 0, 700] {
        triangle.extend_from_slice(&value.to_be_bytes());
    }
    while triangle.len() % 4 != 0 {
        triangle.push(0);
    }
    let glyf = [triangle.clone(), triangle.clone()].concat();
    let loca = [
        0u32,
        triangle.len() as u32,
        glyf.len() as u32,
        glyf.len() as u32,
    ]
    .into_iter()
    .flat_map(u32::to_be_bytes)
    .collect::<Vec<_>>();
    let mut cmap = vec![0; 12];
    u16_at(&mut cmap, 2, 1);
    u16_at(&mut cmap, 4, 3);
    u16_at(&mut cmap, 6, 10);
    u32_at(&mut cmap, 8, 12);
    let mut format12 = vec![0; 16];
    u16_at(&mut format12, 0, 12);
    u32_at(&mut format12, 4, 52);
    u32_at(&mut format12, 12, 3);
    for (character, glyph) in [(' ', 2u32), ('A', 1), ('가', 1)] {
        for value in [character as u32, character as u32, glyph] {
            format12.extend_from_slice(&value.to_be_bytes());
        }
    }
    cmap.extend_from_slice(&format12);
    let mut tables = vec![
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ];
    tables.sort_by_key(|(tag, _)| *tag);
    let mut font = vec![0; 12 + tables.len() * 16];
    u32_at(&mut font, 0, 0x0001_0000);
    u16_at(&mut font, 4, tables.len() as u16);
    u16_at(&mut font, 6, 64);
    u16_at(&mut font, 8, 2);
    u16_at(&mut font, 10, 48);
    let mut head_offset = 0;
    for (index, (tag, data)) in tables.into_iter().enumerate() {
        let row = 12 + index * 16;
        let offset = font.len();
        font[row..row + 4].copy_from_slice(&tag);
        u32_at(&mut font, row + 4, checksum(&data));
        u32_at(&mut font, row + 8, offset as u32);
        u32_at(&mut font, row + 12, data.len() as u32);
        if tag == *b"head" {
            head_offset = offset;
        }
        font.extend_from_slice(&data);
        while font.len() % 4 != 0 {
            font.push(0);
        }
    }
    let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&font));
    u32_at(&mut font, head_offset + 8, adjustment);
    font
}

#[test]
fn actual_font_unicode_raster_cache_padding_missing_and_space() {
    let mut atlas = FontAtlas::new(font_bytes(), 32.0, 128, 128, 16).unwrap();
    let latin = atlas.prepare('A').unwrap();
    assert!(!latin.missing);
    assert!(latin.advance > 0.0 && latin.bounds[2] > 0 && latin.bounds[3] > 0);
    assert!(latin.uv.is_some());
    let pixels = atlas.image().pixels().to_vec();
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    assert!(
        pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .all(|pixel| pixel[..3] == [255; 3])
    );
    assert!(pixels[..128 * 4].iter().all(|byte| *byte == 0));
    assert_eq!(atlas.prepare('A').unwrap(), latin);
    assert_eq!(atlas.get('A'), Some(latin));
    assert_eq!(atlas.len(), 1);
    assert_eq!(atlas.image().pixels(), pixels);
    let hangul = atlas.prepare('가').unwrap();
    assert!(!hangul.missing);
    assert_eq!(hangul.advance, latin.advance);
    assert_ne!(hangul.uv, latin.uv);
    let space = atlas.prepare(' ').unwrap();
    assert_eq!(space.uv, None);
    assert!(space.advance > 0.0);
    assert!(!space.missing);
    assert!(atlas.prepare('別').unwrap().missing);
    assert_eq!(atlas.get('A'), Some(latin));
}

#[test]
fn prepared_font_uvs_compose_through_the_existing_clipped_sprite_path() {
    use crate::{scene::Scene, texture::TextureId};
    let mut atlas = FontAtlas::new(font_bytes(), 32.0, 128, 128, 16).unwrap();
    let texture = TextureId::allocate().unwrap();
    let mut scene = Scene::new(64, 64);
    for character in ['A', '가'] {
        let glyph = atlas.prepare(character).unwrap();
        let [x, y, width, height] = glyph.bounds;
        scene
            .sprite(
                texture,
                [
                    i64::from(x) - 2,
                    32 + i64::from(y),
                    i64::from(width),
                    i64::from(height),
                ],
                glyph.uv.unwrap(),
                0xabcdef,
            )
            .unwrap();
    }
    assert_eq!(scene.rectangles().len(), 2);
    assert_eq!(scene.batches().len(), 1);
    assert_eq!(scene.batches()[0].texture, texture);
    for rectangle in scene.rectangles() {
        assert_eq!(rectangle.bounds[0], 0.0);
        assert!(rectangle.uv[0] > 0.0 && rectangle.uv[2] > 0.0);
        assert!(rectangle.uv[0] + rectangle.uv[2] <= 1.0);
        assert!(rectangle.uv[1] + rectangle.uv[3] <= 1.0);
    }
}

#[test]
fn actual_atlas_extent_and_cache_capacity_failures_preserve_admitted_glyphs() {
    for (width, height, max_glyphs) in [(24, 32, 16), (128, 128, 1)] {
        let mut atlas = FontAtlas::new(font_bytes(), 32.0, width, height, max_glyphs).unwrap();
        let first = atlas.prepare('A').unwrap();
        let before = atlas.image().pixels().to_vec();
        for character in ['가', '\n', '\t', '\0'] {
            assert!(atlas.prepare(character).is_err());
            assert_eq!(atlas.len(), 1);
            assert_eq!(atlas.get('A'), Some(first));
            assert_eq!(atlas.get(character), None);
            assert_eq!(atlas.image().pixels(), before);
        }
        assert_eq!(atlas.prepare('A').unwrap(), first);
    }
}

#[test]
fn font_and_config_reject_invalid_or_unbounded_preparation() {
    for bytes in [vec![], vec![0; 64]] {
        assert!(FontAtlas::new(bytes, 32.0, 128, 128, 16).is_err());
    }
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 129.0] {
        assert!(FontAtlas::new(font_bytes(), scale, 128, 128, 16).is_err());
    }
    for (width, height, count) in [
        (0, 128, 16),
        (128, 0, 16),
        (2049, 128, 16),
        (128, 2049, 16),
        (128, 128, 0),
        (128, 128, 4097),
    ] {
        assert!(FontAtlas::new(font_bytes(), 32.0, width, height, count).is_err());
    }
}

#[test]
fn cached_font_text_uses_real_advances_and_preflights_uncached_visible_text() {
    use crate::{font_text::FontText, scene::Scene, texture::TextureId};
    use std::sync::Arc;
    let mut atlas = FontAtlas::new(font_bytes(), 14.0, 128, 128, 16).unwrap();
    let first = atlas.prepare('A').unwrap();
    let space = atlas.prepare(' ').unwrap();
    atlas.prepare('가').unwrap();
    let atlas = Arc::new(atlas);
    let texture = TextureId::allocate().unwrap();
    let font = FontText::new(Arc::clone(&atlas), texture).unwrap();
    let mut scene = Scene::new(128, 64);
    font.draw(&mut scene, 4, 2, "A 가", 0xabcdef).unwrap();
    assert_eq!(scene.rectangles().len(), 2);
    assert_eq!(scene.batches()[0].texture, texture);
    assert_eq!(
        scene.rectangles()[1].bounds[0],
        (4.0 + f64::from(first.advance) + f64::from(space.advance)).round() as f32
            + atlas.get('가').unwrap().bounds[0] as f32
    );
    assert_eq!(
        scene.rectangles()[0].bounds[1],
        2.0 + atlas.ascent().ceil() + first.bounds[1] as f32
    );
    let before: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.uv, r.color))
        .collect();
    assert!(font.draw(&mut scene, 4, 2, "A未", 0xffffff).is_err());
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|r| (r.bounds, r.uv, r.color))
            .collect::<Vec<_>>(),
        before
    );
    assert!(font.draw(&mut scene, -1, 2, "A", 0xffffff).is_err());
    font.draw(&mut scene, 128, 2, "未", 0xffffff).unwrap();
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|r| (r.bounds, r.uv, r.color))
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(atlas.len(), 3);
}

#[test]
fn cached_font_text_does_not_require_glyphs_beyond_clip_or_scalar_budget() {
    use crate::{
        font_text::{FontText, MAX_TEXT_GLYPHS},
        scene::Scene,
        texture::TextureId,
    };
    use std::sync::Arc;
    let mut atlas = FontAtlas::new(font_bytes(), 14.0, 128, 128, 16).unwrap();
    atlas.prepare('A').unwrap();
    let font = FontText::new(Arc::new(atlas), TextureId::allocate().unwrap()).unwrap();
    let mut narrow = Scene::new(1, 64);
    font.draw(&mut narrow, 0, 2, "A未", 0xffffff).unwrap();
    assert_eq!(narrow.rectangles().len(), 1);
    assert_eq!(narrow.rectangles()[0].bounds[2], 1.0);
    let mut wide = Scene::new(16384, 64);
    let value = "A".repeat(MAX_TEXT_GLYPHS) + "未";
    font.draw(&mut wide, 0, 2, &value, 0xffffff).unwrap();
    assert_eq!(wide.rectangles().len(), MAX_TEXT_GLYPHS);
    assert!(wide.status().is_ok());
}

#[test]
fn retained_selection_uses_prepared_title_texture_and_recovers_with_new_identity() {
    use crate::{
        font_text::FontText,
        scene::Scene,
        screen_lifecycle::ScreenInstanceId,
        texture::TextureId,
        ui::selection::{SelectionFrame, SelectionItem, SelectionView},
    };
    use std::sync::Arc;
    let mut atlas = FontAtlas::new(font_bytes(), 14.0, 128, 128, 16).unwrap();
    atlas.prepare('A').unwrap();
    atlas.prepare('가').unwrap();
    let atlas = Arc::new(atlas);
    let items: Arc<[SelectionItem]> = vec![
        SelectionItem {
            title: "A".into(),
            artist: String::new(),
        },
        SelectionItem {
            title: "가".into(),
            artist: String::new(),
        },
    ]
    .into();
    let old_texture = TextureId::allocate().unwrap();
    let new_texture = TextureId::allocate().unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    for texture in [old_texture, new_texture] {
        let view = SelectionView::new_with_font(
            ScreenInstanceId(7),
            Arc::clone(&items),
            Arc::from([]),
            960,
            720,
            Some(FontText::new(Arc::clone(&atlas), texture).unwrap()),
        )
        .unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            scene
                .batches()
                .iter()
                .filter(|b| b.texture == texture)
                .map(|b| b.count)
                .sum::<u32>(),
            2
        );
        view.update(SelectionFrame {
            selected: 0,
            hovered: None,
            armed: None,
            error: None,
            backend_pending: false,
        });
        assert!(!view.dirty());
        view.set_projection(Arc::from([1]), Some(0)).unwrap();
        assert!(view.dirty());
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            scene
                .batches()
                .iter()
                .filter(|b| b.texture == texture)
                .map(|b| b.count)
                .sum::<u32>(),
            1
        );
        assert!(hits.iter().any(|(id, _)| id.0 == 101));
        assert!(!hits.iter().any(|(id, _)| id.0 == 100));
        if texture == new_texture {
            assert!(scene.batches().iter().all(|b| b.texture != old_texture));
        }
    }
    let uncached = Arc::new(FontAtlas::new(font_bytes(), 14.0, 128, 128, 16).unwrap());
    assert!(
        SelectionView::new_with_font(
            ScreenInstanceId(8),
            items,
            Arc::from([]),
            960,
            720,
            Some(FontText::new(uncached, new_texture).unwrap())
        )
        .is_err()
    );
}

#[test]
fn retained_paint_rejection_prevents_snapshot_until_scene_clear() {
    use crate::scene::Scene;
    let mut scene = Scene::new(64, 64);
    scene.reject("uncached title".into());
    scene.reject("later failure".into());
    assert_eq!(scene.status().unwrap_err(), "uncached title");
    scene.clear();
    assert!(scene.status().is_ok());
    assert!(scene.geometry_snapshot().is_ok());
    let mut rejected = Scene::new(64, 64);
    rejected.reject("uncached title".into());
    assert!(rejected.geometry_snapshot().is_err());
}
