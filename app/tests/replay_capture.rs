use beatkernel::{
    audio::{command_queue, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId,
        DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    replay::{
        codec::{decode_replay, encode_replay, ReplayCodecLimits, ReplayFile},
        ReplayOperation, ReplaySession,
    },
    runtime::{Runtime, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::replay_capture::{CaptureError, LiveReplayCapture};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Barrier,
    },
};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

struct InputClock;
impl ClockMapper for InputClock {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == ClockDomainId(9) && to == ClockDomainId(1))
            .then(|| from.timestamp.checked_add(Duration::from_nanos(10)))
            .flatten()
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn limits(bytes: usize, records: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        bytes,
        records,
        bytes.min(4096),
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}

fn engine() -> JudgeEngine {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    for id in 1..=3 {
        chart.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(id as i64 * 100).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(id as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    JudgeEngine::new(
        chart.compile().unwrap(),
        (1..=3)
            .map(|control| Rule {
                interaction: InteractionId(control),
                control: GameControlId(control),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(10),
        )
        .unwrap(),
    )
    .unwrap()
}

fn runtime(capacity: usize) -> (Runtime, CommandConsumer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let bindings = BindingMap::from_bindings((1..=3).map(|control| Binding {
        device: DeviceSelector::Exact(DeviceId(7)),
        physical: PhysicalControlId::keyboard(control as u16 + 3),
        game_control: GameControlId(control),
    }))
    .unwrap();
    (
        Runtime::new(
            ClockDomainId(1),
            ClockDomainId(2),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
            bindings,
            engine(),
            producer,
            (1..=3)
                .map(|id| SoundBinding {
                    object: ObjectId(id),
                    stage: JudgeStage::Instant,
                    sample: SampleId(1),
                    voice: VoiceId(id),
                    gain: 1.0,
                })
                .collect(),
            8,
        )
        .unwrap(),
        consumer,
    )
}

fn input(control: u16, nanos: i64, sequence: u64) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(7), point(9, nanos), sequence);
    meta.original_clock_point = Some(point(8, -123));
    meta.native = Some(NativeEventMeta {
        backend: BackendId(4),
        code: Some(30),
        timestamp: Some(point(9, nanos)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(control + 3),
        state: ButtonState::Down,
    })
}

fn reports() -> (Runtime, CommandConsumer, Vec<RuntimeReport>) {
    let (mut runtime, consumer) = runtime(1);
    let first = runtime
        .process_input(input(1, 80, 40), &InputClock, point(2, 5000))
        .unwrap();
    let second = runtime
        .process_input(input(2, 180, 41), &InputClock, point(2, 6000))
        .unwrap();
    let last = runtime
        .advance_to(point(1, 301), &InputClock, point(2, 7000))
        .unwrap();
    (runtime, consumer, vec![first, second, last])
}

#[test]
fn actual_live_reports_replay_with_one_offset_and_original_provenance_despite_queue_full() {
    let pristine = engine();
    let hash = pristine.stable_hash().unwrap();
    let mut capture =
        LiveReplayCapture::new(&pristine, ClockDomainId(1), limits(1 << 20, 100)).unwrap();
    let mut identity = b"bms-judge-setup/v1:".to_vec();
    identity.extend_from_slice(&hash.to_le_bytes());
    assert_eq!(capture.header().chart_identity, identity);
    let mut profile_bytes = b"bms-judge-profile/v1:".to_vec();
    profile_bytes.extend_from_slice(&10_i64.to_le_bytes());
    profile_bytes.extend_from_slice(&1_u64.to_le_bytes());
    profile_bytes.extend_from_slice(&1_u32.to_le_bytes());
    profile_bytes.extend_from_slice(&0_i64.to_le_bytes());
    profile_bytes.extend_from_slice(&0_i64.to_le_bytes());
    assert_eq!(capture.header().options, profile_bytes);
    let (runtime, _consumer, reports) = reports();
    assert_eq!(reports[1].audio_failures.len(), 1);
    assert_eq!(reports[1].audio_failures[0].reason, QueuePushError::Full);
    for report in &reports {
        capture.record_report(report).unwrap();
    }
    assert_eq!(capture.records().len(), 3);
    assert_eq!(capture.records()[0].song_time, Timestamp::from_nanos(90));
    assert_eq!(capture.records()[1].song_time, Timestamp::from_nanos(190));
    assert!(matches!(
        capture.records()[2].operation,
        ReplayOperation::Advance
    ));
    let ReplayOperation::Input(bound) = &capture.records()[0].operation else {
        panic!("expected input")
    };
    assert_eq!(bound, &reports[0].bound_inputs[0]);
    assert_eq!(bound.physical.meta().timestamp, Timestamp::from_nanos(90));
    assert_eq!(bound.physical.meta().clock_domain, ClockDomainId(1));
    assert_eq!(
        bound.physical.meta().original_clock_point,
        Some(point(8, -123))
    );
    assert_eq!(
        bound.physical.meta().native.unwrap().timestamp,
        Some(point(9, 80))
    );
    let replay = ReplaySession::from_records(
        capture.header().clone(),
        pristine,
        capture.records().to_vec(),
    )
    .unwrap();
    let live_results: Vec<_> = reports
        .iter()
        .flat_map(|report| report.judge_events.iter().copied())
        .collect();
    assert_eq!(replay.results(), live_results);
    assert_eq!(live_results.len(), 3);
    for event in &live_results[..2] {
        assert_eq!(
            event.outcome,
            JudgeOutcome::Hit {
                grade: JudgeGrade(1),
                delta: Duration::ZERO
            }
        );
    }
    assert!(matches!(live_results[2].outcome, JudgeOutcome::Miss { .. }));
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        runtime.judge().stable_hash().unwrap()
    );
}

#[test]
fn exact_canonical_byte_accounting_includes_empty_header_and_each_complete_report() {
    let policy = limits(1 << 20, 100);
    let mut capture = LiveReplayCapture::new(&engine(), ClockDomainId(1), policy).unwrap();
    let (_, _consumer, reports) = reports();
    for report in std::iter::once(None).chain(reports.iter().map(Some)) {
        if let Some(report) = report {
            capture.record_report(report).unwrap();
        }
        let file = ReplayFile::new(capture.header().clone(), capture.records().to_vec());
        assert_eq!(
            capture.encoded_bytes(),
            encode_replay(&file, policy).unwrap().len()
        );
    }
    let expected = capture.encoded_bytes();
    let file = capture.into_file();
    let encoded = encode_replay(&file, policy).unwrap();
    assert_eq!(encoded.len(), expected);
    assert_eq!(decode_replay(&encoded, policy).unwrap(), file);
}

#[test]
fn malformed_domain_and_song_regression_preserve_prefix_and_future_admission() {
    let mut capture =
        LiveReplayCapture::new(&engine(), ClockDomainId(1), limits(1 << 20, 100)).unwrap();
    let (_, _consumer, reports) = reports();
    capture.record_report(&reports[0]).unwrap();
    let prefix = capture.records().to_vec();
    let bytes = capture.encoded_bytes();
    let mut wrong_domain = reports[1].clone();
    wrong_domain.bound_inputs[0]
        .physical
        .meta_mut()
        .clock_domain = ClockDomainId(99);
    let mut regressed = reports[1].clone();
    regressed.song_time = Timestamp::from_nanos(89);
    for malformed in [&wrong_domain, &regressed] {
        assert!(capture.record_report(malformed).is_err());
        assert_eq!(capture.records(), prefix);
        assert_eq!(capture.encoded_bytes(), bytes);
    }
    capture.record_report(&reports[1]).unwrap();
    assert_eq!(capture.records()[1].ordinal, 1);

    let mut oversized = reports[2].clone();
    oversized.input = reports[1].input.clone();
    oversized.bound_inputs = reports[1].bound_inputs.clone();
    oversized.bound_inputs[0].physical =
        PhysicalInputEvent::Custom(beatkernel::input::CustomInputEvent {
            meta: *reports[1].bound_inputs[0].physical.meta(),
            namespace: beatkernel::input::VendorNamespaceId(17),
            type_id: 3,
            payload: vec![0x5a; 1025],
        });
    let prefix = capture.records().to_vec();
    let bytes = capture.encoded_bytes();
    assert!(capture.record_report(&oversized).is_err());
    assert_eq!(capture.records(), prefix);
    assert_eq!(capture.encoded_bytes(), bytes);
}

#[test]
fn count_limit_rejects_whole_report_and_retains_prior_complete_prefix() {
    let (_, _consumer, reports) = reports();
    let mut capture =
        LiveReplayCapture::new(&engine(), ClockDomainId(1), limits(1 << 20, 2)).unwrap();
    capture.record_report(&reports[0]).unwrap();
    let mut fanout = reports[1].clone();
    fanout.bound_inputs.push(fanout.bound_inputs[0].clone());
    let before = capture.encoded_bytes();
    assert!(capture.record_report(&fanout).is_err());
    assert_eq!(capture.records().len(), 1);
    assert_eq!(capture.encoded_bytes(), before);
    capture.record_report(&reports[1]).unwrap();
    assert_eq!(capture.records().len(), 2);
    assert!(capture.record_report(&reports[2]).is_err());
    assert_eq!(capture.records().len(), 2);
}

#[test]
fn byte_limit_and_nested_input_limit_do_not_damage_earlier_operations() {
    let (_, _consumer, reports) = reports();
    let generous = limits(1 << 20, 100);
    let header = LiveReplayCapture::new(&engine(), ClockDomainId(1), generous)
        .unwrap()
        .header()
        .clone();
    let mut reference = LiveReplayCapture::new(&engine(), ClockDomainId(1), generous).unwrap();
    reference.record_report(&reports[0]).unwrap();
    reference.record_report(&reports[1]).unwrap();
    let two_size = encode_replay(
        &ReplayFile::new(header, reference.records().to_vec()),
        generous,
    )
    .unwrap()
    .len();
    let mut bounded =
        LiveReplayCapture::new(&engine(), ClockDomainId(1), limits(two_size - 1, 100)).unwrap();
    bounded.record_report(&reports[0]).unwrap();
    let before = bounded.encoded_bytes();
    assert!(bounded.record_report(&reports[1]).is_err());
    assert_eq!(bounded.records(), &reference.records()[..1]);
    assert_eq!(bounded.encoded_bytes(), before);

    let nested =
        ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(6, 0).unwrap()).unwrap();
    let mut bounded = LiveReplayCapture::new(&engine(), ClockDomainId(1), nested).unwrap();
    let (mut runtime, _consumer) = runtime(1);
    let advance = runtime
        .advance_to(point(1, 0), &InputClock, point(2, 0))
        .unwrap();
    bounded.record_report(&advance).unwrap();
    let before = bounded.encoded_bytes();
    assert!(bounded.record_report(&reports[0]).is_err());
    assert_eq!(bounded.records().len(), 1);
    assert!(matches!(
        bounded.records()[0].operation,
        ReplayOperation::Advance
    ));
    assert_eq!(bounded.encoded_bytes(), before);
}

#[test]
fn capture_requires_pristine_judge_even_after_result_free_advance() {
    let mut judge = engine();
    assert!(judge.advance_to(Timestamp::ZERO).unwrap().is_empty());
    assert!(LiveReplayCapture::new(&judge, ClockDomainId(1), limits(1 << 20, 100)).is_err());
    let (runtime, _consumer, _) = reports();
    assert!(
        LiveReplayCapture::new(runtime.judge(), ClockDomainId(1), limits(1 << 20, 100)).is_err()
    );
}

struct TempDirectory(PathBuf);
impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..1000 {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-replay-capture-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("temporary directory: {error}"),
            }
        }
        panic!("temporary directory collision budget exhausted")
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exclusive_save_writes_canonical_log_and_never_overwrites_existing_file() {
    let directory = TempDirectory::new();
    let path = directory.0.join("session.bkr");
    let policy = limits(1 << 20, 100);
    let (_, _consumer, reports) = reports();
    let make_capture = || {
        let mut capture = LiveReplayCapture::new(&engine(), ClockDomainId(1), policy).unwrap();
        capture.record_report(&reports[0]).unwrap();
        capture
    };
    let capture = make_capture();
    let expected = encode_replay(
        &ReplayFile::new(capture.header().clone(), capture.records().to_vec()),
        policy,
    )
    .unwrap();
    assert_eq!(capture.save_new(&path).unwrap(), expected.len());
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert!(make_capture().save_new(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), expected);
    let existing = directory.0.join("unrelated.txt");
    fs::write(&existing, b"keep existing bytes").unwrap();
    assert!(make_capture().save_new(&existing).is_err());
    assert_eq!(fs::read(existing).unwrap(), b"keep existing bytes");
}

