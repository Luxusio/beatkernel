//! Native filesystem preparation across distinct joined publisher Sessions.
use beatkernel::{
    audio::{AudioFormat, PcmLimits, SampleId},
    time::{ClockDomainId, Timestamp},
};
use beatkernel_bms_runtime::{
    native_chart::{prepare_retained_chart_with_policy, NativeChartConfig, PreparedRetainedChart},
    native_judge::NativeJudgeConfig,
    play_policy::GaugeSelection,
    player,
    preparation::{PreparationCache, PreparationCacheLimits},
    session_launch::SessionLaunch,
    ChannelPolicy,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};
type Cache = Arc<Mutex<PreparationCache>>;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-native-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("chart.bms"),
            b"#BPM 120\n#WAV01 key.wav\n#00011:0101\n",
        )
        .unwrap();
        fs::write(path.join("key.wav"), wav(8192)).unwrap();
        Self(path)
    }
    fn chart(&self) -> PathBuf {
        self.0.join("chart.bms")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wav(sample: i16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&38u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24_000u32.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&sample.to_le_bytes());
    bytes
}
fn limits() -> PcmLimits {
    PcmLimits::new(1024, 4096, 8).unwrap()
}
fn cache() -> Cache {
    Arc::new(Mutex::new(
        PreparationCache::new(PreparationCacheLimits::default()).unwrap(),
    ))
}
fn load(
    path: &Path,
    start: i64,
    seed: u64,
    format: AudioFormat,
    limits: PcmLimits,
    channels: ChannelPolicy,
) -> Result<PreparedRetainedChart, String> {
    let bindings = BTreeMap::from([(0x11, 4)]);
    prepare_retained_chart_with_policy(
        NativeChartConfig {
            path,
            format,
            limits,
            channels,
            chart_seed: seed,
            start: Timestamp::from_nanos(start),
            bindings: &bindings,
        },
        &NativeJudgeConfig {
            early: 0,
            late: 0,
            offset: 0,
            preroll: 0,
            output: ClockDomainId(9),
            end: None,
        },
        GaugeSelection::BeatKernel,
        None,
    )
    .map_err(|error| error.to_string())
}
fn session(
    path: PathBuf,
    cache: Option<Cache>,
    start: i64,
    seed: u64,
    format: AudioFormat,
    limits: PcmLimits,
    channels: ChannelPolicy,
) -> Result<PreparedRetainedChart, String> {
    thread::spawn(move || {
        let (publisher, viewer) = player::channel();
        let prepared = player::with_publisher(publisher, || {
            let launch = SessionLaunch::new(vec!["--chart".into(), path.to_str().unwrap().into()])?;
            player::pin_native_launch_with_preparation(launch, cache)?;
            load(&path, start, seed, format, limits, channels)
        });
        let state = viewer.take_latest().unwrap();
        assert!(matches!(
            state.status,
            player::PlayerStatus::Finished | player::PlayerStatus::Failed(_)
        ));
        prepared
    })
    .join()
    .unwrap()
}
fn pointer(prepared: &PreparedRetainedChart) -> *const f32 {
    prepared
        .original
        .bank
        .get(SampleId(1))
        .unwrap()
        .samples()
        .as_ptr()
}
fn format() -> AudioFormat {
    AudioFormat::new(24_000, 1).unwrap()
}

#[test]
fn two_different_joined_sessions_share_original_pcm_and_keep_fresh_section_state() {
    let directory = Directory::new();
    let owner = cache();
    let cold = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let retry = session(
        directory.chart(),
        Some(owner.clone()),
        500_000_000,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(pointer(&cold), pointer(&retry));
    assert_eq!(
        cold.original.source.notes.len(),
        retry.original.source.notes.len()
    );
    assert!(retry.initial_source.notes.len() < retry.original.source.notes.len());
    assert_eq!(retry.section.start, Timestamp::from_nanos(500_000_000));
    assert_eq!(cold.section.start, Timestamp::ZERO);
    drop(owner);
    drop(cold);
    assert_eq!(
        retry.original.bank.get(SampleId(1)).unwrap().samples(),
        [0.25]
    );
}

#[test]
fn same_length_same_mtime_audio_changes_invalidate_native_reuse() {
    let directory = Directory::new();
    let owner = cache();
    let first = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let path = directory.0.join("key.wav");
    let before = fs::metadata(&path).unwrap();
    fs::write(&path, wav(16384)).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), before.len());
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        before.modified().unwrap()
    );
    let changed = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_ne!(pointer(&first), pointer(&changed));
    assert_eq!(
        first.original.bank.get(SampleId(1)).unwrap().samples(),
        [0.25]
    );
    assert_eq!(
        changed.original.bank.get(SampleId(1)).unwrap().samples(),
        [0.5]
    );
    let warm = session(
        directory.chart(),
        Some(owner),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(pointer(&changed), pointer(&warm));
}

