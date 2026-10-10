//! Public phase/cancellation contracts, with genuine audio and fresh image work.
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleId},
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile},
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_bms_runtime::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryAssetLimits, MemoryFiles},
    image_assets::{ImageAssetLimits, ImagePreparationJob},
    preparation::{
        PreparationAdvance, PreparationCache, PreparationCacheLimits, PreparationCandidate,
        PreparationJob, PreparationProgress, PreparationRequest, PreparationStage,
    },
    video_assets::{VideoAssetLimits, VideoPreparationJob, VideoPreparationMode, VideoResource},
    ChannelPolicy,
};
use std::{
    borrow::Cow,
    cell::Cell,
    io,
    path::{Path, PathBuf},
};

const CHART: &[u8] = b"#BPM 120\n#WAV01 key.wav\n#00011:01\n";
fn wav() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&40u32.to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&24_000u32.to_le_bytes());
    v.extend_from_slice(&48_000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&4u32.to_le_bytes());
    v.extend_from_slice(&8192i16.to_le_bytes());
    v.extend_from_slice(&(-4096i16).to_le_bytes());
    v
}

struct VisualSource<'a> {
    source: beatkernel_bms_runtime::asset_source::MemoryAssetSource<'a>,
    reads: Cell<usize>,
    resolves: Cell<usize>,
    fail: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_file_video_jobs_publish_fresh_bounded_locators_without_encoded_reads() {
    use beatkernel_bms_runtime::asset_source::FileAssetSource;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let mut directory = None;
    for _ in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "beatkernel-preparation-video-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                directory = Some(Directory(path));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("fixture directory: {error}"),
        }
    }
    let directory = directory.expect("bounded temporary fixture directory");
    struct NativeSource {
        inner: FileAssetSource,
        resolves: Cell<usize>,
    }
    impl AssetSource for NativeSource {
        fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
            self.resolves.set(self.resolves.get() + 1);
            self.inner.resolve(name, policy)
        }
        fn read<'a>(&'a self, _: &Path, _: usize) -> io::Result<Cow<'a, [u8]>> {
            panic!("NativeFile mode must not acquire a second encoded payload")
        }
    }
    let source = NativeSource {
        inner: FileAssetSource::new(&directory.0).unwrap(),
        resolves: Cell::new(0),
    };
    let path = directory.0.join("clip.mp4");
    let chart = beatkernel_bms::parse("#BPM 120\n#BMP01 clip.mp4\n#00004:01\n", Default::default())
        .unwrap();
    for length in [32usize, 64] {
        std::fs::write(&path, vec![7; length]).unwrap();
        let mut job = VideoPreparationJob::new(
            &chart,
            VideoAssetLimits::default(),
            VideoPreparationMode::NativeFile,
        )
        .unwrap();
        let mut ready = None;
        for _ in 0..16 {
            if let Some(bank) = job
                .advance(&source, &mut |p| {
                    assert_eq!(p.stage, PreparationStage::Visual);
                    assert!(p.completed_assets <= p.total_assets);
                    Ok(())
                })
                .unwrap()
            {
                ready = Some(bank);
                break;
            }
        }
        let bank = ready.expect("actual fresh native movie descriptor");
        let descriptor = bank.resource(0).unwrap();
        assert_eq!(descriptor.encoded_bytes, length as u64);
        let VideoResource::File(locator) = &descriptor.data else {
            panic!("native mode cannot publish copied encoded bytes")
        };
        assert_eq!(*locator, path.canonicalize().unwrap());
        assert!(job.advance(&source, &mut |_| Ok(())).is_err());
    }
    assert_eq!(
        source.resolves.get(),
        2,
        "new jobs resolve current filesystem metadata"
    );
}
impl AssetSource for VisualSource<'_> {
    fn resolve(&self, name: &str, policy: AssetPathPolicy) -> io::Result<PathBuf> {
        self.resolves.set(self.resolves.get() + 1);
        if self.fail {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected visual path refusal",
            ));
        }
        self.source.resolve(name, policy)
    }
    fn read<'a>(&'a self, key: &Path, maximum: usize) -> io::Result<Cow<'a, [u8]>> {
        self.reads.set(self.reads.get() + 1);
        self.source.read(key, maximum)
    }
}

