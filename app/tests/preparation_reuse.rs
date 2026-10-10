//! Public preparation output and source freshness, independent of native IO.
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleId},
    judge::JudgeStage,
};
use beatkernel_bms_runtime::{
    asset_paths::AssetPathPolicy,
    asset_source::{AssetSource, MemoryAssetLimits, MemoryFiles},
    preparation::{
        PreparationAdvance, PreparationCache, PreparationCacheLimits, PreparationCandidate,
        PreparationJob, PreparationPlan, PreparationRequest,
    },
    AssetDecoder, ChannelPolicy, DefaultAssetDecoder, PreparedBms,
};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    error::Error,
    io,
    path::{Path, PathBuf},
};

const CHART: &[u8] = b"#BPM 120\n#WAV01 key.wav\n#00011:01\n";
const ALIASES: &[u8] = b"#BPM 120\n#WAV01 key.wav\n#WAV02 key.wav\n#00011:0102\n";
fn limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}
fn format() -> AudioFormat {
    AudioFormat::new(24_000, 1).unwrap()
}
fn wav(channels: u16, samples: &[i16]) -> Vec<u8> {
    let payload = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + payload).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&24_000u32.to_le_bytes());
    bytes.extend_from_slice(&(24_000u32 * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
struct Source {
    bytes: RefCell<Vec<u8>>,
    next_after_read: RefCell<Option<Vec<u8>>>,
    resolved: Cell<usize>,
    reads: Cell<usize>,
    fail_resolve: Cell<bool>,
    fail_read: Cell<bool>,
    key: PathBuf,
}
impl Source {
    fn new(samples: &[i16]) -> Self {
        Self {
            bytes: RefCell::new(wav(1, samples)),
            next_after_read: RefCell::new(None),
            resolved: Cell::new(0),
            reads: Cell::new(0),
            fail_resolve: Cell::new(false),
            fail_read: Cell::new(false),
            key: "songs/key.wav".into(),
        }
    }
}
impl AssetSource for Source {
    fn resolve(&self, name: &str, _: AssetPathPolicy) -> io::Result<PathBuf> {
        assert_eq!(name, "key.wav");
        self.resolved.set(self.resolved.get() + 1);
        if self.fail_resolve.get() {
            return Err(io::Error::other("injected resolve failure"));
        }
        Ok(self.key.clone())
    }
    fn read<'a>(&'a self, key: &Path, maximum: usize) -> io::Result<Cow<'a, [u8]>> {
        assert_eq!(key, self.key);
        self.reads.set(self.reads.get() + 1);
        if self.fail_read.get() {
            return Err(io::Error::other("injected read failure"));
        }
        let acquired = self.bytes.borrow().clone();
        assert!(acquired.len() <= maximum);
        if let Some(next) = self.next_after_read.borrow_mut().take() {
            *self.bytes.borrow_mut() = next;
        }
        Ok(Cow::Owned(acquired))
    }
}
struct CountingDecoder {
    calls: Cell<usize>,
}
impl AssetDecoder for CountingDecoder {
    fn decode(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: PcmLimits,
    ) -> Result<PcmSample, Box<dyn Error>> {
        self.calls.set(self.calls.get() + 1);
        DefaultAssetDecoder.decode(path, bytes, limits)
    }
}
fn bits(value: &PreparedBms, sample: u64) -> Vec<u32> {
    value
        .bank
        .get(SampleId(sample))
        .unwrap()
        .samples()
        .iter()
        .map(|v| v.to_bits())
        .collect()
}
fn request() -> PreparationRequest {
    PreparationRequest {
        source_identity: "songs/chart.bms".into(),
        seed: 0,
        format: format(),
        limits: limits(),
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
    panic!("bounded fixture did not reach its actual ready state")
}

fn query_plan(
    options: PreparationRequest,
    chart: &[u8],
    source: &Source,
) -> (
    PreparationPlan,
    *const beatkernel_bms_runtime::preparation::ParsedPreparation,
) {
    let mut query = PreparationJob::uncached(options, chart.to_vec()).unwrap();
    for _ in 0..8 {
        assert!(matches!(
            query.advance(source, None, &mut |_| Ok(())).unwrap(),
            PreparationAdvance::Pending(_)
        ));
        if let Some(parsed) = query.parsed() {
            let pointer = parsed as *const _;
            assert_eq!(query.counters().parses, 1);
            assert_eq!(query.counters().compiles, 1);
            assert_eq!(source.reads.get(), 0);
            assert_eq!(source.resolved.get(), 0);
            let plan = query.shared_plan().expect("owned dependency query plan");
            query.cancel();
            return (plan, pointer);
        }
    }
    panic!("canonical dependency query never produced its parsed plan")
}

#[test]
fn dependency_query_plan_continuation_shares_exact_parsed_owner_without_second_parse_or_compile() {
    let source = Source::new(&[8192, -4096]);
    let mut cache = cache();
    let (plan, pointer) = query_plan(request(), CHART, &source);
    let mut job = cache
        .begin_with_plan(request(), CHART.to_vec(), &plan)
        .unwrap();
    let candidate = finish(&mut job, &source);
    assert_eq!(job.parsed().unwrap() as *const _, pointer);
    assert_eq!(candidate.counters().parses, 0);
    assert_eq!(candidate.counters().compiles, 0);
    assert_eq!(candidate.counters().decodes, 1);
    assert_eq!(source.reads.get(), 1);
    assert_eq!(
        bits(candidate.prepared(), 1),
        vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
    );
    cache.commit(&candidate).unwrap();
    let mut warm = cache
        .begin_with_plan(request(), CHART.to_vec(), &plan)
        .unwrap();
    let reused = finish(&mut warm, &source);
    assert_eq!(warm.parsed().unwrap() as *const _, pointer);
    assert_eq!(reused.counters().parses, 0);
    assert_eq!(reused.counters().compiles, 0);
    assert_eq!(reused.counters().decodes, 0);
    assert_eq!(
        reused
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        candidate
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr()
    );
}

#[test]
fn dependency_plan_source_seed_and_exact_chart_byte_mismatches_fall_back_to_valid_cold_parse() {
    let source = Source::new(&[8192, -4096]);
    let cache = cache();
    let (plan, pointer) = query_plan(request(), CHART, &source);
    for scenario in 0..3 {
        let mut options = request();
        let mut chart = CHART.to_vec();
        match scenario {
            0 => options.source_identity = "different/chart.bms".into(),
            1 => options.seed = 7,
            2 => chart = b"#VOLWAV 200\n#BPM 120\n#WAV01 key.wav\n#00011:01\n".to_vec(),
            _ => unreachable!(),
        }
        let mut job = cache.begin_with_plan(options, chart, &plan).unwrap();
        let candidate = finish(&mut job, &source);
        assert_ne!(job.parsed().unwrap() as *const _, pointer);
        assert_eq!(candidate.counters().parses, 1);
        assert_eq!(candidate.counters().compiles, 1);
        assert_eq!(candidate.counters().decodes, 1);
        assert_eq!(
            bits(candidate.prepared(), 1),
            vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
        );
        assert_eq!(
            candidate.prepared().sounds[0].gain.to_bits(),
            if scenario == 2 {
                2f32.to_bits()
            } else {
                1f32.to_bits()
            }
        );
    }
}

#[test]
fn reused_dependency_plan_revalidates_output_channels_and_narrower_reference_limits() {
    let source = Source::new(&[8192, -4096]);
    let cache = cache();
    let (plan, pointer) = query_plan(request(), ALIASES, &source);
    let mut changed = request();
    changed.format = AudioFormat::new(48_000, 2).unwrap();
    changed.channels = ChannelPolicy::MonoToStereo;
    let mut job = cache
        .begin_with_plan(changed, ALIASES.to_vec(), &plan)
        .unwrap();
    let ready = finish(&mut job, &source);
    assert_eq!(job.parsed().unwrap() as *const _, pointer);
    assert_eq!(ready.counters().parses, 0);
    assert_eq!(ready.counters().compiles, 0);
    assert_eq!(
        ready.prepared().bank.format(),
        AudioFormat::new(48_000, 2).unwrap()
    );
    assert_eq!(
        bits(ready.prepared(), 1),
        vec![
            0.25f32.to_bits(),
            0.25f32.to_bits(),
            (-0.125f32).to_bits(),
            (-0.125f32).to_bits()
        ]
    );
    let source = Source::new(&[8192, -4096]);
    let mut narrow = request();
    narrow.limits = PcmLimits::new(1024, 4096, 1).unwrap();
    let mut job = cache
        .begin_with_plan(narrow, ALIASES.to_vec(), &plan)
        .unwrap();
    let mut refused = false;
    for _ in 0..128 {
        match job.advance(&source, None, &mut |_| Ok(())) {
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
    assert_eq!(source.resolved.get(), 0);
    let mut exact_stereo = request();
    exact_stereo.format = AudioFormat::new(24_000, 2).unwrap();
    let mut job = cache
        .begin_with_plan(exact_stereo, ALIASES.to_vec(), &plan)
        .unwrap();
    let mut refused = false;
    for _ in 0..128 {
        match job.advance(&source, None, &mut |_| Ok(())) {
            Err(_) => {
                refused = true;
                break;
            }
            Ok(PreparationAdvance::Pending(_)) => {}
            Ok(PreparationAdvance::Ready(_)) => break,
        }
    }
    assert!(
        refused,
        "a parsed plan cannot authorize unrequested channel conversion"
    );
}

#[test]
fn exact_original_reuses_parsed_plan_and_pcm_but_acquires_current_resource_once() {
    let mut cache = cache();
    let source = Source::new(&[8192, -4096]);
    let mut cold_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let cold = finish(&mut cold_job, &source);
    let plan = cold_job.parsed().unwrap() as *const _;
    assert_eq!(cold.counters().parses, 1);
    assert_eq!(cold.counters().compiles, 1);
    assert_eq!(cold.counters().reads, 1);
    assert_eq!(cold.counters().decodes, 1);
    let pointer = cold
        .prepared()
        .bank
        .get(SampleId(1))
        .unwrap()
        .samples()
        .as_ptr();
    assert!(cache.commit(&cold).unwrap());
    let mut warm_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let warm = finish(&mut warm_job, &source);
    assert_eq!(warm_job.parsed().unwrap() as *const _, plan);
    assert_eq!(warm.counters().parses, 0);
    assert_eq!(warm.counters().compiles, 0);
    assert_eq!(warm.counters().reads, 1);
    assert_eq!(warm.counters().decodes, 0);
    assert_eq!(source.reads.get(), 2);
    assert_eq!(source.resolved.get(), 2);
    assert_eq!(
        warm.prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        pointer
    );
    assert_eq!(bits(cold.prepared(), 1), bits(warm.prepared(), 1));
    assert_eq!(cold.prepared().sounds.len(), warm.prepared().sounds.len());
    assert_eq!(
        cold.prepared().sounds[0].gain.to_bits(),
        warm.prepared().sounds[0].gain.to_bits()
    );
    // One consumer's independent mutable extraction cannot mutate another.
    let mut extracted = warm
        .into_prepared()
        .bank
        .get(SampleId(1))
        .unwrap()
        .try_clone(limits())
        .unwrap()
        .into_samples();
    extracted[0] = 99.0;
    assert_eq!(
        bits(cold.prepared(), 1),
        vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
    );
    drop(cache);
    assert_eq!(
        bits(cold.prepared(), 1),
        vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
    );
}

#[test]
fn compare_and_decode_use_the_same_single_acquired_bytes_even_when_source_changes() {
    let mut cache = cache();
    let source = Source::new(&[8192, -4096]);
    *source.next_after_read.borrow_mut() = Some(wav(1, &[4096, -8192]));
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut job, &source);
    assert_eq!(source.reads.get(), 1);
    assert_eq!(
        bits(original.prepared(), 1),
        vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
    );
    assert!(cache.commit(&original).unwrap());
    let mut replacement_job = cache.begin(request(), CHART.to_vec()).unwrap();
    let replacement = finish(&mut replacement_job, &source);
    assert_eq!(source.reads.get(), 2);
    assert_eq!(replacement.counters().decodes, 1);
    assert_eq!(
        bits(replacement.prepared(), 1),
        vec![0.125f32.to_bits(), (-0.25f32).to_bits()]
    );
    assert_ne!(
        original
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        replacement
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr()
    );
    assert!(cache.commit(&replacement).unwrap());
    assert_eq!(
        bits(original.prepared(), 1),
        vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
    );
}

#[test]
fn identity_seed_output_channel_path_policy_and_limits_never_authorize_wrong_pcm_reuse() {
    let mut cache = cache();
    let source = Source::new(&[8192, -4096]);
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut job, &source);
    cache.commit(&original).unwrap();
    let mut variants = Vec::new();
    let mut identity = request();
    identity.source_identity = "different/chart.bms".into();
    variants.push(identity);
    let mut seed = request();
    seed.seed = 7;
    variants.push(seed);
    let mut output = request();
    output.format = AudioFormat::new(48_000, 1).unwrap();
    variants.push(output);
    let mut paths = request();
    paths.paths = AssetPathPolicy::AudioVariants;
    variants.push(paths);
    let mut capacity = request();
    capacity.limits = PcmLimits::new(512, 2048, 4).unwrap();
    variants.push(capacity);
    let mut policy = request();
    policy.channels = ChannelPolicy::MonoToStereo;
    variants.push(policy);
    for variant in variants {
        let mut job = cache.begin(variant, CHART.to_vec()).unwrap();
        let candidate = finish(&mut job, &source);
        assert_eq!(candidate.counters().decodes, 1);
        assert_eq!(bits(candidate.prepared(), 1), bits(original.prepared(), 1));
    }
    // Equal bytes at a changed resolved canonical path are a new resource identity.
    let mut renamed = Source::new(&[8192, -4096]);
    renamed.key = "songs/renamed.wav".into();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    assert_eq!(finish(&mut job, &renamed).counters().decodes, 1);
}

#[test]
fn channel_conversion_and_alias_sharing_preserve_logical_bank_limits() {
    let mut cache = cache();
    let source = Source::new(&[8192, -4096]);
    let mut options = request();
    options.format = AudioFormat::new(24_000, 2).unwrap();
    options.channels = ChannelPolicy::MonoToStereo;
    let mut job = cache.begin(options, ALIASES.to_vec()).unwrap();
    let converted = finish(&mut job, &source);
    assert_eq!(source.reads.get(), 1);
    assert_eq!(converted.counters().decodes, 1);
    assert_eq!(
        bits(converted.prepared(), 1),
        vec![
            0.25f32.to_bits(),
            0.25f32.to_bits(),
            (-0.125f32).to_bits(),
            (-0.125f32).to_bits()
        ]
    );
    assert_eq!(bits(converted.prepared(), 1), bits(converted.prepared(), 2));
    assert_eq!(
        converted
            .prepared()
            .bank
            .get(SampleId(1))
            .unwrap()
            .samples()
            .as_ptr(),
        converted
            .prepared()
            .bank
            .get(SampleId(2))
            .unwrap()
            .samples()
            .as_ptr()
    );
    assert!(cache.commit(&converted).unwrap());
    for (per_asset, total, count) in [(15, 32, 8), (16, 31, 8), (16, 32, 1)] {
        let mut options = request();
        options.format = AudioFormat::new(24_000, 2).unwrap();
        options.channels = ChannelPolicy::MonoToStereo;
        options.limits = PcmLimits::new(per_asset, total, count).unwrap();
        let mut job = cache.begin(options, ALIASES.to_vec()).unwrap();
        let mut rejected = false;
        for _ in 0..128 {
            match job.advance(&source, None, &mut |_| Ok(())) {
                Err(_) => {
                    rejected = true;
                    break;
                }
                Ok(PreparationAdvance::Ready(_)) => break,
                Ok(PreparationAdvance::Pending(_)) => {}
            }
        }
        assert!(
            rejected,
            "physical alias sharing must not relieve logical PCM limits"
        );
    }
    let mut strict = request();
    strict.format = AudioFormat::new(24_000, 2).unwrap();
    let mut job = cache.begin(strict, CHART.to_vec()).unwrap();
    let mut failed = false;
    for _ in 0..128 {
        match job.advance(&source, None, &mut |_| Ok(())) {
            Err(_) => {
                failed = true;
                break;
            }
            Ok(PreparationAdvance::Ready(_)) => break,
            Ok(PreparationAdvance::Pending(_)) => {}
        }
    }
    assert!(failed, "Exact never infers mono-to-stereo conversion");
}

#[test]
fn exact_and_one_byte_over_retention_budgets_do_not_change_legal_preparation() {
    let source = Source::new(&[8192, -4096]);
    let encoded = CHART.len() + source.bytes.borrow().len();
    for (encoded_cap, pcm_cap, chart_cap, retain) in [
        (encoded, 8, CHART.len(), true),
        (encoded - 1, 8, CHART.len(), false),
        (encoded, 7, CHART.len(), false),
        (encoded, 8, CHART.len() - 1, false),
    ] {
        let mut cache = PreparationCache::new(PreparationCacheLimits {
            max_encoded_bytes: encoded_cap,
            max_pcm_bytes: pcm_cap,
            max_chart_bytes: chart_cap,
        })
        .unwrap();
        let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
        let candidate = finish(&mut job, &source);
        assert_eq!(
            bits(candidate.prepared(), 1),
            vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
        );
        assert_eq!(cache.commit(&candidate).unwrap(), retain);
        let bytes = cache.retained_bytes();
        assert!(bytes.encoded_bytes <= encoded_cap);
        assert!(bytes.pcm_bytes <= pcm_cap);
        assert!(bytes.chart_bytes <= chart_cap);
        let mut second = cache.begin(request(), CHART.to_vec()).unwrap();
        let candidate = finish(&mut second, &source);
        assert_eq!(candidate.counters().decodes, if retain { 0 } else { 1 });
    }
}

#[test]
fn oversized_new_original_is_valid_uncached_and_preserves_last_usable_cache() {
    let source = Source::new(&[8192, -4096]);
    let mut cache = PreparationCache::new(PreparationCacheLimits {
        max_encoded_bytes: CHART.len() + source.bytes.borrow().len(),
        max_pcm_bytes: 8,
        max_chart_bytes: CHART.len(),
    })
    .unwrap();
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let original = finish(&mut job, &source);
    assert!(cache.commit(&original).unwrap());
    let retained = cache.retained_bytes();
    let large = Source::new(&[8192, -4096, 4096]);
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let candidate = finish(&mut job, &large);
    assert_eq!(bits(candidate.prepared(), 1).len(), 3);
    assert!(!cache.commit(&candidate).unwrap());
    assert_eq!(cache.retained_bytes(), retained);
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    let recovered = finish(&mut job, &source);
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

#[test]
fn custom_decoders_are_never_cached_even_when_they_delegate_to_default_codec() {
    let source = Source::new(&[8192, -4096]);
    let decoder = CountingDecoder {
        calls: Cell::new(0),
    };
    let mut cache = cache();
    for _ in 0..2 {
        let mut job = PreparationJob::uncached(request(), CHART.to_vec()).unwrap();
        let mut ready = None;
        for _ in 0..128 {
            match job
                .advance_with_decoder(&source, &decoder, None, &mut |_| Ok(()))
                .unwrap()
            {
                PreparationAdvance::Pending(_) => {}
                PreparationAdvance::Ready(candidate) => {
                    ready = Some(candidate);
                    break;
                }
            }
        }
        let candidate = ready.expect("custom decoder actual ready");
        assert_eq!(
            bits(candidate.prepared(), 1),
            vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
        );
        assert!(!cache.commit(&candidate).unwrap());
    }
    assert_eq!(decoder.calls.get(), 2);
    assert_eq!(source.reads.get(), 2);
    assert_eq!(cache.retained_bytes().pcm_bytes, 0);
    let mut job = cache.begin(request(), CHART.to_vec()).unwrap();
    assert!(job
        .advance_with_decoder(&source, &decoder, None, &mut |_| Ok(()))
        .is_err());
    assert_eq!(
        decoder.calls.get(),
        2,
        "a custom decoder cannot silently consume cached ownership"
    );
}

#[test]
fn inventory_revision_changes_only_for_new_normalized_keys_and_refusal_is_atomic() {
    let mut files = MemoryFiles::new(MemoryAssetLimits {
        max_files: 3,
        max_file_bytes: 64,
        max_total_bytes: 128,
        max_path_bytes: 64,
    })
    .unwrap();
    let zero = files.inventory_revision();
    files.declare_file("songs/key.wav", 48).unwrap();
    let declared = files.inventory_revision();
    assert!(declared > zero);
    assert!(files.insert("songs/key.wav", vec![0; 47]).is_err());
    assert_eq!(files.inventory_revision(), declared);
    assert_eq!(files.total_bytes(), 48);
    files
        .insert("songs/key.wav", wav(1, &[8192, -4096]))
        .unwrap();
    assert_eq!(files.inventory_revision(), declared);
    assert_eq!(files.total_bytes(), 48);
    for name in ["songs/./key.wav", "../escape.wav", "songs/key.wav/child"] {
        assert!(files.declare_file(name, 1).is_err());
        assert_eq!(files.inventory_revision(), declared);
        assert_eq!(files.len(), 1);
        assert_eq!(files.total_bytes(), 48);
    }
    files.insert("songs/second.wav", vec![0; 1]).unwrap();
    assert!(files.inventory_revision() > declared);
}

#[test]
fn changed_chart_metadata_recompiles_original_gain_and_bgm_without_mutating_old_consumers() {
    let mut cache = cache();
    let source = Source::new(&[8192, -4096]);
    let original_chart = b"#VOLWAV 100\n#BPM 120\n#WAV01 key.wav\n#00011:01\n#00001:01\n";
    let changed_chart = b"#VOLWAV 200\n#BPM 120\n#WAV01 key.wav\n#00011:01\n#00001:01\n";
    let mut job = cache.begin(request(), original_chart.to_vec()).unwrap();
    let original = finish(&mut job, &source);
    cache.commit(&original).unwrap();
    let mut job = cache.begin(request(), changed_chart.to_vec()).unwrap();
    let changed = finish(&mut job, &source);
    assert_eq!(changed.counters().parses, 1);
    assert_eq!(changed.counters().compiles, 1);
    assert_eq!(original.prepared().sounds[0].gain.to_bits(), 1f32.to_bits());
    assert_eq!(changed.prepared().sounds[0].gain.to_bits(), 2f32.to_bits());
    let beatkernel::audio::AudioCommand::Play { gain, .. } = changed.prepared().bgm_commands[0]
    else {
        panic!("actual BGM playback mapping")
    };
    assert_eq!(gain.to_bits(), 2f32.to_bits());
    assert_eq!(bits(original.prepared(), 1), bits(changed.prepared(), 1));
}

#[test]
fn original_reference_plan_preserves_invisible_and_nonfatal_mine_zero_sample_aliases() {
    let chart = b"#BPM 120\n#WAV00 key.wav\n#WAV01 key.wav\n#00031:01\n#001D1:01\n";
    let mut cache = cache();
    let source = Source::new(&[8192, -4096]);
    let mut job = cache.begin(request(), chart.to_vec()).unwrap();
    let original = finish(&mut job, &source);
    assert_eq!(original.prepared().source.invisible.len(), 1);
    assert_eq!(original.prepared().source.mines.len(), 1);
    assert_eq!(original.prepared().bank.len(), 2);
    assert!(original.prepared().sounds.is_empty());
    assert_eq!(source.reads.get(), 1);
    assert_eq!(original.counters().decodes, 1);
    assert_eq!(bits(original.prepared(), 0), bits(original.prepared(), 1));
    cache.commit(&original).unwrap();
    let mut job = cache.begin(request(), chart.to_vec()).unwrap();
    let warm = finish(&mut job, &source);
    assert_eq!(warm.counters().decodes, 0);
    assert_eq!(warm.prepared().bank.len(), 2);
    assert_eq!(bits(warm.prepared(), 0), bits(original.prepared(), 0));
}

#[test]
fn compatibility_preparation_keeps_alias_gain_and_real_pcm_bits() {
    let source = Source::new(&[8192, -4096]);
    let prepared = beatkernel_bms_runtime::prepare_from_source(
        ALIASES,
        &source,
        format(),
        limits(),
        ChannelPolicy::Exact,
        &DefaultAssetDecoder,
        AssetPathPolicy::Exact,
        0,
        None,
    )
    .unwrap();
    assert_eq!(source.reads.get(), 1);
    assert_eq!(
        bits(&prepared, 1),
        vec![0.25f32.to_bits(), (-0.125f32).to_bits()]
    );
    assert_eq!(bits(&prepared, 1), bits(&prepared, 2));
    assert_eq!(prepared.sounds.len(), 2);
    assert!(prepared
        .sounds
        .iter()
        .all(|binding| binding.stage == JudgeStage::Instant
            && binding.gain.to_bits() == 1f32.to_bits()));
}

#[test]
fn inventory_declaration_keeps_exact_unloaded_candidate_ahead_of_loaded_variant() {
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.insert("songs/chart.bms", CHART.to_vec()).unwrap();
    let pcm = wav(1, &[8192]);
    files.declare_file("songs/key.wav", pcm.len()).unwrap();
    files.insert("songs/key.ogg", pcm.clone()).unwrap();
    let source = files.scope("songs/chart.bms").unwrap();
    assert_eq!(
        source
            .resolve("key.wav", AssetPathPolicy::AudioVariants)
            .unwrap(),
        PathBuf::from("songs/key.wav")
    );
    assert!(source.read(Path::new("songs/key.wav"), 1024).is_err());
    files.insert("songs/key.wav", pcm).unwrap();
    let source = files.scope("songs/chart.bms").unwrap();
    let prepared = beatkernel_bms_runtime::prepare_from_source(
        CHART,
        &source,
        format(),
        limits(),
        ChannelPolicy::Exact,
        &DefaultAssetDecoder,
        AssetPathPolicy::AudioVariants,
        0,
        None,
    )
    .unwrap();
    assert_eq!(bits(&prepared, 1), vec![0.25f32.to_bits()]);
}

#[test]
fn seeded_random_originals_match_uncached_branch_provenance_and_warm_reuse() {
    let chart = b"#BPM 120\n#RANDOM 2\n#IF 1\n#WAV01 first.wav\n#00011:01\n#ELSE\n#WAV02 second.wav\n#00012:02\n#ENDIF\n";
    let mut files = MemoryFiles::new(MemoryAssetLimits::default()).unwrap();
    files.insert("chart.bms", chart.to_vec()).unwrap();
    files.insert("first.wav", wav(1, &[8192])).unwrap();
    files.insert("second.wav", wav(1, &[-8192])).unwrap();
    let source = files.scope("chart.bms").unwrap();
    let mut cache = cache();
    for seed in [0, 1, 7, u64::MAX] {
        let mut options = request();
        options.seed = seed;
        let expected = beatkernel_bms_runtime::prepare_from_source(
            chart,
            &source,
            format(),
            limits(),
            ChannelPolicy::Exact,
            &DefaultAssetDecoder,
            AssetPathPolicy::Exact,
            seed,
            None,
        )
        .unwrap();
        let mut job = cache.begin(options.clone(), chart.to_vec()).unwrap();
        let candidate = finish(&mut job, &source);
        assert_eq!(candidate.prepared().source.samples, expected.source.samples);
        assert_eq!(candidate.prepared().sounds.len(), 1);
        assert_eq!(
            candidate.prepared().sounds[0].sample,
            expected.sounds[0].sample
        );
        let selected = expected.sounds[0].sample.0;
        assert_eq!(
            bits(candidate.prepared(), selected),
            bits(&expected, selected)
        );
        cache.commit(&candidate).unwrap();
        let mut warm = cache.begin(options, chart.to_vec()).unwrap();
        let reused = finish(&mut warm, &source);
        assert_eq!(reused.counters().parses, 0);
        assert_eq!(reused.counters().compiles, 0);
        assert_eq!(reused.counters().decodes, 0);
        assert_eq!(bits(reused.prepared(), selected), bits(&expected, selected));
    }
}

#[test]
fn invalid_cache_budgets_reject_without_creating_or_acquiring_a_job() {
    let defaults = PreparationCacheLimits::default();
    for limits in [
        PreparationCacheLimits {
            max_encoded_bytes: 0,
            ..defaults
        },
        PreparationCacheLimits {
            max_pcm_bytes: 0,
            ..defaults
        },
        PreparationCacheLimits {
            max_chart_bytes: 0,
            ..defaults
        },
        PreparationCacheLimits {
            max_encoded_bytes: 256 * 1024 * 1024 + 1,
            ..defaults
        },
        PreparationCacheLimits {
            max_pcm_bytes: 256 * 1024 * 1024 + 1,
            ..defaults
        },
        PreparationCacheLimits {
            max_chart_bytes: 32 * 1024 * 1024 + 1,
            ..defaults
        },
    ] {
        assert!(PreparationCache::new(limits).is_err());
    }
}
