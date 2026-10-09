//! AC-003 resource acquisition and independent pixel goldens, without codec claims.
use beatkernel_bms::{BgaChannel, BgaCrop, ImageId, ParseOptions};
use beatkernel_bms_runtime::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, FileAssetSource, MemoryAssetLimits, MemoryFiles},
    texture::RgbaImage,
    video_assets::{
        is_movie_name, VideoAssetLimits, VideoAssets, VideoResource, VideoTransform,
        VideoUnavailable,
    },
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

// Recognizable encoded container bytes for acquisition only; no decoder is invoked.
const MOVIE: &[u8] = b"\0\0\0\x18ftypisom\0\0\0\0isommp42original-payload";

fn chart(text: &str) -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(text, ParseOptions::default()).unwrap()
}
fn files(entries: &[(&str, &[u8])]) -> MemoryFiles {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert("song/chart.bms", b"#BPM 120".to_vec())
        .unwrap();
    for (name, bytes) in entries {
        files
            .insert(&format!("song/{name}"), bytes.to_vec())
            .unwrap();
    }
    files
}
fn transform(crop: Option<BgaCrop>, canvas: Option<[u32; 2]>, keyed: bool) -> VideoTransform {
    VideoTransform {
        crop,
        canvas,
        keyed,
    }
}
fn pixels(width: u32, height: u32, bytes: &[u8]) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(width, height, bytes.to_vec()).unwrap())
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-video-assets-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("temporary directory: {e}"),
            }
        }
    }
    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).unwrap();
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn movie_classification_is_case_insensitive_and_excludes_static_extensions() {
    for extension in [
        "mp4", "m4v", "mov", "webm", "mkv", "avi", "mpg", "mpeg", "wmv",
    ] {
        assert!(is_movie_name(&format!("dir/movie.{extension}")));
        assert!(is_movie_name(&format!(
            "dir/movie.{}",
            extension.to_ascii_uppercase()
        )));
    }
    for name in ["a.bmp", "a.png", "a.jpeg", "a.mp4.png", "a", "mp4"] {
        assert!(!is_movie_name(name), "{name}");
    }
}

#[test]
fn static_image_preparation_never_resolves_or_reads_declared_movies() {
    struct NoMovieIo;
    impl AssetSource for NoMovieIo {
        fn resolve(&self, name: &str, _: AssetPathPolicy) -> std::io::Result<PathBuf> {
            panic!("static image loader must not resolve movie {name}");
        }
        fn read<'a>(
            &'a self,
            key: &std::path::Path,
            _: usize,
        ) -> std::io::Result<std::borrow::Cow<'a, [u8]>> {
            panic!("static image loader must not read movie {}", key.display());
        }
    }
    let chart = chart("#BPM 120\n#BMP01 clip.MP4\n#00104:01\n");
    let assets = beatkernel_bms_runtime::image_assets::ImageAssets::prepare_from_source(
        &NoMovieIo,
        &chart,
        Default::default(),
    )
    .unwrap();
    assert!(assets.get(ImageId(1)).is_none());
    assert_eq!(assets.unique_images(), 0);
    assert_eq!(assets.decoded_bytes(), 0);
    assert_eq!(
        assets.unavailable(ImageId(1)),
        Some(&beatkernel_bms_runtime::image_assets::ImageUnavailable::Unsupported)
    );
}

#[test]
fn memory_movie_lookup_prefers_literal_then_case_and_movie_family() {
    let files = files(&[
        ("clip.MP4", b"literal"),
        ("clip.mp4", MOVIE),
        ("other.WeBm", MOVIE),
    ]);
    let source = files.scope("song/chart.bms").unwrap();
    assert_eq!(
        source
            .resolve("clip.MP4", AssetPathPolicy::VideoVariants)
            .unwrap(),
        PathBuf::from("song/clip.MP4")
    );
    assert_eq!(
        source
            .resolve("clip.mP4", AssetPathPolicy::VideoVariants)
            .unwrap(),
        PathBuf::from("song/clip.mp4")
    );
    assert_eq!(
        source
            .resolve("other.avi", AssetPathPolicy::VideoVariants)
            .unwrap(),
        PathBuf::from("song/other.WeBm")
    );
}

