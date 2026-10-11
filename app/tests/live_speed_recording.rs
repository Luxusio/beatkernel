//! Source-speed identity is metadata; captured operations remain original-song data.
use beatkernel::{
    audio::{command_queue, AudioFormat, PcmLimits, SampleBank},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome},
    replay::{
        codec::{encode_replay, ReplayCodecLimits, ReplayFile},
        ReplayOperation,
    },
    runtime::Runtime,
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::Rate,
};
use beatkernel_bms::{BmsChart, BmsInputMode};
use beatkernel_bms_runtime::{
    gauge::BmsGauge,
    live_speed::LiveSpeedSetup,
    play_policy::ResolvedPlayPolicy,
    record_catalog::RecordPreview,
    replay_audio::plan_section_audio_with_rate,
    replay_capture::{
        setup_play_policy_header, setup_play_policy_header_with_speed, LiveReplayCapture,
    },
    replay_playback::{
        decode_chart_setup, decode_profile, decode_section_setup, decode_setup,
        reconstruct_section, validate_section_setup, PlaybackError,
    },
    replay_speed::ReplaySpeed,
    replay_speed_policy::{split_options, wrap_header},
    settings::{NativeSettings, SettingsHost},
    PreparedBms,
};
use std::path::Path;

const PREFIX: &[u8] = b"bms-speed-setup/v1:";
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn speed(value: &str) -> ReplaySpeed {
    value.parse().unwrap()
}
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(nanos),
    }
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 80\n#WAV01 note.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
fn policy() -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::builtin(0, 0, 0).unwrap()
}
fn judge(source: &BmsChart, policy: &ResolvedPlayPolicy) -> JudgeEngine {
    JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        policy.judge().clone(),
    )
    .unwrap()
}
fn capture(source_speed: ReplaySpeed) -> LiveReplayCapture {
    let source = source();
    let policy = policy();
    LiveReplayCapture::new_with_policy_and_speed(
        &judge(&source, &policy),
        ClockDomainId(1),
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &policy,
        source_speed,
    )
    .unwrap()
}
// Construct literal wire inputs, independently of the policy encoder under test.
fn wire(inner: &[u8], numerator: i64, denominator: u64) -> Vec<u8> {
    let mut bytes = PREFIX.to_vec();
    bytes.extend_from_slice(&(inner.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&numerator.to_le_bytes());
    bytes.extend_from_slice(&denominator.to_le_bytes());
    bytes.extend_from_slice(inner);
    bytes
}

#[test]
fn normal_header_and_empty_capture_are_exact_legacy_bytes() {
    let source = source();
    let policy = policy();
    let judge = judge(&source, &policy);
    let old = setup_play_policy_header(
        &judge,
        ClockDomainId(1),
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &policy,
    )
    .unwrap();
    let new = setup_play_policy_header_with_speed(
        &judge,
        ClockDomainId(1),
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &policy,
        speed("1/1"),
    )
    .unwrap();
    assert_eq!(old, new);
    assert_eq!(
        wrap_header(old.clone(), speed("2/2"), limits()).unwrap(),
        old
    );
    let old_capture = LiveReplayCapture::new_with_policy(
        &judge,
        ClockDomainId(1),
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &policy,
    )
    .unwrap();
    assert_eq!(
        old_capture.into_bytes().unwrap(),
        capture(speed("1/1")).into_bytes().unwrap()
    );
    assert_eq!(
        decode_section_setup(&new.options).unwrap().source_speed,
        speed("1/1")
    );
}

#[test]
fn four_rates_have_literal_canonical_outer_layout_and_reduced_identity() {
    let old = capture(speed("1/1")).header().clone();
    let rates: [(&str, i64, u64); 4] = [("1/2", 1, 2), ("1/1", 1, 1), ("2/1", 2, 1), ("3/2", 3, 2)];
    for (text, num, den) in rates {
        let header = capture(speed(text)).header().clone();
        assert_eq!(header.chart_identity, old.chart_identity);
        assert_eq!(header.rules_identity, old.rules_identity);
        assert_eq!(
            header.options,
            if i128::from(num) == i128::from(den) {
                old.options.clone()
            } else {
                wire(&old.options, num, den)
            }
        );
        let (inner, decoded) = split_options(&header.options).unwrap();
        assert_eq!(inner, old.options);
        assert_eq!(decoded, speed(text));
        assert_eq!(
            decode_section_setup(&header.options).unwrap().source_speed,
            speed(text)
        );
    }
    assert_eq!(
        capture(speed("6/4")).header(),
        capture(speed("3/2")).header()
    );
    assert!(beatkernel_bms_runtime::competition::compatible_headers(
        capture(speed("6/4")).header(),
        capture(speed("3/2")).header()
    ));
    assert!(!beatkernel_bms_runtime::competition::compatible_headers(
        capture(speed("1/1")).header(),
        capture(speed("3/2")).header()
    ));
}

#[test]
fn absent_envelope_borrows_original_storage_and_preserves_legacy_tuple() {
    let header = capture(speed("1/1")).header().clone();
    let (inner, decoded) = split_options(&header.options).unwrap();
    assert_eq!(inner.as_ptr(), header.options.as_ptr());
    assert_eq!(decoded.rate(), Rate::NORMAL);
    assert_eq!(decode_setup(inner).unwrap().1, Timestamp::ZERO);
    assert_eq!(decode_chart_setup(inner).unwrap().2, 0);
    assert_eq!(decode_profile(inner).unwrap(), policy().judge().clone());
}

#[test]
fn legacy_consumers_refuse_non_normal_metadata_with_policy_aware_error() {
    let options = capture(speed("3/2")).header().options.clone();
    for result in [
        decode_profile(&options).map(|_| ()),
        decode_setup(&options).map(|_| ()),
        decode_chart_setup(&options).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(PlaybackError::Metadata(message)) if message.contains("policy-aware"))
        );
    }
    assert_eq!(
        decode_section_setup(&options).unwrap().source_speed,
        speed("3/2")
    );
}