fn capture_prefix(count: usize, policy: ReplayCodecLimits) -> LiveReplayCapture {
    let mut capture = LiveReplayCapture::new(&engine(), ClockDomainId(1), policy).unwrap();
    let (_, _consumer, reports) = reports();
    for report in reports.iter().take(count) {
        capture.record_report(report).unwrap();
    }
    capture
}

fn assert_only_entry(directory: &TempDirectory, expected: &std::path::Path) {
    let entries: Vec<_> = fs::read_dir(&directory.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries, vec![expected.to_path_buf()]);
}

#[test]
fn save_new_preserves_rejected_report_prefix_header_identity_and_original_limits() {
    let directory = TempDirectory::new();
    let path = directory.0.join("accepted-prefix.bkr");
    let policy = limits(1 << 20, 1);
    let mut capture = capture_prefix(1, policy);
    let (_, _consumer, reports) = reports();
    assert!(capture.record_report(&reports[1]).is_err());
    let expected = ReplayFile::new(capture.header().clone(), capture.records().to_vec());
    let expected_bytes = encode_replay(&expected, policy).unwrap();
    assert_eq!(capture.save_new(&path).unwrap(), expected_bytes.len());
    let actual = fs::read(&path).unwrap();
    assert_eq!(actual, expected_bytes);
    assert_eq!(decode_replay(&actual, policy).unwrap(), expected);
    assert_eq!(expected.records.len(), 1);
    assert_only_entry(&directory, &path);
}

