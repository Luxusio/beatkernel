//! Bounded, ordered geometry shared by native and browser renderers.

pub const MAX_RECTANGLES: usize = 65_536;
use crate::playfield_gpu::{MAX_PLAYFIELDS, PlayfieldCache, PlayfieldFrame};
use crate::texture::TextureId;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DrawBatch {
    pub texture: TextureId,
    pub first: u32,
    pub count: u32,
    pub playfield: Option<usize>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Rectangle {
    pub bounds: [f32; 4],
    pub color: [f32; 4],
    pub uv: [f32; 4],
}

/// Immutable ordered UI geometry for one retained component. Timed playfields
/// remain separate; these packets never own transport or native resources.
pub struct GeometrySnapshot {
    width: u32,
    height: u32,
    rectangles: Arc<[Rectangle]>,
    batches: Arc<[DrawBatch]>,
}

pub struct Scene {
    width: u32,
    height: u32,
    rectangles: Vec<Rectangle>,
    batches: Vec<DrawBatch>,
    overflow: bool,
    playfields: Vec<PlayfieldFrame>,
    playfield_caches: Vec<PlayfieldCache>,
    geometry_identity: Arc<()>,
    geometry_epoch: u64,
}

impl Scene {
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_capacity(width, height, MAX_RECTANGLES)
    }

    /// Small retained components need only their own initial allocation. The
    /// same hard geometry limit applies regardless of the initial capacity.
    pub fn with_capacity(width: u32, height: u32, capacity: usize) -> Self {
        Self {
            width,
            height,
            rectangles: Vec::with_capacity(capacity.min(MAX_RECTANGLES)),
            batches: Vec::with_capacity(capacity.min(MAX_RECTANGLES)),
            overflow: false,
            playfields: Vec::with_capacity(MAX_PLAYFIELDS),
            playfield_caches: (0..MAX_PLAYFIELDS)
                .map(|_| PlayfieldCache::default())
                .collect(),
            geometry_identity: Arc::new(()),
            geometry_epoch: 0,
        }
    }

    pub fn clear(&mut self) {
        self.geometry_changed();
        self.rectangles.clear();
        self.batches.clear();
        self.overflow = false;
        self.playfields.clear();
    }

    fn geometry_changed(&mut self) {
        if let Some(next) = self.geometry_epoch.checked_add(1) {
            self.geometry_epoch = next;
        } else {
            // Never alias an earlier upload when the counter is exhausted.
            self.geometry_identity = Arc::new(());
            self.geometry_epoch = 0;
        }
    }

    pub(crate) fn geometry_stamp(&self) -> (&Arc<()>, u64) {
        (&self.geometry_identity, self.geometry_epoch)
    }

    /// Freeze component geometry without cloning its rectangles or batches.
    pub fn geometry_snapshot(self) -> Result<GeometrySnapshot, String> {
        self.status()?;
        if !self.playfields.is_empty() {
            return Err("timed playfields cannot become static UI geometry".into());
        }
        Ok(GeometrySnapshot {
            width: self.width,
            height: self.height,
            rectangles: self.rectangles.into(),
            batches: self.batches.into(),
        })
    }

    /// Concatenate an existing retained packet with checked extent/capacity and
    /// original painter order. Rejection leaves this scene unchanged.
    pub fn append_geometry(&mut self, geometry: &GeometrySnapshot) -> Result<(), String> {
        self.status()?;
        if (self.width, self.height) != (geometry.width, geometry.height) {
            return Err("retained geometry logical viewport mismatch".into());
        }
        if geometry.rectangles.len() > MAX_RECTANGLES - self.rectangles.len() {
            return Err(format!("scene exceeds {MAX_RECTANGLES} rectangles"));
        }
        let offset = self.rectangles.len() as u32;
        self.rectangles.extend_from_slice(&geometry.rectangles);
        for source in geometry.batches.iter() {
            if let Some(last) = self
                .batches
                .last_mut()
                .filter(|last| last.playfield.is_none() && last.texture == source.texture)
            {
                last.count += source.count;
            } else {
                self.batches.push(DrawBatch {
                    texture: source.texture,
                    first: source.first + offset,
                    count: source.count,
                    playfield: None,
                });
            }
        }
        if !geometry.rectangles.is_empty() {
            self.geometry_changed();
        }
        Ok(())
    }

    /// Insert a retained GPU note layer at this exact position in painter order.
    /// Cache lifetime crosses `clear`; membership/geometry/seek invalidate it.
    pub(crate) fn playfield(
        &mut self,
        chart: &crate::player_chart::PlayerChart,
        now: beatkernel::time::Timestamp,
        lookahead: i64,
        bounds: crate::ui::interaction::Bounds,
    ) -> Result<(), String> {
        if lookahead <= 0 {
            return Err("playfield lookahead must be positive".into());
        }
        let slot = self.playfields.len();
        if slot == MAX_PLAYFIELDS {
            return Err(format!(
                "scene exceeds {MAX_PLAYFIELDS} displayed playfields"
            ));
        }
        let notes = chart.visible_notes_checked(now, lookahead, 150_000_000)?;
        if notes
            .iter()
            .any(|note| note.lane_index >= chart.lanes.len())
        {
            return Err("playfield note references an unavailable lane".into());
        }
        let frame =
            self.playfield_caches[slot].frame(&notes, chart.lanes.len(), bounds, now, lookahead);
        self.playfields.push(frame);
        self.batches.push(DrawBatch {
            texture: TextureId::WHITE,
            first: 0,
            count: 0,
            playfield: Some(slot),
        });
        Ok(())
    }

    /// Clips to the logical viewport. Capacity exhaustion is sticky until clear,
    /// and must be checked by the renderer before submitting this scene.
    pub fn rect(&mut self, x: i64, y: i64, width: i64, height: i64, color: u32) {
        self.push(
            TextureId::WHITE,
            [x, y, width, height],
            [0.0, 0.0, 1.0, 1.0],
            color,
        );
    }

    /// UV is normalized [left, top, width, height]. Clipping crops rather than stretches it.
    pub fn sprite(
        &mut self,
        texture: TextureId,
        bounds: [i64; 4],
        uv: [f32; 4],
        tint: u32,
    ) -> Result<(), String> {
        if uv.iter().any(|value| !value.is_finite() || *value < 0.0)
            || uv[2] <= 0.0
            || uv[3] <= 0.0
            || uv[0] + uv[2] > 1.0
            || uv[1] + uv[3] > 1.0
        {
            return Err("sprite UV rectangle must lie inside normalized texture bounds".into());
        }
        self.push(texture, bounds, uv, tint);
        self.status()
    }

    pub(crate) fn glyph(&mut self, bounds: [i64; 4], uv: [f32; 4], color: u32) {
        self.push(TextureId::FONT, bounds, uv, color);
    }

    fn push(&mut self, texture: TextureId, bounds: [i64; 4], uv: [f32; 4], color: u32) {
        let [x, y, width, height] = bounds;
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
        self.geometry_changed();
        let first = self.rectangles.len() as u32;
        if let Some(batch) = self
            .batches
            .last_mut()
            .filter(|batch| batch.playfield.is_none() && batch.texture == texture)
        {
            batch.count += 1;
        } else {
            self.batches.push(DrawBatch {
                texture,
                first,
                count: 1,
                playfield: None,
            });
        }
        let horizontal = (i128::from(left) - i128::from(x)) as f64 / width as f64;
        let vertical = (i128::from(top) - i128::from(y)) as f64 / height as f64;
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
            uv: [
                uv[0] + uv[2] * horizontal as f32,
                uv[1] + uv[3] * vertical as f32,
                uv[2] * ((right - left) as f64 / width as f64) as f32,
                uv[3] * ((bottom - top) as f64 / height as f64) as f32,
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
    pub(crate) fn batches(&self) -> &[DrawBatch] {
        &self.batches
    }
    pub(crate) fn playfields(&self) -> &[PlayfieldFrame] {
        &self.playfields
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
    #[test]
    fn sprites_crop_uvs_and_batches_keep_painter_order() {
        let mut scene = Scene::new(960, 720);
        scene
            .sprite(
                TextureId::FONT,
                [-10, 0, 20, 20],
                [0.0, 0.0, 1.0, 1.0],
                0xffffff,
            )
            .unwrap();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 10.0, 20.0]);
        assert_eq!(scene.rectangles()[0].uv, [0.5, 0.0, 0.5, 1.0]);
        scene.rect(0, 0, 1, 1, 0);
        scene
            .sprite(
                TextureId::FONT,
                [0, 0, 1, 1],
                [0.0, 0.0, 1.0, 1.0],
                0xffffff,
            )
            .unwrap();
        scene
            .sprite(
                TextureId::FONT,
                [1, 0, 1, 1],
                [0.0, 0.0, 1.0, 1.0],
                0xffffff,
            )
            .unwrap();
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|batch| (batch.texture, batch.first, batch.count))
                .collect::<Vec<_>>(),
            vec![
                (TextureId::FONT, 0, 1),
                (TextureId::WHITE, 1, 1),
                (TextureId::FONT, 2, 2)
            ]
        );
        assert!(
            scene
                .sprite(TextureId::FONT, [0, 0, 1, 1], [f32::NAN, 0.0, 1.0, 1.0], 0)
                .is_err()
        );
        assert_eq!(scene.rectangles().len(), 4);
        scene
            .sprite(TextureId::FONT, [0, 0, 1, 1], [0.2, 0.2, 0.8, 0.8], 0)
            .unwrap();
    }

    #[test]
    fn retained_packets_preserve_order_and_reject_without_mutation() {
        let mut first = Scene::with_capacity(10, 10, 1);
        first.rect(0, 0, 1, 1, 0xff0000);
        first.glyph([1, 0, 1, 1], [0.0, 0.0, 1.0, 1.0], 0x00ff00);
        let first = first.geometry_snapshot().unwrap();
        let mut second = Scene::with_capacity(10, 10, 1);
        second.glyph([2, 0, 1, 1], [0.0, 0.0, 1.0, 1.0], 0x0000ff);
        second.rect(3, 0, 1, 1, 0xffffff);
        let second = second.geometry_snapshot().unwrap();
        let mut output = Scene::new(10, 10);
        output.append_geometry(&first).unwrap();
        output.append_geometry(&second).unwrap();
        assert_eq!(
            output
                .rectangles
                .iter()
                .map(|r| r.bounds[0])
                .collect::<Vec<_>>(),
            [0.0, 1.0, 2.0, 3.0]
        );
        assert_eq!(
            output
                .batches
                .iter()
                .map(|b| (b.texture, b.first, b.count))
                .collect::<Vec<_>>(),
            [
                (TextureId::WHITE, 0, 1),
                (TextureId::FONT, 1, 2),
                (TextureId::WHITE, 3, 1)
            ]
        );
        let epoch = output.geometry_epoch;
        let wrong_extent = Scene::new(11, 10).geometry_snapshot().unwrap();
        assert!(output.append_geometry(&wrong_extent).is_err());
        assert_eq!(output.geometry_epoch, epoch);
        assert_eq!(output.rectangles.len(), 4);
        for _ in 4..MAX_RECTANGLES {
            output.rect(0, 0, 1, 1, 0);
        }
        let epoch = output.geometry_epoch;
        assert!(output.append_geometry(&first).is_err());
        assert_eq!(output.geometry_epoch, epoch);
        assert_eq!(output.rectangles.len(), MAX_RECTANGLES);
        assert!(output.status().is_ok());
    }

    #[test]
    fn geometry_stamp_tracks_mutation_and_cannot_alias_other_scenes_or_wrap() {
        let mut first = Scene::new(10, 10);
        let second = Scene::new(10, 10);
        let identity = Arc::clone(first.geometry_stamp().0);
        assert!(!Arc::ptr_eq(&identity, second.geometry_stamp().0));
        first.rect(20, 20, 1, 1, 0);
        assert_eq!(first.geometry_stamp().1, 0);
        first.rect(0, 0, 1, 1, 0);
        assert_eq!(first.geometry_stamp().1, 1);
        assert!(Arc::ptr_eq(&identity, first.geometry_stamp().0));
        first.geometry_epoch = u64::MAX;
        first.clear();
        assert_eq!(first.geometry_stamp().1, 0);
        assert!(!Arc::ptr_eq(&identity, first.geometry_stamp().0));
        assert!(first.rectangles.is_empty());
    }

    #[test]
    fn retained_note_layer_splits_rectangles_and_survives_clear() {
        let source = beatkernel_bms::parse(
            "#BPM 60\n#00011:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = crate::player_chart::PlayerChart::from_compiled(
            &source,
            &source.compile().unwrap().chart,
        )
        .unwrap();
        let bounds = crate::ui::interaction::Bounds {
            x: 80,
            y: 106,
            width: 640,
            height: 528,
        };
        let mut scene = Scene::new(960, 720);
        scene.rect(0, 0, 1, 1, 0);
        scene
            .playfield(
                &chart,
                beatkernel::time::Timestamp::ZERO,
                1_000_000_000,
                bounds,
            )
            .unwrap();
        scene.rect(0, 0, 1, 1, 0);
        assert_eq!(scene.batches().len(), 3);
        assert_eq!(scene.batches()[0].first, 0);
        assert_eq!(scene.batches()[1].playfield, Some(0));
        assert_eq!(scene.batches()[2].first, 1);
        let cached = std::sync::Arc::clone(&scene.playfields()[0].instances);
        scene.clear();
        assert!(scene.playfields().is_empty());
        scene
            .playfield(
                &chart,
                beatkernel::time::Timestamp::ZERO,
                1_000_000_000,
                bounds,
            )
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &cached,
            &scene.playfields()[0].instances
        ));
        for _ in 1..MAX_PLAYFIELDS {
            scene
                .playfield(
                    &chart,
                    beatkernel::time::Timestamp::ZERO,
                    1_000_000_000,
                    bounds,
                )
                .unwrap();
        }
        assert!(
            scene
                .playfield(
                    &chart,
                    beatkernel::time::Timestamp::ZERO,
                    1_000_000_000,
                    bounds
                )
                .is_err()
        );
        assert_eq!(scene.playfields().len(), MAX_PLAYFIELDS);
    }
}