#[test]
fn movie_and_static_variant_families_do_not_cross_and_static_behavior_survives() {
    let files = files(&[
        ("still.PNG", b"static"),
        ("movie.MOV", MOVIE),
        ("lost.bmp", b"static"),
    ]);
    let source = files.scope("song/chart.bms").unwrap();
    assert_eq!(
        source
            .resolve("still.bmp", AssetPathPolicy::ImageVariants)
            .unwrap(),
        PathBuf::from("song/still.PNG")
    );
    assert!(source
        .resolve("still.mp4", AssetPathPolicy::VideoVariants)
        .is_err());
    assert!(source
        .resolve("movie.bmp", AssetPathPolicy::ImageVariants)
        .is_err());
    assert!(source
        .resolve("lost.mp4", AssetPathPolicy::VideoVariants)
        .is_err());
}

#[test]
fn browser_aliases_share_original_compressed_payload_and_one_byte_charge() {
    let files = files(&[("clip.mp4", MOVIE)]);
    let source = files.scope("song/chart.bms").unwrap();
    let chart = chart("#BMP00 clip.mp4\n#BMP01 ./clip.mp4\n#BMP02 clip.mp4\n#00004:01\n#00007:02");
    let limits = VideoAssetLimits {
        max_encoded_file_bytes: MOVIE.len(),
        max_encoded_total_bytes: MOVIE.len() as u64,
        ..VideoAssetLimits::default()
    };
    let bank = VideoAssets::prepare_from_source(&source, &chart, limits).unwrap();
    assert_eq!(bank.len(), 3);
    assert_eq!(bank.resources().len(), 1);
    assert_eq!(bank.encoded_bytes(), MOVIE.len() as u64);
    let index = bank.get(ImageId(0)).unwrap().resource;
    assert_eq!(bank.get(ImageId(1)).unwrap().resource, index);
    assert_eq!(bank.get(ImageId(2)).unwrap().resource, index);
    match &bank.resource(index).unwrap().data {
        VideoResource::Encoded(bytes) => assert_eq!(&**bytes, MOVIE),
        _ => panic!("selected-file resources must own compressed bytes"),
    }
    let packet = bank.export_video().unwrap();
    assert_eq!(packet.resources.len(), 1);
    match &bank.resource(index).unwrap().data {
        VideoResource::Encoded(bytes) => assert!(Arc::ptr_eq(bytes, &packet.resources[0])),
        _ => unreachable!(),
    }
    let imported = VideoAssets::import_video(packet, limits).unwrap();
    assert_eq!(imported.encoded_bytes(), MOVIE.len() as u64);
    assert_eq!(imported.resources().len(), 1);
}

#[test]
fn missing_movie_is_explicit_and_static_references_stay_out_of_movie_table() {
    let files = files(&[("lost.bmp", b"BMP"), ("still.png", b"PNG")]);
    let bank = VideoAssets::prepare_from_source(
        &files.scope("song/chart.bms").unwrap(),
        &chart("#BMP01 lost.mp4\n#BMP02 still.png\n#00004:0102"),
        VideoAssetLimits::default(),
    )
    .unwrap();
    assert!(bank.get(ImageId(1)).is_none());
    assert!(matches!(
        bank.unavailable(ImageId(1)),
        Some(VideoUnavailable::Missing)
    ));
    assert!(bank.get(ImageId(2)).is_none());
    assert!(bank.unavailable(ImageId(2)).is_none());
    assert_eq!(bank.len(), 1);
    assert!(bank.resources().is_empty());
}

#[test]
fn movie_crop_dependency_and_canvas_are_retained_even_without_source_event() {
    let files = files(&[("clip.mp4", MOVIE)]);
    let bank = VideoAssets::prepare_from_source(
        &files.scope("song/chart.bms").unwrap(),
        &chart("#BMP01 clip.mp4\n#BGA02 01 0 0 1 2 1 0\n#CANVASSIZE 3 2\n#00007:02"),
        VideoAssetLimits::default(),
    )
    .unwrap();
    let descriptor = bank.get(ImageId(2)).unwrap();
    assert_eq!(descriptor.transform.canvas, Some([3, 2]));
    let crop = descriptor.transform.crop.unwrap();
    assert_eq!(crop.source, ImageId(1));
    assert_eq!(crop.source_rect, [0, 0, 1, 2]);
    assert_eq!(crop.destination, [1, 0]);
    assert!(descriptor.transform.keyed);
    assert_eq!(bank.resources().len(), 1);
}