#[test]
fn fresh_video_job_has_real_phase_io_bounds_cancel_and_exact_one_shot_registration() {
    // Container-looking registration bytes; this test does not invoke a movie codec.
    let movie = b"\0\0\0\x18ftypisom\0\0\0\0isommp42original-payload";
    let chart = beatkernel_bms::parse(
        "#BPM 120\n#BMP01 clip.mp4\n#BMP02 clip.mp4\n#00004:0102\n",
        Default::default(),
    )
    .unwrap();
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert(
            "chart.bms",
            b"#BPM 120\n#BMP01 clip.mp4\n#BMP02 clip.mp4\n#00004:0102\n".to_vec(),
        )
        .unwrap();
    files.insert("clip.mp4", movie.to_vec()).unwrap();
    for occurrence in [1, 2] {
        let source = VisualSource {
            source: files.scope("chart.bms").unwrap(),
            reads: Cell::new(0),
            resolves: Cell::new(0),
            fail: false,
        };
        let mut job = VideoPreparationJob::new(
            &chart,
            VideoAssetLimits::default(),
            VideoPreparationMode::Encoded,
        )
        .unwrap();
        // Resolve is its own cooperative advance. Cancel at the following
        // genuine acquisition boundary, rather than counting resolve as a read.
        assert!(job.advance(&source, &mut |_| Ok(())).unwrap().is_none());
        assert_eq!(job.pending_resource(), Some(Path::new("clip.mp4")));
        assert_eq!(source.reads.get(), 0);
        let mut checkpoints = 0;
        assert!(job
            .advance(&source, &mut |p| {
                assert_eq!(p.stage, PreparationStage::Visual);
                checkpoints += 1;
                if checkpoints == occurrence {
                    Err("video checkpoint cancelled".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert_eq!(source.reads.get(), if occurrence == 1 { 0 } else { 1 });
        let reads = source.reads.get();
        assert!(job.advance(&source, &mut |_| Ok(())).is_err());
        assert_eq!(source.reads.get(), reads);
    }
    let source = VisualSource {
        source: files.scope("chart.bms").unwrap(),
        reads: Cell::new(0),
        resolves: Cell::new(0),
        fail: false,
    };
    let mut job = VideoPreparationJob::new(
        &chart,
        VideoAssetLimits::default(),
        VideoPreparationMode::Encoded,
    )
    .unwrap();
    assert_eq!(job.progress().total_assets, 2);
    let mut ready = None;
    for _ in 0..16 {
        let before = source.reads.get();
        if let Some(bank) = job
            .advance(&source, &mut |p| {
                assert_eq!(p.stage, PreparationStage::Visual);
                assert!(p.completed_assets <= p.total_assets);
                Ok(())
            })
            .unwrap()
        {
            ready = Some(bank);
            break;
        }
        assert!(source.reads.get() - before <= 1);
    }
    let first = ready.expect("actual movie registrations ready");
    assert_eq!(first.len(), 2);
    assert_eq!(first.resources().len(), 1);
    assert_eq!(source.reads.get(), 1);
    match &first.resource(0).unwrap().data {
        VideoResource::Encoded(bytes) => assert_eq!(bytes.as_ref(), movie),
        #[cfg(not(target_arch = "wasm32"))]
        VideoResource::File(_) => panic!("encoded mode cannot publish native file locator"),
    }
    assert!(job.advance(&source, &mut |_| Ok(())).is_err());
    let mut next = VideoPreparationJob::new(
        &chart,
        VideoAssetLimits::default(),
        VideoPreparationMode::Encoded,
    )
    .unwrap();
    let mut ready = None;
    for _ in 0..16 {
        if let Some(bank) = next.advance(&source, &mut |_| Ok(())).unwrap() {
            ready = Some(bank);
            break;
        }
    }
    let next = ready.unwrap();
    assert_eq!(
        source.reads.get(),
        2,
        "fresh video jobs reacquire current payload"
    );
    let (VideoResource::Encoded(a), VideoResource::Encoded(b)) = (
        &first.resource(0).unwrap().data,
        &next.resource(0).unwrap().data,
    ) else {
        panic!("expected independent encoded registrations")
    };
    assert_eq!(a.as_ref(), b.as_ref());
    assert_ne!(a.as_ptr(), b.as_ptr());
}

#[test]
fn actual_visual_job_permission_and_video_budget_errors_are_terminal() {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert(
            "image-chart.bms",
            b"#BPM 120\n#BMP01 image.bmp\n#00004:01\n".to_vec(),
        )
        .unwrap();
    files
        .insert(
            "movie-chart.bms",
            b"#BPM 120\n#BMP01 clip.mp4\n#00004:01\n".to_vec(),
        )
        .unwrap();
    files.insert("image.bmp", bitmap()).unwrap();
    files
        .insert("clip.mp4", b"original movie payload".to_vec())
        .unwrap();
    let image_chart = beatkernel_bms::parse(
        "#BPM 120\n#BMP01 image.bmp\n#00004:01\n",
        Default::default(),
    )
    .unwrap();
    let movie_chart =
        beatkernel_bms::parse("#BPM 120\n#BMP01 clip.mp4\n#00004:01\n", Default::default())
            .unwrap();
    let refused = VisualSource {
        source: files.scope("image-chart.bms").unwrap(),
        reads: Cell::new(0),
        resolves: Cell::new(0),
        fail: true,
    };
    let mut image = ImagePreparationJob::new(&image_chart, ImageAssetLimits::default()).unwrap();
    let mut failed = false;
    for _ in 0..16 {
        match image.advance(&refused, &mut |_| Ok(())) {
            Err(error) => {
                assert!(error.contains("injected visual path refusal"));
                failed = true;
                break;
            }
            Ok(None) => {}
            Ok(Some(_)) => break,
        }
    }
    assert!(failed);
    assert!(image.advance(&refused, &mut |_| Ok(())).is_err());
    let source = VisualSource {
        source: files.scope("movie-chart.bms").unwrap(),
        reads: Cell::new(0),
        resolves: Cell::new(0),
        fail: false,
    };
    let mut limits = VideoAssetLimits::default();
    limits.max_encoded_file_bytes = 1;
    let mut video =
        VideoPreparationJob::new(&movie_chart, limits, VideoPreparationMode::Encoded).unwrap();
    assert!(video.advance(&source, &mut |_| Ok(())).unwrap().is_none());
    assert_eq!(video.pending_resource(), Some(Path::new("clip.mp4")));
    assert!(video.advance(&source, &mut |_| Ok(())).is_err());
    let reads = source.reads.get();
    assert!(video.advance(&source, &mut |_| Ok(())).is_err());
    assert_eq!(source.reads.get(), reads);
}
struct Source {
    encoded: Vec<u8>,
    reads: Cell<usize>,
    resolves: Cell<usize>,
    refuse_resolve: bool,
    refuse_read: bool,
}
impl Source {
    fn new() -> Self {
        Self {
            encoded: wav(),
            reads: Cell::new(0),
            resolves: Cell::new(0),
            refuse_resolve: false,
            refuse_read: false,
        }
    }
}
impl AssetSource for Source {
    fn resolve(&self, name: &str, _: AssetPathPolicy) -> io::Result<PathBuf> {
        assert_eq!(name, "key.wav");
        self.resolves.set(self.resolves.get() + 1);
        if self.refuse_resolve {
            return Err(io::Error::other("injected resolver refusal"));
        }
        Ok(name.into())
    }
    fn read<'a>(&'a self, key: &Path, _: usize) -> io::Result<Cow<'a, [u8]>> {
        assert_eq!(key, Path::new("key.wav"));
        self.reads.set(self.reads.get() + 1);
        if self.refuse_read {
            return Err(io::Error::other("injected read refusal"));
        }
        Ok(Cow::Borrowed(&self.encoded))
    }
}
fn request() -> PreparationRequest {
    PreparationRequest {
        source_identity: "chart.bms".into(),
        seed: 0,
        format: AudioFormat::new(24_000, 1).unwrap(),
        limits: PcmLimits::new(1024, 4096, 8).unwrap(),
        channels: ChannelPolicy::Exact,
        paths: AssetPathPolicy::Exact,
    }
}
fn cache() -> PreparationCache {
    PreparationCache::new(PreparationCacheLimits::default()).unwrap()
}
fn finish(job: &mut PreparationJob, source: &dyn AssetSource) -> PreparationCandidate {
    for _ in 0..128 {
        match job.advance(source, None, &mut |_| Ok(())).unwrap() {
            PreparationAdvance::Pending(_) => {}
            PreparationAdvance::Ready(candidate) => return candidate,
        }
    }
    panic!("bounded fixture never became ready")
}
const AUDIO_STAGES: [PreparationStage; 8] = [
    PreparationStage::Parse,
    PreparationStage::Compile,
    PreparationStage::Resolve,
    PreparationStage::Read,
    PreparationStage::Decode,
    PreparationStage::Channels,
    PreparationStage::Admit,
    PreparationStage::Ready,
];

#[test]
fn progress_reports_real_one_phase_work_and_ready_transfers_exactly_once() {
    let source = Source::new();
    let cache = cache();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let mut checkpoints = Vec::new();
    let mut pending = Vec::new();
    let mut candidate = None;
    for _ in 0..128 {
        let before_reads = source.reads.get();
        match job
            .advance(&source, None, &mut |p| {
                assert!(p.completed_assets <= p.total_assets);
                checkpoints.push((p, source.reads.get()));
                Ok(())
            })
            .unwrap()
        {
            PreparationAdvance::Pending(p) => {
                pending.push(p);
                assert!(source.reads.get() - before_reads <= 1);
            }
            PreparationAdvance::Ready(value) => {
                candidate = Some(value);
                break;
            }
        }
    }
    let candidate = candidate.expect("actual ready candidate");
    for stage in AUDIO_STAGES {
        assert_eq!(
            checkpoints.iter().filter(|(p, _)| p.stage == stage).count(),
            2,
            "pre/post boundary {stage:?}"
        );
    }
    let reads: Vec<_> = checkpoints
        .iter()
        .filter(|(p, _)| p.stage == PreparationStage::Read)
        .map(|(_, reads)| *reads)
        .collect();
    assert_eq!(reads, [0, 1]);
    assert_eq!(
        candidate
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples(),
        &[0.25, -0.125]
    );
    assert_eq!(candidate.counters().reads, 1);
    assert_eq!(candidate.counters().decodes, 1);
    assert!(pending
        .iter()
        .all(|p| p.completed_assets <= 1 && p.total_assets <= 1));
    let mut calls = 0;
    assert!(job
        .advance(&source, None, &mut |_| {
            calls += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(
        calls, 0,
        "terminal advancement must not rerun callbacks or IO"
    );
    assert_eq!(source.reads.get(), 1);
}

#[test]
fn zero_asset_original_reports_zero_real_work_without_synthetic_io_or_decode() {
    let source = Source::new();
    let mut cache = cache();
    let mut job = cache.begin(request(), b"#BPM 120\n".to_vec()).unwrap();
    let candidate = finish(&mut job, &source);
    assert_eq!(candidate.prepared().bank.len(), 0);
    assert_eq!(candidate.counters().reads, 0);
    assert_eq!(candidate.counters().decodes, 0);
    assert_eq!(source.reads.get(), 0);
    assert_eq!(source.resolves.get(), 0);
    assert!(cache.commit(&candidate).unwrap());
    let mut warm = cache.begin(request(), b"#BPM 120\n".to_vec()).unwrap();
    let candidate = finish(&mut warm, &source);
    assert_eq!(candidate.counters().parses, 0);
    assert_eq!(candidate.counters().compiles, 0);
    assert_eq!(candidate.prepared().bank.len(), 0);
    assert_eq!(source.reads.get(), 0);
}

#[test]
fn cancellation_at_every_pre_and_post_audio_boundary_preserves_old_cache_and_is_terminal() {
    for stage in AUDIO_STAGES {
        for occurrence in [1, 2] {
            let mut cache = cache();
            let original_source = Source::new();
            let mut first = cache.begin(request(), CHART.to_vec()).unwrap();
            let original = finish(&mut first, &original_source);
            cache.commit(&original).unwrap();
            let retained = cache.retained_bytes();
            let source = Source::new();
            let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
            let mut seen = 0;
            let mut cancelled = false;
            for _ in 0..128 {
                let result = job.advance(&source, None, &mut |p| {
                    if p.stage == stage {
                        seen += 1;
                        if seen == occurrence {
                            return Err("injected cancellation checkpoint".into());
                        }
                    }
                    Ok(())
                });
                match result {
                    Err(error) => {
                        assert!(error.to_string().contains("injected cancellation"));
                        cancelled = true;
                        break;
                    }
                    Ok(PreparationAdvance::Pending(_)) => {}
                    Ok(PreparationAdvance::Ready(_)) => {
                        panic!("cancellation missed {stage:?}/{occurrence}")
                    }
                }
            }
            assert!(cancelled);
            assert_eq!(cache.retained_bytes(), retained);
            let side_effects = (source.resolves.get(), source.reads.get());
            let mut callbacks = 0;
            assert!(job
                .advance(&source, None, &mut |_| {
                    callbacks += 1;
                    Ok(())
                })
                .is_err());
            assert_eq!(callbacks, 0);
            assert_eq!((source.resolves.get(), source.reads.get()), side_effects);
            let mut recovered = cache.begin(request(), CHART.to_vec()).unwrap();
            let candidate = finish(&mut recovered, &original_source);
            assert_eq!(candidate.counters().decodes, 0);
        }
    }
}

#[test]
fn ready_then_cancel_foreign_commit_and_repeated_commit_do_not_replace_last_valid_original() {
    let mut cache = cache();
    let source = Source::new();
    let mut first_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut first_job, &source);
    assert!(cache.commit(&original).unwrap());
    let retained = cache.retained_bytes();
    assert!(cache.commit(&original).is_err());
    assert_eq!(cache.retained_bytes(), retained);
    let mut changed = request();
    changed.seed = 9;
    let mut job = cache.begin(changed, CHART.to_vec()).unwrap();
    let ready = finish(&mut job, &source);
    job.cancel();
    assert!(cache.commit(&ready).is_err());
    assert_eq!(cache.retained_bytes(), retained);
    let mut foreign = PreparationCache::new(PreparationCacheLimits::default()).unwrap();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let ready = finish(&mut job, &source);
    assert!(foreign.commit(&ready).is_err());
    assert_eq!(foreign.retained_bytes().pcm_bytes, 0);
    assert!(cache.commit(&ready).unwrap());
}

#[test]
fn split_prepared_output_can_be_validated_then_admitted_without_clone_and_exactly_once() {
    let mut cache = cache();
    let source = Source::new();
    let mut original_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut original_job, &source);
    cache.commit(&original).unwrap();
    let mut changed_source = Source::new();
    changed_source.encoded[44..46].copy_from_slice(&16_384i16.to_le_bytes());
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let candidate = finish(&mut job, &changed_source);
    let pointer = candidate
        .prepared()
        .bank
        .get(SampleId(1))
        .unwrap()
        .samples()
        .as_ptr();
    let (prepared, admission) = candidate.into_prepared_and_admission();
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        pointer
    );
    // Genuine derived replay identity validation runs on the moved owner before promotion.
    let (file, limits) = replay(&prepared);
    beatkernel_bms_runtime::replay_playback::validate_section_setup(
        &prepared.source,
        &file,
        limits,
    )
    .unwrap();
    let mut foreign = PreparationCache::new(PreparationCacheLimits::default()).unwrap();
    assert!(foreign.commit_admission(&admission).is_err());
    assert_eq!(foreign.retained_bytes().pcm_bytes, 0);
    assert!(cache.commit_admission(&admission).unwrap());
    let retained = cache.retained_bytes();
    assert!(cache.commit_admission(&admission).is_err());
    assert_eq!(cache.retained_bytes(), retained);
    let mut warm = cache.begin(request(), CHART.to_vec()).unwrap();
    let reused = finish(&mut warm, &changed_source);
    assert_eq!(reused.counters().decodes, 0);
    assert_eq!(
        reused
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        pointer
    );
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[0.5, -0.125]
    );
    assert_eq!(
        original.prepared().bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, -0.125]
    );
}

#[test]
fn abandoned_or_cancelled_split_admission_preserves_previous_cache_and_owned_candidate_pcm() {
    for cancel in [false, true] {
        let mut cache = cache();
        let source = Source::new();
        let mut original_job = cache.begin(request(), CHART.to_vec()).unwrap();
        let original = finish(&mut original_job, &source);
        cache.commit(&original).unwrap();
        let retained = cache.retained_bytes();
        let mut changed_source = Source::new();
        changed_source.encoded[44..46].copy_from_slice(&16_384i16.to_le_bytes());
        let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
        let candidate = finish(&mut job, &changed_source);
        let (prepared, admission) = candidate.into_prepared_and_admission();
        if cancel {
            job.cancel();
            assert!(cache.commit_admission(&admission).is_err());
        }
        // An outer section/policy/visual failure abandons this admission. The
        // candidate remains a real owned value, never an automatically published cache.
        drop(admission);
        assert_eq!(cache.retained_bytes(), retained);
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples(),
            &[0.5, -0.125]
        );
        let mut recovered = cache.begin(request(), CHART.to_vec()).unwrap();
        let recovered = finish(&mut recovered, &source);
        assert_eq!(recovered.counters().decodes, 0);
        assert_eq!(
            recovered
                .prepared()
                .bank
                .get(SampleId(1))
                .unwrap()
                .samples()
                .as_ptr(),
            original
                .prepared()
                .bank
                .get(SampleId(1))
                .unwrap()
                .samples()
                .as_ptr()
        );
    }
}

