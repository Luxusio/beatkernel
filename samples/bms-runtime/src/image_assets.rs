//! Contained static visual resources, prepared explicitly before playback.
use crate::{
    asset_paths::{AssetPathPolicy, resolve_asset},
    image_decode::{ImageDecodeError, ImageDecodeLimits, decode},
    texture::RgbaImage,
};
use beatkernel_bms::{BmsChart, ImageId};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Largest admitted number of base36 image references, including BMP00.
pub const MAX_IMAGE_REFERENCES: usize = 36 * 36;
/// Hard ceiling for retained decoded image data; decoder scratch is additional.
pub const MAX_IMAGE_BANK_BYTES: u64 = 256 * 1024 * 1024;

/// Configurable preparation bounds, independent of concurrent GPU texture slots.
#[derive(Clone, Copy, Debug)]
pub struct ImageAssetLimits {
    /// Maximum referenced image IDs, including unavailable resources.
    pub max_images: usize,
    /// Maximum bytes in unique retained RGBA images, counted once per file.
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
    /// The literal contained asset is absent; no extension substitution was tried.
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
    images: BTreeMap<ImageId, Arc<RgbaImage>>,
    unavailable: BTreeMap<ImageId, ImageUnavailable>,
    decoded_bytes: u64,
    unique_images: usize,
}

#[derive(Clone)]
enum Cached {
    Loaded(Arc<RgbaImage>),
    Unavailable(ImageUnavailable),
}
enum ReadError {
    Io(io::Error),
    Limit(String),
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, ReadError> {
    let mut file = File::open(path).map_err(ReadError::Io)?;
    if file.metadata().map_err(ReadError::Io)?.len() > limit as u64 {
        return Err(ReadError::Limit("encoded image file exceeds limit".into()));
    }
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    loop {
        let count = file.read(&mut block).map_err(ReadError::Io)?;
        if count == 0 {
            break;
        }
        if bytes.len().checked_add(count).is_none_or(|len| len > limit) {
            return Err(ReadError::Limit("encoded image file exceeds limit".into()));
        }
        bytes
            .try_reserve(count)
            .map_err(|e| ReadError::Limit(e.to_string()))?;
        bytes.extend_from_slice(&block[..count]);
    }
    Ok(bytes)
}

impl ImageAssets {
    /// Loads only referenced images and defined BMP00 using exact contained paths.
    /// Bad/missing raster data is unavailable; unsafe IO and exhausted limits
    /// reject the entire preparation. Filesystem stability is assumed, as for audio.
    pub fn prepare(
        root: &Path,
        chart: &BmsChart,
        limits: ImageAssetLimits,
    ) -> Result<Self, String> {
        limits.validate()?;
        if chart.bga.len() > beatkernel::chart::MAX_SOURCE_ITEMS {
            return Err("visual source item capacity exceeded".into());
        }
        let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        if !std::fs::metadata(&root)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return Err("image root must be a directory".into());
        }
        let mut references: BTreeSet<_> = chart.bga.iter().map(|event| event.image).collect();
        if chart.images.contains_key(&ImageId(0)) {
            references.insert(ImageId(0));
        }
        if references.len() > limits.max_images
            || references
                .iter()
                .any(|id| usize::from(id.0) >= MAX_IMAGE_REFERENCES)
        {
            return Err("image reference capacity exceeded".into());
        }
        let mut bank = Self::default();
        let mut cache = BTreeMap::<PathBuf, Cached>::new();
        for id in references {
            let Some(name) = chart.images.get(&id) else {
                bank.unavailable.insert(id, ImageUnavailable::Undefined);
                continue;
            };
            let path = match resolve_asset(&root, name, AssetPathPolicy::Exact) {
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
                let encoded = match read_bounded(&path, limits.decode.max_encoded_bytes) {
                    Ok(encoded) => encoded,
                    Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                        bank.unavailable.insert(id, ImageUnavailable::Missing);
                        continue;
                    }
                    Err(ReadError::Io(e)) => {
                        return Err(format!("image read {}: {e}", path.display()));
                    }
                    Err(ReadError::Limit(e)) => return Err(e),
                };
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
        Ok(bank)
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
    /// Retained RGBA byte count, excluding codec scratch and encoded inputs.
    pub fn decoded_bytes(&self) -> u64 {
        self.decoded_bytes
    }
}
