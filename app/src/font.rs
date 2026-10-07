//! Original bitmap font primitives, independent of window and GPU ownership.
use crate::texture::RgbaImage;
pub fn glyph_uv(character: char) -> [f32; 4] {
    let code = if character.is_ascii() {
        character.to_ascii_uppercase() as u32
    } else {
        b'?' as u32
    };
    [
        (code % 16 * 8) as f32 / 128.0,
        (code / 16 * 8) as f32 / 64.0,
        5.0 / 128.0,
        7.0 / 64.0,
    ]
}
pub fn atlas() -> Result<RgbaImage, String> {
    let mut pixels = vec![0; 128 * 64 * 4];
    for code in 0u8..128 {
        for (row, bits) in glyph((code as char).to_ascii_uppercase())
            .into_iter()
            .enumerate()
        {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let x = usize::from(code % 16) * 8 + column;
                    let y = usize::from(code / 16) * 8 + row;
                    pixels[(y * 128 + x) * 4..(y * 128 + x) * 4 + 4].fill(255);
                }
            }
        }
    }
    RgbaImage::new(128, 64, pixels)
}
// Original five-column bitmap glyphs; non-ASCII metadata remains in the native title.
pub(crate) fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atlas_has_coverage_padding_and_utf8_fallback() {
        let atlas = atlas().unwrap();
        let x = usize::from(b'A' % 16) * 8;
        let y = usize::from(b'A' / 16) * 8;
        assert_eq!(
            &atlas.pixels()[((y * 128 + x + 1) * 4)..((y * 128 + x + 1) * 4 + 4)],
            &[255; 4]
        );
        assert_eq!(
            &atlas.pixels()[((y * 128 + x + 7) * 4)..((y * 128 + x + 7) * 4 + 4)],
            &[0; 4]
        );
        assert_eq!(glyph_uv('a'), glyph_uv('A'));
        assert_eq!(glyph_uv('곡'), glyph_uv('?'));
    }
}