#[test]
fn cleared_cache_identity_rejects_stale_split_admission_after_a_new_valid_commit() {
    let mut cache = cache();
    let source = Source::new();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let candidate = finish(&mut job, &source);
    let (old_consumer, stale) = candidate.into_prepared_and_admission();
    cache.clear();
    let mut changed_source = Source::new();
    changed_source.encoded[44..46].copy_from_slice(&16_384i16.to_le_bytes());
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let current = finish(&mut job, &changed_source);
    cache.commit(&current).unwrap();
    let retained = cache.retained_bytes();
    assert!(cache.commit_admission(&stale).is_err());
    assert_eq!(cache.retained_bytes(), retained);
    assert_eq!(
        old_consumer.bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, -0.125]
    );
    let mut warm = cache.begin(request(), CHART.to_vec()).unwrap();
    assert_eq!(finish(&mut warm, &changed_source).counters().decodes, 0);
}

#[test]
fn valid_oversize_split_admission_is_uncached_and_keeps_previous_original() {
    let source = Source::new();
    let mut cache = PreparationCache::new(PreparationCacheLimits {
        max_encoded_bytes: CHART.len() + source.encoded.len(),
        max_pcm_bytes: 8,
        max_chart_bytes: CHART.len(),
    })
    .unwrap();
    let mut original_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut original_job, &source);
    cache.commit(&original).unwrap();
    let retained = cache.retained_bytes();
    let mut larger = Source::new();
    larger.encoded.extend_from_slice(&4096i16.to_le_bytes());
    larger.encoded[4..8].copy_from_slice(&42u32.to_le_bytes());
    larger.encoded[40..44].copy_from_slice(&6u32.to_le_bytes());
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let candidate = finish(&mut job, &larger);
    let (prepared, admission) = candidate.into_prepared_and_admission();
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, -0.125, 0.125]
    );
    assert!(!cache.commit_admission(&admission).unwrap());
    assert_eq!(cache.retained_bytes(), retained);
    assert!(cache.commit_admission(&admission).is_err());
    let mut warm = cache.begin(request(), CHART.to_vec()).unwrap();
    let recovered = finish(&mut warm, &source);
    assert_eq!(recovered.counters().decodes, 0);
    assert_eq!(
        recovered
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples(),
        &[0.25, -0.125]
    );
}

