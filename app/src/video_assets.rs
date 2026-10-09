//! Movie registrations and off-thread frame transforms; no decoder or GPU ownership.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::AssetSource,
    image_crop::crop_canvas_sized,
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

pub fn is_movie_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            [
                "mp4", "m4v", "mov", "webm", "mkv", "avi", "mpg", "mpeg", "wmv",
            ]
            .iter()
            .any(|movie| extension.eq_ignore_ascii_case(movie))
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoAssetLimits {
    pub max_assets: usize,
    pub max_encoded_file_bytes: usize,
    pub max_encoded_total_bytes: u64,
    pub max_dimension: u32,
    /// Bounds a frame and all uniquely retained raw/keyed transform outputs.
    pub max_frame_bytes: u64,
}
impl Default for VideoAssetLimits {
    fn default() -> Self {
        Self {
            max_assets: 3844,
            max_encoded_file_bytes: 64 * 1024 * 1024,
            max_encoded_total_bytes: 256 * 1024 * 1024,
            max_dimension: 16384,
            max_frame_bytes: 64 * 1024 * 1024,
        }
    }
}
impl VideoAssetLimits {
    pub fn validate(self) -> Result<(), String> {
        if self.max_assets == 0
            || self.max_assets > 3844
            || self.max_encoded_file_bytes == 0
            || self.max_encoded_file_bytes > isize::MAX as usize
            || self.max_encoded_total_bytes == 0
            || self.max_encoded_file_bytes as u64 > self.max_encoded_total_bytes
            || self.max_dimension == 0
            || self.max_dimension > 16384
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > crate::texture::MAX_TEXTURE_BYTES
        {
            return Err("video asset limits invalid".into());
        }
        Ok(())
    }
    /// Decoder adapters must call this with the header extent before allocating pixels.
    pub fn validate_frame(self, width: u32, height: u32) -> Result<u64, String> {
        self.validate()?;
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("video frame extent overflow")?;
        if width == 0
            || height == 0
            || width > self.max_dimension
            || height > self.max_dimension
            || bytes > self.max_frame_bytes
        {
            return Err("video frame extent or byte limit exceeded".into());
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
pub enum VideoResource {
    Encoded(Arc<[u8]>),
    #[cfg(not(target_arch = "wasm32"))]
    File(PathBuf),
}
#[derive(Clone, Debug)]
pub struct VideoResourceDescriptor {
    pub data: VideoResource,
    pub encoded_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoUnavailable {
    Missing,
    Unsupported(String),
    InvalidData(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoTransform {
    pub crop: Option<BgaCrop>,
    pub canvas: Option<[u32; 2]>,
    pub keyed: bool,
}
impl VideoTransform {
    fn validate(self, limits: VideoAssetLimits) -> Result<(), String> {
        limits.validate()?;
        if let Some(crop) = self.crop {
            crop.validate().map_err(|error| error.to_string())?;
            if usize::from(crop.source.0) >= 3844 {
                return Err("video crop source invalid".into());
            }
            if self.canvas.is_none() {
                return Err("video crop requires a canvas".into());
            }
        }
        if let Some([width, height]) = self.canvas {
            limits.validate_frame(width, height)?;
        }
        Ok(())
    }
    /// Call on the decode/preparation owner, before publishing immutable pixels.
    pub fn apply(
        self,
        source: &Arc<RgbaImage>,
        limits: VideoAssetLimits,
    ) -> Result<VideoFrameVariants, String> {
        self.validate(limits)?;
        limits.validate_frame(source.width(), source.height())?;
        let extent = [source.width(), source.height()];
        let crop = self.crop.or_else(|| {
            self.canvas
                .filter(|canvas| *canvas != extent)
                .map(|_| BgaCrop {
                    source: ImageId(0),
                    source_rect: [0, 0, source.width() as i32, source.height() as i32],
                    destination: [0, 0],
                })
        });
        let raw = if let Some(crop) = crop {
            let canvas = self.canvas.ok_or("video crop requires a canvas")?;
            limits.validate_frame(canvas[0], canvas[1])?;
            crop_canvas_sized(source, crop, canvas)?
        } else {
            Arc::clone(source)
        };
        let retained_bytes = if self.keyed && needs_key(&raw) {
            raw.byte_len()
                .checked_mul(2)
                .filter(|bytes| *bytes <= limits.max_frame_bytes)
                .ok_or("video transformed frame byte budget exceeded")?
        } else {
            raw.byte_len()
        };
        let layer = if self.keyed {
            Some(black_to_transparent(&raw)?)
        } else {
            None
        };
        Ok(VideoFrameVariants {
            raw,
            layer,
            retained_bytes,
        })
    }
}
pub struct VideoFrameVariants {
    pub raw: Arc<RgbaImage>,
    pub layer: Option<Arc<RgbaImage>>,
    pub retained_bytes: u64,
}
impl VideoFrameVariants {
    pub fn get(&self, channel: BgaChannel) -> Option<&Arc<RgbaImage>> {
        match channel {
            BgaChannel::Base | BgaChannel::Poor => Some(&self.raw),
            BgaChannel::Layer | BgaChannel::Layer2 => self.layer.as_ref(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoDescriptor {
    pub resource: usize,
    pub transform: VideoTransform,
}
#[derive(Clone, Default)]
pub struct VideoAssets {
    resources: Vec<VideoResourceDescriptor>,
    images: BTreeMap<ImageId, VideoDescriptor>,
    unavailable: BTreeMap<ImageId, VideoUnavailable>,
    encoded_bytes: u64,
}
#[derive(Clone)]
pub struct VideoAssetsTransfer {
    pub resources: Vec<Arc<[u8]>>,
    pub images: Vec<(ImageId, VideoDescriptor)>,
    pub unavailable: Vec<(ImageId, VideoUnavailable)>,
}

impl VideoAssets {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare(
        root: &Path,
        chart: &BmsChart,
        limits: VideoAssetLimits,
    ) -> Result<Self, String> {
        // Validate the chart before any native IO.
        let plan = Self::plan(chart, limits)?;
        let source =
            crate::asset_source::FileAssetSource::new(root).map_err(|error| error.to_string())?;
        Self::prepare_plan(&source, plan, limits, |path, maximum| {
            let bytes = std::fs::metadata(path)?.len();
            if bytes > maximum as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "encoded video file exceeds limit",
                ));
            }
            Ok(VideoResourceDescriptor {
                data: VideoResource::File(path.to_owned()),
                encoded_bytes: bytes,
            })
        })
    }
    pub fn prepare_from_source(
        source: &dyn AssetSource,
        chart: &BmsChart,
        limits: VideoAssetLimits,
    ) -> Result<Self, String> {
        let plan = Self::plan(chart, limits)?;
        Self::prepare_plan(source, plan, limits, |path, maximum| {
            let bytes = source.read(path, maximum)?;
            if bytes.len() > maximum {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "encoded video file exceeds limit",
                ));
            }
            Ok(VideoResourceDescriptor {
                encoded_bytes: bytes.len() as u64,
                data: VideoResource::Encoded(Arc::from(bytes.as_ref())),
            })
        })
    }
    fn plan(
        chart: &BmsChart,
        limits: VideoAssetLimits,
    ) -> Result<Vec<(ImageId, String, VideoTransform)>, String> {
        limits.validate()?;
        let explicit_canvas = chart.canvas_size().map_err(|error| error.to_string())?;
        if let Some([width, height]) = explicit_canvas {
            limits.validate_frame(width, height)?;
        }
        if chart
            .bga
            .len()
            .checked_add(chart.bga_opacity.len())
            .is_none_or(|count| count > beatkernel::chart::MAX_SOURCE_ITEMS)
            || chart.bga_crops.len() > 3844
        {
            return Err("video source item capacity exceeded".into());
        }
        for (&id, crop) in &chart.bga_crops {
            if usize::from(id.0) >= 3844 || usize::from(crop.source.0) >= 3844 {
                return Err("video crop identity invalid".into());
            }
            crop.validate().map_err(|error| error.to_string())?;
        }
        let mut references: BTreeSet<_> = chart.bga.iter().map(|event| event.image).collect();
        if chart.images.contains_key(&ImageId(0)) || chart.bga_crops.contains_key(&ImageId(0)) {
            references.insert(ImageId(0));
        }
        let sources: BTreeSet<_> = references
            .iter()
            .map(|id| chart.bga_crops.get(id).map_or(*id, |crop| crop.source))
            .collect();
        if references.union(&sources).count() > limits.max_assets
            || references
                .union(&sources)
                .any(|id| usize::from(id.0) >= 3844)
        {
            return Err("video reference capacity exceeded".into());
        }
        let layers: BTreeSet<_> = chart
            .bga
            .iter()
            .filter(|event| matches!(event.channel, BgaChannel::Layer | BgaChannel::Layer2))
            .map(|event| event.image)
            .collect();
        let mut plan = Vec::new();
        for id in references {
            let crop = chart.bga_crops.get(&id).copied();
            let source = crop.map_or(id, |crop| crop.source);
            let Some(name) = chart.images.get(&source).filter(|name| is_movie_name(name)) else {
                continue;
            };
            let transform = VideoTransform {
                crop,
                canvas: if crop.is_some() {
                    Some(explicit_canvas.unwrap_or([256, 256]))
                } else {
                    explicit_canvas
                },
                keyed: layers.contains(&id),
            };
            transform.validate(limits)?;
            plan.push((id, name.clone(), transform));
        }
        Ok(plan)
    }
    fn prepare_plan(
        source: &dyn AssetSource,
        plan: Vec<(ImageId, String, VideoTransform)>,
        limits: VideoAssetLimits,
        mut load: impl FnMut(&Path, usize) -> io::Result<VideoResourceDescriptor>,
    ) -> Result<Self, String> {
        let mut bank = Self::default();
        let mut cache = BTreeMap::<PathBuf, usize>::new();
        for (id, name, transform) in plan {
            let path = match source.resolve(&name, AssetPathPolicy::VideoVariants) {
                Ok(path) => path,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    bank.unavailable.insert(id, VideoUnavailable::Missing);
                    continue;
                }
                Err(error) => return Err(format!("video {} path {name}: {error}", id.0)),
            };
            if !path.to_str().is_some_and(is_movie_name) {
                bank.unavailable.insert(
                    id,
                    VideoUnavailable::Unsupported(
                        "resolved resource is not a movie extension".into(),
                    ),
                );
                continue;
            }
            let resource = if let Some(&index) = cache.get(&path) {
                index
            } else {
                let remaining = limits.max_encoded_total_bytes - bank.encoded_bytes;
                let maximum = limits
                    .max_encoded_file_bytes
                    .min(usize::try_from(remaining).unwrap_or(usize::MAX));
                let resource = match load(&path, maximum) {
                    Ok(resource) => resource,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        bank.unavailable.insert(id, VideoUnavailable::Missing);
                        continue;
                    }
                    Err(error) => return Err(format!("video read {}: {error}", path.display())),
                };
                bank.encoded_bytes = bank
                    .encoded_bytes
                    .checked_add(resource.encoded_bytes)
                    .filter(|total| *total <= limits.max_encoded_total_bytes)
                    .ok_or("encoded video total byte budget exceeded")?;
                let index = bank.resources.len();
                bank.resources.push(resource);
                cache.insert(path, index);
                index
            };
            bank.images.insert(
                id,
                VideoDescriptor {
                    resource,
                    transform,
                },
            );
        }
        Ok(bank)
    }
    pub fn get(&self, image: ImageId) -> Option<&VideoDescriptor> {
        self.images.get(&image)
    }
    pub fn unavailable(&self, image: ImageId) -> Option<&VideoUnavailable> {
        self.unavailable.get(&image)
    }
    pub fn resource(&self, index: usize) -> Option<&VideoResourceDescriptor> {
        self.resources.get(index)
    }
    pub fn resources(&self) -> &[VideoResourceDescriptor] {
        &self.resources
    }
    pub fn len(&self) -> usize {
        self.images.len() + self.unavailable.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn encoded_bytes(&self) -> u64 {
        self.encoded_bytes
    }
    /// Portable registration never serializes native file locators.
    pub fn export_video(&self) -> Result<VideoAssetsTransfer, String> {
        let resources = self
            .resources
            .iter()
            .map(|resource| match &resource.data {
                VideoResource::Encoded(bytes) => Ok(Arc::clone(bytes)),
                #[cfg(not(target_arch = "wasm32"))]
                VideoResource::File(_) => {
                    Err("native video locator cannot be exported as browser bytes".into())
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(VideoAssetsTransfer {
            resources,
            images: self
                .images
                .iter()
                .map(|(&id, &value)| (id, value))
                .collect(),
            unavailable: self
                .unavailable
                .iter()
                .map(|(&id, value)| (id, value.clone()))
                .collect(),
        })
    }
    pub fn import_video(
        data: VideoAssetsTransfer,
        limits: VideoAssetLimits,
    ) -> Result<Self, String> {
        limits.validate()?;
        if data
            .images
            .len()
            .checked_add(data.unavailable.len())
            .is_none_or(|count| count > limits.max_assets)
            || data.resources.len() > limits.max_assets
        {
            return Err("video transfer reference capacity exceeded".into());
        }
        let mut bank = Self::default();
        let mut allocations = BTreeSet::new();
        for bytes in data.resources {
            if bytes.len() > limits.max_encoded_file_bytes
                || !allocations.insert(Arc::as_ptr(&bytes) as *const u8)
            {
                return Err("video transfer encoded resource invalid".into());
            }
            bank.encoded_bytes = bank
                .encoded_bytes
                .checked_add(bytes.len() as u64)
                .filter(|total| *total <= limits.max_encoded_total_bytes)
                .ok_or("encoded video total byte budget exceeded")?;
            bank.resources.push(VideoResourceDescriptor {
                encoded_bytes: bytes.len() as u64,
                data: VideoResource::Encoded(bytes),
            });
        }
        let mut ids = BTreeSet::new();
        for (id, descriptor) in data.images {
            if usize::from(id.0) >= 3844
                || !ids.insert(id)
                || descriptor.resource >= bank.resources.len()
            {
                return Err("video transfer alias invalid".into());
            }
            descriptor.transform.validate(limits)?;
            bank.images.insert(id, descriptor);
        }
        for (id, reason) in data.unavailable {
            if usize::from(id.0) >= 3844 || !ids.insert(id) {
                return Err("video transfer unavailable identity invalid".into());
            }
            bank.unavailable.insert(id, reason);
        }
        let dependencies: BTreeSet<_> = bank
            .images
            .iter()
            .map(|(&id, descriptor)| descriptor.transform.crop.map_or(id, |crop| crop.source))
            .collect();
        if ids.union(&dependencies).count() > limits.max_assets {
            return Err("video transfer dependency capacity exceeded".into());
        }
        let used: BTreeSet<_> = bank
            .images
            .values()
            .map(|descriptor| descriptor.resource)
            .collect();
        if used.len() != bank.resources.len() {
            return Err("video transfer contains unused resource".into());
        }
        Ok(bank)
    }
}
