//! Contained static visual resources, prepared explicitly before playback.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, FileAssetSource},
    image_crop::crop_canvas_sized,
    image_decode::{ImageDecodeError, ImageDecodeLimits, decode},
    image_key::{black_to_transparent, needs_key},
    texture::RgbaImage,
};
use beatkernel_bms::{BgaChannel, BgaCrop, BmsChart, ImageId};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Largest admitted number of two-digit base62 image references, including BMP00.
pub const MAX_IMAGE_REFERENCES: usize = 62 * 62;
/// Hard ceiling for retained decoded image data; decoder scratch is additional.
pub const MAX_IMAGE_BANK_BYTES: u64 = 256 * 1024 * 1024;

/// Configurable preparation bounds, independent of concurrent GPU texture slots.
#[derive(Clone, Copy, Debug)]
pub struct ImageAssetLimits {
    /// Maximum referenced image IDs, including unavailable resources.
    pub max_images: usize,
    /// Maximum retained RGBA bytes, counting raw files, crops and Layer variants.
    pub max_decoded_bytes: u64,
    /// Encoded input, dimensions and output bounds for one image.
    pub decode: ImageDecodeLimits,
}
impl Default for ImageAssetLimits {
    fn default() -> Self {
        Self {
            max_images: MAX_IMAGE_REFERENCES,
            max_decoded_bytes: 64 * 1024 * 1024,
            decode: ImageDecodeLimits::default(),
        }
    }
}
impl ImageAssetLimits {
    /// Rejects invalid configuration before resource lookup or decoding.
    pub fn validate(self) -> Result<(), String> {
        if self.max_images == 0 || self.max_images > MAX_IMAGE_REFERENCES {
            return Err("image reference capacity invalid".into());
        }
        if self.max_decoded_bytes == 0 || self.max_decoded_bytes > MAX_IMAGE_BANK_BYTES {
            return Err("image bank byte budget invalid".into());
        }
        self.decode.validate().map_err(|e| e.to_string())
    }
}

/// Explicit reason a selected visual reference will be blank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageUnavailable {
    /// Nonzero visual token has no BMP definition.
    Undefined,
    /// The contained asset is absent after supported filename variants, or vanished before reading.
    Missing,
    /// Encoded signature is not a supported static BMP/PNG/JPEG image.
    Unsupported,
    /// A supported raster could not be decoded.
    InvalidData(String),
}

/// Immutable CPU data shared across image IDs and cloned presentation owners.
/// No GPU resource or native input/audio ownership lives here.
#[derive(Clone, Default)]
pub struct ImageAssets {
    // Keep original dependencies even when a crop replaces their displayed ID.
    sources: Vec<Arc<RgbaImage>>,
    source_ids: BTreeMap<ImageId, Option<Arc<RgbaImage>>>,
    images: BTreeMap<ImageId, Arc<RgbaImage>>,
    layers: BTreeMap<ImageId, Arc<RgbaImage>>,
    unavailable: BTreeMap<ImageId, ImageUnavailable>,
    decoded_bytes: u64,
    unique_images: usize,
}

/// Immutable resource table and receiver-local aliases for visual registration.
/// Each resource occurs once; sources retain original crop dependencies.
#[derive(Clone)]
pub struct ImageAssetsTransfer {
    pub resources: Vec<Arc<RgbaImage>>,
    pub sources: Vec<usize>,
    /// Original source IDs, including unavailable hidden crop dependencies.
    pub source_ids: Vec<(ImageId, Option<usize>)>,
    pub images: Vec<(ImageId, usize)>,
    pub layers: Vec<(ImageId, usize)>,
    pub unavailable: Vec<(ImageId, ImageUnavailable)>,
}

#[derive(Clone)]
enum Cached {
    Loaded(Arc<RgbaImage>),
    Unavailable(ImageUnavailable),
}

