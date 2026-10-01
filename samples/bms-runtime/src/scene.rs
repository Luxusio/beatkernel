//! Bounded, ordered geometry shared by native and browser renderers.

pub const MAX_RECTANGLES: usize = 65_536;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Rectangle {
    pub bounds: [f32; 4],
    pub color: [f32; 4],
}

pub struct Scene {
    width: u32,
    height: u32,
    rectangles: Vec<Rectangle>,
    overflow: bool,
}

impl Scene {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rectangles: Vec::with_capacity(MAX_RECTANGLES),
            overflow: false,
        }
    }

    pub fn clear(&mut self) {
        self.rectangles.clear();
        self.overflow = false;
    }

    /// Clips to the logical viewport. Capacity exhaustion is sticky until clear,
    /// and must be checked by the renderer before submitting this scene.
    pub fn rect(&mut self, x: i64, y: i64, width: i64, height: i64, color: u32) {
        if width <= 0 || height <= 0 {
            return;
        }
        let left = x.clamp(0, i64::from(self.width));
        let top = y.clamp(0, i64::from(self.height));
        let right = x.saturating_add(width).clamp(0, i64::from(self.width));
        let bottom = y.saturating_add(height).clamp(0, i64::from(self.height));
        if right <= left || bottom <= top {
            return;
        }
        if self.rectangles.len() == MAX_RECTANGLES {
            self.overflow = true;
            return;
        }
        self.rectangles.push(Rectangle {
            bounds: [
                left as f32,
                top as f32,
                (right - left) as f32,
                (bottom - top) as f32,
            ],
            color: [
                ((color >> 16) & 255) as f32 / 255.0,
                ((color >> 8) & 255) as f32 / 255.0,
                (color & 255) as f32 / 255.0,
                1.0,
            ],
        });
    }

    pub fn status(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            Err("scene logical extent must be nonzero".into())
        } else if self.overflow {
            Err(format!("scene exceeds {MAX_RECTANGLES} rectangles"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn dimensions(&self) -> [f32; 2] {
        [self.width as f32, self.height as f32]
    }

    pub(crate) fn rectangles(&self) -> &[Rectangle] {
        &self.rectangles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_signed_extremes_and_preserves_painter_order() {
        let mut scene = Scene::new(960, 720);
        scene.rect(-5, -7, 10, 12, 0xff0000);
        scene.rect(959, 719, i64::MAX, i64::MAX, 0x00ff00);
        scene.rect(i64::MIN, 0, i64::MAX, 10, 0);
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 5.0, 5.0]);
        assert_eq!(scene.rectangles()[1].bounds, [959.0, 719.0, 1.0, 1.0]);
        assert_eq!(scene.rectangles()[0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn capacity_failure_is_reported_until_clear() {
        let mut scene = Scene::new(1, 1);
        for _ in 0..=MAX_RECTANGLES {
            scene.rect(0, 0, 1, 1, 0);
        }
        assert_eq!(scene.rectangles().len(), MAX_RECTANGLES);
        assert!(scene.status().is_err());
        scene.clear();
        assert!(scene.status().is_ok());
        assert!(scene.rectangles().is_empty());
    }
}
