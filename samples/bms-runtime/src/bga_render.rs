//! UI-owned bounded static-image GPU cache and ordered playfield composition.
use crate::{
    bga::BgaState,
    image_assets::ImageAssets,
    poor_background::BgaPresentation,
    scene::{ClipRect, Scene},
    texture::{MAX_TEXTURE_BYTES, RgbaImage, TextureId},
    ui::interaction::Bounds,
};
use std::sync::Arc;

/// GPU resource operations on the actual renderer owner; no decode or file IO.
pub trait TextureOwner {
    fn upload(&mut self, image: &RgbaImage) -> Result<TextureId, String>;
    fn remove(&mut self, id: TextureId) -> Result<(), String>;
}
impl TextureOwner for crate::graphics::Renderer {
    fn upload(&mut self, image: &RgbaImage) -> Result<TextureId, String> {
        self.upload_texture(image)
    }
    fn remove(&mut self, id: TextureId) -> Result<(), String> {
        self.remove_texture(id)
    }
}
/// One admitted immutable sprite reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgaSprite {
    pub texture: TextureId,
    pub width: u32,
    pub height: u32,
}
/// Current visible Base/two Layers and explicitly activated Poor overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BgaFrame {
    pub active: bool,
    pub base: Option<BgaSprite>,
    pub layer: Option<BgaSprite>,
    pub layer2: Option<BgaSprite>,
    pub poor_overlay: Option<BgaSprite>,
    pub opacity: crate::bga_opacity::BgaOpacity,
    pub unavailable: usize,
}
impl BgaFrame {
    /// Preflights caller frames before any partial playfield geometry.
    pub fn validate(&self) -> Result<(), String> {
        if self.unavailable > 4
            || (!self.active
                && (self.base.is_some()
                    || self.layer.is_some()
                    || self.layer2.is_some()
                    || self.poor_overlay.is_some()
                    || self.unavailable != 0))
        {
            return Err("invalid BGA frame selection".into());
        }
        for sprite in [self.base, self.layer, self.layer2, self.poor_overlay]
            .into_iter()
            .flatten()
        {
            if sprite.width == 0
                || sprite.height == 0
                || sprite.width > 16384
                || sprite.height > 16384
                || u64::from(sprite.width) * u64::from(sprite.height) * 4 > MAX_TEXTURE_BYTES
            {
                return Err("invalid BGA sprite extent".into());
            }
        }
        Ok(())
    }
}
struct Entry {
    image: Arc<RgbaImage>,
    sprite: Option<BgaSprite>,
}
/// At most sixteen distinct active Base/two Layers/Poor resources across four views.
/// Failed uploads are retained blank and retried only when the wanted union changes.
#[derive(Default)]
pub struct BgaTextureCache {
    bank: Option<Arc<ImageAssets>>,
    entries: Vec<Entry>,
}
impl BgaTextureCache {
    /// Removes live GPU resources. On failure, remaining ownership is retained
    /// for an explicit retry rather than silently abandoning renderer resources.
    pub fn clear(&mut self, owner: &mut impl TextureOwner) -> Result<(), String> {
        while let Some(entry) = self.entries.last() {
            if let Some(sprite) = entry.sprite {
                owner.remove(sprite.texture)?;
            }
            self.entries.pop();
        }
        self.bank = None;
        Ok(())
    }
    /// Synchronizes only current visible selections, deduplicating CPU Arc aliases.
    /// None preserves legacy no-image rendering. Release precedes new upload.
    pub fn sync(
        &mut self,
        bank: Option<&Arc<ImageAssets>>,
        states: &[BgaState],
        owner: &mut impl TextureOwner,
    ) -> Result<[BgaFrame; 4], String> {
        if states.len() > 4 {
            return Err("BGA cache admits at most four views".into());
        }
        let mut presentations = [BgaPresentation::default(); 4];
        for (destination, state) in presentations.iter_mut().zip(states) {
            *destination = (*state).into();
        }
        self.sync_presentations(bank, &presentations[..states.len()], owner)
    }

