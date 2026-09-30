use beatkernel::{
    audio::command_queue,
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{
        JudgeEngine, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow,
    },
    replay::{
        codec::{encode_replay, ReplayCodecLimits, ReplayFile},
        ReplaySession,
    },
    runtime::Runtime,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{parse, BmsChart, ParseOptions};
use beatkernel_bms_runtime::{
    replay_capture::LiveReplayCapture,
    replay_playback::{decode_profile, read_replay, reconstruct},
};
use std::io::{self, Read};

const CHART: &str =
    "#BPM 60\n#LNTYPE 1\n#WAV01 tap.wav\n#WAV02 hold.wav\n#00011:00010000\n#00052:00000202\n";
const DOMAIN: ClockDomainId = ClockDomainId(17);
fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn limits(bytes: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        bytes,
        100,
        bytes.min(4096),
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn profile_bytes(offset: i64, windows: &[(u32, i64, i64)]) -> Vec<u8> {
    let mut bytes = b"bms-judge-profile/v1:".to_vec();
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&(windows.len() as u64).to_le_bytes());
    for &(grade, early, late) in windows {
        bytes.extend_from_slice(&grade.to_le_bytes());
        bytes.extend_from_slice(&early.to_le_bytes());
        bytes.extend_from_slice(&late.to_le_bytes());
    }
    bytes
}
fn judge(source: &BmsChart) -> JudgeEngine {
    JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(10),
        )
        .unwrap(),
    )
    .unwrap()
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn fixture() -> (BmsChart, ReplayFile, Vec<JudgeEvent>, u64) {
    let source = parse(CHART, ParseOptions::default()).unwrap();
    let pristine = judge(&source);
    let mut capture = LiveReplayCapture::new(&pristine, DOMAIN, limits(1 << 20)).unwrap();
    let bindings = BindingMap::from_bindings(source.rules().into_iter().map(|rule| Binding {
        device: DeviceSelector::Exact(DeviceId(3)),
        physical: PhysicalControlId::keyboard(rule.control.0 as u16),
        game_control: rule.control,
    }))
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        pristine,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let mut results = Vec::new();
    capture
        .record_report(&runtime.advance_to(point(0), &Identity, point(0)).unwrap())
        .unwrap();
    let instant = source
        .notes
        .iter()
        .find(|note| note.sample.0 == 1)
        .unwrap()
        .lane
        .control();
    let hold = source
        .notes
        .iter()
        .find(|note| note.sample.0 == 2)
        .unwrap()
        .lane
        .control();
    for (index, (control, at, state)) in [
        (instant, 999_999_990, ButtonState::Down),
        (hold, 1_999_999_990, ButtonState::Down),
        (hold, 2_999_999_990, ButtonState::Up),
    ]
    .into_iter()
    .enumerate()
    {
        let mut meta = EventMeta::new(DeviceId(3), point(at), index as u64 + 11);
        meta.original_clock_point = Some(ClockPoint {
            domain: ClockDomainId(90),
            timestamp: Timestamp::from_nanos(at - 100),
        });
        meta.native = Some(NativeEventMeta {
            backend: BackendId(5),
            code: Some(control.0),
            timestamp: Some(point(at)),
        });
        let report = runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control: PhysicalControlId::keyboard(control.0 as u16),
                    state,
                }),
                &Identity,
                point(at),
            )
            .unwrap();
        assert!(report.judge_error.is_none());
        results.extend_from_slice(&report.judge_events);
        capture.record_report(&report).unwrap();
    }
    let final_report = runtime
        .advance_to(point(3_000_000_001), &Identity, point(3_000_000_001))
        .unwrap();
    results.extend_from_slice(&final_report.judge_events);
    capture.record_report(&final_report).unwrap();
    (
        source,
        capture.into_file(),
        results,
        runtime.judge().stable_hash().unwrap(),
    )
}

