//! Original synthetic TrueType generator shared by test-only library and binary fixtures.
// Three glyphs: an original triangle, another triangle and an empty space.
// The same triangle is mapped to Latin A and Hangul GA to exercise Unicode
// lookup without distributing any third-party font.
pub(crate) fn font_bytes() -> Vec<u8> {
    font_bytes_with_space(2)
}

pub(crate) fn font_bytes_with_space(space_glyph: u32) -> Vec<u8> {
    font_bytes_with_map(&[(' ', space_glyph), ('A', 1), ('가', 1)])
}

/// The same three glyphs with a caller-chosen character map, sorted by
/// character as format 12 requires. Glyph 1 is a triangle, 2 is empty.
pub(crate) fn font_bytes_with_map(map: &[(char, u32)]) -> Vec<u8> {
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
    u32_at(&mut format12, 4, 16 + 12 * map.len() as u32);
    u32_at(&mut format12, 12, map.len() as u32);
    for &(character, glyph) in map {
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