struct ImagePlan {
    explicit_canvas: Option<[u32; 2]>,
    canvas: [u32; 2],
    canvas_bytes: u64,
    references: BTreeSet<ImageId>,
    sources: BTreeSet<ImageId>,
}
impl ImagePlan {
    fn new(chart: &BmsChart, limits: ImageAssetLimits) -> Result<Self, String> {
        limits.validate()?;
        let explicit_canvas = chart.canvas_size().map_err(|e| e.to_string())?;
        let canvas = explicit_canvas.unwrap_or([256, 256]);
        if chart
            .bga
            .len()
            .checked_add(chart.bga_opacity.len())
            .is_none_or(|count| count > beatkernel::chart::MAX_SOURCE_ITEMS)
        {
            return Err("visual source item capacity exceeded".into());
        }
        if chart.bga_crops.len() > MAX_IMAGE_REFERENCES {
            return Err("image crop definition capacity exceeded".into());
        }
        for (id, crop) in &chart.bga_crops {
            if usize::from(id.0) >= MAX_IMAGE_REFERENCES {
                return Err("image crop identity invalid".into());
            }
            crop.validate()
                .map_err(|e| format!("image crop {}: {e}", id.0))?;
        }
        let mut references: BTreeSet<_> = chart.bga.iter().map(|event| event.image).collect();
        if chart.images.contains_key(&ImageId(0)) || chart.bga_crops.contains_key(&ImageId(0)) {
            references.insert(ImageId(0));
        }
        let canvas_bytes = u64::from(canvas[0])
            .checked_mul(u64::from(canvas[1]))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("canvas extent overflow")?;
        if explicit_canvas.is_some() || references.iter().any(|id| chart.bga_crops.contains_key(id))
        {
            if canvas[0] == 0
                || canvas[1] == 0
                || canvas[0] > limits.decode.max_width
                || canvas[1] > limits.decode.max_height
                || canvas_bytes > limits.decode.max_decoded_bytes
            {
                return Err("canvas extent or decoded byte limit exceeded".into());
            }
        }
        let sources: BTreeSet<_> = references
            .iter()
            .map(|id| chart.bga_crops.get(id).map_or(*id, |crop| crop.source))
            .collect();
        if references.union(&sources).count() > limits.max_images
            || references
                .union(&sources)
                .any(|id| usize::from(id.0) >= MAX_IMAGE_REFERENCES)
        {
            return Err("image reference capacity exceeded".into());
        }
        Ok(Self {
            explicit_canvas,
            canvas,
            canvas_bytes,
            references,
            sources,
        })
    }
}

impl ImageAssets {
    /// Exports immutable pixels once and keeps sharing explicit in alias indices.
    pub fn export_visual(&self) -> ImageAssetsTransfer {
        let mut resources = Vec::new();
        let mut indices = BTreeMap::new();
        let mut alias = |image: &Arc<RgbaImage>| {
            let address = Arc::as_ptr(image);
            *indices.entry(address).or_insert_with(|| {
                let index = resources.len();
                resources.push(Arc::clone(image));
                index
            })
        };
        let sources = self.sources.iter().map(&mut alias).collect();
        let source_ids = self
            .source_ids
            .iter()
            .map(|(id, resource)| (*id, resource.as_ref().map(&mut alias)))
            .collect();
        let images = self
            .images
            .iter()
            .map(|(id, image)| (*id, alias(image)))
            .collect();
        let layers = self
            .layers
            .iter()
            .map(|(id, image)| (*id, alias(image)))
            .collect();
        ImageAssetsTransfer {
            resources,
            sources,
            source_ids,
            images,
            layers,
            unavailable: self
                .unavailable
                .iter()
                .map(|(id, reason)| (*id, reason.clone()))
                .collect(),
        }
    }