#[test]
fn shared_dependency_plan_still_validates_current_replay_before_asset_io() {
    let mut cache = cache();
    let source = Source::new();
    let mut original_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut original_job, &source);
    cache.commit(&original).unwrap();
    let plan = original_job.shared_plan().unwrap();
    let (valid, limits) = replay(original.prepared());
    for scenario in 0..3 {
        let source = Source::new();
        let mut invalid = valid.clone();
        match scenario {
            0 => invalid.header.chart_identity.push(0),
            1 => invalid.header.rules_identity.push(0),
            2 => invalid.header.options = b"bms-judge-profile/v3:".to_vec(),
            _ => unreachable!(),
        }
        let mut job = cache
            .begin_with_plan(request(), CHART.to_vec(), &plan)
            .unwrap();
        let mut refused = false;
        for _ in 0..128 {
            match job.advance(&source, Some((&invalid, limits)), &mut |_| Ok(())) {
                Err(_) => {
                    refused = true;
                    break;
                }
                Ok(PreparationAdvance::Pending(_)) => {}
                Ok(PreparationAdvance::Ready(_)) => break,
            }
        }
        assert!(refused);
        assert_eq!(source.reads.get(), 0);
        assert_eq!(source.resolves.get(), 0);
    }
    let source = Source::new();
    let mut job = cache
        .begin_with_plan(request(), CHART.to_vec(), &plan)
        .unwrap();
    let mut ready = None;
    for _ in 0..128 {
        match job
            .advance(&source, Some((&valid, limits)), &mut |_| Ok(()))
            .unwrap()
        {
            PreparationAdvance::Pending(_) => {}
            PreparationAdvance::Ready(candidate) => {
                ready = Some(candidate);
                break;
            }
        }
    }
    let ready = ready.unwrap();
    assert_eq!(ready.counters().parses, 0);
    assert_eq!(ready.counters().compiles, 0);
    assert_eq!(ready.counters().decodes, 0);
    assert_eq!(
        ready.prepared().bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, -0.125]
    );
}

