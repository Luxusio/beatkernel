//! Canonical finite metadata and actual Runtime reports, without file or native I/O.
use crate::{
    replay_capture::{CaptureError, LiveReplayCapture, setup_header, setup_section_header},
    replay_playback::{
        decode_chart_setup, decode_profile, decode_section_setup, decode_setup, reconstruct,
        reconstruct_section, validate_section_setup, validate_setup,
    },
    section_start::source_at,
};
use beatkernel::{
    audio::{CommandConsumer, command_queue},
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    interaction::InteractionState,
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecError, ReplayCodecLimits, ReplayFile, decode_replay, encode_replay},
    },
    runtime::{Runtime, RuntimeProcessingClock, RuntimeReport},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, parse_seeded};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn limits(header: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, header, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn profile(offset: i64) -> JudgeProfile {
    JudgeProfile::new(
        vec![
            JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::from_nanos(11),
                late: Duration::from_nanos(23),
            },
            JudgeWindow {
                grade: JudgeGrade(9),
                early: Duration::from_nanos(101),
                late: Duration::from_nanos(203),
            },
        ],
        Duration::from_nanos(offset),
    )
    .unwrap()
}
fn source(seed: u64) -> BmsChart {
    parse_seeded("#BPM 60\n#WAV01 key.wav\n#00011:01010101\n#RANDOM 2\n#IF 1\n#00112:01\n#ELSE\n#00113:01\n#ENDIF\n#ENDRANDOM\n",
        Default::default(), seed).unwrap()
}
fn judge(source: &BmsChart, start: i64, offset: i64) -> JudgeEngine {
    let selected = source_at(source, Timestamp::from_nanos(start)).unwrap();
    JudgeEngine::new(
        selected.compile().unwrap().chart,
        selected.rules(),
        profile(offset),
    )
    .unwrap()
}
fn literal_body(offset: i64) -> Vec<u8> {
    let mut bytes = offset.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2u64.to_le_bytes());
    for (grade, early, late) in [(7u32, 11i64, 23i64), (9, 101, 203)] {
        bytes.extend_from_slice(&grade.to_le_bytes());
        bytes.extend_from_slice(&early.to_le_bytes());
        bytes.extend_from_slice(&late.to_le_bytes());
    }
    bytes
}
fn literal_finite(seed: u64, start: i64, end: i64, offset: i64) -> Vec<u8> {
    let mut bytes = b"bms-judge-profile/v4:".to_vec();
    bytes.extend_from_slice(&seed.to_le_bytes());
    bytes.extend_from_slice(&start.to_le_bytes());
    bytes.extend_from_slice(&end.to_le_bytes());
    bytes.extend(literal_body(offset));
    bytes
}
fn snapshot(capture: &LiveReplayCapture) -> Vec<u8> {
    encode_replay(
        &ReplayFile::new(capture.header().clone(), capture.records().to_vec()),
        limits(4096),
    )
    .unwrap()
}

struct Rig {
    runtime: Runtime,
    _consumer: CommandConsumer,
    mapper: AffineClockMapper,
    song_origin: i64,
    sequence: u64,
}
impl Rig {
    fn new(judge: JudgeEngine, start: i64) -> Self {
        let bindings = BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(0x11),
        }])
        .unwrap();
        let (producer, consumer) = command_queue(8).unwrap();
        let song_origin = start - 100_000_000;
        let mut runtime = Runtime::new(
            ClockDomainId(11),
            ClockDomainId(22),
            Transport::new(
                Timestamp::from_nanos(10_000_000_000),
                Timestamp::from_nanos(song_origin),
                Rate::NORMAL,
            ),
            bindings,
            judge,
            producer,
            Vec::new(),
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mapper = AffineClockMapper::exact_offset(
            ClockPair {
                source: point(11, 10_000_000_000),
                target: point(22, 0),
            },
            ClockInterval {
                start: Timestamp::from_nanos(10_000_000_000),
                end: Timestamp::from_nanos(30_000_000_000),
            },
        )
        .unwrap();
        Self {
            runtime,
            _consumer: consumer,
            mapper,
            song_origin,
            sequence: 0,
        }
    }
    fn report(&mut self, song: i64, key: Option<(u16, ButtonState)>) -> RuntimeReport {
        let elapsed = song - self.song_origin;
        let host = point(11, 10_000_000_000 + elapsed);
        let output = point(22, elapsed);
        let report = if let Some((key, state)) = key {
            self.sequence += 1;
            let mut meta = EventMeta::new(DeviceId(77), host, self.sequence);
            meta.native = Some(NativeEventMeta {
                backend: BackendId(7),
                code: Some(42),
                timestamp: Some(host),
            });
            self.runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta,
                        control: PhysicalControlId::keyboard(key),
                        state,
                    }),
                    &self.mapper,
                    output,
                )
                .unwrap()
        } else {
            self.runtime.advance_to(host, &self.mapper, output).unwrap()
        };
        assert!(report.judge_error.is_none());
        assert!(report.audio_failures.is_empty());
        report
    }
}