    /// Validates complete dimensions, budgets and aliases before publishing a bank.
    /// Pixel storage is immutable; transport adapters construct this table locally.
    pub fn import_visual(
        data: ImageAssetsTransfer,
        limits: ImageAssetLimits,
    ) -> Result<Self, String> {
        limits.validate()?;
        if data
            .images
            .len()
            .checked_add(data.unavailable.len())
            .is_none_or(|count| count > limits.max_images)
            || data.sources.len() > limits.max_images
            || data.source_ids.len() > limits.max_images
            || data.layers.len() > data.images.len()
            || data.resources.len() > limits.max_images * 3
        {
            return Err("image transfer count budget exceeded".into());
        }
        let mut decoded_bytes = 0u64;
        for image in &data.resources {
            let extent = u64::from(image.width())
                .checked_mul(u64::from(image.height()))
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or("image transfer extent overflow")?;
            if image.width() == 0
                || image.height() == 0
                || image.width() > limits.decode.max_width
                || image.height() > limits.decode.max_height
                || extent != image.byte_len()
                || extent > limits.decode.max_decoded_bytes
            {
                return Err("image transfer extent or byte limit exceeded".into());
            }
            decoded_bytes = decoded_bytes
                .checked_add(extent)
                .filter(|bytes| *bytes <= limits.max_decoded_bytes)
                .ok_or("image transfer aggregate byte budget exceeded")?;
        }
        data.unavailable
            .iter()
            .try_fold(0usize, |total, (_, reason)| {
                let bytes = match reason {
                    ImageUnavailable::InvalidData(reason) => reason.len(),
                    _ => 0,
                };
                total.checked_add(bytes)
            })
            .ok_or("image transfer diagnostic byte count overflow")?;
        // All numeric/byte ceilings are checked before validation scratch or maps.
        let valid_id = |id: ImageId| usize::from(id.0) < MAX_IMAGE_REFERENCES;
        for entries in [&data.images, &data.layers] {
            if entries
                .iter()
                .any(|(id, index)| !valid_id(*id) || *index >= data.resources.len())
                || entries.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            {
                return Err("image transfer alias order or reference invalid".into());
            }
        }
        if data.unavailable.iter().any(|(id, _)| !valid_id(*id))
            || data
                .unavailable
                .windows(2)
                .any(|pair| pair[0].0 >= pair[1].0)
            || data.unavailable.iter().any(|(id, _)| {
                data.images
                    .binary_search_by_key(id, |entry| entry.0)
                    .is_ok()
            })
        {
            return Err("image transfer unavailable role invalid".into());
        }
        if data.source_ids.iter().any(|(id, index)| {
            !valid_id(*id) || index.is_some_and(|index| index >= data.resources.len())
        }) || data
            .source_ids
            .windows(2)
            .any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err("image transfer source identity or order invalid".into());
        }
        let hidden_source_count = data
            .source_ids
            .iter()
            .filter(|(id, _)| {
                data.images
                    .binary_search_by_key(id, |entry| entry.0)
                    .is_err()
                    && data
                        .unavailable
                        .binary_search_by_key(id, |entry| entry.0)
                        .is_err()
            })
            .count();
        if data
            .images
            .len()
            .checked_add(data.unavailable.len())
            .and_then(|count| count.checked_add(hidden_source_count))
            .is_none_or(|count| count > limits.max_images)
        {
            return Err("image transfer source and display reference budget exceeded".into());
        }
        for (id, index) in &data.layers {
            let raw = data
                .images
                .binary_search_by_key(id, |entry| entry.0)
                .map_err(|_| "image transfer layer has no raw role")?;
            let raw = &data.resources[data.images[raw].1];
            let layer = &data.resources[*index];
            if (raw.width(), raw.height()) != (layer.width(), layer.height()) {
                return Err("image transfer layer extent differs from raw role".into());
            }
        }
        let mut referenced = vec![false; data.resources.len()];
        let mut sources = BTreeSet::new();
        for &index in &data.sources {
            if index >= data.resources.len() || !sources.insert(index) {
                return Err("image transfer source alias invalid".into());
            }
            referenced[index] = true;
        }
        let source_aliases: BTreeSet<_> =
            data.source_ids.iter().filter_map(|entry| entry.1).collect();
        if source_aliases != sources {
            return Err(
                "image transfer source identities do not cover original allocations".into(),
            );
        }
        for (_, index) in data.images.iter().chain(&data.layers) {
            referenced[*index] = true;
        }
        if referenced.iter().any(|used| !used) {
            return Err("image transfer contains unreferenced resource".into());
        }
        let mut allocations = BTreeSet::new();
        if data
            .resources
            .iter()
            .any(|resource| !allocations.insert(Arc::as_ptr(resource)))
        {
            return Err("image transfer resource table contains duplicate allocation".into());
        }
        let unique_images = data.sources.len();
        Ok(Self {
            sources: data
                .sources
                .iter()
                .map(|&index| Arc::clone(&data.resources[index]))
                .collect(),
            source_ids: data
                .source_ids
                .into_iter()
                .map(|(id, index)| (id, index.map(|index| Arc::clone(&data.resources[index]))))
                .collect(),
            images: data
                .images
                .into_iter()
                .map(|(id, index)| (id, Arc::clone(&data.resources[index])))
                .collect(),
            layers: data
                .layers
                .into_iter()
                .map(|(id, index)| (id, Arc::clone(&data.resources[index])))
                .collect(),
            unavailable: data.unavailable.into_iter().collect(),
            decoded_bytes,
            unique_images,
        })
    }

    /// Loads referenced images, crop dependencies and initial BMP00/BGA00.
    /// Bad/missing raster data is unavailable; unsafe IO and exhausted limits
    /// reject the entire preparation. Filesystem stability is assumed, as for audio.
    pub fn prepare(
        root: &Path,
        chart: &BmsChart,
        limits: ImageAssetLimits,
    ) -> Result<Self, String> {
        let plan = ImagePlan::new(chart, limits)?;
        let source = FileAssetSource::new(root).map_err(|e| e.to_string())?;
        Self::prepare_validated(&source, chart, limits, plan)
    }

    /// Prepares identical decoded/cropped/keyed resources from a scoped source.
    pub fn prepare_from_source(
        source: &dyn AssetSource,
        chart: &BmsChart,
        limits: ImageAssetLimits,
    ) -> Result<Self, String> {
        let plan = ImagePlan::new(chart, limits)?;
        Self::prepare_validated(source, chart, limits, plan)
    }

    fn prepare_validated(
        source: &dyn AssetSource,
        chart: &BmsChart,
        limits: ImageAssetLimits,
        plan: ImagePlan,
    ) -> Result<Self, String> {
        let ImagePlan {
            explicit_canvas,
            canvas,
            canvas_bytes,
            references,
            sources,
        } = plan;
        let mut bank = Self::default();
        let mut cache = BTreeMap::<PathBuf, Cached>::new();
        for &id in &sources {
            let Some(name) = chart.images.get(&id) else {
                bank.unavailable.insert(id, ImageUnavailable::Undefined);
                continue;
            };
            let path = match source.resolve(name, AssetPathPolicy::ImageVariants) {
                Ok(path) => path,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    bank.unavailable.insert(id, ImageUnavailable::Missing);
                    continue;
                }
                Err(e) => return Err(format!("image {} path {name}: {e}", id.0)),
            };
            let resource = if let Some(resource) = cache.get(&path) {
                resource.clone()
            } else {
                let encoded = match source.read(&path, limits.decode.max_encoded_bytes) {
                    Ok(encoded) => encoded,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        bank.unavailable.insert(id, ImageUnavailable::Missing);
                        continue;
                    }
                    Err(e) => return Err(format!("image read {}: {e}", path.display())),
                };
                if encoded.len() > limits.decode.max_encoded_bytes {
                    return Err("encoded image file exceeds limit".into());
                }
                let resource = match decode(&encoded, limits.decode) {
                    Ok(image) => {
                        let total = bank
                            .decoded_bytes
                            .checked_add(image.byte_len())
                            .filter(|total| *total <= limits.max_decoded_bytes)
                            .ok_or("image bank byte budget exceeded")?;
                        bank.decoded_bytes = total;
                        bank.unique_images += 1;
                        Cached::Loaded(Arc::new(image))
                    }
                    Err(ImageDecodeError::Unsupported) => {
                        Cached::Unavailable(ImageUnavailable::Unsupported)
                    }
                    Err(ImageDecodeError::InvalidData(e)) => {
                        Cached::Unavailable(ImageUnavailable::InvalidData(e))
                    }
                    Err(ImageDecodeError::Limit(e)) => return Err(e),
                };
                cache.insert(path, resource.clone());
                resource
            };
            match resource {
                Cached::Loaded(image) => {
                    bank.images.insert(id, image);
                }
                Cached::Unavailable(reason) => {
                    bank.unavailable.insert(id, reason);
                }
            }
        }
        bank.sources
            .try_reserve_exact(bank.unique_images)
            .map_err(|e| e.to_string())?;
        bank.sources
            .extend(cache.into_values().filter_map(|resource| match resource {
                Cached::Loaded(image) => Some(image),
                Cached::Unavailable(_) => None,
            }));
        let originals = std::mem::take(&mut bank.images);
        bank.source_ids.extend(sources.into_iter().map(|id| (id, originals.get(&id).cloned())));
        let unavailable = std::mem::take(&mut bank.unavailable);
        let mut variants: Vec<(Arc<RgbaImage>, [i32; 4], [i32; 2], Arc<RgbaImage>)> = Vec::new();
        variants
            .try_reserve_exact(references.len())
            .map_err(|e| e.to_string())?;
        for id in references {
            let crop = chart.bga_crops.get(&id);
            let source = crop.map_or(id, |crop| crop.source);
            let Some(original) = originals.get(&source) else {
                bank.unavailable.insert(
                    id,
                    unavailable
                        .get(&source)
                        .cloned()
                        .unwrap_or(ImageUnavailable::Undefined),
                );
                continue;
            };
            let transform = if let Some(crop) = crop {
                Some(*crop)
            } else if explicit_canvas.is_some() && [original.width(), original.height()] != canvas {
                Some(BgaCrop {
                    source,
                    source_rect: [
                        0,
                        0,
                        i32::try_from(original.width())
                            .map_err(|_| "canvas source width overflow")?,
                        i32::try_from(original.height())
                            .map_err(|_| "canvas source height overflow")?,
                    ],
                    destination: [0, 0],
                })
            } else {
                None
            };
            let image = if let Some(crop) = transform {
                if let Some((_, _, _, image)) =
                    variants.iter().find(|(raw, rect, destination, _)| {
                        Arc::ptr_eq(raw, original)
                            && *rect == crop.source_rect
                            && *destination == crop.destination
                    })
                {
                    Arc::clone(image)
                } else {
                    let total = bank
                        .decoded_bytes
                        .checked_add(canvas_bytes)
                        .filter(|total| *total <= limits.max_decoded_bytes)
                        .ok_or("image bank crop byte budget exceeded")?;
                    let image = crop_canvas_sized(original, crop, canvas)?;
                    bank.decoded_bytes = total;
                    variants.push((
                        Arc::clone(original),
                        crop.source_rect,
                        crop.destination,
                        Arc::clone(&image),
                    ));
                    image
                }
            } else {
                Arc::clone(original)
            };
            bank.images.insert(id, image);
        }
        let layer_ids = chart
            .bga
            .iter()
            .filter(|event| matches!(event.channel, BgaChannel::Layer | BgaChannel::Layer2))
            .map(|event| event.image)
            .collect();
        let (layers, decoded_bytes) = bank.keyed_layers(&layer_ids, limits.max_decoded_bytes)?;
        bank.layers = layers;
        bank.decoded_bytes = decoded_bytes;
        Ok(bank)
    }

    fn keyed_layers(
        &self,
        ids: &BTreeSet<ImageId>,
        budget: u64,
    ) -> Result<(BTreeMap<ImageId, Arc<RgbaImage>>, u64), String> {
        let mut layers = BTreeMap::new();
        let mut variants: Vec<(Arc<RgbaImage>, Arc<RgbaImage>)> = Vec::new();
        variants
            .try_reserve_exact(ids.len())
            .map_err(|e| e.to_string())?;
        let mut total = self.decoded_bytes;
        for &id in ids {
            let Some(original) = self.images.get(&id) else {
                continue;
            };
            let keyed = if let Some((_, keyed)) =
                variants.iter().find(|(raw, _)| Arc::ptr_eq(raw, original))
            {
                Arc::clone(keyed)
            } else {
                if needs_key(original) {
                    total = total
                        .checked_add(original.byte_len())
                        .filter(|bytes| *bytes <= budget)
                        .ok_or("image bank Layer variant byte budget exceeded")?;
                }
                let keyed = black_to_transparent(original)?;
                variants.push((Arc::clone(original), Arc::clone(&keyed)));
                keyed
            };
            layers.insert(id, keyed);
        }
        Ok((layers, total))
    }

    /// Borrows only declared Layer/Layer2 pixels, with exact black made transparent.
    /// Non-Layer IDs and unavailable resources have no fallback to raw pixels.
    pub fn get_layer(&self, image: ImageId) -> Option<&Arc<RgbaImage>> {
        self.layers.get(&image)
    }

    /// Borrows shared decoded pixels; unavailable selections have no image.
    pub fn get(&self, image: ImageId) -> Option<&Arc<RgbaImage>> {
        self.images.get(&image)
    }
    /// Borrows the retained reason a referenced image is blank.
    pub fn unavailable(&self, image: ImageId) -> Option<&ImageUnavailable> {
        self.unavailable.get(&image)
    }
    /// Total referenced identities, including unavailable assets.
    pub fn len(&self) -> usize {
        self.images.len() + self.unavailable.len()
    }
    /// Whether no visual resources were requested.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Number of unique decoded canonical files.
    pub fn unique_images(&self) -> usize {
        self.unique_images
    }
    /// Retained raw sources plus unique crop and changed Layer RGBA bytes.
    pub fn decoded_bytes(&self) -> u64 {
        self.decoded_bytes
    }
}

