//! Execute the portable implementation used by the actual BrowserLibrary owner.
//! These tests run natively; generated WASM export acceptance is separate.
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleId},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::ReplayFile,
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_bms::{parse_seeded, ImageId, ParseOptions};
use beatkernel_bms_runtime::{
    asset_source::{MemoryAssetLimits, MemoryFiles},
    browser_library_assets::{
        BrowserAssetPreparationJob, BrowserAssetPrepared, BrowserPreparationCache,
    },
    competition_live,
    preparation::PreparationStage,
    replay_capture::LiveReplayCapture,
    replay_playback::PlaybackError,
    video_assets::VideoResource,
};
use image::ImageEncoder;
use std::sync::Arc;

const CHART: &str = "#BPM 120\n#WAV01 key.wav\n#WAV02 key.wav\n#00011:0102\n";
fn format() -> AudioFormat {
    AudioFormat::new(24_000, 2).unwrap()
}
fn limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}
fn wav() -> Vec<u8> {
    let samples = [8192i16, -4096];
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24_000u32.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&4u32.to_le_bytes());
    for value in samples {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn files(chart: &str, hydrate: bool) -> MemoryFiles {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.declare_file("pack/chart.bms", chart.len()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.declare_file("pack/key.wav", wav().len()).unwrap();
    if hydrate {
        files.insert("pack/key.wav", wav()).unwrap();
    }
    files
}
fn finish(
    job: &mut BrowserAssetPreparationJob,
    cache: &BrowserPreparationCache,
    files: &MemoryFiles,
) -> BrowserAssetPrepared {
    for _ in 0..512 {
        if job.advance(cache, files).unwrap() {
            assert_eq!(job.progress().stage, PreparationStage::Ready);
            return job.take_ready(cache, files).unwrap();
        }
    }
    panic!("bounded fixture did not become genuinely ready")
}
fn recording(chart: &str, seed: u64) -> ReplayFile {
    let chart = parse_seeded(chart, ParseOptions::default(), seed).unwrap();
    let judge = JudgeEngine::new(
        chart.compile().unwrap().chart,
        chart.rules(),
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
fn browser_original_cache_shares_alias_pcm_and_warm_consumers_without_reparse_or_redecode() {
    let files = files(CHART, true);
    let cache = BrowserPreparationCache::new().unwrap();
    let mut cold = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let first = finish(&mut cold, &cache, &files);
    let counts = cold.counters();
    assert_eq!(counts.parses, 1);
    assert_eq!(counts.compiles, 1);
    assert_eq!(counts.decodes, 1);
    let one = first.prepared.bank.get(SampleId(1)).unwrap();
    let alias = first.prepared.bank.get(SampleId(2)).unwrap();
    assert_eq!(one.samples(), &[0.25, 0.25, -0.125, -0.125]);
    assert_eq!(
        one.samples().as_ptr(),
        alias.samples().as_ptr(),
        "cold aliases share actual normalized PCM storage"
    );
    let pointer = one.samples().as_ptr();
    let mut warm = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let next = finish(&mut warm, &cache, &files);
    let counts = warm.counters();
    assert_eq!(counts.parses, 0);
    assert_eq!(counts.compiles, 0);
    assert_eq!(counts.decodes, 0);
    assert!(counts.shared_assets >= 2);
    assert_eq!(
        next.prepared
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        pointer
    );
    assert!(warm.take_ready(&cache, &files).is_err());
    assert!(warm.advance(&cache, &files).is_err());
    drop(cache);
    drop(next);
    assert_eq!(
        first.prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, 0.25, -0.125, -0.125]
    );
}

#[test]
fn browser_job_accepts_declared_hydration_but_refuses_inventory_and_library_owner_drift() {
    let mut files = files(CHART, false);
    let cache = BrowserPreparationCache::new().unwrap();
    let mut job = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let revision = files.inventory_revision();
    files.insert("pack/key.wav", wav()).unwrap();
    assert_eq!(files.inventory_revision(), revision);
    let ready = finish(&mut job, &cache, &files);
    assert_eq!(ready.prepared.bank.len(), 2);
    let mut obsolete = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    files.declare_file("pack/new.wav", 1).unwrap();
    assert!(obsolete.advance(&cache, &files).is_err());
    assert!(obsolete.take_ready(&cache, &files).is_err());
    let foreign = BrowserPreparationCache::new().unwrap();
    let mut valid = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    assert!(valid.advance(&foreign, &files).is_err());
    assert!(valid.take_ready(&foreign, &files).is_err());
    let mut staged = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let mut ready = false;
    for _ in 0..128 {
        if staged.advance(&cache, &files).unwrap() {
            ready = true;
            break;
        }
    }
    assert!(ready);
    files.declare_file("pack/after-ready.wav", 1).unwrap();
    assert!(
        staged.take_ready(&cache, &files).is_err(),
        "inventory must be revalidated at final admission, after Ready"
    );
}

#[test]
fn browser_cancel_and_failed_replacement_keep_the_previous_valid_original() {
    let mut files = files(CHART, true);
    files
        .insert(
            "pack/bad.bms",
            b"#BPM 120\n#WAV01 bad.wav\n#00011:01\n".to_vec(),
        )
        .unwrap();
    files.insert("pack/bad.wav", vec![1, 2, 3]).unwrap();
    files
        .insert(
            "pack/other.bms",
            b"#BPM 120\n#WAV01 key.wav\n#00011:01\n".to_vec(),
        )
        .unwrap();
    let cache = BrowserPreparationCache::new().unwrap();
    let mut cold = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let original = finish(&mut cold, &cache, &files);
    let pointer = original
        .prepared
        .bank
        .get(SampleId(1))
        .unwrap()
        .samples()
        .as_ptr();
    let mut cancelled = cache
        .begin(&files, "pack/bad.bms", format(), 0, limits(), None)
        .unwrap();
    cancelled.cancel();
    assert!(cancelled.advance(&cache, &files).is_err());
    assert!(cancelled.take_ready(&cache, &files).is_err());
    let mut staged = cache
        .begin(&files, "pack/other.bms", format(), 0, limits(), None)
        .unwrap();
    let mut staged_ready = false;
    for _ in 0..128 {
        if staged.advance(&cache, &files).unwrap() {
            staged_ready = true;
            break;
        }
    }
    assert!(
        staged_ready,
        "replacement is genuinely ready but not admitted yet"
    );
    staged.cancel();
    assert!(staged.take_ready(&cache, &files).is_err());
    let mut broken = cache
        .begin(&files, "pack/bad.bms", format(), 0, limits(), None)
        .unwrap();
    let mut failed = false;
    for _ in 0..128 {
        match broken.advance(&cache, &files) {
            Err(_) => {
                failed = true;
                break;
            }
            Ok(false) => {}
            Ok(true) => panic!("invalid WAV became ready"),
        }
    }
    assert!(failed);
    assert!(broken.take_ready(&cache, &files).is_err());
    let mut retry = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let prepared = finish(&mut retry, &cache, &files);
    assert_eq!(retry.counters().decodes, 0);
    assert_eq!(
        prepared
            .prepared
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        pointer
    );
}

#[test]
fn browser_warm_audio_prepares_fresh_image_and_encoded_movie_owners() {
    let chart = format!("{CHART}#BMP01 image.png\n#BMP02 movie.mp4\n#00004:01\n#00007:02\n");
    let mut files = files(&chart, true);
    let mut image = Vec::new();
    image::codecs::png::PngEncoder::new(&mut image)
        .write_image(&[10, 20, 30, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    files.insert("pack/image.png", image).unwrap();
    files.insert("pack/movie.mp4", vec![1, 2, 3, 4]).unwrap();
    let cache = BrowserPreparationCache::new().unwrap();
    let mut cold = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let first = finish(&mut cold, &cache, &files);
    let mut warm = cache
        .begin(&files, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let next = finish(&mut warm, &cache, &files);
    assert_eq!(warm.counters().decodes, 0);
    let first_image = first.images.get(ImageId(1)).unwrap();
    let next_image = next.images.get(ImageId(1)).unwrap();
    assert!(
        !Arc::ptr_eq(first_image, next_image),
        "mutable presentation image ownership is rebuilt for each request"
    );
    assert_eq!(first_image.pixels(), next_image.pixels());
    match (
        &first.movies.resources()[0].data,
        &next.movies.resources()[0].data,
    ) {
        (VideoResource::Encoded(first), VideoResource::Encoded(next)) => {
            assert_eq!(first.as_ref(), next.as_ref());
            assert!(
                !Arc::ptr_eq(first, next),
                "encoded movie owners remain fresh rather than inheriting decoder position"
            );
        }
        _ => panic!("memory source must publish encoded movie resources"),
    }
}

#[test]
fn browser_replay_checks_genuine_recording_setup_before_declared_media_acquisition() {
    let files = files(CHART, false);
    let cache = BrowserPreparationCache::new().unwrap();
    let genuine = recording(CHART, 3);
    let mut wrong = genuine.clone();
    wrong.header.chart_identity[0] ^= 1;
    let mut missing = genuine.clone();
    missing.header.options.clear();
    for invalid in [wrong, missing] {
        match cache.begin(
            &files,
            "pack/chart.bms",
            format(),
            3,
            limits(),
            Some(invalid),
        ) {
            Err(error) => {
                assert!(
                    error.downcast_ref::<PlaybackError>().is_some(),
                    "expected recording identity/setup failure, got {error}"
                );
            }
            Ok(mut job) => {
                let mut failed = false;
                for _ in 0..128 {
                    match job.advance(&cache, &files) {
                        Err(error) => {
                            assert!(
                                error.downcast_ref::<PlaybackError>().is_some(),
                                "expected recording identity/setup failure, got {error}"
                            );
                            failed = true;
                            break;
                        }
                        Ok(false) => {
                            assert_eq!(job.counters().reads, 0);
                            assert_eq!(job.counters().decodes, 0);
                        }
                        Ok(true) => panic!("invalid replay became ready"),
                    }
                }
                assert!(failed);
                assert_eq!(job.counters().reads, 0);
                assert!(job.take_ready(&cache, &files).is_err());
            }
        }
    }
    assert_eq!(
        files
            .read_file("pack/key.wav", wav().len())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn browser_dependency_query_counts_actual_work_and_continues_its_canonical_parsed_plan() {
    let mut inventory = files(CHART, false);
    let cache = BrowserPreparationCache::new().unwrap();
    assert_eq!(cache.query_counters().parses, 0);
    assert_eq!(cache.query_counters().compiles, 0);
    assert_eq!(
        cache
            .referenced_asset_paths(&inventory, "pack/chart.bms", 0, 8)
            .unwrap(),
        ["pack/key.wav"]
    );
    assert_eq!(cache.query_counters().parses, 1);
    assert_eq!(cache.query_counters().compiles, 1);
    assert_eq!(cache.query_counters().decodes, 0);
    let revision = inventory.inventory_revision();
    inventory.insert("pack/key.wav", wav()).unwrap();
    assert_eq!(inventory.inventory_revision(), revision);
    let mut preparation = cache
        .begin(&inventory, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let cold = finish(&mut preparation, &cache, &inventory);
    assert_eq!(
        preparation.counters().parses,
        0,
        "query's real parse must not be repeated by preparation"
    );
    assert_eq!(preparation.counters().compiles, 0);
    assert_eq!(preparation.counters().decodes, 1);
    assert_eq!(
        cache.query_counters().parses,
        1,
        "query work is observable, not hidden behind job counters"
    );
    assert_eq!(cache.query_counters().compiles, 1);
    for _ in 0..3 {
        assert_eq!(
            cache
                .referenced_asset_paths(&inventory, "pack/chart.bms", 0, 8)
                .unwrap(),
            ["pack/key.wav"]
        );
    }
    assert_eq!(cache.query_counters().parses, 1);
    assert_eq!(cache.query_counters().compiles, 1);
    let mut warm = cache
        .begin(&inventory, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let next = finish(&mut warm, &cache, &inventory);
    assert_eq!(warm.counters().parses, 0);
    assert_eq!(warm.counters().compiles, 0);
    assert_eq!(warm.counters().decodes, 0);
    assert_eq!(
        cold.prepared
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        next.prepared
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr()
    );
}

#[test]
fn browser_dependency_plan_invalidates_seed_inventory_and_changed_source_bytes() {
    let mut inventory = files(CHART, true);
    let cache = BrowserPreparationCache::new().unwrap();
    cache
        .referenced_asset_paths(&inventory, "pack/chart.bms", 0, 8)
        .unwrap();
    assert_eq!(cache.query_counters().parses, 1);
    cache
        .referenced_asset_paths(&inventory, "pack/chart.bms", 1, 8)
        .unwrap();
    assert_eq!(cache.query_counters().parses, 2);
    assert_eq!(cache.query_counters().compiles, 2);
    let mut seeded = cache
        .begin(&inventory, "pack/chart.bms", format(), 1, limits(), None)
        .unwrap();
    let original = finish(&mut seeded, &cache, &inventory);
    assert_eq!(seeded.counters().parses, 0);
    inventory.declare_file("pack/additional.wav", 1).unwrap();
    cache
        .referenced_asset_paths(&inventory, "pack/chart.bms", 1, 8)
        .unwrap();
    assert_eq!(cache.query_counters().parses, 3);
    assert_eq!(cache.query_counters().compiles, 3);
    // Loaded entries are immutable. Changing selected bytes necessarily creates
    // a new admitted source owner, even with identical paths/length/revision.
    let changed_text = CHART.replace("120", "121");
    assert_eq!(changed_text.len(), CHART.len());
    let changed = files(&changed_text, true);
    cache
        .referenced_asset_paths(&changed, "pack/chart.bms", 1, 8)
        .unwrap();
    assert_eq!(cache.query_counters().parses, 4);
    assert_eq!(cache.query_counters().compiles, 4);
    let mut changed_job = cache
        .begin(&changed, "pack/chart.bms", format(), 1, limits(), None)
        .unwrap();
    let next = finish(&mut changed_job, &cache, &changed);
    assert_eq!(changed_job.counters().parses, 0);
    assert_eq!(changed_job.counters().compiles, 0);
    assert_ne!(
        original.prepared.compiled.chart.objects()[1].time.start,
        next.prepared.compiled.chart.objects()[1].time.start,
        "new equal-length chart bytes must produce their actual new timing"
    );
}

fn advance_to_asset_ready(
    job: &mut BrowserAssetPreparationJob,
    cache: &BrowserPreparationCache,
    files: &MemoryFiles,
) {
    for _ in 0..512 {
        if job.advance(cache, files).unwrap() {
            return;
        }
    }
    panic!("asset candidate did not reach Ready");
}

#[test]
fn same_revision_foreign_inventory_refuses_midjob_ready_and_staged_admission_without_mixing_pcm() {
    for phase in 0..3 {
        let inventory = files(CHART, true);
        let mut foreign = files(CHART, false);
        let mut other_pcm = wav();
        other_pcm[44..46].copy_from_slice(&(-8192i16).to_le_bytes());
        foreign.insert("pack/key.wav", other_pcm).unwrap();
        assert_eq!(inventory.inventory_revision(), foreign.inventory_revision());
        let cache = BrowserPreparationCache::new().unwrap();
        let mut original = cache
            .begin(&inventory, "pack/chart.bms", format(), 0, limits(), None)
            .unwrap();
        match phase {
            0 => {
                assert!(original.advance(&cache, &foreign).is_err());
            }
            1 => {
                advance_to_asset_ready(&mut original, &cache, &inventory);
                assert!(original.take_ready(&cache, &foreign).is_err());
            }
            _ => {
                advance_to_asset_ready(&mut original, &cache, &inventory);
                let (candidate, admission) = original.stage_ready(&cache, &inventory).unwrap();
                assert_eq!(
                    candidate.prepared.bank.get(SampleId(1)).unwrap().samples(),
                    &[0.25, 0.25, -0.125, -0.125]
                );
                assert!(original.admit(&cache, &foreign, &admission).is_err());
            }
        }
        let mut recovery = cache
            .begin(&foreign, "pack/chart.bms", format(), 0, limits(), None)
            .unwrap();
        let prepared = finish(&mut recovery, &cache, &foreign);
        assert_eq!(
            prepared.prepared.bank.get(SampleId(1)).unwrap().samples(),
            &[-0.25, -0.25, -0.125, -0.125]
        );
        assert_eq!(recovery.counters().decodes, 1);
        assert!(
            original.advance(&cache, &inventory).is_err(),
            "recovery changes the cache's current admitted source owner"
        );
    }
}

#[test]
fn moving_the_same_inventory_preserves_a_job_and_its_declared_hydration_identity() {
    let inventory = files(CHART, false);
    let cache = BrowserPreparationCache::new().unwrap();
    let mut job = cache
        .begin(&inventory, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let revision = inventory.inventory_revision();
    let mut relocated = Box::new(inventory);
    relocated.insert("pack/key.wav", wav()).unwrap();
    assert_eq!(relocated.inventory_revision(), revision);
    let prepared = finish(&mut job, &cache, &relocated);
    assert_eq!(
        prepared.prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, 0.25, -0.125, -0.125]
    );
}

#[test]
fn dropping_or_cancelling_a_staged_full_result_does_not_replace_the_prior_original_cache() {
    let mut inventory = files(CHART, true);
    inventory
        .insert(
            "pack/replacement.bms",
            b"#BPM 120\n#WAV01 key.wav\n#00011:01\n".to_vec(),
        )
        .unwrap();
    let cache = BrowserPreparationCache::new().unwrap();
    let mut original_job = cache
        .begin(&inventory, "pack/chart.bms", format(), 0, limits(), None)
        .unwrap();
    let original = finish(&mut original_job, &cache, &inventory);
    let pointer = original
        .prepared
        .bank
        .get(SampleId(1))
        .unwrap()
        .samples()
        .as_ptr();
    for cancel in [false, true] {
        let mut replacement = cache
            .begin(
                &inventory,
                "pack/replacement.bms",
                format(),
                0,
                limits(),
                None,
            )
            .unwrap();
        advance_to_asset_ready(&mut replacement, &cache, &inventory);
        let (candidate, admission) = replacement.stage_ready(&cache, &inventory).unwrap();
        if cancel {
            replacement.cancel();
            assert!(replacement.admit(&cache, &inventory, &admission).is_err());
        }
        // A derived consumer can fail after asset readiness. Dropping its result
        // and permit is the real refusal path; neither implies cache admission.
        drop(candidate);
        drop(admission);
        drop(replacement);
        let mut retry = cache
            .begin(&inventory, "pack/chart.bms", format(), 0, limits(), None)
            .unwrap();
        let ready = finish(&mut retry, &cache, &inventory);
        assert_eq!(retry.counters().decodes, 0);
        assert_eq!(
            ready
                .prepared
                .bank
                .get(SampleId(1))
                .unwrap()
                .samples()
                .as_ptr(),
            pointer
        );
    }
}
