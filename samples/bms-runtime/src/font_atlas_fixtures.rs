//! Original minimal TrueType data exercises the real parser and rasterizer.
use crate::font_atlas::FontAtlas;

use crate::font_fixture::{font_bytes, font_bytes_with_space};

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
    assert_eq!(hangul, latin);
    assert_eq!(atlas.image().pixels(), pixels);
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
        // Glyph zero is distinct from A even though this font gives both the
        // same triangle. It still needs a new placement in a full tiny atlas.
        for character in ['別', '\n', '\t', '\0'] {
            assert!(atlas.prepare(character).is_err());
            assert_eq!(atlas.len(), 1);
            assert_eq!(atlas.get('A'), Some(first));
            assert_eq!(atlas.get(character), None);
            assert_eq!(atlas.image().pixels(), before);
        }
        if max_glyphs == 1 {
            assert!(atlas.prepare('가').is_err());
            assert_eq!(atlas.get('가'), None);
            assert_eq!(atlas.len(), 1);
        } else {
            assert_eq!(atlas.prepare('가').unwrap(), first);
            assert_eq!(atlas.len(), 2);
        }
        assert_eq!(atlas.image().pixels(), before);
        assert_eq!(atlas.prepare('A').unwrap(), first);
    }
}

#[test]
fn aliases_share_exact_fit_atlas_and_missing_zero_placements() {
    let mut probe = FontAtlas::new(font_bytes(), 32.0, 128, 128, 16).unwrap();
    let bounds = probe.prepare('A').unwrap().bounds;
    let mut exact = FontAtlas::new(
        font_bytes(),
        32.0,
        bounds[2] as u32 + 2,
        bounds[3] as u32 + 2,
        4,
    )
    .unwrap();
    let first = exact.prepare('A').unwrap();
    let pixels = exact.image().pixels().to_vec();
    assert_eq!(exact.prepare('가').unwrap(), first);
    assert_eq!(exact.image().pixels(), pixels);
    assert_eq!(exact.len(), 2);
    assert!(exact.prepare('別').is_err());
    assert_eq!(exact.len(), 2);
    assert_eq!(exact.get('別'), None);
    assert_eq!(exact.image().pixels(), pixels);
    assert_eq!(exact.prepare('가').unwrap(), first);

    let mut missing = FontAtlas::new(
        font_bytes(),
        32.0,
        bounds[2] as u32 + 2,
        bounds[3] as u32 + 2,
        3,
    )
    .unwrap();
    let zero = missing.prepare('別').unwrap();
    assert!(zero.missing && zero.uv.is_some());
    let pixels = missing.image().pixels().to_vec();
    assert_eq!(missing.prepare('未').unwrap(), zero);
    assert_eq!(missing.image().pixels(), pixels);
    assert_eq!(missing.len(), 2);
    assert!(missing.prepare('A').is_err());
    assert_eq!(missing.len(), 2);
    assert_eq!(missing.get('A'), None);
    assert_eq!(missing.image().pixels(), pixels);
    // The failed distinct glyph must not occupy a cache entry or consume the
    // remaining character slot: another glyph-zero alias still succeeds.
    assert_eq!(missing.prepare('知').unwrap(), zero);
    assert_eq!(missing.len(), 3);
    assert!(missing.prepare('不').is_err());
    assert_eq!(missing.get('不'), None);
    assert_eq!(missing.image().pixels(), pixels);
}

#[test]
fn whitespace_suppresses_visible_and_missing_glyphs_before_and_after_real_aliases() {
    for (space_glyph, character) in [(1, 'A'), (0, '別')] {
        for whitespace_first in [true, false] {
            let mut atlas =
                FontAtlas::new(font_bytes_with_space(space_glyph), 32.0, 24, 32, 4).unwrap();
            if whitespace_first {
                let space = atlas.prepare(' ').unwrap();
                assert_eq!(space.uv, None);
                assert_eq!(space.bounds, [0; 4]);
                assert_eq!(space.missing, space_glyph == 0);
                assert!(atlas.image().pixels().iter().all(|&byte| byte == 0));
            }
            let visible = atlas.prepare(character).unwrap();
            assert!(visible.uv.is_some());
            assert_eq!(visible.missing, space_glyph == 0);
            let pixels = atlas.image().pixels().to_vec();
            let space = atlas.prepare(' ').unwrap();
            assert_eq!(space.uv, None);
            assert_eq!(space.bounds, [0; 4]);
            assert_eq!(space.advance, visible.advance);
            assert_eq!(atlas.prepare(character).unwrap(), visible);
            assert_eq!(atlas.image().pixels(), pixels);
            assert_eq!(atlas.len(), 2);
        }
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
