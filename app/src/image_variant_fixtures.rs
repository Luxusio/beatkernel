//! Deferred public resolver and actual image-bank fixtures; no files are touched
//! until tests are explicitly executed. No GPU or platform presentation evidence.
use crate::{
    asset_paths::{AssetPathPolicy, resolve_asset},
    asset_source::{AssetSource, MemoryAssetLimits, MemoryAssetSource, MemoryFiles},
    image_assets::{ImageAssetLimits, ImageAssets, ImageUnavailable},
};
use beatkernel_bms::{ImageId, parse};
use std::{
    borrow::Cow,
    cell::RefCell,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

// Literal contract order, independent of the production candidate generator.
const CASES: [&str; 40] = [
    "bmp", "Bmp", "bMp", "BMp", "bmP", "BmP", "bMP", "BMP", "png", "Png", "pNg", "PNg", "pnG",
    "PnG", "pNG", "PNG", "jpg", "Jpg", "jPg", "JPg", "jpG", "JpG", "jPG", "JPG", "jpeg", "Jpeg",
    "jPeg", "JPeg", "jpEg", "JpEg", "jPEg", "JPEg", "jpeG", "JpeG", "jPeG", "JPeG", "jpEG", "JpEG",
    "jPEG", "JPEG",
];
fn memory() -> MemoryFiles {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.insert("pack/chart.bms", vec![]).unwrap();
    files
}
#[test]
fn actual_selected_file_lookup_pins_all_forty_candidates_and_recognized_family_priority() {
    for (index, &expected) in CASES.iter().enumerate() {
        let mut files = memory();
        for suffix in &CASES[index..] {
            files.insert(&format!("pack/絵.{suffix}"), vec![1]).unwrap();
        }
        let source = files.scope("pack/chart.bms").unwrap();
        assert_eq!(
            source
                .resolve("絵", AssetPathPolicy::ImageVariants)
                .unwrap(),
            PathBuf::from(format!("pack/絵.{expected}"))
        );
    }
    for (literal, first) in [
        ("BMP", "bmp"),
        ("PNG", "png"),
        ("JPG", "jpg"),
        ("JPEG", "jpeg"),
    ] {
        let mut files = memory();
        for suffix in CASES {
            if suffix != literal {
                files
                    .insert(&format!("pack/art.{suffix}"), vec![1])
                    .unwrap();
            }
        }
        let source = files.scope("pack/chart.bms").unwrap();
        assert_eq!(
            source
                .resolve(&format!("art.{literal}"), AssetPathPolicy::ImageVariants)
                .unwrap(),
            PathBuf::from(format!("pack/art.{first}"))
        );
    }
    for (reference, available, expected) in [
        ("art.jpg", &["bmp", "png", "JPG", "jpeg"][..], "JPG"),
        ("art.jpeg", &["jpg", "png", "bmp"][..], "bmp"),
        ("art.png", &["jpeg", "jpg", "bmp"][..], "bmp"),
        ("art", &["jpeg", "jpg", "png"][..], "png"),
        ("art", &["jpeg", "jpg"][..], "jpg"),
    ] {
        let mut files = memory();
        for suffix in available {
            files
                .insert(&format!("pack/art.{suffix}"), vec![1])
                .unwrap();
        }
        assert_eq!(
            files
                .scope("pack/chart.bms")
                .unwrap()
                .resolve(reference, AssetPathPolicy::ImageVariants)
                .unwrap(),
            PathBuf::from(format!("pack/art.{expected}"))
        );
    }
    let mut files = memory();
    files.insert("pack/art.PNG", vec![255]).unwrap();
    files.insert("pack/art.bmp", vec![1]).unwrap();
    assert_eq!(
        files
            .scope("pack/chart.bms")
            .unwrap()
            .resolve("art.PNG", AssetPathPolicy::ImageVariants)
            .unwrap(),
        Path::new("pack/art.PNG")
    );
}

struct Files {
    temporary: PathBuf,
    root: PathBuf,
}
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = std::env::temp_dir().join(format!(
            "beatkernel-image-variants-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&temporary).unwrap();
        let root = temporary.join("root");
        fs::create_dir(&root).unwrap();
        Self { temporary, root }
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn resolved(&self, name: &str) -> PathBuf {
        fs::canonicalize(self.root.join(name)).unwrap()
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temporary);
    }
}
#[test]
fn filesystem_literal_and_candidate_errors_never_search_around_nonfiles_or_unsafe_links() {
    let files = Files::new();
    files.write("日本/絵.PNG", b"damaged literal");
    files.write("日本/絵.bmp", &[1]);
    assert_eq!(
        resolve_asset(&files.root, "日本\\絵.PNG", AssetPathPolicy::ImageVariants).unwrap(),
        files.resolved("日本/絵.PNG")
    );
    assert_eq!(
        resolve_asset(&files.root, "日本/絵.jpg", AssetPathPolicy::ImageVariants).unwrap(),
        files.resolved("日本/絵.bmp")
    );
    assert!(resolve_asset(&files.root, "日本/絵.jpg", AssetPathPolicy::Exact).is_err());
    for suffix in ["mp4", "avi", "gif", "webp", "asset"] {
        let reference = format!("video.{suffix}");
        files.write("video.bmp", &[1]);
        assert_eq!(
            resolve_asset(&files.root, &reference, AssetPathPolicy::ImageVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        files.write(&reference, b"literal");
        assert_eq!(
            resolve_asset(&files.root, &reference, AssetPathPolicy::ImageVariants).unwrap(),
            files.resolved(&reference)
        );
    }
    fs::create_dir(files.root.join("directory.png")).unwrap();
    files.write("directory.bmp", &[1]);
    assert_eq!(
        resolve_asset(&files.root, "directory.png", AssetPathPolicy::ImageVariants)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    fs::create_dir(files.root.join("early.bmp")).unwrap();
    files.write("early.png", &[1]);
    assert_eq!(
        resolve_asset(&files.root, "early", AssetPathPolicy::ImageVariants)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    for unsafe_name in ["../outside.png", "/outside.png", "C:\\outside.png"] {
        assert!(resolve_asset(&files.root, unsafe_name, AssetPathPolicy::ImageVariants).is_err());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::write(files.temporary.join("outside.bmp"), bitmap()).unwrap();
        symlink(
            files.temporary.join("outside.bmp"),
            files.root.join("escape.png"),
        )
        .unwrap();
        files.write("escape.bmp", &bitmap());
        assert_eq!(
            resolve_asset(&files.root, "escape.png", AssetPathPolicy::ImageVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        symlink(
            files.root.join("absent-target"),
            files.root.join("dangling.jpg"),
        )
        .unwrap();
        files.write("dangling.bmp", &bitmap());
        assert_eq!(
            resolve_asset(&files.root, "dangling.jpg", AssetPathPolicy::ImageVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        symlink(
            files.root.join("absent-target"),
            files.root.join("first.bmp"),
        )
        .unwrap();
        files.write("first.png", &bitmap());
        assert_eq!(
            resolve_asset(&files.root, "first", AssetPathPolicy::ImageVariants)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        for name in [
            "escape.png",
            "dangling.jpg",
            "first",
            "directory.png",
            "early",
        ] {
            let chart = parse(&format!("#BMP01 {name}\n#00004:01"), Default::default()).unwrap();
            assert!(
                ImageAssets::prepare(&files.root, &chart, ImageAssetLimits::default()).is_err(),
                "{name}"
            );
        }
    }
}

#[test]
fn selected_file_scope_keeps_literal_stems_and_audio_exact_policies_and_storage_read_bounds() {
    let mut files = memory();
    files.insert("pack/日本/絵.PNG", vec![1, 2]).unwrap();
    files.insert("pack/Tone.wav", vec![3]).unwrap();
    files.insert("pack/Tone.bmp", vec![4]).unwrap();
    files.insert("other/絵.bmp", vec![5]).unwrap();
    files.insert("pack/blocked.bmp/child", vec![6]).unwrap();
    files.insert("pack/blocked.png", vec![7]).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    let key = source
        .resolve(".\\日本\\絵.jpg", AssetPathPolicy::ImageVariants)
        .unwrap();
    assert_eq!(key, Path::new("pack/日本/絵.PNG"));
    assert_eq!(&*source.read(&key, 2).unwrap(), [1, 2]);
    assert!(source.read(&key, 1).is_err());
    assert!(source.read(Path::new("other/絵.bmp"), 8).is_err());
    assert_eq!(AssetPathPolicy::default(), AssetPathPolicy::Exact);
    assert!(source.resolve("Tone", AssetPathPolicy::Exact).is_err());
    assert_eq!(
        source
            .resolve("Tone", AssetPathPolicy::AudioVariants)
            .unwrap(),
        Path::new("pack/Tone.wav")
    );
    assert_eq!(
        source
            .resolve("Tone", AssetPathPolicy::ImageVariants)
            .unwrap(),
        Path::new("pack/Tone.bmp")
    );
    assert_eq!(
        source
            .resolve("tone", AssetPathPolicy::ImageVariants)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        source
            .resolve("絵", AssetPathPolicy::ImageVariants)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        source
            .resolve("blocked", AssetPathPolicy::ImageVariants)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    for name in ["../other/絵", "日本/../../絵", "C:/絵", "/絵"] {
        assert!(
            source
                .resolve(name, AssetPathPolicy::ImageVariants)
                .is_err()
        );
    }
    let mut bounded = MemoryFiles::new(MemoryAssetLimits {
        max_files: 2,
        max_file_bytes: 2,
        max_total_bytes: 3,
        max_path_bytes: 16,
    })
    .unwrap();
    bounded.insert("chart.bms", vec![0]).unwrap();
    bounded.insert("art.PNG", vec![1, 2]).unwrap();
    assert!(bounded.insert("more.bmp", vec![]).is_err());
    assert!(bounded.insert("12345678901234567", vec![]).is_err());
    assert_eq!(
        bounded
            .scope("chart.bms")
            .unwrap()
            .resolve("art", AssetPathPolicy::ImageVariants)
            .unwrap(),
        Path::new("art.PNG")
    );
    assert!(
        bounded
            .scope("chart.bms")
            .unwrap()
            .resolve("12345678901234567", AssetPathPolicy::ImageVariants)
            .is_err()
    );
}

fn bitmap() -> Vec<u8> {
    // Original 2x1 24-bit BMP: opaque black, then opaque red; two padding bytes.
    let mut bytes = vec![0; 62];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&62_u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&2_i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1_i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&8_u32.to_le_bytes());
    bytes[57..60].copy_from_slice(&[0, 0, 255]);
    bytes
}
struct Tracked<'a> {
    source: MemoryAssetSource<'a>,
    resolved: RefCell<Vec<AssetPathPolicy>>,
    reads: RefCell<Vec<PathBuf>>,
    deny: Option<&'static str>,
}
impl<'a> Tracked<'a> {
    fn new(files: &'a MemoryFiles) -> Self {
        Self {
            source: files.scope("pack/chart.bms").unwrap(),
            resolved: RefCell::new(vec![]),
            reads: RefCell::new(vec![]),
            deny: None,
        }
    }
}
impl AssetSource for Tracked<'_> {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        self.resolved.borrow_mut().push(policy);
        self.source.resolve(name, policy)
    }
    fn read<'a>(&'a self, key: &Path, bound: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.borrow_mut().push(key.into());
        if self.deny.is_some_and(|name| key == Path::new(name)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "controlled selected-file read refusal",
            ));
        }
        self.source.read(key, bound)
    }
}
const ALIASES: &str = "#CANVASSIZE 2 1\n#BMP00 art.jpeg\n#BMP01 ./art.jpg\n#BMP02 art.png\n#BGA03 01 0 0 1 1 1 0\n#BGA04 02 0 0 1 1 1 0\n#00004:01020304\n#00007:0103\n#0000A:0204";
#[test]
fn actual_variant_image_banks_share_raw_crop_and_layer_data_with_exact_budgets_and_original_chart_identity()
 {
    let chart = parse(ALIASES, Default::default()).unwrap();
    let original = chart.clone();
    let compiled = chart.compile().unwrap();
    let mut files = memory();
    files.insert("pack/art.BMP", bitmap()).unwrap();
    let source = Tracked::new(&files);
    let exact = ImageAssetLimits {
        max_images: 5,
        max_decoded_bytes: 32,
        ..ImageAssetLimits::default()
    };
    let bank = ImageAssets::prepare_from_source(&source, &chart, exact).unwrap();
    assert_eq!(
        (bank.len(), bank.unique_images(), bank.decoded_bytes()),
        (5, 1, 32)
    );
    assert_eq!(*source.reads.borrow(), [PathBuf::from("pack/art.BMP")]);
    assert_eq!(
        *source.resolved.borrow(),
        [AssetPathPolicy::ImageVariants; 3]
    );
    assert_eq!(
        bank.get(ImageId(0)).unwrap().pixels(),
        [0, 0, 0, 255, 255, 0, 0, 255]
    );
    assert!(Arc::ptr_eq(
        bank.get(ImageId(0)).unwrap(),
        bank.get(ImageId(1)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        bank.get(ImageId(1)).unwrap(),
        bank.get(ImageId(2)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        bank.get(ImageId(3)).unwrap(),
        bank.get(ImageId(4)).unwrap()
    ));
    assert_eq!(
        bank.get(ImageId(3)).unwrap().pixels(),
        [0, 0, 0, 0, 0, 0, 0, 255]
    );
    assert!(Arc::ptr_eq(
        bank.get_layer(ImageId(1)).unwrap(),
        bank.get_layer(ImageId(2)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        bank.get_layer(ImageId(3)).unwrap(),
        bank.get_layer(ImageId(4)).unwrap()
    ));
    assert!(!Arc::ptr_eq(
        bank.get(ImageId(1)).unwrap(),
        bank.get_layer(ImageId(1)).unwrap()
    ));
    assert_eq!(
        bank.get_layer(ImageId(1)).unwrap().pixels(),
        [0, 0, 0, 0, 255, 0, 0, 255]
    );
    assert_eq!(bank.get_layer(ImageId(3)).unwrap().pixels(), [0; 8]);
    assert!(bank.get_layer(ImageId(0)).is_none());
    let snapshot = bank.clone();
    assert!(Arc::ptr_eq(
        snapshot.get(ImageId(3)).unwrap(),
        bank.get(ImageId(3)).unwrap()
    ));
    let source = Tracked::new(&files);
    assert!(
        ImageAssets::prepare_from_source(
            &source,
            &chart,
            ImageAssetLimits {
                max_images: 4,
                ..exact
            }
        )
        .is_err()
    );
    assert!(source.resolved.borrow().is_empty() && source.reads.borrow().is_empty());
    for limit in [7, 15, 23, 31] {
        assert!(
            ImageAssets::prepare_from_source(
                &source,
                &chart,
                ImageAssetLimits {
                    max_decoded_bytes: limit,
                    ..exact
                }
            )
            .is_err()
        );
    }
    let mut encoded = exact;
    encoded.decode.max_encoded_bytes = 61;
    assert!(ImageAssets::prepare_from_source(&source, &chart, encoded).is_err());
    encoded.decode.max_encoded_bytes = 62;
    assert_eq!(
        ImageAssets::prepare_from_source(&source, &chart, encoded)
            .unwrap()
            .decoded_bytes(),
        32
    );
    let mut width = exact;
    width.decode.max_width = 1;
    assert!(ImageAssets::prepare_from_source(&source, &chart, width).is_err());
    let disk = Files::new();
    disk.write("art.BMP", &bitmap());
    let actual = ImageAssets::prepare(&disk.root, &chart, exact).unwrap();
    assert_eq!((actual.unique_images(), actual.decoded_bytes()), (1, 32));
    for id in [0, 1, 2, 3, 4] {
        assert_eq!(
            actual.get(ImageId(id)).unwrap().pixels(),
            bank.get(ImageId(id)).unwrap().pixels()
        );
    }
    assert_eq!(chart, original);
    assert_eq!(chart.compile().unwrap(), compiled);
    assert_eq!(chart.images[&ImageId(0)], "art.jpeg");
}

#[test]
fn literal_decode_failures_stay_blank_and_read_failures_or_exhausted_limits_never_try_a_different_image()
 {
    let chart = parse("#BMP01 literal.png\n#BMP02 damaged.jpg\n#BMP03 missing.png\n#BMP04 clip.mp4\n#BMP05 unknown.asset\n#00004:010203040506", Default::default()).unwrap();
    let mut files = memory();
    files
        .insert("pack/literal.png", b"not a raster".to_vec())
        .unwrap();
    files.insert("pack/literal.bmp", bitmap()).unwrap();
    files.insert("pack/damaged.jpg", b"BM".to_vec()).unwrap();
    files.insert("pack/damaged.bmp", bitmap()).unwrap();
    files.insert("pack/clip.bmp", bitmap()).unwrap();
    files.insert("pack/unknown.asset", bitmap()).unwrap();
    let source = Tracked::new(&files);
    let bank =
        ImageAssets::prepare_from_source(&source, &chart, ImageAssetLimits::default()).unwrap();
    assert_eq!(
        (bank.len(), bank.unique_images(), bank.decoded_bytes()),
        (6, 1, 8)
    );
    assert_eq!(
        bank.unavailable(ImageId(1)),
        Some(&ImageUnavailable::Unsupported)
    );
    assert!(matches!(
        bank.unavailable(ImageId(2)),
        Some(ImageUnavailable::InvalidData(_))
    ));
    for (id, reason) in [
        (3, ImageUnavailable::Missing),
        (4, ImageUnavailable::Unsupported),
    ] {
        assert_eq!(
            bank.unavailable(ImageId(id)),
            Some(&reason)
        );
    }
    assert_eq!(
        bank.unavailable(ImageId(6)),
        Some(&ImageUnavailable::Undefined)
    );
    assert_eq!(
        bank.get(ImageId(5)).unwrap().pixels(),
        [0, 0, 0, 255, 255, 0, 0, 255]
    );
    assert_eq!(
        *source.reads.borrow(),
        [
            PathBuf::from("pack/literal.png"),
            PathBuf::from("pack/damaged.jpg"),
            PathBuf::from("pack/unknown.asset")
        ]
    );
    let mut denied = Tracked::new(&files);
    denied.deny = Some("pack/literal.png");
    assert!(
        ImageAssets::prepare_from_source(&denied, &chart, ImageAssetLimits::default()).is_err()
    );
    assert_eq!(*denied.reads.borrow(), [PathBuf::from("pack/literal.png")]);
    let mut tiny = ImageAssetLimits::default();
    tiny.decode.max_encoded_bytes = 2;
    let source = Tracked::new(&files);
    assert!(ImageAssets::prepare_from_source(&source, &chart, tiny).is_err());
    assert_eq!(*source.reads.borrow(), [PathBuf::from("pack/literal.png")]);
    let missing = parse("#BMP01 nothing\n#00004:01", Default::default()).unwrap();
    let source = Tracked::new(&files);
    let blank =
        ImageAssets::prepare_from_source(&source, &missing, ImageAssetLimits::default()).unwrap();
    assert_eq!(
        blank.unavailable(ImageId(1)),
        Some(&ImageUnavailable::Missing)
    );
    assert!(source.reads.borrow().is_empty());
}