#[test]
fn warm_cache_never_relaxes_asset_limits_and_failed_request_keeps_previous_original() {
    let directory = Directory::new();
    let owner = cache();
    let good = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert!(session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        PcmLimits::new(3, 4096, 8).unwrap(),
        ChannelPolicy::Exact
    )
    .is_err());
    let recovered = session(
        directory.chart(),
        Some(owner),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(pointer(&good), pointer(&recovered));
}

#[test]
fn canonical_alias_replacement_and_output_channel_policy_revalidate_current_resources() {
    let directory = Directory::new();
    let owner = cache();
    let original = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    fs::rename(directory.0.join("key.wav"), directory.0.join("key.WAV")).unwrap();
    let alias = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_ne!(pointer(&original), pointer(&alias));
    assert_eq!(
        alias.original.bank.get(SampleId(1)).unwrap().samples(),
        [0.25]
    );
    let stereo = AudioFormat::new(48_000, 2).unwrap();
    assert!(session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        stereo,
        limits(),
        ChannelPolicy::Exact
    )
    .is_err());
    let converted = session(
        directory.chart(),
        Some(owner),
        0,
        0,
        stereo,
        limits(),
        ChannelPolicy::MonoToStereo,
    )
    .unwrap();
    assert_eq!(converted.original.bank.format(), stereo);
    assert_eq!(
        converted.original.bank.get(SampleId(1)).unwrap().samples(),
        [0.25, 0.25]
    );
    assert_eq!(original.original.bank.format(), format());
}

