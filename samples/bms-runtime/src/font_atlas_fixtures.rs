//! Original minimal TrueType data exercises the real parser and rasterizer.
use crate::font_atlas::FontAtlas;

// Three glyphs: an original triangle, another triangle and an empty space.
// The same triangle is mapped to Latin A and Hangul GA to exercise Unicode
// lookup without distributing any third-party font.
fn font_bytes() -> Vec<u8> {
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