#[test]
fn encoded_file_total_and_asset_limits_reject_actual_unique_inputs() {
    let files = files(&[("a.mp4", MOVIE), ("b.mp4", MOVIE)]);
    let source = files.scope("song/chart.bms").unwrap();
    let chart = chart("#BMP01 a.mp4\n#BMP02 b.mp4\n#00004:0102");
    for limits in [
        VideoAssetLimits {
            max_encoded_file_bytes: MOVIE.len() - 1,
            ..VideoAssetLimits::default()
        },
        VideoAssetLimits {
            max_encoded_total_bytes: MOVIE.len() as u64,
            max_encoded_file_bytes: MOVIE.len(),
            ..VideoAssetLimits::default()
        },
        VideoAssetLimits {
            max_assets: 1,
            ..VideoAssetLimits::default()
        },
    ] {
        assert!(VideoAssets::prepare_from_source(&source, &chart, limits).is_err());
    }
}

#[test]
fn native_lookup_and_registration_keep_canonical_file_descriptors() {
    let directory = Directory::new();
    directory.write("clip.Mp4", MOVIE);
    let source = FileAssetSource::new(&directory.0).unwrap();
    let canonical = std::fs::canonicalize(directory.0.join("clip.Mp4")).unwrap();
    assert_eq!(
        source
            .resolve("./clip.avi", AssetPathPolicy::VideoVariants)
            .unwrap(),
        canonical
    );
    let bank = VideoAssets::prepare(
        &directory.0,
        &chart("#BMP01 clip.mp4\n#BMP02 ./clip.avi\n#00004:0102"),
        VideoAssetLimits::default(),
    )
    .unwrap();
    assert_eq!(bank.resources().len(), 1);
    assert_eq!(
        bank.get(ImageId(1)).unwrap().resource,
        bank.get(ImageId(2)).unwrap().resource
    );
    let resource = &bank.resources()[0];
    assert_eq!(resource.encoded_bytes, MOVIE.len() as u64);
    match &resource.data {
        VideoResource::File(path) => assert_eq!(path, &canonical),
        _ => panic!("native registration must retain a path instead of copying the movie"),
    }
    assert!(bank.export_video().is_err());
}

#[cfg(unix)]
#[test]
fn native_canonical_aliases_share_identity_and_nonmovie_target_is_unsupported() {
    use std::os::unix::fs::symlink;
    let directory = Directory::new();
    directory.write("clip.mp4", MOVIE);
    directory.write("still.png", b"static");
    symlink("clip.mp4", directory.0.join("alias.mov")).unwrap();
    symlink("still.png", directory.0.join("unsupported.mp4")).unwrap();
    let bank = VideoAssets::prepare(
        &directory.0,
        &chart("#BMP01 clip.mp4\n#BMP02 alias.mov\n#BMP03 unsupported.mp4\n#00004:010203"),
        VideoAssetLimits::default(),
    )
    .unwrap();
    assert_eq!(
        bank.get(ImageId(1)).unwrap().resource,
        bank.get(ImageId(2)).unwrap().resource
    );
    assert_eq!(bank.resources().len(), 1);
    assert!(
        matches!(bank.unavailable(ImageId(3)), Some(VideoUnavailable::Unsupported(reason)) if !reason.is_empty())
    );
}

#[test]
fn raw_base_poor_and_exact_black_keyed_layers_preserve_independent_pixels() {
    let original = pixels(3, 1, &[0, 0, 0, 255, 0, 0, 1, 127, 0, 0, 0, 0]);
    let variants = transform(None, None, true)
        .apply(&original, VideoAssetLimits::default())
        .unwrap();
    assert!(Arc::ptr_eq(&variants.raw, &original));
    assert_eq!(
        variants.get(BgaChannel::Base).unwrap().pixels(),
        original.pixels()
    );
    assert!(Arc::ptr_eq(
        variants.get(BgaChannel::Poor).unwrap(),
        &original
    ));
    let layer = variants.get(BgaChannel::Layer).unwrap();
    assert_eq!(layer.pixels(), &[0, 0, 0, 0, 0, 0, 1, 127, 0, 0, 0, 0]);
    assert!(!Arc::ptr_eq(layer, &original));
    assert!(Arc::ptr_eq(
        layer,
        variants.get(BgaChannel::Layer2).unwrap()
    ));
    assert_eq!(original.pixels(), &[0, 0, 0, 255, 0, 0, 1, 127, 0, 0, 0, 0]);
    assert_eq!(variants.retained_bytes, 24);
}

#[test]
fn no_key_change_reuses_data_and_undeclared_layers_have_no_raw_fallback() {
    let original = pixels(2, 1, &[0, 0, 0, 0, 1, 0, 0, 255]);
    let variants = transform(None, None, true)
        .apply(&original, VideoAssetLimits::default())
        .unwrap();
    assert!(Arc::ptr_eq(&variants.raw, variants.layer.as_ref().unwrap()));
    assert_eq!(variants.retained_bytes, 8);
    let raw = transform(None, None, false)
        .apply(&original, VideoAssetLimits::default())
        .unwrap();
    assert!(raw.get(BgaChannel::Layer).is_none());
    assert!(raw.get(BgaChannel::Layer2).is_none());
    assert_eq!(raw.retained_bytes, 8);
}