    /// Synchronizes explicit drawing intent, including raw Poor overlays.
    pub fn sync_presentations(
        &mut self,
        bank: Option<&Arc<ImageAssets>>,
        states: &[BgaPresentation],
        owner: &mut impl TextureOwner,
    ) -> Result<[BgaFrame; 4], String> {
        if states.len() > 4 {
            return Err("BGA cache admits at most four views".into());
        }
        let same_bank = match (&self.bank, bank) {
            (None, None) => true,
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            _ => false,
        };
        if !same_bank {
            self.clear(owner)?;
            self.bank = bank.cloned();
        }
        let Some(bank) = bank else {
            return Ok([BgaFrame::default(); 4]);
        };
        let mut wanted: [Option<&Arc<RgbaImage>>; 16] = [None; 16];
        let mut count = 0;
        for presentation in states {
            let state = presentation.state;
            for image in [
                state.base.and_then(|id| bank.get(id)),
                state.layer.and_then(|id| bank.get_layer(id)),
                state.layer2.and_then(|id| bank.get_layer(id)),
                presentation.poor_overlay.and_then(|id| bank.get(id)),
            ]
            .into_iter()
            .flatten()
            {
                if !wanted[..count]
                    .iter()
                    .flatten()
                    .any(|old| Arc::ptr_eq(old, image))
                {
                    wanted[count] = Some(image);
                    count += 1;
                }
            }
        }
        let changed = self.entries.len() != count
            || self.entries.iter().any(|entry| {
                !wanted[..count]
                    .iter()
                    .flatten()
                    .any(|image| Arc::ptr_eq(&entry.image, image))
            });
        let mut index = 0;
        while index < self.entries.len() {
            if wanted[..count]
                .iter()
                .flatten()
                .any(|image| Arc::ptr_eq(&self.entries[index].image, image))
            {
                index += 1;
                continue;
            }
            if let Some(sprite) = self.entries[index].sprite {
                owner.remove(sprite.texture)?;
            }
            self.entries.remove(index);
        }
        self.entries
            .try_reserve(count.saturating_sub(self.entries.len()))
            .map_err(|e| e.to_string())?;
        for image in wanted[..count].iter().flatten() {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| Arc::ptr_eq(&entry.image, image))
            {
                if changed && entry.sprite.is_none() {
                    entry.sprite = upload(owner, image);
                }
            } else {
                self.entries.push(Entry {
                    image: Arc::clone(image),
                    sprite: upload(owner, image),
                });
            }
        }
        let mut frames = [BgaFrame::default(); 4];
        for (frame, presentation) in frames.iter_mut().zip(states) {
            let state = presentation.state;
            frame.opacity = presentation.opacity;
            frame.active = state.base.is_some()
                || state.layer.is_some()
                || state.layer2.is_some()
                || presentation.poor_overlay.is_some();
            for (id, image, destination) in [
                (
                    state.base,
                    state.base.and_then(|id| bank.get(id)),
                    &mut frame.base,
                ),
                (
                    state.layer,
                    state.layer.and_then(|id| bank.get_layer(id)),
                    &mut frame.layer,
                ),
                (
                    state.layer2,
                    state.layer2.and_then(|id| bank.get_layer(id)),
                    &mut frame.layer2,
                ),
                (
                    presentation.poor_overlay,
                    presentation.poor_overlay.and_then(|id| bank.get(id)),
                    &mut frame.poor_overlay,
                ),
            ] {
                if id.is_some() {
                    *destination = image
                        .and_then(|image| {
                            self.entries
                                .iter()
                                .find(|entry| Arc::ptr_eq(&entry.image, image))
                        })
                        .and_then(|entry| entry.sprite);
                    if destination.is_none() {
                        frame.unavailable += 1;
                    }
                }
            }
            frame.validate()?;
        }
        Ok(frames)
    }
}
fn upload(owner: &mut impl TextureOwner, image: &RgbaImage) -> Option<BgaSprite> {
    owner.upload(image).ok().map(|texture| BgaSprite {
        texture,
        width: image.width(),
        height: image.height(),
    })
}
/// Paints black then Base, Layer, Layer2 and Poor inside the above-judgement field.
/// Sprites use centered integer aspect fit, straight alpha and dim tint.
pub fn paint(scene: &mut Scene, frame: BgaFrame, bounds: Bounds) -> Result<(), String> {
    frame.validate()?;
    let [width, height] = scene.dimensions();
    if bounds.x < 0
        || bounds.y < 0
        || bounds.width <= 0
        || bounds.height <= 0
        || i128::from(bounds.x) + i128::from(bounds.width) > width as i128
        || i128::from(bounds.y) + i128::from(bounds.height) > height as i128
    {
        return Err("BGA bounds do not fit viewport".into());
    }
    let clip = ClipRect::new([bounds.x, bounds.y, bounds.width, bounds.height])?;
    if !frame.active {
        return Ok(());
    }
    scene.rect(bounds.x, bounds.y, bounds.width, bounds.height, 0);
    for (sprite, alpha) in [
        (frame.base, frame.opacity.base),
        (frame.layer, frame.opacity.layer),
        (frame.layer2, frame.opacity.layer2),
        (frame.poor_overlay, frame.opacity.poor),
    ] {
        let Some(sprite) = sprite else {
            continue;
        };
        let (fit_width, fit_height) = if i128::from(bounds.width) * i128::from(sprite.height)
            <= i128::from(bounds.height) * i128::from(sprite.width)
        {
            (
                bounds.width,
                (i128::from(bounds.width) * i128::from(sprite.height) / i128::from(sprite.width))
                    .max(1) as i64,
            )
        } else {
            (
                (i128::from(bounds.height) * i128::from(sprite.width) / i128::from(sprite.height))
                    .max(1) as i64,
                bounds.height,
            )
        };
        scene.sprite_clipped_alpha(
            sprite.texture,
            [
                bounds.x + (bounds.width - fit_width) / 2,
                bounds.y + (bounds.height - fit_height) / 2,
                fit_width,
                fit_height,
            ],
            [0., 0., 1., 1.],
            0x606060,
            alpha,
            clip,
        )?;
    }
    scene.status()
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::image_assets::ImageAssetLimits;
    use beatkernel_bms::{ImageId, ParseOptions};
    use std::{
        collections::BTreeSet,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Bank {
        bank: Arc<ImageAssets>,
        path: PathBuf,
    }
    impl Drop for Bank {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    fn bank() -> Bank {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-bga-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        for (name, color) in [
            ("red.bmp", [0, 0, 255]),
            ("blue.bmp", [255, 0, 0]),
            ("green.bmp", [0, 255, 0]),
            ("black.bmp", [0, 0, 0]),
        ] {
            let mut data = vec![0u8; 58];
            data[..2].copy_from_slice(b"BM");
            data[2..6].copy_from_slice(&58u32.to_le_bytes());
            data[10..14].copy_from_slice(&54u32.to_le_bytes());
            data[14..18].copy_from_slice(&40u32.to_le_bytes());
            data[18..22].copy_from_slice(&1i32.to_le_bytes());
            data[22..26].copy_from_slice(&1i32.to_le_bytes());
            data[26..28].copy_from_slice(&1u16.to_le_bytes());
            data[28..30].copy_from_slice(&24u16.to_le_bytes());
            data[54..57].copy_from_slice(&color);
            std::fs::write(path.join(name), data).unwrap();
        }
        let chart=beatkernel_bms::parse("#BMP00 red.bmp\n#BMP01 red.bmp\n#BMP02 ./red.bmp\n#BMP03 blue.bmp\n#BMP04 green.bmp\n#BMP06 black.bmp\n#BMP07 ./black.bmp\n#00004:010203040607\n#00007:010203040607\n#00104:05",ParseOptions::default()).unwrap();
        let bank =
            Arc::new(ImageAssets::prepare(&path, &chart, ImageAssetLimits::default()).unwrap());
        Bank { bank, path }
    }
    #[derive(Default)]
    struct Owner {
        capacity: usize,
        attempts: usize,
        live: BTreeSet<TextureId>,
        actions: Vec<&'static str>,
        fail_remove: bool,
        pixels: Vec<Vec<u8>>,
    }
    impl TextureOwner for Owner {
        fn upload(&mut self, image: &RgbaImage) -> Result<TextureId, String> {
            self.attempts += 1;
            self.actions.push("upload");
            if self.live.len() >= self.capacity {
                return Err("fixture GPU budget".into());
            }
            let id = TextureId::allocate()?;
            self.pixels.push(image.pixels().to_vec());
            self.live.insert(id);
            Ok(id)
        }
        fn remove(&mut self, id: TextureId) -> Result<(), String> {
            self.actions.push("remove");
            if self.fail_remove {
                return Err("fixture removal failure".into());
            }
            assert!(self.live.remove(&id));
            Ok(())
        }
    }
    fn state(base: u16, layer: Option<u16>) -> BgaState {
        BgaState {
            base: Some(ImageId(base)),
            layer: layer.map(ImageId),
            layer2: None,
            poor: Some(ImageId(0)),
        }
    }
    #[test]
    fn cropped_aliases_share_gpu_ownership_and_keep_canvas_placement_beneath_layers() {
        let mut prepared = bank();
        let chart = beatkernel_bms::parse("#BMP01 red.bmp\n#BMP02 ./red.bmp\n#BGA01 01 0 0 1 1 10 20\n#@BGA02 02 0 0 1 1 10 20\n#BGA03 01 0 0 1 1 30 40\n#00004:010203\n#00007:02", ParseOptions::default()).unwrap();
        prepared.bank =
            Arc::new(ImageAssets::prepare(&prepared.path, &chart, Default::default()).unwrap());
        assert!(Arc::ptr_eq(
            prepared.bank.get(ImageId(1)).unwrap(),
            prepared.bank.get_layer(ImageId(2)).unwrap()
        ));
        let mut owner = Owner {
            capacity: 2,
            ..Default::default()
        };
        let mut cache = BgaTextureCache::default();
        let first = cache
            .sync(Some(&prepared.bank), &[state(1, Some(2))], &mut owner)
            .unwrap();
        assert_eq!(owner.attempts, 1);
        assert_eq!(first[0].base.unwrap().width, 256);
        assert_eq!(
            first[0].base.unwrap().texture,
            first[0].layer.unwrap().texture
        );
        let before = owner.actions.len();
        let changed = cache
            .sync(Some(&prepared.bank), &[state(3, Some(2))], &mut owner)
            .unwrap();
        assert_eq!(&owner.actions[before..], &["upload"]);
        assert_eq!(owner.live.len(), 2);
        let mut scene = Scene::new(300, 300);
        paint(
            &mut scene,
            changed[0],
            Bounds {
                x: 50,
                y: 25,
                width: 100,
                height: 200,
            },
        )
        .unwrap();
        assert_eq!(scene.rectangles().len(), 3); // black, fitted Base canvas, Layer canvas
        assert_eq!(scene.rectangles()[1].bounds, [50.0, 75.0, 100.0, 100.0]);
        assert_eq!(scene.rectangles()[1].uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(scene.rectangles()[2].bounds, scene.rectangles()[1].bounds);
        cache.clear(&mut owner).unwrap();
        assert!(owner.live.is_empty());
    }
    #[test]
    fn raw_only_image_is_unavailable_in_layer_role_without_opaque_fallback() {
        let bank = bank();
        assert!(bank.bank.get(ImageId(0)).is_some());
        let mut owner = Owner {
            capacity: 8,
            ..Owner::default()
        };
        let frames = BgaTextureCache::default()
            .sync(Some(&bank.bank), &[state(1, Some(0))], &mut owner)
            .unwrap();
        assert!(frames[0].base.is_some());
        assert!(frames[0].layer.is_none());
        assert_eq!(frames[0].unavailable, 1);
        assert_eq!(owner.attempts, 1);
    }
    #[test]
    fn black_source_keeps_base_pixels_and_uploads_separate_transparent_layer() {
        let bank = bank();
        let mut owner = Owner {
            capacity: 8,
            ..Owner::default()
        };
        let mut cache = BgaTextureCache::default();
        let frames = cache
            .sync(
                Some(&bank.bank),
                &[state(6, Some(7)), state(7, Some(6))],
                &mut owner,
            )
            .unwrap();
        assert_eq!(owner.attempts, 2);
        assert_eq!(owner.pixels, [vec![0, 0, 0, 255], vec![0, 0, 0, 0]]);
        assert_ne!(frames[0].base, frames[0].layer);
        assert_eq!(frames[0].base, frames[1].base);
        assert_eq!(frames[0].layer, frames[1].layer);
        assert_eq!(frames[0].unavailable, 0);
        cache
            .sync(Some(&bank.bank), &[state(7, Some(6))], &mut owner)
            .unwrap();
        assert_eq!(owner.attempts, 2);
        cache.clear(&mut owner).unwrap();
        assert!(owner.live.is_empty());
    }
    #[test]
    fn actual_bank_aliases_share_upload_and_steady_frames_change_release_before_upload() {
        let bank = bank();
        let mut owner = Owner {
            capacity: 8,
            ..Owner::default()
        };
        let mut cache = BgaTextureCache::default();
        let states = [state(1, Some(2)), state(2, None)];
        let first = cache.sync(Some(&bank.bank), &states, &mut owner).unwrap();
        assert_eq!(owner.attempts, 1);
        assert_eq!(first[0].base, first[0].layer);
        assert_eq!(first[0].base, first[1].base);
        assert_eq!(
            cache.sync(Some(&bank.bank), &states, &mut owner).unwrap(),
            first
        );
        assert_eq!(owner.attempts, 1);
        let before = owner.actions.len();
        cache
            .sync(Some(&bank.bank), &[state(3, None)], &mut owner)
            .unwrap();
        assert_eq!(&owner.actions[before..], &["remove", "upload"]);
        let replacement = Arc::new(bank.bank.as_ref().clone());
        cache
            .sync(Some(&replacement), &[state(3, None)], &mut owner)
            .unwrap();
        assert_eq!(owner.attempts, 3);
        assert!(
            cache
                .sync(Some(&replacement), &[BgaState::default(); 5], &mut owner)
                .is_err()
        );
        assert_eq!(owner.live.len(), 1);
        cache.clear(&mut owner).unwrap();
        assert!(owner.live.is_empty());
        assert!(cache.bank.is_none());
        assert_eq!(
            cache.sync(None, &states, &mut owner).unwrap(),
            [BgaFrame::default(); 4]
        );
    }
    #[test]
    fn failed_upload_is_blank_until_wanted_union_changes_and_removal_errors_retain_ownership() {
        let bank = bank();
        let mut owner = Owner {
            capacity: 1,
            ..Owner::default()
        };
        let mut cache = BgaTextureCache::default();
        let wanted = [state(1, Some(3))];
        let frames = cache.sync(Some(&bank.bank), &wanted, &mut owner).unwrap();
        assert!(frames[0].base.is_some());
        assert!(frames[0].layer.is_none());
        assert_eq!(frames[0].unavailable, 1);
        assert_eq!(owner.attempts, 2);
        cache.sync(Some(&bank.bank), &wanted, &mut owner).unwrap();
        assert_eq!(owner.attempts, 2);
        let resumed = cache
            .sync(Some(&bank.bank), &[state(3, None)], &mut owner)
            .unwrap();
        assert!(resumed[0].base.is_some());
        assert_eq!(owner.attempts, 3);
        owner.fail_remove = true;
        assert!(cache.clear(&mut owner).is_err());
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(owner.live.len(), 1);
        owner.fail_remove = false;
        cache.clear(&mut owner).unwrap();
        assert!(owner.live.is_empty());
        let undefined = cache
            .sync(Some(&bank.bank), &[state(5, None)], &mut owner)
            .unwrap();
        assert!(undefined[0].active);
        assert_eq!(undefined[0].unavailable, 1);
        assert_eq!(owner.attempts, 3);
        cache
            .sync(
                Some(&bank.bank),
                &[BgaState {
                    poor: Some(ImageId(0)),
                    ..BgaState::default()
                }],
                &mut owner,
            )
            .unwrap();
        assert_eq!(owner.attempts, 3);
    }
    #[test]
    fn four_two_layer_views_bound_sixteen_resources_and_release_expired_overlays() {
        let mut prepared = bank();
        let template = std::fs::read(prepared.path.join("red.bmp")).unwrap();
        let mut source = String::new();
        for id in 1..=16u16 {
            let code = if id < 10 {
                format!("0{id}")
            } else {
                format!("0{}", char::from(b'A' + (id - 10) as u8))
            };
            let name = format!("distinct-{id}.bmp");
            let mut bytes = template.clone();
            bytes[54..57].copy_from_slice(&[id as u8, 1, 1]);
            std::fs::write(prepared.path.join(&name), bytes).unwrap();
            source.push_str(&format!("#BMP{code} {name}\n"));
        }
        source.push_str("#00004:01020304\n#00007:05060708\n#0000A:090A0B0C\n#00006:0D0E0F0G");
        let chart = beatkernel_bms::parse(&source, ParseOptions::default()).unwrap();
        prepared.bank = Arc::new(
            ImageAssets::prepare(&prepared.path, &chart, ImageAssetLimits::default()).unwrap(),
        );
        let selections: [BgaPresentation; 4] = std::array::from_fn(|i| BgaPresentation {
            state: BgaState {
                layer2: Some(ImageId(i as u16 + 9)),
                ..state(i as u16 + 1, Some(i as u16 + 5))
            },
            poor_overlay: Some(ImageId(i as u16 + 13)),
            opacity: Default::default(),
        });
        let normal = selections.map(|p| p.state);
        let mut owner = Owner {
            capacity: 16,
            ..Owner::default()
        };
        let mut cache = BgaTextureCache::default();
        cache
            .sync(Some(&prepared.bank), &normal, &mut owner)
            .unwrap();
        assert_eq!(owner.live.len(), 12); // Legacy selections never infer Poor activation.
        let frames = cache
            .sync_presentations(Some(&prepared.bank), &selections, &mut owner)
            .unwrap();
        assert_eq!((owner.live.len(), owner.attempts), (16, 16));
        assert!(
            frames
                .iter()
                .all(|f| f.layer2.is_some() && f.poor_overlay.is_some() && f.unavailable == 0)
        );
        cache
            .sync_presentations(Some(&prepared.bank), &selections, &mut owner)
            .unwrap();
        assert_eq!(owner.attempts, 16);
        let before = owner.actions.len();
        let faded = selections.map(|mut presentation| {
            presentation.opacity = crate::bga_opacity::BgaOpacity {
                base: 1,
                layer: 128,
                layer2: 64,
                poor: 32,
            };
            presentation
        });
        let faded_frames = cache
            .sync_presentations(Some(&prepared.bank), &faded, &mut owner)
            .unwrap();
        assert_eq!(owner.actions.len(), before);
        assert_eq!((owner.live.len(), owner.attempts), (16, 16));
        for (frame, presentation) in faded_frames.iter().zip(faded) {
            assert_eq!(frame.opacity, presentation.opacity);
        }
        cache
            .sync(Some(&prepared.bank), &normal, &mut owner)
            .unwrap();
        assert_eq!(owner.live.len(), 12);
        cache.clear(&mut owner).unwrap();
        assert!(owner.live.is_empty());
        let mut owner = Owner {
            capacity: 15,
            ..Owner::default()
        };
        let frames = cache
            .sync_presentations(Some(&prepared.bank), &selections, &mut owner)
            .unwrap();
        assert_eq!(frames.iter().map(|f| f.unavailable).sum::<usize>(), 1);
        cache
            .sync_presentations(Some(&prepared.bank), &selections, &mut owner)
            .unwrap();
        assert_eq!((owner.live.len(), owner.attempts), (15, 16));
        let before = owner.actions.len();
        assert!(
            cache
                .sync_presentations(
                    Some(&prepared.bank),
                    &[BgaPresentation::default(); 5],
                    &mut owner
                )
                .is_err()
        );
        assert_eq!(owner.actions.len(), before);
        cache.clear(&mut owner).unwrap();
        assert!(owner.live.is_empty());
    }
    #[test]
    fn poor_overlay_uses_raw_pixels_and_shares_base_aliases() {
        let bank = bank();
        let mut owner = Owner {
            capacity: 12,
            ..Owner::default()
        };
        let mut cache = BgaTextureCache::default();
        let alias = BgaPresentation {
            state: state(1, Some(7)),
            poor_overlay: Some(ImageId(0)),
            opacity: Default::default(),
        };
        let frames = cache
            .sync_presentations(Some(&bank.bank), &[alias], &mut owner)
            .unwrap();
        assert_eq!(frames[0].base, frames[0].poor_overlay);
        assert_ne!(frames[0].layer, frames[0].poor_overlay);
        assert_eq!(owner.attempts, 2);
        let black = BgaPresentation {
            poor_overlay: Some(ImageId(6)),
            ..alias
        };
        let frames = cache
            .sync_presentations(Some(&bank.bank), &[black], &mut owner)
            .unwrap();
        assert_eq!(owner.pixels.last().unwrap(), &[0, 0, 0, 255]);
        assert_ne!(frames[0].layer, frames[0].poor_overlay);
        assert_eq!(frames[0].unavailable, 0);
        cache.clear(&mut owner).unwrap();
    }
    #[test]
    fn centered_base_and_layer_geometry_is_ordered_dimmed_and_does_not_leak_clip() {
        let base = TextureId::allocate().unwrap();
        let layer = TextureId::allocate().unwrap();
        let frame = BgaFrame {
            active: true,
            base: Some(BgaSprite {
                texture: base,
                width: 400,
                height: 100,
            }),
            layer: Some(BgaSprite {
                texture: layer,
                width: 100,
                height: 200,
            }),
            unavailable: 0,
            poor_overlay: None,
            layer2: None,
            opacity: Default::default(),
        };
        let mut scene = Scene::new(300, 300);
        paint(
            &mut scene,
            frame,
            Bounds {
                x: 20,
                y: 30,
                width: 200,
                height: 200,
            },
        )
        .unwrap();
        assert_eq!(scene.rectangles()[0].bounds, [20., 30., 200., 200.]);
        assert_eq!(scene.rectangles()[1].bounds, [20., 105., 200., 50.]);
        assert_eq!(scene.rectangles()[2].bounds, [70., 30., 100., 200.]);
        assert_eq!(scene.rectangles()[1].uv, [0., 0., 1., 1.]);
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|b| b.texture)
                .collect::<Vec<_>>(),
            vec![TextureId::WHITE, base, layer]
        );
        assert_eq!(
            scene.rectangles()[1].color,
            [96. / 255., 96. / 255., 96. / 255., 1.]
        );
        scene.rect(0, 0, 5, 5, 0xffffff);
        assert_eq!(scene.rectangles().last().unwrap().bounds, [0., 0., 5., 5.]);
        let before = scene.rectangles().len();
        let invalid = BgaFrame {
            base: Some(BgaSprite {
                width: 0,
                ..frame.base.unwrap()
            }),
            ..frame
        };
        assert!(
            paint(
                &mut scene,
                invalid,
                Bounds {
                    x: 20,
                    y: 30,
                    width: 200,
                    height: 200
                }
            )
            .is_err()
        );
        assert_eq!(scene.rectangles().len(), before);
        assert!(
            paint(
                &mut scene,
                frame,
                Bounds {
                    x: i64::MAX,
                    y: 0,
                    width: 1,
                    height: 1
                }
            )
            .is_err()
        );
        assert_eq!(scene.rectangles().len(), before);
    }
}