#[test]
fn captured_bms_tap_and_hold_reconstruct_literal_hits_offset_once_and_provenance() {
    let (source, file, live, hash) = fixture();
    assert_eq!(file.header.options, profile_bytes(10, &[(7, 0, 0)]));
    let replay = reconstruct(&source, file, limits(1 << 20)).unwrap();
    assert_eq!(replay.results(), live);
    assert_eq!(live.len(), 3);
    assert_eq!(
        live.iter().map(|event| event.stage).collect::<Vec<_>>(),
        vec![
            JudgeStage::Instant,
            JudgeStage::HoldHead,
            JudgeStage::HoldTail
        ]
    );
    for (index, event) in live.iter().enumerate() {
        assert_eq!(
            event.outcome,
            JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::ZERO
            }
        );
        assert_eq!(
            event.at,
            Timestamp::from_nanos((index as i64 + 1) * 1_000_000_000)
        );
        let meta = event.input.unwrap();
        assert_eq!(meta.sequence, index as u64 + 11);
        assert_eq!(meta.original_clock_point.unwrap().domain, ClockDomainId(90));
        assert_eq!(meta.native.unwrap().backend, BackendId(5));
        assert_eq!(meta.timestamp.as_nanos(), event.at.as_nanos() - 10);
    }
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
}

#[test]
fn empty_and_failed_session_prefixes_do_not_invent_end_of_chart_operations() {
    let (source, file, live, _) = fixture();
    let mut empty = file.clone();
    empty.records.clear();
    let replay = reconstruct(&source, empty, limits(1 << 20)).unwrap();
    assert_eq!(replay.cursor(), 0);
    assert!(replay.results().is_empty());
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        judge(&source).stable_hash().unwrap()
    );
    let mut prefix = file;
    prefix.records.truncate(2);
    let replay = reconstruct(&source, prefix, limits(1 << 20)).unwrap();
    assert_eq!(replay.cursor(), 2);
    assert_eq!(replay.results(), &live[..1]);
    assert_eq!(
        replay.engine().effective_song_time(),
        Some(Timestamp::from_nanos(1_000_000_000))
    );
}

#[test]
fn reverse_cursor_and_time_seek_match_fresh_linear_prefixes() {
    let (source, file, _, _) = fixture();
    let mut replay = reconstruct(&source, file.clone(), limits(1 << 20)).unwrap();
    for cursor in [4, 1, 3, 0, 5, 2] {
        replay.seek_cursor(cursor).unwrap();
        let prefix = ReplaySession::from_records(
            file.header.clone(),
            judge(&source),
            file.records[..cursor].to_vec(),
        )
        .unwrap();
        assert_eq!(replay.results(), prefix.results());
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            prefix.engine().stable_hash().unwrap()
        );
    }
    for at in [2_500_000_000, 1_500_000_000, 0, 3_000_000_001] {
        let at = Timestamp::from_nanos(at);
        replay.seek(at).unwrap();
        let mut prefix = ReplaySession::from_records(
            file.header.clone(),
            judge(&source),
            file.records
                .iter()
                .filter(|record| record.song_time <= at)
                .cloned(),
        )
        .unwrap();
        prefix.advance_to(at).unwrap();
        assert_eq!(replay.results(), prefix.results());
        assert_eq!(
            replay.engine().stable_hash().unwrap(),
            prefix.engine().stable_hash().unwrap()
        );
    }
}

