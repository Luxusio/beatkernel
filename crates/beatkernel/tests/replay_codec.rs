use beatkernel::{
    chart::*,
    input::*,
    interaction::HoldEvaluator,
    judge::*,
    replay::{codec::*, *},
    time::*,
};
fn limits(bytes: usize) -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        bytes,
        128,
        256.min(bytes),
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn fixture() -> ReplayFile {
    let header = ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"hold-chart/v1".to_vec(),
        rules_identity: b"hold-rule/v1".to_vec(),
        options: vec![0, 255],
        seed: u64::MAX,
        normalized_clock: ClockDomainId(1),
    };
    let mut records = Vec::new();
    for (ordinal, nanos, state) in [(0, 100, ButtonState::Down), (1, 200, ButtonState::Up)] {
        let point = ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(nanos),
        };
        let mut meta = EventMeta::new(DeviceId(7), point, ordinal);
        meta.native = Some(NativeEventMeta {
            backend: BackendId(3),
            code: Some(42),
            timestamp: Some(point),
        });
        meta.original_clock_point = Some(ClockPoint {
            domain: ClockDomainId(9),
            timestamp: Timestamp::MIN,
        });
        records.push(ReplayRecord {
            ordinal,
            song_time: point.timestamp,
            operation: ReplayOperation::Input(GameInputEvent {
                game_control: GameControlId(1),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control: PhysicalControlId::keyboard(4),
                    state,
                }),
            }),
        });
    }
    records.push(ReplayRecord {
        ordinal: 2,
        song_time: Timestamp::from_nanos(201),
        operation: ReplayOperation::Advance,
    });
    let mut file = ReplayFile::new(header, records);
    file.calibration_metadata = Some(vec![0, 1, 255]);
    file
}
fn engine() -> JudgeEngine {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(100).unwrap(),
        end: Some(Beat::new(200).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(1),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    JudgeEngine::new(
        chart.compile().unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(HoldEvaluator),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(10),
                late: Duration::from_nanos(10),
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn durable_roundtrip_retains_provenance_and_reconstructs_same_judge_results_and_hash() {
    let file = fixture();
    let bytes = encode_replay(&file, limits(1_000_000)).unwrap();
    assert_eq!(&bytes[..8], b"BKREPLAY");
    assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
    let decoded = decode_replay(&bytes, limits(1_000_000)).unwrap();
    assert_eq!(decoded, file);
    assert_eq!(encode_replay(&decoded, limits(bytes.len())).unwrap(), bytes);
    let direct = ReplaySession::from_records(file.header, engine(), file.records).unwrap();
    let mut restored =
        ReplaySession::from_records(decoded.header, engine(), decoded.records).unwrap();
    assert_eq!(direct.results(), restored.results());
    assert_eq!(
        direct.stable_hash().unwrap(),
        restored.stable_hash().unwrap()
    );
    restored.seek_cursor(1).unwrap();
    restored.seek_cursor(3).unwrap();
    assert_eq!(
        direct.stable_hash().unwrap(),
        restored.stable_hash().unwrap()
    );
}
#[test]
fn every_truncated_prefix_and_trailing_bytes_fail_without_partial_log() {
    let bytes = encode_replay(&fixture(), limits(1_000_000)).unwrap();
    for cut in 0..bytes.len() {
        assert!(
            decode_replay(&bytes[..cut], limits(1_000_000)).is_err(),
            "cut {cut}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        decode_replay(&trailing, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::TrailingBytes
    );
    assert_eq!(
        encode_replay(&fixture(), limits(bytes.len() - 1)).unwrap_err(),
        ReplayCodecError::FileTooLarge
    );
}
#[test]
fn encode_and_decode_reject_bad_versions_ordinals_domains_tags_and_count_extents() {
    let mut file = fixture();
    file.records[0].ordinal = 1;
    assert_eq!(
        encode_replay(&file, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::Replay(ReplayError::InvalidOrdinal)
    );
    file = fixture();
    file.header.normalized_clock = ClockDomainId(2);
    assert_eq!(
        encode_replay(&file, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::Replay(ReplayError::ClockDomainMismatch)
    );
    file = fixture();
    file.records[2].song_time = Timestamp::ZERO;
    assert_eq!(
        encode_replay(&file, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::Replay(ReplayError::NonMonotonicSongTime)
    );
    file = fixture();
    let mut empty = file.clone();
    empty.records.clear();
    let start = encode_replay(&empty, limits(1_000_000)).unwrap().len();
    let original = encode_replay(&file, limits(1_000_000)).unwrap();
    let mut bytes = original.clone();
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        decode_replay(&bytes, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::UnsupportedVersion(2)
    );
    bytes = original.clone();
    bytes[start..start + 8].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        decode_replay(&bytes, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::Replay(ReplayError::InvalidOrdinal)
    );
    bytes = original.clone();
    bytes[start + 16] = 9;
    assert_eq!(
        decode_replay(&bytes, limits(1_000_000)).unwrap_err(),
        ReplayCodecError::InvalidTag(9)
    );
    bytes = original.clone();
    bytes[start - 8..start].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        decode_replay(&bytes, limits(1_000_000)),
        Err(ReplayCodecError::TooManyRecords | ReplayCodecError::LengthOverflow)
    ));
}
#[test]
fn header_and_operation_caps_are_independent_of_nested_payload_limits() {
    let file = fixture();
    let header_limits =
        ReplayCodecLimits::new(1_000_000, 128, 1, CodecLimits::new(4096, 0).unwrap()).unwrap();
    assert_eq!(
        encode_replay(&file, header_limits).unwrap_err(),
        ReplayCodecError::HeaderTooLarge
    );
    let bytes = encode_replay(&file, limits(1_000_000)).unwrap();
    assert_eq!(
        decode_replay(&bytes, header_limits).unwrap_err(),
        ReplayCodecError::HeaderTooLarge
    );
    let count_limits =
        ReplayCodecLimits::new(1_000_000, 1, 256, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    assert_eq!(
        decode_replay(&bytes, count_limits).unwrap_err(),
        ReplayCodecError::TooManyRecords
    );
}

#[test]
fn borrowed_parts_match_default_envelope_bytes_for_original_provenance_and_empty_logs() {
    let source = fixture();
    for records in [source.records, Vec::new()] {
        let file = ReplayFile::new(source.header.clone(), records);
        let owned = encode_replay(&file, limits(1_000_000)).unwrap();
        let borrowed =
            encode_replay_parts(&file.header, &file.records, limits(owned.len())).unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(decode_replay(&borrowed, limits(owned.len())).unwrap(), file);
    }
}

#[test]
fn borrowed_parts_preserve_header_chronology_domain_and_all_independent_limits() {
    let source = fixture();
    let file = ReplayFile::new(source.header, source.records);
    let check = |candidate: &ReplayFile, bounds, expected| {
        assert_eq!(
            encode_replay_parts(&candidate.header, &candidate.records, bounds).unwrap_err(),
            expected
        );
        assert_eq!(encode_replay(candidate, bounds).unwrap_err(), expected);
    };
    let mut invalid = file.clone();
    invalid.header.version = REPLAY_VERSION + 1;
    check(
        &invalid,
        limits(1_000_000),
        ReplayCodecError::Replay(ReplayError::UnsupportedVersion(REPLAY_VERSION + 1)),
    );
    invalid = file.clone();
    invalid.records[1].ordinal = 2;
    check(
        &invalid,
        limits(1_000_000),
        ReplayCodecError::Replay(ReplayError::InvalidOrdinal),
    );
    invalid = file.clone();
    invalid.records[2].song_time = Timestamp::from_nanos(199);
    check(
        &invalid,
        limits(1_000_000),
        ReplayCodecError::Replay(ReplayError::NonMonotonicSongTime),
    );
    invalid = file.clone();
    invalid.header.normalized_clock = ClockDomainId(2);
    check(
        &invalid,
        limits(1_000_000),
        ReplayCodecError::Replay(ReplayError::ClockDomainMismatch),
    );
    let bytes = encode_replay(&file, limits(1_000_000)).unwrap();
    check(
        &file,
        limits(bytes.len() - 1),
        ReplayCodecError::FileTooLarge,
    );
    let header_size = file.runtime_version.len()
        + file.header.chart_identity.len()
        + file.header.rules_identity.len()
        + file.header.options.len();
    check(
        &file,
        ReplayCodecLimits::new(
            1_000_000,
            128,
            header_size - 1,
            CodecLimits::new(4096, 1024).unwrap(),
        )
        .unwrap(),
        ReplayCodecError::HeaderTooLarge,
    );
    check(
        &file,
        ReplayCodecLimits::new(1_000_000, 2, 256, CodecLimits::new(4096, 1024).unwrap()).unwrap(),
        ReplayCodecError::TooManyRecords,
    );
    check(
        &file,
        ReplayCodecLimits::new(1_000_000, 128, 256, CodecLimits::new(6, 0).unwrap()).unwrap(),
        ReplayCodecError::Input(InputCodecError::EncodedTooLarge),
    );
    invalid = file.clone();
    let ReplayOperation::Input(input) = &mut invalid.records[0].operation else {
        unreachable!();
    };
    input.physical = PhysicalInputEvent::Custom(CustomInputEvent {
        meta: *input.physical.meta(),
        namespace: VendorNamespaceId(17),
        type_id: 23,
        payload: vec![1, 2, 3],
    });
    check(
        &invalid,
        ReplayCodecLimits::new(1_000_000, 128, 256, CodecLimits::new(4096, 2).unwrap()).unwrap(),
        ReplayCodecError::Input(InputCodecError::PayloadTooLarge),
    );
}

#[test]
fn owned_encoder_keeps_custom_runtime_version_and_calibration_envelope() {
    let mut file = fixture();
    file.runtime_version = "custom-runtime/with-original-envelope".into();
    let bytes = encode_replay(&file, limits(1_000_000)).unwrap();
    assert_eq!(decode_replay(&bytes, limits(bytes.len())).unwrap(), file);
    let canonical = encode_replay_parts(&file.header, &file.records, limits(1_000_000)).unwrap();
    assert_ne!(canonical, bytes);
    let restored = decode_replay(&canonical, limits(canonical.len())).unwrap();
    assert_eq!(restored, ReplayFile::new(file.header, file.records));
}
