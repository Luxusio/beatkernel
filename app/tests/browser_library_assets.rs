//! Portable demand-loading plan tests: declarations are inventory, not acquired PCM.
use beatkernel::{
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::{encode_replay, ReplayFile},
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_bms::{parse_seeded, ParseOptions};
use beatkernel_bms_runtime::{
    asset_source::{MemoryAssetLimits, MemoryFiles},
    browser_library_assets::{referenced_asset_paths, replay_referenced_asset_paths},
    competition_live,
    replay_capture::LiveReplayCapture,
};
use std::io::ErrorKind;

fn inventory(chart: &[u8], media: &[&str]) -> MemoryFiles {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.declare_file("pack/chart.bms", chart.len()).unwrap();
    files.insert("pack/chart.bms", chart.to_vec()).unwrap();
    for name in media {
        files.declare_file(&format!("pack/{name}"), 1).unwrap();
    }
    files
}

fn paths(files: &MemoryFiles, seed: u64, cap: usize) -> Vec<String> {
    referenced_asset_paths(files, "pack/chart.bms", seed, cap).unwrap()
}

#[test]
fn audio_plan_includes_visible_bgm_invisible_and_nonfatal_mine_zero_without_loading() {
    let chart = b"#BPM 120\n#WAV00 mine.wav\n#WAV01 key.wav\n#WAV02 hidden.wav\n#WAV03 bgm.wav\n#WAV04 unused.wav\n#WAV05 ./key.wav\n#00011:01\n#00012:05\n#00031:02\n#00001:03\n#000D1:01\n";
    let files = inventory(
        chart,
        &["mine.wav", "key.wav", "hidden.wav", "bgm.wav", "unused.wav"],
    );
    let before = (files.len(), files.total_bytes());
    assert_eq!(
        paths(&files, 0, 5),
        [
            "pack/bgm.wav",
            "pack/hidden.wav",
            "pack/key.wav",
            "pack/mine.wav"
        ]
    );
    // PCM limits count sample identities, even when resolved paths deduplicate.
    assert!(referenced_asset_paths(&files, "pack/chart.bms", 0, 4).is_err());
    for name in ["mine.wav", "key.wav", "hidden.wav", "bgm.wav", "unused.wav"] {
        assert_eq!(
            files
                .read_file(&format!("pack/{name}"), 1)
                .unwrap_err()
                .kind(),
            ErrorKind::WouldBlock
        );
    }
    assert_eq!((files.len(), files.total_bytes()), before);
}

#[test]
fn audio_exact_declared_candidate_shadows_loaded_compatible_extension() {
    let mut files = inventory(b"#BPM 120\n#WAV01 tone.wav\n#00011:01\n", &["tone.wav"]);
    files.insert("pack/tone.flac", vec![7]).unwrap();
    assert_eq!(paths(&files, 0, 1), ["pack/tone.wav"]);
    assert_eq!(
        files.read_file("pack/tone.wav", 1).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    files.insert("pack/tone.wav", vec![8]).unwrap();
    assert_eq!(paths(&files, 0, 1), ["pack/tone.wav"]);
}

#[test]
fn fatal_only_mines_and_unreferenced_wav_zero_do_not_acquire_mine_sound() {
    for chart in [
        b"#BPM 120\n#WAV00 mine.wav\n#WAV01 key.wav\n#00011:01\n#000D1:ZZ\n".as_slice(),
        b"#BPM 120\n#WAV00 mine.wav\n#WAV01 key.wav\n#00011:01\n".as_slice(),
    ] {
        let files = inventory(chart, &["mine.wav", "key.wav"]);
        assert_eq!(paths(&files, 0, 1), ["pack/key.wav"]);
        assert_eq!(
            files.read_file("pack/mine.wav", 1).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
    }
}

#[test]
fn visual_plan_uses_poor_crop_sources_and_movies_but_not_unused_or_missing_optional_assets() {
    let text = b"#BPM 120\n#BMP00 poor.png\n#BMP01 crop.png\n#BGA02 01 0 0 1 1 0 0\n#BMP03 clip.mp4\n#BMP04 missing.mp4\n#BMP05 missing.png\n#BMP06 ../unused.png\n#BMP07 unused.png\n#00004:020405\n#00007:03\n";
    let files = inventory(text, &["poor.PNG", "crop.png", "clip.mp4", "unused.png"]);
    assert_eq!(
        paths(&files, 0, 8),
        ["pack/clip.mp4", "pack/crop.png", "pack/poor.PNG"]
    );
    assert_eq!(
        files.read_file("pack/crop.png", 1).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    assert_eq!(
        files.read_file("pack/clip.mp4", 1).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
}

#[test]
fn invalid_selected_audio_or_visual_path_and_directory_fail_before_hydration() {
    for text in [
        "#BPM 120\n#WAV01 ../key.wav\n#00011:01\n",
        "#BPM 120\n#BMP01 ../image.png\n#00004:01\n",
        "#BPM 120\n#BMP01 ../clip.mp4\n#00004:01\n",
        "#BPM 120\n#WAV01 key.wav\n#00011:01\n",
    ] {
        let mut files = inventory(text.as_bytes(), &[]);
        files.declare_file("pack/key.wav/child", 1).unwrap();
        files.insert("pack/key.flac", vec![4]).unwrap();
        let before = (files.len(), files.total_bytes());
        assert!(
            referenced_asset_paths(&files, "pack/chart.bms", 0, 8).is_err(),
            "accepted {text}"
        );
        assert_eq!((files.len(), files.total_bytes()), before);
    }
}

#[test]
fn random_resource_plan_uses_seeded_parser_and_leaves_other_branch_unacquired() {
    let text = "#BPM 120\n#RANDOM 2\n#IF 1\n#WAV01 first.wav\n#00011:01\n#ELSE\n#WAV02 second.wav\n#00012:02\n#ENDIF\n";
    let files = inventory(text.as_bytes(), &["first.wav", "second.wav"]);
    let mut branches = std::collections::BTreeSet::new();
    for seed in 0..16 {
        let source = parse_seeded(text, ParseOptions::default(), seed).unwrap();
        let sample = source.notes[0].sample.0 as u16;
        let expected = format!("pack/{}", source.samples[&sample]);
        assert_eq!(paths(&files, seed, 1), [expected.clone()]);
        branches.insert(expected);
    }
    assert_eq!(branches.len(), 2);
    for name in ["first.wav", "second.wav"] {
        assert_eq!(
            files
                .read_file(&format!("pack/{name}"), 1)
                .unwrap_err()
                .kind(),
            ErrorKind::WouldBlock
        );
    }
}

fn recording(text: &str, seed: u64) -> ReplayFile {
    let source = parse_seeded(text, ParseOptions::default(), seed).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    LiveReplayCapture::new_at_with_chart_seed(
        &judge,
        ClockDomainId(17),
        competition_live::replay_limits().unwrap(),
        Timestamp::ZERO,
        seed,
    )
    .unwrap()
    .into_file()
}

#[test]
fn replay_resource_plan_derives_recorded_seed_and_validates_authentic_setup() {
    let text = "#BPM 120\n#RANDOM 2\n#IF 1\n#WAV01 first.wav\n#00011:01\n#ELSE\n#WAV02 second.wav\n#00012:02\n#ENDIF\n";
    let files = inventory(text.as_bytes(), &["first.wav", "second.wav"]);
    let cap = competition_live::replay_limits().unwrap();
    let file = recording(text, 3);
    let bytes = encode_replay(&file, cap).unwrap();
    let expected = paths(&files, 3, 1);
    assert_ne!(
        expected,
        paths(&files, 0, 1),
        "fixture requires genuinely different RANDOM branches"
    );
    assert_eq!(
        replay_referenced_asset_paths(&files, "pack/chart.bms", &bytes, 1).unwrap(),
        expected
    );
    let mut wrong_chart = file.clone();
    wrong_chart.header.chart_identity[0] ^= 1;
    let mut wrong_rule_seed = file.clone();
    wrong_rule_seed.header.seed = 1;
    let mut no_setup = file;
    no_setup.header.options.clear();
    for invalid in [wrong_chart, wrong_rule_seed, no_setup] {
        let encoded = encode_replay(&invalid, cap).unwrap();
        assert!(replay_referenced_asset_paths(&files, "pack/chart.bms", &encoded, 1).is_err());
    }
    assert!(replay_referenced_asset_paths(&files, "pack/chart.bms", b"not replay", 1).is_err());
    assert_eq!(
        files.read_file("pack/first.wav", 1).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    assert_eq!(
        files.read_file("pack/second.wav", 1).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
}

#[test]
fn chart_must_be_acquired_and_strict_text_decoding_matches_existing_encoding_policy() {
    let mut declared = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    declared.declare_file("chart.bms", 1).unwrap();
    assert!(referenced_asset_paths(&declared, "chart.bms", 0, 1).is_err());
    assert_eq!(
        declared.read_file("chart.bms", 1).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    let text = "#BPM 120\n#WAV01 音.wav\n#00011:01\n";
    let (encoded, _, lossy) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!lossy);
    let files = inventory(&encoded, &["音.wav"]);
    assert_eq!(paths(&files, 0, 1), ["pack/音.wav"]);
    let bad = inventory(&[0xff, 0xfe, 0, 0], &[]);
    assert!(referenced_asset_paths(&bad, "pack/chart.bms", 0, 1).is_err());
}