#[test]
fn incompatible_chart_profile_version_rules_and_seed_reject_before_replay() {
    let (source, file, _, _) = fixture();
    let changed = parse(
        &CHART.replace("#BPM 60", "#BPM 120"),
        ParseOptions::default(),
    )
    .unwrap();
    assert!(reconstruct(&changed, file.clone(), limits(1 << 20)).is_err());
    let mut variants = Vec::new();
    let mut changed = file.clone();
    changed.header.options = profile_bytes(11, &[(7, 0, 0)]);
    variants.push(changed);
    let mut changed = file.clone();
    changed.runtime_version.push_str("-incompatible");
    variants.push(changed);
    let mut changed = file.clone();
    changed.header.version += 1;
    variants.push(changed);
    let mut changed = file.clone();
    changed.header.rules_identity.push(0);
    variants.push(changed);
    let mut changed = file.clone();
    changed.header.seed = 1;
    variants.push(changed);
    let mut changed = file.clone();
    changed.header.chart_identity[0] ^= 1;
    variants.push(changed);
    let mut changed = file.clone();
    changed.records[1].ordinal = 99;
    variants.push(changed);
    let mut changed = file.clone();
    changed.records[2].song_time = Timestamp::from_nanos(-1);
    variants.push(changed);
    let mut changed = file.clone();
    if let beatkernel::replay::ReplayOperation::Input(input) = &mut changed.records[1].operation {
        input.physical.meta_mut().clock_domain = ClockDomainId(99);
    } else {
        panic!("fixture input record missing");
    }
    variants.push(changed);
    for changed in variants {
        assert!(reconstruct(&source, changed, limits(1 << 20)).is_err());
    }
}

#[test]
fn profile_metadata_rejects_bad_extent_tags_counts_and_windows() {
    let valid = profile_bytes(-23, &[(8, 10, 20), (3, 30, 40)]);
    let decoded = decode_profile(&valid).unwrap();
    let expected = JudgeProfile::new(
        vec![
            JudgeWindow {
                grade: JudgeGrade(8),
                early: Duration::from_nanos(10),
                late: Duration::from_nanos(20),
            },
            JudgeWindow {
                grade: JudgeGrade(3),
                early: Duration::from_nanos(30),
                late: Duration::from_nanos(40),
            },
        ],
        Duration::from_nanos(-23),
    )
    .unwrap();
    assert_eq!(decoded, expected);
    let mut cases = vec![
        vec![],
        profile_bytes(0, &[]),
        profile_bytes(0, &[(1, -1, 0)]),
        profile_bytes(0, &[(1, 0, -1)]),
        profile_bytes(0, &[(1, 0, 0), (1, 1, 1)]),
        profile_bytes(0, &[(1, 10, 20), (2, 9, 21)]),
        profile_bytes(0, &[(1, 10, 20), (2, 11, 19)]),
    ];
    let mut trailing = valid.clone();
    trailing.push(0);
    cases.push(trailing);
    let mut wrong_tag = valid.clone();
    wrong_tag[0] ^= 1;
    cases.push(wrong_tag);
    let mut enormous = profile_bytes(0, &[]);
    let count_start = b"bms-judge-profile/v1:".len() + 8;
    enormous[count_start..count_start + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    cases.push(enormous);
    for length in [1, count_start, count_start + 7, valid.len() - 1] {
        cases.push(valid[..length].to_vec());
    }
    for invalid in cases {
        assert!(decode_profile(&invalid).is_err());
    }
}

struct ShortReads<'a>(&'a [u8]);
impl Read for ShortReads<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let length = output.len().min(self.0.len()).min(3);
        output[..length].copy_from_slice(&self.0[..length]);
        self.0 = &self.0[length..];
        Ok(length)
    }
}
struct BrokenReader;
impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("literal reader failure"))
    }
}

#[test]
fn bounded_reader_accepts_exact_cap_and_short_reads_rejects_growth_truncation_and_io() {
    let (_, file, _, _) = fixture();
    let bytes = encode_replay(&file, limits(1 << 20)).unwrap();
    let exact = limits(bytes.len());
    assert_eq!(read_replay(&mut ShortReads(&bytes), exact).unwrap(), file);
    let mut growing = bytes.clone();
    growing.push(0);
    assert!(read_replay(&mut ShortReads(&growing), exact).is_err());
    assert!(read_replay(&mut ShortReads(&bytes[..bytes.len() - 1]), exact).is_err());
    assert!(read_replay(&mut ShortReads(&[]), exact).is_err());
    assert!(read_replay(&mut BrokenReader, exact).is_err());
}