#[test]
fn real_parse_compile_resolve_read_decode_channels_and_bank_failures_preserve_previous_cache() {
    let mut cache = cache();
    let original_source = Source::new();
    let mut first = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut first, &original_source);
    cache.commit(&original).unwrap();
    let retained = cache.retained_bytes();
    for scenario in 0..7 {
        let mut source = Source::new();
        let mut chart = CHART.to_vec();
        let mut options = request();
        match scenario {
            0 => chart = b"#BPM 0\n".to_vec(),
            1 => chart = b"#BPM 0.000001\n#WAV01 key.wav\n#99911:01\n".to_vec(),
            2 => source.refuse_resolve = true,
            3 => source.refuse_read = true,
            4 => source.encoded = b"not a supported audio stream".to_vec(),
            5 => options.format = AudioFormat::new(24_000, 2).unwrap(),
            6 => {
                chart = b"#BPM 120\n#WAV01 key.wav\n#WAV02 key.wav\n#00011:0102\n".to_vec();
                options.limits = PcmLimits::new(8, 15, 8).unwrap();
            }
            _ => unreachable!(),
        }
        let mut job = cache.begin(options, chart).unwrap();
        let mut error = None;
        for _ in 0..128 {
            match job.advance(&source, None, &mut |_| Ok(())) {
                Err(value) => {
                    error = Some(value);
                    break;
                }
                Ok(PreparationAdvance::Pending(_)) => {}
                Ok(PreparationAdvance::Ready(_)) => break,
            }
        }
        assert!(error.is_some(), "actual failure scenario {scenario}");
        assert_eq!(cache.retained_bytes(), retained);
        assert!(job.advance(&source, None, &mut |_| Ok(())).is_err());
        if scenario <= 1 {
            assert_eq!(source.reads.get(), 0);
            assert_eq!(source.resolves.get(), 0);
        }
    }
}