#[test]
fn malformed_fraction_and_noncanonical_normal_envelopes_are_rejected() {
    let inner = capture(speed("1/1")).header().options.clone();
    for (n, d) in [
        (0, 1),
        (-1, 2),
        (1, 0),
        (2, 4),
        (6, 4),
        (1, 1),
        (2, 2),
        (i64::MIN, u64::MAX),
    ] {
        let bytes = wire(&inner, n, d);
        assert!(split_options(&bytes).is_err(), "accepted {n}/{d}");
        assert!(decode_section_setup(&bytes).is_err());
    }
}

#[test]
fn every_truncated_extent_and_trailing_byte_are_rejected() {
    let inner = capture(speed("1/1")).header().options.clone();
    let valid = wire(&inner, 3, 2);
    for length in PREFIX.len()..valid.len() {
        assert!(
            split_options(&valid[..length]).is_err(),
            "accepted truncation at {length}"
        );
    }
    for length in [0u32, 1, u32::MAX] {
        let mut bad = valid.clone();
        bad[PREFIX.len()..PREFIX.len() + 4].copy_from_slice(&length.to_le_bytes());
        assert!(split_options(&bad).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(split_options(&trailing).is_err());
}

#[test]
fn unknown_version_and_nested_source_envelopes_are_rejected_even_in_normal_wrap() {
    let old = capture(speed("1/1")).header().clone();
    let canonical = wire(&old.options, 3, 2);
    let mut unknown = canonical.clone();
    unknown[b"bms-speed-setup/v".len()] = b'2';
    assert!(split_options(&unknown).is_err());
    assert!(split_options(&wire(&canonical, 2, 1)).is_err());
    for options in [canonical, unknown] {
        let mut header = old.clone();
        header.options = options;
        assert!(wrap_header(header.clone(), speed("1/1"), limits()).is_err());
        assert!(wrap_header(header, speed("2/1"), limits()).is_err());
    }
}

#[test]
fn header_budget_includes_every_identity_and_package_version_at_exact_boundary() {
    let mut old = capture(speed("1/1")).header().clone();
    old.chart_identity = vec![0x31; 73];
    old.rules_identity = vec![0x42; 59];
    let total = wire(&old.options, 3, 2).len() + 73 + 59 + env!("CARGO_PKG_VERSION").len();
    let bounded = |header_bytes| {
        ReplayCodecLimits::new(
            1 << 20,
            100,
            header_bytes,
            CodecLimits::new(4096, 1024).unwrap(),
        )
        .unwrap()
    };
    assert!(wrap_header(old.clone(), speed("3/2"), bounded(total)).is_ok());
    assert!(wrap_header(old.clone(), speed("3/2"), bounded(total - 1)).is_err());
    let mut huge_identity = old;
    huge_identity.chart_identity = vec![0; total];
    assert!(wrap_header(huge_identity, speed("3/2"), bounded(total)).is_err());
}

#[test]
fn maximal_positive_components_roundtrip_without_float_or_product_overflow() {
    let old = capture(speed("1/1")).header().clone();
    let value = ReplaySpeed::new(Rate::new(i64::MAX, u64::MAX).unwrap()).unwrap();
    let wrapped = wrap_header(old.clone(), value, limits()).unwrap();
    assert_eq!(wrapped.options, wire(&old.options, i64::MAX, u64::MAX));
    assert_eq!(split_options(&wrapped.options).unwrap().1, value);
    assert_eq!(
        decode_section_setup(&wrapped.options).unwrap().source_speed,
        value
    );
}

#[test]
fn actual_runtime_capture_reconstructs_original_inputs_provenance_judge_hash_and_gauge() {
    let source = source();
    let policy = policy();
    // This is a declared exact fixture-domain offset using the real core mapper,
    // not a host/device clock or a fabricated hardware correlation.
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: point(9, 0),
            target: point(1, 0),
        },
        ClockInterval {
            start: Timestamp::ZERO,
            end: ts(8_000_000_000),
        },
    )
    .unwrap();
    for (rate, output_note) in [
        ("1/2", 6_000_000_000),
        ("1/1", 3_000_000_000),
        ("2/1", 1_500_000_000),
        ("3/2", 2_000_000_000),
    ] {
        let selected = speed(rate);
        let mut capture = capture(selected);
        let (producer, _consumer) = command_queue(8).unwrap();
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let setup = LiveSpeedSetup::new(Timestamp::ZERO, Duration::ZERO, selected, None).unwrap();
        let mut runtime = Runtime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            setup.transport(Timestamp::ZERO).unwrap(),
            bindings,
            judge(&source, &policy),
            producer,
            vec![],
            0,
        )
        .unwrap();
        let mut live_gauge = BmsGauge::new(policy.gauge().clone());
        let mut live_events = Vec::new();
        for (sequence, at) in [0, output_note].into_iter().enumerate() {
            let mut meta = EventMeta::new(DeviceId(17), point(9, at), (sequence * 2) as u64);
            meta.original_clock_point = Some(point(11, at + 7));
            let input = PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: PhysicalControlId::keyboard(7),
                state: ButtonState::Down,
            });
            let report = runtime.process_input(input, &mapper, point(2, 17)).unwrap();
            assert_eq!(
                report.song_time,
                ts(if sequence == 0 { 0 } else { 3_000_000_000 })
            );
            assert_eq!(report.audio_at, point(2, 17));
            assert!(matches!(
                report.judge_events[0].outcome,
                JudgeOutcome::Hit {
                    grade: JudgeGrade(1),
                    delta: Duration::ZERO
                }
            ));
            live_gauge
                .observe(&report.judge_events, &report.hazard_events)
                .unwrap();
            live_events.extend_from_slice(&report.judge_events);
            capture.record_report(&report).unwrap();
            if sequence == 0 {
                // Release the physical key before the next tap; two Down events
                // without Up are a held-key repeat, not two independent presses.
                let mut meta = EventMeta::new(DeviceId(17), point(9, 1), 1);
                meta.original_clock_point = Some(point(11, 8));
                let release = PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control: PhysicalControlId::keyboard(7),
                    state: ButtonState::Up,
                });
                let report = runtime
                    .process_input(release, &mapper, point(2, 17))
                    .unwrap();
                assert!(report.judge_events.is_empty());
                assert_eq!(report.audio_at, point(2, 17));
                live_gauge
                    .observe(&report.judge_events, &report.hazard_events)
                    .unwrap();
                capture.record_report(&report).unwrap();
            }
        }
        assert_eq!(capture.records().len(), 3);
        assert_eq!(capture.records()[2].song_time, ts(3_000_000_000));
        let ReplayOperation::Input(bound) = &capture.records()[2].operation else {
            panic!("input record");
        };
        assert_eq!(bound.physical.meta().timestamp, ts(output_note));
        assert_eq!(
            bound.physical.meta().original_clock_point,
            Some(point(11, output_note + 7))
        );
        let file = capture.into_file();
        let canonical = encode_replay(&file, limits()).unwrap();
        let replay = reconstruct_section(&source, file.clone(), limits()).unwrap();
        assert_eq!(replay.results(), live_events);
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            runtime.judge().stable_hash().unwrap()
        );
        let mut replay_gauge = BmsGauge::new(policy.gauge().clone());
        replay_gauge.observe(replay.results(), &[]).unwrap();
        assert_eq!(replay_gauge.snapshot(), live_gauge.snapshot());
        // Choosing another output speed changes presentation only, never the
        // source condition or original operation/log content.
        for output_rate in [speed("1/2"), speed("2/1")] {
            let presentation =
                LiveSpeedSetup::new(Timestamp::ZERO, Duration::ZERO, output_rate, None).unwrap();
            assert_eq!(
                presentation
                    .output_at(point(2, 0), ts(3_000_000_000))
                    .unwrap()
                    .timestamp,
                ts(if output_rate == speed("1/2") {
                    6_000_000_000
                } else {
                    1_500_000_000
                })
            );
            assert_eq!(
                decode_section_setup(&file.header.options)
                    .unwrap()
                    .source_speed,
                selected
            );
            assert_eq!(encode_replay(&file, limits()).unwrap(), canonical);
            let prepared = PreparedBms {
                source: source.clone(),
                compiled: source.compile().unwrap(),
                bank: SampleBank::new(
                    AudioFormat::new(48_000, 1).unwrap(),
                    PcmLimits::new(4096, 16384, 8).unwrap(),
                )
                .unwrap(),
                sounds: vec![],
                bgm_commands: vec![],
            };
            let planned = plan_section_audio_with_rate(
                &prepared,
                file.clone(),
                limits(),
                point(2, 0),
                Duration::ZERO,
                output_rate.rate(),
            )
            .unwrap();
            assert_eq!(planned.judge_events, live_events);
            assert_eq!(
                planned.final_judge_hash,
                runtime.judge().stable_hash().unwrap()
            );
            assert_eq!(planned.recorded_until, Some(ts(3_000_000_000)));
        }
    }
}