#[test]
fn crop_canvas_and_key_have_literal_pixel_goldens() {
    let original = pixels(
        2,
        2,
        &[0, 0, 0, 255, 7, 8, 9, 127, 10, 11, 12, 64, 13, 14, 15, 255],
    );
    let crop = BgaCrop {
        source: ImageId(1),
        source_rect: [0, 0, 1, 2],
        destination: [1, 0],
    };
    let variants = transform(Some(crop), Some([3, 2]), true)
        .apply(&original, VideoAssetLimits::default())
        .unwrap();
    assert_eq!((variants.raw.width(), variants.raw.height()), (3, 2));
    assert_eq!(
        variants.raw.pixels(),
        &[0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 10, 11, 12, 64, 0, 0, 0, 0]
    );
    assert_eq!(
        variants.layer.unwrap().pixels(),
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 11, 12, 64, 0, 0, 0, 0]
    );
    assert_eq!(variants.retained_bytes, 48);
    assert_eq!(
        original.pixels(),
        &[0, 0, 0, 255, 7, 8, 9, 127, 10, 11, 12, 64, 13, 14, 15, 255]
    );
}

#[test]
fn canvas_only_and_negative_crop_clipping_use_transparent_padding() {
    let original = pixels(
        2,
        2,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    );
    let resized = transform(None, Some([3, 1]), false)
        .apply(&original, VideoAssetLimits::default())
        .unwrap();
    assert_eq!(resized.raw.pixels(), &[1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0]);
    let crop = BgaCrop {
        source: ImageId(1),
        source_rect: [-20, -30, 99, 99],
        destination: [-1, -1],
    };
    let clipped = transform(Some(crop), Some([2, 2]), false)
        .apply(&original, VideoAssetLimits::default())
        .unwrap();
    assert_eq!(
        clipped.raw.pixels(),
        &[13, 14, 15, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn transform_rejects_zero_oversized_extent_bad_crop_and_variant_budget() {
    let original = pixels(1, 1, &[0, 0, 0, 255]);
    let limits = VideoAssetLimits::default();
    for canvas in [[0, 1], [1, 0], [16385, 1], [u32::MAX, u32::MAX]] {
        assert!(transform(None, Some(canvas), false)
            .apply(&original, limits)
            .is_err());
    }
    let crop = BgaCrop {
        source: ImageId(1),
        source_rect: [2, 0, 1, 1],
        destination: [0, 0],
    };
    assert!(transform(Some(crop), Some([1, 1]), false)
        .apply(&original, limits)
        .is_err());
    assert!(transform(None, None, true)
        .apply(
            &original,
            VideoAssetLimits {
                max_frame_bytes: 7,
                ..limits
            }
        )
        .is_err());
    let exact = transform(None, None, true)
        .apply(
            &original,
            VideoAssetLimits {
                max_frame_bytes: 8,
                ..limits
            },
        )
        .unwrap();
    assert_eq!(exact.retained_bytes, 8);
    let wide = pixels(2, 1, &[1; 8]);
    assert!(transform(None, None, false)
        .apply(
            &wide,
            VideoAssetLimits {
                max_dimension: 1,
                ..limits
            }
        )
        .is_err());
    assert_eq!(original.pixels(), &[0, 0, 0, 255]);
}

#[test]
fn default_limits_and_zero_configuration_are_explicit() {
    let limits = VideoAssetLimits::default();
    assert_eq!(
        (
            limits.max_assets,
            limits.max_encoded_file_bytes,
            limits.max_encoded_total_bytes,
            limits.max_dimension,
            limits.max_frame_bytes
        ),
        (
            3844,
            64 * 1024 * 1024,
            256 * 1024 * 1024,
            16384,
            64 * 1024 * 1024
        )
    );
    assert!(limits.validate().is_ok());
    for invalid in [
        VideoAssetLimits {
            max_assets: 0,
            ..limits
        },
        VideoAssetLimits {
            max_encoded_file_bytes: 0,
            ..limits
        },
        VideoAssetLimits {
            max_encoded_total_bytes: 0,
            ..limits
        },
        VideoAssetLimits {
            max_dimension: 0,
            ..limits
        },
        VideoAssetLimits {
            max_frame_bytes: 0,
            ..limits
        },
    ] {
        assert!(invalid.validate().is_err());
    }
}