fn replay(prepared: &beatkernel_bms_runtime::PreparedBms) -> (ReplayFile, ReplayCodecLimits) {
    let limits = beatkernel_bms_runtime::competition_live::replay_limits().unwrap();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        profile,
    )
    .unwrap();
    let capture = beatkernel_bms_runtime::replay_capture::LiveReplayCapture::new_section(
        &judge,
        ClockDomainId(17),
        limits,
        Timestamp::ZERO,
        0,
        None,
    )
    .unwrap();
    (capture.into_file(), limits)
}

#[test]
fn warm_original_revalidates_current_replay_before_any_asset_lookup_or_acquisition() {
    let mut cache = cache();
    let source = Source::new();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut job, &source);
    cache.commit(&original).unwrap();
    let (valid, limits) = replay(original.prepared());
    for scenario in 0..3 {
        let mut invalid = valid.clone();
        match scenario {
            0 => invalid.header.chart_identity.push(0),
            1 => invalid.header.options = b"bms-judge-profile/v3:".to_vec(),
            2 => invalid.header.rules_identity.push(0),
            _ => unreachable!(),
        }
        let source = Source::new();
        let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
        let mut refused = false;
        for _ in 0..128 {
            match job.advance(&source, Some((&invalid, limits)), &mut |_| Ok(())) {
                Err(_) => {
                    refused = true;
                    break;
                }
                Ok(PreparationAdvance::Ready(_)) => break,
                Ok(PreparationAdvance::Pending(_)) => {}
            }
        }
        assert!(refused);
        assert_eq!(source.reads.get(), 0);
        assert_eq!(source.resolves.get(), 0);
    }
    let source = Source::new();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let mut ready = None;
    for _ in 0..128 {
        match job
            .advance(&source, Some((&valid, limits)), &mut |_| Ok(()))
            .unwrap()
        {
            PreparationAdvance::Ready(value) => {
                ready = Some(value);
                break;
            }
            PreparationAdvance::Pending(_) => {}
        }
    }
    let ready = ready.expect("fresh valid replay admits cached original");
    assert_eq!(ready.counters().decodes, 0);
    assert_eq!(
        ready.prepared().bank.get(SampleId(1)).unwrap().samples(),
        &[0.25, -0.125]
    );
}