#[test]
fn competing_public_replay_saves_publish_exactly_one_complete_capture() {
    let directory = TempDirectory::new();
    let path = directory.0.join("race.bkr");
    let policy = limits(1 << 20, 100);
    let expected: Vec<_> = (1..=2)
        .map(|count| capture_prefix(count, policy).into_bytes().unwrap())
        .collect();
    assert_ne!(expected[0], expected[1]);
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (1..=2)
        .map(|count| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let capture = capture_prefix(count, policy);
                barrier.wait();
                capture.save_new(&path)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    let winners: Vec<_> = outcomes
        .iter()
        .enumerate()
        .filter_map(|(index, result)| result.as_ref().ok().map(|length| (index, *length)))
        .collect();
    assert_eq!(winners.len(), 1);
    let (winner, length) = winners[0];
    assert_eq!(length, expected[winner].len());
    assert!(matches!(
        &outcomes[1 - winner],
        Err(CaptureError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists
    ));
    let actual = fs::read(&path).unwrap();
    assert_eq!(actual, expected[winner]);
    assert_eq!(
        decode_replay(&actual, policy).unwrap().records.len(),
        winner + 1
    );
    assert_only_entry(&directory, &path);
}

#[test]
fn replay_save_missing_parent_preserves_io_error_and_creates_no_paths() {
    let directory = TempDirectory::new();
    let missing_parent = directory.0.join("missing");
    let path = missing_parent.join("session.bkr");
    assert!(matches!(
        capture_prefix(1, limits(1 << 20, 100)).save_new(&path),
        Err(CaptureError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
    ));
    assert!(!missing_parent.exists());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn replay_save_existing_directory_is_preserved_and_owned_stage_is_cleaned() {
    let directory = TempDirectory::new();
    let target = directory.0.join("occupied.bkr");
    fs::create_dir(&target).unwrap();
    let sentinel = target.join("keep.txt");
    fs::write(&sentinel, b"foreign directory content").unwrap();
    assert!(matches!(
        capture_prefix(1, limits(1 << 20, 100)).save_new(&target),
        Err(CaptureError::Io(_))
    ));
    assert_eq!(fs::read(sentinel).unwrap(), b"foreign directory content");
    assert_only_entry(&directory, &target);
}

#[test]
fn replay_save_rejects_reserved_staging_names_and_native_aliases_without_effects() {
    let directory = TempDirectory::new();
    for name in [
        "0123ABCD.0EF",
        "0123abcd.0ef",
        " 0123ABCD.0EF",
        "0123ABCD.0EF. ",
        "0123ABCD.0EF:stream",
    ] {
        let path = directory.0.join(name);
        assert!(
            matches!(
                capture_prefix(1, limits(1 << 20, 100)).save_new(&path),
                Err(CaptureError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput
            ),
            "reserved basename {name:?}"
        );
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
    }
}

#[test]
fn replay_save_reserved_name_validation_precedes_missing_parent_io() {
    let directory = TempDirectory::new();
    let missing_parent = directory.0.join("missing");
    let path = missing_parent.join("0123ABCD.0EF");
    assert!(matches!(
        capture_prefix(1, limits(1 << 20, 100)).save_new(&path),
        Err(CaptureError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput
    ));
    assert!(!missing_parent.exists());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn replay_save_reserved_existing_foreign_file_is_untouched() {
    let directory = TempDirectory::new();
    let path = directory.0.join("0123ABCD.0EF");
    fs::write(&path, b"foreign staging namespace occupant").unwrap();
    assert!(matches!(
        capture_prefix(1, limits(1 << 20, 100)).save_new(&path),
        Err(CaptureError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput
    ));
    assert_eq!(
        fs::read(&path).unwrap(),
        b"foreign staging namespace occupant"
    );
    assert_only_entry(&directory, &path);
}

#[cfg(unix)]
#[test]
fn replay_save_never_replaces_dangling_symlink_or_creates_its_target() {
    let directory = TempDirectory::new();
    let path = directory.0.join("foreign.bkr");
    let foreign_target = directory.0.join("missing-target");
    std::os::unix::fs::symlink(&foreign_target, &path).unwrap();
    assert!(matches!(
        capture_prefix(1, limits(1 << 20, 100)).save_new(&path),
        Err(CaptureError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists
    ));
    assert_eq!(fs::read_link(&path).unwrap(), foreign_target);
    assert!(!foreign_target.exists());
    assert_only_entry(&directory, &path);
}

#[cfg(unix)]
#[test]
fn replay_save_accepts_long_native_basename_without_extending_it_for_staging() {
    let directory = TempDirectory::new();
    let path = directory.0.join(format!("{}.bkr", "r".repeat(240)));
    let policy = limits(1 << 20, 100);
    let expected = capture_prefix(1, policy).into_bytes().unwrap();
    assert_eq!(
        capture_prefix(1, policy).save_new(&path).unwrap(),
        expected.len()
    );
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_only_entry(&directory, &path);
}

#[cfg(unix)]
#[test]
fn replay_save_accepts_non_utf8_native_basename_and_removes_owned_stage() {
    use std::os::unix::ffi::OsStringExt;
    let directory = TempDirectory::new();
    let path = directory
        .0
        .join(std::ffi::OsString::from_vec(b"native-\xff.bkr".to_vec()));
    let policy = limits(1 << 20, 100);
    let expected = capture_prefix(1, policy).into_bytes().unwrap();
    assert_eq!(
        capture_prefix(1, policy).save_new(&path).unwrap(),
        expected.len()
    );
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_only_entry(&directory, &path);
}