#[test]
fn source_condition_mismatch_refuses_and_normalized_fraction_matches_catalog() {
    let source = source();
    let file = capture(speed("3/2")).into_file();
    let setup = decode_section_setup(&file.header.options).unwrap();
    let path = Path::new("records/source-speed.bkr");
    assert!(
        RecordPreview::from_file_with_setup(path, &source, &setup, file.clone(), None, None)
            .is_ok()
    );
    let mut wrong = setup.clone();
    wrong.source_speed = speed("1/1");
    assert!(
        RecordPreview::from_file_with_setup(path, &source, &wrong, file.clone(), None, None)
            .is_err()
    );
    for host in [
        SettingsHost::Linux,
        SettingsHost::Windows,
        SettingsHost::Macos,
    ] {
        let settings = |text: &str| {
            NativeSettings::from_args(
                &[
                    "--speed".into(),
                    text.into(),
                    "--early-ns".into(),
                    "0".into(),
                    "--late-ns".into(),
                    "0".into(),
                ],
                host,
            )
            .unwrap()
        };
        assert!(RecordPreview::from_file_with_archive(
            path,
            &source,
            &settings("6/4"),
            file.clone(),
            None,
            None
        )
        .is_ok());
        assert!(RecordPreview::from_file_with_archive(
            path,
            &source,
            &settings("1/1"),
            file.clone(),
            None,
            None
        )
        .is_err());
    }
}

