//! Bounded RGBA8 resource data. GPU ownership belongs to the renderer.
use std::sync::atomic::{AtomicU64, Ordering};
pub const MAX_TEXTURES: usize = 64;
pub const MAX_TEXTURE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureId(u64);
impl TextureId {
    pub const WHITE: Self = Self(0);
    pub const FONT: Self = Self(1);
    pub(crate) fn allocate() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(2);
        NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map(Self)
        .map_err(|_| "texture ID space exhausted".into())
    }
}

pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl RgbaImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, String> {
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|value| value.checked_mul(4))
            .ok_or("RGBA8 size overflow")?;
        if width == 0 || height == 0 || bytes > MAX_TEXTURE_BYTES {
            return Err("RGBA8 image extent/budget invalid".into());
        }
        if usize::try_from(bytes).ok() != Some(pixels.len()) {
            return Err("RGBA8 data length does not match extent".into());
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn byte_len(&self) -> u64 {
        self.pixels.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_images_reject_zero_mismatch_and_overflow_before_upload() {
        assert!(RgbaImage::new(0, 1, vec![]).is_err());
        assert!(RgbaImage::new(1, 1, vec![255; 3]).is_err());
        assert!(RgbaImage::new(u32::MAX, u32::MAX, vec![]).is_err());
        assert!(RgbaImage::new(1, 1, vec![255; 4]).is_ok());
    }
    #[test]
    fn custom_ids_are_distinct_from_builtins_and_each_other() {
        let a = TextureId::allocate().unwrap();
        let b = TextureId::allocate().unwrap();
        assert_ne!(a, b);
        assert_ne!(a, TextureId::WHITE);
        assert_ne!(a, TextureId::FONT);
    }
}
