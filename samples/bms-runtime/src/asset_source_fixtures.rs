//! Selected-file admission and lookup fixtures with no filesystem adapter.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryAssetLimits, MemoryFiles},
};
use std::{borrow::Cow, io::ErrorKind, path::Path};

fn files() -> MemoryFiles {
    MemoryFiles::new(MemoryAssetLimits::default()).unwrap()
}

#[test]
fn normalized_unicode_keys_resolve_from_chart_parent_and_borrow_original_bytes() {
    let mut files = files();
    files.insert("曲\\.\\chart.bms", vec![]).unwrap();
    files.insert("曲//音././鍵.wav", vec![1, 2, 3]).unwrap();
    files.insert("other.wav", vec![9]).unwrap();
    let keys: Vec<_> = files.keys().collect();
    assert!(keys.contains(&"曲/chart.bms"));
    assert!(keys.contains(&"曲/音./鍵.wav"));
    let stored = files.read_file("./曲/音./鍵.wav", 3).unwrap();
    let source = files.scope("曲/./chart.bms").unwrap();
    let key = source
        .resolve(".\\音.\\鍵.wav", AssetPathPolicy::Exact)
        .unwrap();
    assert_eq!(key, Path::new("曲/音./鍵.wav"));
    let encoded = source.read(&key, 3).unwrap();
    assert!(matches!(&encoded, Cow::Borrowed(_)));
    assert_eq!(&*encoded, &[1, 2, 3]);
    assert_eq!(encoded.as_ptr(), stored.as_ptr());
    assert!(source.read(&key, 2).is_err());
    assert!(files.read_file("曲/音./鍵.wav", 2).is_err());
    assert_eq!(
        source
            .resolve("other.wav", AssetPathPolicy::Exact)
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    assert!(
        source
            .resolve("../other.wav", AssetPathPolicy::Exact)
            .is_err()
    );
    assert!(files.scope("曲/missing.bms").is_err());
    assert!(files.scope("曲").is_err());
}

#[test]
fn unsafe_paths_reject_without_consuming_or_replacing_selected_files() {
    let mut files = files();
    files.insert("chart.bms", vec![7]).unwrap();
    for name in [
        "",
        ".",
        "./",
        "/",
        "\\",
        "../x",
        "a/../b",
        "a\\..\\b",
        "/absolute",
        "\\absolute",
        "C:relative",
        "C:\\absolute",
        "./C:/x",
        "\\\\host\\share",
        "x\0y",
    ] {
        assert!(files.insert(name, vec![8]).is_err(), "accepted {name:?}");
        assert_eq!((files.len(), files.total_bytes()), (1, 1));
        assert_eq!(files.read_file("chart.bms", 1).unwrap(), &[7]);
        let source = files.scope("chart.bms").unwrap();
        assert!(
            source
                .resolve(name, AssetPathPolicy::AudioVariants)
                .is_err()
        );
    }
    files.insert("valid.wav", vec![3]).unwrap();
    assert_eq!((files.len(), files.total_bytes()), (2, 2));
}

#[test]
fn aliases_and_file_directory_collisions_reject_in_both_admission_orders() {
    let mut files = files();
    files.insert("folder/tone.wav", vec![1, 2]).unwrap();
    for alias in [
        "./folder/tone.wav",
        "folder\\.\\tone.wav",
        "folder//tone.wav",
    ] {
        assert!(files.insert(alias, vec![9]).is_err());
    }
    assert!(files.insert("folder", vec![9]).is_err());
    assert!(files.insert("folder/tone.wav/child", vec![9]).is_err());
    assert_eq!((files.len(), files.total_bytes()), (1, 2));
    assert_eq!(files.read_file("folder/tone.wav", 2).unwrap(), &[1, 2]);
    files.insert("plain", vec![]).unwrap();
    assert!(files.insert("plain/nested.wav", vec![9]).is_err());
    files.insert("folder/Tone.wav", vec![3]).unwrap();
    assert_eq!((files.len(), files.total_bytes()), (3, 3));
    assert_eq!(files.read_file("folder/Tone.wav", 1).unwrap(), &[3]);
}

#[test]
fn admission_and_read_limits_count_utf8_bytes_and_preserve_capacity_on_failure() {
    let limits = MemoryAssetLimits {
        max_files: 3,
        max_file_bytes: 4,
        max_total_bytes: 6,
        max_path_bytes: 7,
    };
    let mut files = MemoryFiles::new(limits).unwrap();
    files.insert("音.bms", vec![]).unwrap(); // Seven UTF-8 bytes.
    assert!(files.insert("音a.bms", vec![]).is_err());
    assert!(files.insert("large", vec![0; 5]).is_err());
    files.insert("a.wav", vec![1; 4]).unwrap();
    assert!(files.insert("b.wav", vec![2; 3]).is_err());
    assert_eq!((files.len(), files.total_bytes()), (2, 4));
    files.insert("b.wav", vec![2; 2]).unwrap();
    assert_eq!((files.len(), files.total_bytes()), (3, 6));
    assert!(files.insert("empty", vec![]).is_err());
    assert_eq!(files.read_file("音.bms", 0).unwrap(), &[]);
    assert_eq!(files.read_file("a.wav", 4).unwrap(), &[1; 4]);
    assert!(files.read_file("a.wav", 3).is_err());

    for invalid in [
        MemoryAssetLimits {
            max_files: 0,
            ..limits
        },
        MemoryAssetLimits {
            max_file_bytes: 0,
            ..limits
        },
        MemoryAssetLimits {
            max_total_bytes: 0,
            ..limits
        },
        MemoryAssetLimits {
            max_path_bytes: 0,
            ..limits
        },
        MemoryAssetLimits {
            max_file_bytes: 7,
            ..limits
        },
    ] {
        assert!(MemoryFiles::new(invalid).is_err());
    }
}

#[test]
fn audio_lookup_is_literal_first_with_finite_extension_only_variants() {
    let mut files = files();
    files.insert("pack/chart.bms", vec![]).unwrap();
    files.insert("pack/tone.WaV", vec![1]).unwrap();
    files.insert("pack/tone.FLAC", vec![2]).unwrap();
    files.insert("pack/tone.mp3", vec![3]).unwrap();
    files.insert("pack/sub/Case.WAV", vec![4]).unwrap();
    {
        let source = files.scope("pack/chart.bms").unwrap();
        for name in ["tone.wav", "tone", "tone.ogg"] {
            assert_eq!(
                source
                    .resolve(name, AssetPathPolicy::AudioVariants)
                    .unwrap(),
                Path::new("pack/tone.WaV")
            );
        }
        assert_eq!(
            source
                .resolve("tone.flac", AssetPathPolicy::AudioVariants)
                .unwrap(),
            Path::new("pack/tone.FLAC")
        );
        assert_eq!(
            source
                .resolve("tone.mp3", AssetPathPolicy::AudioVariants)
                .unwrap(),
            Path::new("pack/tone.mp3")
        );
        assert_eq!(
            source
                .resolve("sub\\Case.wav", AssetPathPolicy::AudioVariants)
                .unwrap(),
            Path::new("pack/sub/Case.WAV")
        );
        for (name, policy) in [
            ("tone.wav", AssetPathPolicy::Exact),
            ("tone.aac", AssetPathPolicy::AudioVariants),
            ("Tone.wav", AssetPathPolicy::AudioVariants),
            ("Sub/Case.wav", AssetPathPolicy::AudioVariants),
        ] {
            assert_eq!(
                source.resolve(name, policy).unwrap_err().kind(),
                ErrorKind::NotFound
            );
        }
    }
    files
        .insert("pack/tone.wav", b"invalid audio still wins lookup".to_vec())
        .unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    assert_eq!(
        source
            .resolve("tone.wav", AssetPathPolicy::AudioVariants)
            .unwrap(),
        Path::new("pack/tone.wav")
    );
}

#[test]
fn existing_directory_shadows_audio_variants_instead_of_falling_through() {
    let mut files = files();
    files.insert("chart.bms", vec![]).unwrap();
    files.insert("tone.wav/child", vec![1]).unwrap();
    files.insert("tone.flac", vec![2]).unwrap();
    let source = files.scope("chart.bms").unwrap();
    for name in ["tone.wav", "tone.ogg", "tone"] {
        assert_eq!(
            source
                .resolve(name, AssetPathPolicy::AudioVariants)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
    }
    assert!(source.read(Path::new("tone.wav"), 8).is_err());
}