#[test]
fn reconstruction_checks_speed_envelope_before_accepting_original_setup() {
    let source = source();
    let mut file: ReplayFile = capture(speed("3/2")).into_file();
    assert!(validate_section_setup(&source, &file, limits()).is_ok());
    file.header.options.push(0);
    assert!(validate_section_setup(&source, &file, limits()).is_err());
}

#[test]
fn source_speed_wraps_complete_timing_judgment_gauge_policy_without_losing_sections() {
    use beatkernel_bms::{BmsGaugeKind, BmsRankPrecedence, BmsTimingPreset};
    use beatkernel_bms_runtime::play_policy::TimingPresetSelection;
    let source = beatkernel_bms::parse(
        "#BPM 80\n#RANK 3\n#WAV01 note.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap();
    let policy = ResolvedPlayPolicy::bms_with_timing(
        &source,
        BmsGaugeKind::Groove,
        TimingPresetSelection {
            preset: BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
            precedence: BmsRankPrecedence::RankFirst,
        },
        5_000_000,
    )
    .unwrap();
    let judge = beatkernel_bms_runtime::mine_plan::prepare_judge_with_timing(
        &source,
        source.compile().unwrap().chart,
        policy.judge().clone(),
        BmsInputMode::ButtonOnly,
        1024,
        Some(policy.timing().unwrap().profiles()),
    )
    .unwrap();
    let capture = LiveReplayCapture::new_with_policy_and_speed(
        &judge,
        ClockDomainId(1),
        limits(),
        Timestamp::ZERO,
        0,
        Some(ts(4_000_000_000)),
        BmsInputMode::ButtonOnly,
        None,
        &policy,
        speed("3/2"),
    )
    .unwrap();
    let file = capture.into_file();
    let (timing_bytes, selected) = split_options(&file.header.options).unwrap();
    assert_eq!(selected, speed("3/2"));
    assert!(timing_bytes.starts_with(b"bms-timing-setup/v1:"));
    let decoded = decode_section_setup(&file.header.options).unwrap();
    assert_eq!(decoded.source_speed, speed("3/2"));
    assert_eq!(decoded.timing.as_ref(), policy.timing());
    assert_eq!(decoded.judgments.as_ref(), policy.judgments());
    assert_eq!(&decoded.gauge, policy.gauge());
    assert_eq!(&decoded.profile, policy.judge());
    assert_eq!(decoded.end, Some(ts(4_000_000_000)));
    assert!(validate_section_setup(&source, &file, limits()).is_ok());
}