#[test]
fn optional_end_none_preserves_literal_legacy_options_headers_and_exported_files() {
    for (start, seed, prefix) in [
        (0, 0, &b"bms-judge-profile/v1:"[..]),
        (1, 0, &b"bms-judge-profile/v2:"[..]),
        (0, u64::MAX, &b"bms-judge-profile/v3:"[..]),
        (i64::MAX, 1, &b"bms-judge-profile/v3:"[..]),
    ] {
        let source = source(seed);
        let judge = judge(&source, start, -19);
        let start = Timestamp::from_nanos(start);
        let old = setup_header(&judge, ClockDomainId(11), limits(4096), start, seed).unwrap();
        let section =
            setup_section_header(&judge, ClockDomainId(11), limits(4096), start, seed, None)
                .unwrap();
        let mut literal = prefix.to_vec();
        if seed != 0 {
            literal.extend_from_slice(&seed.to_le_bytes());
        }
        if seed != 0 || start != Timestamp::ZERO {
            literal.extend_from_slice(&start.as_nanos().to_le_bytes());
        }
        literal.extend(literal_body(-19));
        assert_eq!(section.options, literal);
        assert_eq!(section, old);
        let old = LiveReplayCapture::new_at_with_chart_seed(
            &judge,
            ClockDomainId(11),
            limits(4096),
            start,
            seed,
        )
        .unwrap();
        let section = LiveReplayCapture::new_section(
            &judge,
            ClockDomainId(11),
            limits(4096),
            start,
            seed,
            None,
        )
        .unwrap();
        assert_eq!(old.into_bytes().unwrap(), section.into_bytes().unwrap());
        let decoded = decode_section_setup(&literal).unwrap();
        assert_eq!(
            (decoded.start, decoded.chart_seed, decoded.end),
            (start, seed, None)
        );
        assert_eq!(decoded.profile, *judge.profile());
        assert_eq!(
            decode_chart_setup(&literal).unwrap(),
            (decoded.profile, start, seed)
        );
    }
}

#[test]
fn finite_v4_has_exact_seed_start_end_and_profile_bytes_at_signed_boundaries_and_header_caps() {
    for (seed, start, end) in [
        (0, 0, 1),
        (u64::MAX, 604_800_000_000_001, 604_800_000_000_002),
        (0, i64::MAX - 1, i64::MAX),
    ] {
        let source = source(seed);
        let judge = judge(&source, start, -19);
        let capture = LiveReplayCapture::new_section(
            &judge,
            ClockDomainId(11),
            limits(4096),
            Timestamp::from_nanos(start),
            seed,
            Some(Timestamp::from_nanos(end)),
        )
        .unwrap();
        let header = capture.header();
        assert_eq!(header.options, literal_finite(seed, start, end, -19));
        assert_eq!(
            header.seed, 0,
            "chart branch provenance does not change the builtin rule seed"
        );
        let decoded = decode_section_setup(&header.options).unwrap();
        assert_eq!(
            (
                decoded.start.as_nanos(),
                decoded.end.unwrap().as_nanos(),
                decoded.chart_seed
            ),
            (start, end, seed)
        );
        assert_eq!(&decoded.profile, judge.profile());
        let cap = header.options.len()
            + header.chart_identity.len()
            + header.rules_identity.len()
            + env!("CARGO_PKG_VERSION").len();
        assert_eq!(
            setup_section_header(
                &judge,
                ClockDomainId(11),
                limits(cap),
                Timestamp::from_nanos(start),
                seed,
                Some(Timestamp::from_nanos(end))
            )
            .unwrap(),
            *header
        );
        assert!(matches!(
            setup_section_header(
                &judge,
                ClockDomainId(11),
                limits(cap - 1),
                Timestamp::from_nanos(start),
                seed,
                Some(Timestamp::from_nanos(end))
            ),
            Err(CaptureError::Codec(ReplayCodecError::HeaderTooLarge))
        ));
        let bytes = capture.into_bytes().unwrap();
        assert_eq!(
            encode_replay(&decode_replay(&bytes, limits(4096)).unwrap(), limits(4096)).unwrap(),
            bytes
        );
    }
}