fn bitmap() -> Vec<u8> {
    let mut bytes = vec![0; 58];
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&58u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&4u32.to_le_bytes());
    bytes[54..58].copy_from_slice(&[0, 0, 255, 0]);
    bytes
}

#[test]
fn visual_phase_is_real_fresh_image_work_with_pre_post_cancel_and_terminal_transfer() {
    let chart = beatkernel_bms::parse(
        "#BPM 120\n#BMP01 image.bmp\n#00004:01\n",
        Default::default(),
    )
    .unwrap();
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files
        .insert(
            "chart.bms",
            b"#BPM 120\n#BMP01 image.bmp\n#00004:01\n".to_vec(),
        )
        .unwrap();
    files.insert("image.bmp", bitmap()).unwrap();
    let source = files.scope("chart.bms").unwrap();
    for occurrence in [1, 2] {
        let mut job = ImagePreparationJob::new(&chart, ImageAssetLimits::default()).unwrap();
        let mut callbacks = 0;
        assert!(job
            .advance(&source, &mut |p: PreparationProgress| {
                assert_eq!(p.stage, PreparationStage::Visual);
                callbacks += 1;
                if callbacks == occurrence {
                    Err("visual checkpoint cancelled".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert!(job.advance(&source, &mut |_| Ok(())).is_err());
    }
    let mut job = ImagePreparationJob::new(&chart, ImageAssetLimits::default()).unwrap();
    let mut ready = None;
    for _ in 0..32 {
        if let Some(value) = job
            .advance(&source, &mut |p| {
                assert_eq!(p.stage, PreparationStage::Visual);
                assert!(p.completed_assets <= p.total_assets);
                Ok(())
            })
            .unwrap()
        {
            ready = Some(value);
            break;
        }
    }
    let first = ready.expect("actual fresh image ready");
    let image = first.get(beatkernel_bms::ImageId(1)).unwrap();
    assert_eq!(&image.pixels()[..4], &[255, 0, 0, 255]);
    assert!(job.advance(&source, &mut |_| Ok(())).is_err());
    let mut second = ImagePreparationJob::new(&chart, ImageAssetLimits::default()).unwrap();
    let mut second_ready = None;
    for _ in 0..32 {
        if let Some(v) = second.advance(&source, &mut |_| Ok(())).unwrap() {
            second_ready = Some(v);
            break;
        }
    }
    let second = second_ready.unwrap();
    let other = second.get(beatkernel_bms::ImageId(1)).unwrap();
    assert_eq!(image.pixels(), other.pixels());
    assert_ne!(
        image.pixels().as_ptr(),
        other.pixels().as_ptr(),
        "visual payloads remain fresh owners"
    );
}