#[cfg(test)]
mod layer_fixtures {
    use super::*;
    fn raw(pixels: Vec<u8>) -> Arc<RgbaImage> {
        Arc::new(RgbaImage::new((pixels.len() / 4) as u32, 1, pixels).unwrap())
    }
    #[test]
    fn changed_alias_variants_count_once_and_raw_base_poor_remain_immutable() {
        let original = raw(vec![0, 0, 0, 255, 1, 0, 0, 127]);
        let unchanged = raw(vec![0, 0, 0, 0, 7, 8, 9, 255]);
        let mut bank = ImageAssets::default();
        for id in [0, 1, 2] {
            bank.images.insert(ImageId(id), original.clone());
        }
        bank.images.insert(ImageId(3), unchanged.clone());
        bank.unavailable
            .insert(ImageId(4), ImageUnavailable::Undefined);
        bank.decoded_bytes = 16;
        bank.unique_images = 2;
        let ids = [ImageId(1), ImageId(2), ImageId(3), ImageId(4)]
            .into_iter()
            .collect();
        assert!(bank.keyed_layers(&ids, 23).is_err());
        assert_eq!(bank.decoded_bytes(), 16);
        assert!(bank.layers.is_empty());
        let (layers, total) = bank.keyed_layers(&ids, 24).unwrap();
        bank.layers = layers;
        bank.decoded_bytes = total;
        assert_eq!(bank.decoded_bytes(), 24);
        assert_eq!(bank.unique_images(), 2);
        assert_eq!(bank.len(), 5);
        assert!(Arc::ptr_eq(
            bank.get_layer(ImageId(1)).unwrap(),
            bank.get_layer(ImageId(2)).unwrap()
        ));
        assert!(!Arc::ptr_eq(
            bank.get(ImageId(1)).unwrap(),
            bank.get_layer(ImageId(1)).unwrap()
        ));
        assert!(Arc::ptr_eq(bank.get(ImageId(0)).unwrap(), &original));
        assert_eq!(
            bank.get(ImageId(0)).unwrap().pixels(),
            &[0, 0, 0, 255, 1, 0, 0, 127]
        );
        assert_eq!(
            bank.get_layer(ImageId(1)).unwrap().pixels(),
            &[0, 0, 0, 0, 1, 0, 0, 127]
        );
        assert!(Arc::ptr_eq(bank.get_layer(ImageId(3)).unwrap(), &unchanged));
        assert!(bank.get_layer(ImageId(0)).is_none());
        assert!(bank.get_layer(ImageId(4)).is_none());
        let snapshot = bank.clone();
        assert!(Arc::ptr_eq(
            snapshot.get_layer(ImageId(1)).unwrap(),
            bank.get_layer(ImageId(1)).unwrap()
        ));
    }
    #[test]
    fn declared_channel_filter_has_no_raw_fallback_and_noop_needs_no_extra_budget() {
        let chart = beatkernel_bms::parse(
            "#BMP01 same.png\n#BMP02 same.png\n#BMP03 same.png\n#00004:01\n#00007:02\n#00006:03",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let original = raw(vec![0, 0, 0, 0]);
        let mut bank = ImageAssets::default();
        for id in [1, 2, 3] {
            bank.images.insert(ImageId(id), original.clone());
        }
        bank.decoded_bytes = 4;
        bank.unique_images = 1;
        let ids = chart
            .bga
            .iter()
            .filter(|event| matches!(event.channel, BgaChannel::Layer | BgaChannel::Layer2))
            .map(|event| event.image)
            .collect();
        let (layers, total) = bank.keyed_layers(&ids, 4).unwrap();
        bank.layers = layers;
        bank.decoded_bytes = total;
        assert!(bank.get(ImageId(1)).is_some());
        assert!(bank.get_layer(ImageId(1)).is_none());
        assert!(bank.get_layer(ImageId(3)).is_none());
        assert!(Arc::ptr_eq(bank.get_layer(ImageId(2)).unwrap(), &original));
        assert_eq!(bank.decoded_bytes(), 4);
    }
    #[test]
    fn layer_and_second_layer_canonical_alias_share_one_changed_variant_and_budget() {
        let chart = beatkernel_bms::parse(
            "#BMP01 same.png\n#BMP02 ./same.png\n#BMP03 same.png\n#00007:01\n#0000A:02\n#00006:03",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let original = raw(vec![0, 0, 0, 255, 0, 0, 1, 64]);
        let mut bank = ImageAssets::default();
        for id in [1, 2, 3] {
            bank.images.insert(ImageId(id), original.clone());
        }
        bank.decoded_bytes = 8;
        bank.unique_images = 1;
        let ids = chart
            .bga
            .iter()
            .filter(|event| matches!(event.channel, BgaChannel::Layer | BgaChannel::Layer2))
            .map(|event| event.image)
            .collect();
        assert!(bank.keyed_layers(&ids, 15).is_err());
        assert!(bank.layers.is_empty());
        assert_eq!(bank.decoded_bytes(), 8);
        let (layers, total) = bank.keyed_layers(&ids, 16).unwrap();
        bank.layers = layers;
        bank.decoded_bytes = total;
        assert_eq!(bank.decoded_bytes(), 16);
        assert_eq!(bank.unique_images(), 1);
        assert!(Arc::ptr_eq(
            bank.get_layer(ImageId(1)).unwrap(),
            bank.get_layer(ImageId(2)).unwrap()
        ));
        assert_eq!(
            bank.get_layer(ImageId(2)).unwrap().pixels(),
            &[0, 0, 0, 0, 0, 0, 1, 64]
        );
        assert!(bank.get_layer(ImageId(3)).is_none());
        assert_eq!(
            bank.get(ImageId(3)).unwrap().pixels(),
            &[0, 0, 0, 255, 0, 0, 1, 64]
        );
    }
}