#[test]
fn finite_metadata_rejects_invalid_extents_truncation_window_shapes_and_noncanonical_legacy_forms()
{
    let valid = literal_finite(u64::MAX, 1, i64::MAX, -19);
    for length in 0..valid.len() {
        assert!(
            decode_section_setup(&valid[..length]).is_err(),
            "truncation at {length}"
        );
    }
    let prefix = b"bms-judge-profile/v4:".len();
    for (offset, bytes) in [
        (8, (-1i64).to_le_bytes()),
        (16, (-1i64).to_le_bytes()),
        (16, 0i64.to_le_bytes()),
        (16, 1i64.to_le_bytes()),
        (32, 0u64.to_le_bytes()),
        (32, u64::MAX.to_le_bytes()),
        (44, (-1i64).to_le_bytes()),
    ] {
        let mut bad = valid.clone();
        bad[prefix + offset..prefix + offset + 8].copy_from_slice(&bytes);
        assert!(decode_section_setup(&bad).is_err());
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(decode_section_setup(&trailing).is_err());
    let mut unknown = valid.clone();
    unknown[b"bms-judge-profile/v".len()] = b'5';
    assert!(decode_section_setup(&unknown).is_err());
    let mut zero_v2 = b"bms-judge-profile/v2:".to_vec();
    zero_v2.extend_from_slice(&0i64.to_le_bytes());
    zero_v2.extend(literal_body(-19));
    let mut zero_v3 = b"bms-judge-profile/v3:".to_vec();
    zero_v3.extend_from_slice(&0u64.to_le_bytes());
    zero_v3.extend_from_slice(&1i64.to_le_bytes());
    zero_v3.extend(literal_body(-19));
    assert!(decode_section_setup(&zero_v2).is_err());
    assert!(decode_section_setup(&zero_v3).is_err());
    let judge = judge(&source(0), 0, -19);
    for (start, end) in [(0, 0), (1, 0), (1, 1), (0, -1), (i64::MAX, i64::MAX)] {
        assert!(matches!(
            LiveReplayCapture::new_section(
                &judge,
                ClockDomainId(11),
                limits(4096),
                Timestamp::from_nanos(start),
                0,
                Some(Timestamp::from_nanos(end))
            ),
            Err(CaptureError::InvalidEnd)
        ));
    }
    assert!(matches!(
        setup_section_header(
            &judge,
            ClockDomainId(11),
            limits(4096),
            Timestamp::from_nanos(-1),
            0,
            Some(Timestamp::from_nanos(1))
        ),
        Err(CaptureError::InvalidStart)
    ));
    assert!(judge.effective_song_time().is_none());
    assert!(
        LiveReplayCapture::new_section(
            &judge,
            ClockDomainId(11),
            limits(4096),
            Timestamp::ZERO,
            0,
            Some(Timestamp::from_nanos(1))
        )
        .is_ok()
    );
}

#[test]
fn accepted_runtime_operations_outside_finite_capture_are_rejected_without_changing_the_prefix() {
    let end = 2_000_000_000;
    for input_at_end in [true, false] {
        let judge = judge(&source(0), 0, 10);
        let mut capture = LiveReplayCapture::new_section(
            &judge,
            ClockDomainId(11),
            limits(4096),
            Timestamp::ZERO,
            0,
            Some(Timestamp::from_nanos(end)),
        )
        .unwrap();
        let mut rig = Rig::new(judge, 0);
        for (song, key) in [
            (-100_000_000, None),
            (-10, Some((4, ButtonState::Down))),
            (100_000_000, Some((4, ButtonState::Up))),
        ] {
            capture.record_report(&rig.report(song, key)).unwrap();
        }
        let before = snapshot(&capture);
        let count = capture.records().len();
        let bytes = capture.encoded_bytes();
        let report = if input_at_end {
            rig.report(end, Some((4, ButtonState::Down)))
        } else {
            rig.report(end + 1, None)
        };
        assert!(matches!(
            capture.record_report(&report),
            Err(CaptureError::OutsideSection)
        ));
        assert_eq!(capture.records().len(), count);
        assert_eq!(capture.encoded_bytes(), bytes);
        assert_eq!(snapshot(&capture), before);
        let unbound = rig.report(end + 2, Some((99, ButtonState::Down)));
        assert!(unbound.bound_inputs.is_empty());
        capture.record_report(&unbound).unwrap();
        assert_eq!(
            snapshot(&capture),
            before,
            "a report with no accepted operation remains a no-op after the fence"
        );
        assert_eq!(capture.into_bytes().unwrap(), before);
    }
}

#[test]
fn finite_section_reconstructs_actual_seeded_runtime_reports_with_offset_once_and_no_synthetic_advance()
 {
    let seed = u64::MAX;
    let source = source(seed);
    let start = 1_000_000_000;
    let end = 2_500_000_000;
    let judge = judge(&source, start, 10);
    let future = judge
        .chart()
        .objects()
        .iter()
        .find(|object| object.time.start.as_nanos() == 3_000_000_000)
        .unwrap()
        .id;
    let mut capture = LiveReplayCapture::new_section(
        &judge,
        ClockDomainId(11),
        limits(4096),
        Timestamp::from_nanos(start),
        seed,
        Some(Timestamp::from_nanos(end)),
    )
    .unwrap();
    let mut rig = Rig::new(judge, start);
    let mut events = Vec::new();
    let mut physical = Vec::new();
    for (song, key) in [
        (900_000_000, None),
        (999_999_990, Some((4, ButtonState::Down))),
        (1_100_000_000, Some((4, ButtonState::Up))),
        (1_999_999_990, Some((4, ButtonState::Down))),
        (end, None),
    ] {
        let report = rig.report(song, key);
        if let Some(input) = &report.input {
            physical.push(input.clone());
        }
        capture.record_report(&report).unwrap();
        events.extend(report.judge_events);
    }
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| matches!(
        event.outcome,
        JudgeOutcome::Hit {
            delta: Duration::ZERO,
            ..
        }
    )));
    assert_eq!(
        rig.runtime.judge().state(future),
        Some(InteractionState::Pending)
    );
    let hash = rig.runtime.judge().stable_hash().unwrap();
    let bytes = capture.into_bytes().unwrap();
    let file = decode_replay(&bytes, limits(4096)).unwrap();
    assert_eq!(
        file.records
            .iter()
            .map(|record| record.song_time.as_nanos())
            .collect::<Vec<_>>(),
        [900_000_000, 999_999_990, 1_100_000_000, 1_999_999_990, end]
    );
    let recorded_inputs = file
        .records
        .iter()
        .filter_map(|record| match &record.operation {
            ReplayOperation::Input(input) => Some(input.physical.clone()),
            ReplayOperation::Advance => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(recorded_inputs, physical);
    let pristine = validate_section_setup(&source, &file, limits(4096)).unwrap();
    assert!(pristine.effective_song_time().is_none());
    let replay = reconstruct_section(&source, file.clone(), limits(4096)).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    assert_eq!(
        replay.engine().state(future),
        Some(InteractionState::Pending)
    );
    assert_eq!(replay.records().len(), 5);
    let mut prefix = file.clone();
    prefix.records.truncate(2);
    let prefix = reconstruct_section(&source, prefix, limits(4096)).unwrap();
    assert_eq!(prefix.results(), &events[..1]);
    assert_eq!(prefix.records().len(), 2);
    assert_eq!(
        prefix.engine().state(future),
        Some(InteractionState::Pending)
    );
    let mut empty = file;
    empty.records.clear();
    let empty = reconstruct_section(&source, empty, limits(4096)).unwrap();
    assert!(empty.results().is_empty());
    assert!(empty.engine().effective_song_time().is_none());
}

#[test]
fn finite_log_admission_checks_real_identity_bounds_and_legacy_consumers_refuse_the_new_contract() {
    let source = source(0);
    let judge = judge(&source, 0, 10);
    let end = Timestamp::from_nanos(2_000_000_000);
    let mut capture = LiveReplayCapture::new_section(
        &judge,
        ClockDomainId(11),
        limits(4096),
        Timestamp::ZERO,
        0,
        Some(end),
    )
    .unwrap();
    let mut rig = Rig::new(judge, 0);
    capture
        .record_report(&rig.report(-10, Some((4, ButtonState::Down))))
        .unwrap();
    capture
        .record_report(&rig.report(end.as_nanos(), None))
        .unwrap();
    let file = capture.into_file();
    assert!(validate_section_setup(&source, &file, limits(4096)).is_ok());
    assert!(decode_profile(&file.header.options).is_err());
    assert!(decode_setup(&file.header.options).is_err());
    assert!(decode_chart_setup(&file.header.options).is_err());
    assert!(validate_setup(&source, &file, limits(4096)).is_err());
    assert!(reconstruct(&source, file.clone(), limits(4096)).is_err());
    for case in 0..8 {
        let mut invalid = file.clone();
        match case {
            0 => invalid.records[0].song_time = end,
            1 => invalid.records[1].song_time = Timestamp::from_nanos(end.as_nanos() + 1),
            2 => invalid.runtime_version.push('x'),
            3 => invalid.header.rules_identity.push(0),
            4 => invalid.header.seed = 1,
            5 => invalid.header.chart_identity[0] ^= 1,
            6 => invalid.header.normalized_clock = ClockDomainId(99),
            7 => invalid.records[0].ordinal = 5,
            _ => unreachable!(),
        }
        assert!(
            validate_section_setup(&source, &invalid, limits(4096)).is_err(),
            "invalid finite log case {case}"
        );
        assert!(reconstruct_section(&source, invalid, limits(4096)).is_err());
    }
    assert!(
        validate_section_setup(&source, &file, limits(4096)).is_ok(),
        "failed candidates leave the original file reusable"
    );
}