#[test]
fn no_attachment_legacy_sessions_remain_valid_and_uncached() {
    let directory = Directory::new();
    let first = session(
        directory.chart(),
        None,
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let second = session(
        directory.chart(),
        None,
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_ne!(pointer(&first), pointer(&second));
    assert_eq!(
        first.original.bank.get(SampleId(1)).unwrap().samples(),
        second.original.bank.get(SampleId(1)).unwrap().samples()
    );
}

#[test]
fn seed_format_and_same_length_chart_edits_are_fresh_request_identity() {
    let directory = Directory::new();
    let owner = cache();
    let first = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let seeded = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        7,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_ne!(
        pointer(&first),
        pointer(&seeded),
        "a different seed must not reuse a differently identified original"
    );
    let changed_format = AudioFormat::new(48_000, 1).unwrap();
    let changed = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        7,
        changed_format,
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(changed.original.bank.format(), changed_format);
    assert_ne!(pointer(&changed), pointer(&seeded));
    let chart = directory.chart();
    let metadata = fs::metadata(&chart).unwrap();
    fs::write(&chart, b"#BPM 121\n#WAV01 key.wav\n#00011:0101\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&chart)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    let modified = session(
        chart,
        Some(owner),
        0,
        7,
        changed_format,
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_ne!(
        modified.original.compiled.chart.objects()[1].time.start,
        changed.original.compiled.chart.objects()[1].time.start
    );
}

#[test]
fn cancelled_new_session_cannot_replace_previous_cache_or_publish_ready() {
    let directory = Directory::new();
    let owner = cache();
    let first = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::pin_native_launch_with_preparation(
            SessionLaunch::new(vec![
                "--chart".into(),
                directory.chart().to_str().unwrap().into(),
            ])?,
            Some(owner.clone()),
        )?;
        viewer.cancel();
        assert!(load(
            &directory.chart(),
            0,
            9,
            format(),
            limits(),
            ChannelPolicy::Exact
        )
        .is_err());
        assert_ne!(
            viewer.preparation_progress().map(|progress| progress.stage),
            Some(beatkernel_bms_runtime::preparation::PreparationStage::Ready)
        );
        Ok(())
    })
    .unwrap();
    let recovered = session(
        directory.chart(),
        Some(owner),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(pointer(&first), pointer(&recovered));
}

#[test]
fn replay_revalidates_genuine_recorded_setup_before_acquiring_missing_assets() {
    use beatkernel::replay::codec::ReplayFile;
    let directory = Directory::new();
    let owner = cache();
    let original = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let judge_config = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(9),
        end: None,
    };
    let judge = judge_config
        .judge_with_policy(
            &original.original.source,
            original.original.compiled.chart.clone(),
            &original.policy,
        )
        .unwrap();
    let header = beatkernel_bms_runtime::native_judge::prepare_policy_header(
        &original.original.source,
        &judge,
        &original.policy,
        ClockDomainId(9),
        Timestamp::ZERO,
        0,
        None,
    )
    .unwrap();
    let file = ReplayFile::new(header, Vec::new());
    let (publisher, _) = player::channel();
    player::with_publisher(publisher, || {
        player::pin_native_launch_with_preparation(
            SessionLaunch::new(vec![
                "--chart".into(),
                directory.chart().to_str().unwrap().into(),
            ])?,
            Some(owner.clone()),
        )?;
        let loaded = beatkernel_bms_runtime::native_chart::load_original_for_section_replay(
            &directory.chart(),
            format(),
            limits(),
            ChannelPolicy::Exact,
            &file,
            beatkernel_bms_runtime::competition_live::replay_limits().unwrap(),
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            loaded.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            pointer(&original)
        );
        Ok(())
    })
    .unwrap();
    let mut invalid = file;
    invalid.header.options.clear();
    let expected =
        beatkernel_bms_runtime::replay_playback::decode_section_setup(&invalid.header.options)
            .err()
            .unwrap()
            .to_string();
    fs::remove_file(directory.0.join("key.wav")).unwrap();
    let (publisher, _) = player::channel();
    player::with_publisher(publisher, || {
        player::pin_native_launch_with_preparation(
            SessionLaunch::new(vec![
                "--chart".into(),
                directory.chart().to_str().unwrap().into(),
            ])?,
            Some(owner),
        )?;
        let error = beatkernel_bms_runtime::native_chart::load_original_for_section_replay(
            &directory.chart(),
            format(),
            limits(),
            ChannelPolicy::Exact,
            &invalid,
            beatkernel_bms_runtime::competition_live::replay_limits().unwrap(),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.contains(&expected),
            "recording metadata must fail before missing-asset acquisition: {error}"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn derived_bindings_section_timing_and_gauge_refusals_preserve_last_admitted_original() {
    use beatkernel_bms_runtime::{native_chart::prepare_chart, play_policy::TimingPresetSelection};
    const GOOD: &[u8] = b"#BPM 120\n#WAV01 key.wav\n#00011:0101\n";
    for refusal in 0..4 {
        let directory = Directory::new();
        let owner = cache();
        let admitted = session(
            directory.chart(),
            Some(owner.clone()),
            0,
            0,
            format(),
            limits(),
            ChannelPolicy::Exact,
        )
        .unwrap();
        fs::write(directory.0.join("key.wav"), wav(16384)).unwrap();
        fs::write(
            directory.chart(),
            if refusal == 0 {
                b"#BPM 121\n#WAV01 key.wav\n#00012:0101\n".as_slice()
            } else {
                b"#BPM 121\n#WAV01 key.wav\n#00011:0101\n".as_slice()
            },
        )
        .unwrap();
        let (publisher, _) = player::channel();
        player::with_publisher(publisher, || {
            player::pin_native_launch_with_preparation(
                SessionLaunch::new(vec![
                    "--chart".into(),
                    directory.chart().to_str().unwrap().into(),
                ])?,
                Some(owner.clone()),
            )?;
            let bindings = BTreeMap::from([(0x11, 4)]);
            let path = directory.chart();
            let config = NativeChartConfig {
                path: &path,
                format: format(),
                limits: limits(),
                channels: ChannelPolicy::Exact,
                chart_seed: 0,
                start: Timestamp::from_nanos(if refusal == 1 { 10_000_000_000 } else { 0 }),
                bindings: &bindings,
            };
            let judge = NativeJudgeConfig {
                early: if refusal == 3 { -1 } else { 0 },
                late: 0,
                offset: 0,
                preroll: 0,
                output: ClockDomainId(9),
                end: None,
            };
            let error = if refusal == 1 {
                prepare_chart(config).err().unwrap().to_string()
            } else {
                let timing = (refusal == 2).then(|| {
                    TimingPresetSelection::parse(
                        beatkernel_bms::BmsTimingPreset::BeatorajaSevenKeys8320241dV1.id(),
                        "rank-first",
                    )
                    .unwrap()
                });
                let gauge = if refusal == 3 {
                    GaugeSelection::Bms(beatkernel_bms::BmsGaugeKind::Groove)
                } else {
                    GaugeSelection::BeatKernel
                };
                prepare_retained_chart_with_policy(config, &judge, gauge, timing)
                    .err()
                    .unwrap()
                    .to_string()
            };
            if refusal == 0 {
                assert!(error.contains("missing --bind"), "{error}");
            }
            if refusal == 1 {
                assert!(error.contains("no remaining"), "{error}");
            }
            if refusal == 2 {
                assert!(error.contains("declared RANK"), "{error}");
            }
            assert!(!error.is_empty());
            Ok(())
        })
        .unwrap();
        fs::write(directory.chart(), GOOD).unwrap();
        fs::write(directory.0.join("key.wav"), wav(8192)).unwrap();
        let retry = session(
            directory.chart(),
            Some(owner),
            0,
            0,
            format(),
            limits(),
            ChannelPolicy::Exact,
        )
        .unwrap();
        assert_eq!(
            pointer(&admitted),
            pointer(&retry),
            "failed derived admission {refusal} must not evict the previous original"
        );
        assert_eq!(
            retry.original.bank.get(SampleId(1)).unwrap().samples(),
            [0.25]
        );
    }
}

#[test]
fn genuine_replay_with_unplayable_derived_section_cannot_evict_previous_original() {
    use beatkernel::replay::codec::ReplayFile;
    let directory = Directory::new();
    let owner = cache();
    let admitted = session(
        directory.chart(),
        Some(owner.clone()),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    fs::write(
        directory.chart(),
        b"#BPM 121\n#WAV01 key.wav\n#00011:0101\n",
    )
    .unwrap();
    fs::write(directory.0.join("key.wav"), wav(16384)).unwrap();
    let changed = load(
        &directory.chart(),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    let start = Timestamp::from_nanos(10_000_000_000);
    let selected =
        beatkernel_bms_runtime::section_start::source_at(&changed.original.source, start).unwrap();
    let judge_config = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(9),
        end: None,
    };
    let judge = judge_config
        .judge_with_policy(
            &selected,
            selected.compile().unwrap().chart,
            &changed.policy,
        )
        .unwrap();
    let header = beatkernel_bms_runtime::native_judge::prepare_policy_header(
        &changed.original.source,
        &judge,
        &changed.policy,
        ClockDomainId(9),
        start,
        0,
        None,
    )
    .unwrap();
    let file = ReplayFile::new(header, Vec::new());
    let replay_limits = beatkernel_bms_runtime::competition_live::replay_limits().unwrap();
    beatkernel_bms_runtime::replay_playback::validate_section_setup(
        &changed.original.source,
        &file,
        replay_limits,
    )
    .unwrap();
    let (publisher, _) = player::channel();
    player::with_publisher(publisher, || {
        player::pin_native_launch_with_preparation(
            SessionLaunch::new(vec![
                "--chart".into(),
                directory.chart().to_str().unwrap().into(),
            ])?,
            Some(owner.clone()),
        )?;
        let error = beatkernel_bms_runtime::native_chart::prepare_original_for_section_replay(
            &directory.chart(),
            format(),
            limits(),
            ChannelPolicy::Exact,
            &file,
            replay_limits,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.contains("no remaining"),
            "genuine metadata is valid; derived section must fail: {error}"
        );
        Ok(())
    })
    .unwrap();
    fs::write(
        directory.chart(),
        b"#BPM 120\n#WAV01 key.wav\n#00011:0101\n",
    )
    .unwrap();
    fs::write(directory.0.join("key.wav"), wav(8192)).unwrap();
    let retry = session(
        directory.chart(),
        Some(owner),
        0,
        0,
        format(),
        limits(),
        ChannelPolicy::Exact,
    )
    .unwrap();
    assert_eq!(pointer(&admitted), pointer(&retry));
}
