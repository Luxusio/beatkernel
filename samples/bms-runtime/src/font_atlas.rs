//! Bounded fixed-scale font rasterization outside native and real-time owners.
use crate::texture::RgbaImage;
use ab_glyph::{Font, FontArc, ScaleFont};
use std::collections::BTreeMap;

/// One cached character's actual font metrics and immutable atlas placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Normalized `[u, v, width, height]`, excluding transparent padding.
    /// Whitespace and glyphs without a drawable outline have no region.
    pub uv: Option<[f32; 4]>,
    /// Baseline-relative pixel `[x, y, width, height]`; y increases downward.
    pub bounds: [i32; 4],
    /// Font-provided horizontal advance at this atlas's fixed scale.
    pub advance: f32,
    /// This character resolved to the font's glyph zero, without substitution.
    pub missing: bool,
}

#[derive(Clone, Copy, Default)]
struct Shelf {
    x: u32,
    y: u32,
    height: u32,
}

/// One supplied font, one fixed scale, and one bounded atlas with stable UVs.
/// Parsing, preparation and image upload belong outside audio callbacks.
/// This component provides individual glyphs, without shaping or fallback fonts.
pub struct FontAtlas {
    font: FontArc,
    pixels: f32,
    max_glyphs: usize,
    glyphs: BTreeMap<char, Glyph>,
    image: RgbaImage,
    shelf: Shelf,
}
impl FontAtlas {
    /// Admits up to 32 MiB font bytes, scale 1..128, atlas extents 1..2048,
    /// and 1..4096 cached characters. Atlas pixels initially remain transparent.
    pub fn new(
        bytes: Vec<u8>,
        pixels: f32,
        width: u32,
        height: u32,
        max_glyphs: usize,
    ) -> Result<Self, String> {
        if bytes.len() > 32 * 1024 * 1024
            || !pixels.is_finite()
            || !(1.0..=128.0).contains(&pixels)
            || !(1..=2048).contains(&width)
            || !(1..=2048).contains(&height)
            || !(1..=4096).contains(&max_glyphs)
        {
            return Err(
                "font atlas configuration exceeds byte, scale, extent or glyph limits".into(),
            );
        }
        let font = FontArc::try_from_vec(bytes).map_err(|_| "invalid supplied font bytes")?;
        let height_unscaled = font.height_unscaled();
        if !height_unscaled.is_finite() || height_unscaled <= 0.0 {
            return Err("font has invalid vertical scale metrics".into());
        }
        let byte_len = usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| "font atlas pixel byte count overflow")?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(byte_len)
            .map_err(|_| "font atlas allocation failed")?;
        rgba.resize(byte_len, 0);
        Ok(Self {
            font,
            pixels,
            max_glyphs,
            glyphs: BTreeMap::new(),
            image: RgbaImage::new(width, height, rgba)?,
            shelf: Shelf::default(),
        })
    }
    pub fn get(&self, character: char) -> Option<Glyph> {
        self.glyphs.get(&character).copied()
    }
    /// Includes characters with advance but no atlas region, such as spaces.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }
    pub fn image(&self) -> &RgbaImage {
        &self.image
    }
    /// Returns a cached glyph or prepares it once. Any returned error preserves
    /// existing cache entries, pixels and shelf placement. Controls are rejected.
    pub fn prepare(&mut self, character: char) -> Result<Glyph, String> {
        if character.is_control() {
            return Err("control characters are not drawable font glyphs".into());
        }
        if let Some(glyph) = self.get(character) {
            return Ok(glyph);
        }
        if self.glyphs.len() >= self.max_glyphs {
            return Err("font atlas cached character limit reached".into());
        }
        let scaled = self.font.as_scaled(self.pixels);
        let id = scaled.glyph_id(character);
        let advance = scaled.h_advance(id);
        if !advance.is_finite() {
            return Err("font glyph advance is not finite".into());
        }
        let mut glyph = Glyph {
            uv: None,
            bounds: [0; 4],
            advance,
            missing: id.0 == 0,
        };
        if character.is_whitespace() {
            self.glyphs.insert(character, glyph);
            return Ok(glyph);
        }
        let Some(outline) = self.font.outline_glyph(id.with_scale(self.pixels)) else {
            self.glyphs.insert(character, glyph);
            return Ok(glyph);
        };
        let bounds = outline.px_bounds();
        let values = [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y];
        if values.iter().any(|&value| {
            !value.is_finite()
                || value.fract() != 0.0
                || f64::from(value) < f64::from(i32::MIN)
                || f64::from(value) > f64::from(i32::MAX)
        }) {
            return Err("font glyph pixel bounds are not representable integers".into());
        }
        let x = values[0] as i32;
        let y = values[1] as i32;
        let width = (values[2] as i32)
            .checked_sub(x)
            .filter(|&width| width >= 0)
            .ok_or("font glyph width is invalid")?;
        let height = (values[3] as i32)
            .checked_sub(y)
            .filter(|&height| height >= 0)
            .ok_or("font glyph height is invalid")?;
        glyph.bounds = [x, y, width, height];
        if width == 0 || height == 0 {
            self.glyphs.insert(character, glyph);
            return Ok(glyph);
        }
        let width = width as u32;
        let height = height as u32;
        let (atlas_x, atlas_y, next_shelf) = self.placement(width, height)?;
        let count = usize::try_from(u64::from(width) * u64::from(height))
            .map_err(|_| "font glyph raster byte count overflow")?;
        let mut coverage = Vec::new();
        coverage
            .try_reserve_exact(count)
            .map_err(|_| "font glyph raster allocation failed")?;
        coverage.resize(count, 0u8);
        let mut invalid = false;
        outline.draw(|x, y, alpha| {
            if x >= width || y >= height || !alpha.is_finite() {
                invalid = true;
                return;
            }
            coverage[(u64::from(y) * u64::from(width) + u64::from(x)) as usize] =
                (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        });
        if invalid {
            return Err("font glyph raster has invalid coordinates or coverage".into());
        }
        glyph.uv = Some([
            atlas_x as f32 / self.image.width() as f32,
            atlas_y as f32 / self.image.height() as f32,
            width as f32 / self.image.width() as f32,
            height as f32 / self.image.height() as f32,
        ]);
        let stride = self.image.width() as usize * 4;
        let pixels = self.image.pixels_mut();
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = (atlas_y as usize + y) * stride + (atlas_x as usize + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[
                    255,
                    255,
                    255,
                    coverage[y * width as usize + x],
                ]);
            }
        }
        self.shelf = next_shelf;
        self.glyphs.insert(character, glyph);
        Ok(glyph)
    }
    /// Computes a candidate with one transparent pixel on every side, without
    /// changing shelf state. No existing glyph is moved or evicted.
    fn placement(&self, width: u32, height: u32) -> Result<(u32, u32, Shelf), String> {
        let cell_width = width
            .checked_add(2)
            .ok_or("font glyph padded width overflow")?;
        let cell_height = height
            .checked_add(2)
            .ok_or("font glyph padded height overflow")?;
        if cell_width > self.image.width() || cell_height > self.image.height() {
            return Err("font glyph does not fit the padded atlas extent".into());
        }
        let mut shelf = self.shelf;
        if shelf.x + cell_width > self.image.width() {
            shelf.x = 0;
            shelf.y += shelf.height;
            shelf.height = 0;
        }
        if shelf.y + cell_height > self.image.height() {
            return Err("font atlas has no room for this glyph".into());
        }
        let origin = (shelf.x + 1, shelf.y + 1);
        shelf.x += cell_width;
        shelf.height = shelf.height.max(cell_height);
        Ok((origin.0, origin.1, shelf))
    }
}
