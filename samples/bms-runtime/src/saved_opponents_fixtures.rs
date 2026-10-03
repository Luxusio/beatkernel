//! Real canonical replay and judge fixtures; source authored for deferred execution.
use crate::{
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    competition::{CompetitionError, OpponentKind, ScoreSummary},
    multiplayer::competition_identity,
    prepare_from_source,
    replay_capture::{CaptureError, LiveReplayCapture},
    saved_opponents::{SavedOpponents, SavedOpponentsError},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
    ChannelPolicy, WavDecoder,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, GameInputEvent, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeEvent, JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        codec::{decode_replay, encode_replay, ReplayCodecError, ReplayCodecLimits, ReplayFile},
        ReplayHeader, ReplaySession,
    },
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::{parse_seeded, BmsChart, ParseOptions};

const CHART: &str = "#BPM 60\n#WAV01 key.wav\n#00011:00010001\n";
const HIT_OPERATION: i64 = 999_999_990;

fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(nanos),
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1 << 20, 100, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}

struct Recording {
    source: BmsChart,
    file: ReplayFile,
    bytes: Vec<u8>,
    events: Vec<JudgeEvent>,
}

fn recording(chart: &str, domain: u32, seed: u64, offset: i64, complete: bool) -> Recording {
    let source = parse_seeded(chart, ParseOptions::default(), seed).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(offset),
        )
        .unwrap(),
    )
    .unwrap();
    let header = LiveReplayCapture::new_at_with_chart_seed(
        &judge,
        ClockDomainId(domain),
        limits(),
        Timestamp::ZERO,
        seed,
    )
    .unwrap()
    .header()
    .clone();
    let mut replay = ReplaySession::new(header.clone(), judge).unwrap();
    replay.advance_to(ts(-100_000_000)).unwrap();
    replay
        .push_input(
            GameInputEvent {
                game_control: source.notes[0].lane.control(),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(7), point(domain, 99), 0),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            },
            ts(1_000_000_000 - offset),
        )
        .unwrap();
    if complete {
        replay.advance_to(ts(3_000_000_000)).unwrap();
    }
    let events = replay.results().to_vec();
    let file = ReplayFile::new(header, replay.records().to_vec());
    let bytes = encode_replay(&file, limits()).unwrap();
    Recording {
        source,
        file,
        bytes,
        events,
    }
}

fn fixture(complete: bool) -> Recording {
    recording(CHART, 20, 7, 10, complete)
}
fn owner(header: ReplayHeader, count: usize, bytes: usize) -> SavedOpponents {
    SavedOpponents::new(header, limits(), count, bytes).unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct View {
    count: usize,
    bytes: usize,
    time: Option<Timestamp>,
    opponents: Vec<(
        OpponentKind,
        String,
        ScoreSummary,
        Option<Timestamp>,
        Option<Timestamp>,
    )>,
}
fn view(owner: &SavedOpponents) -> View {
    View {
        count: owner.count(),
        bytes: owner.encoded_bytes(),
        time: owner.song_time(),
        opponents: owner
            .opponents()
            .iter()
            .map(|ghost| {
                (
                    ghost.kind(),
                    ghost.label().to_owned(),
                    ghost.score().clone(),
                    ghost.song_time(),
                    ghost.recorded_until(),
                )
            })
            .collect(),
    }
}

#[test]
fn own_and_other_replays_show_real_results_at_recorded_operations_instead_of_judge_offsets() {
    let prefix = fixture(false);
    let complete = recording(CHART, 91, 7, 10, true);
    assert_eq!(prefix.events[0].at, ts(1_000_000_000));
    assert_eq!(prefix.file.records[1].song_time, ts(HIT_OPERATION));
    let mut expected = prefix.file.header.clone();
    expected.normalized_clock = ClockDomainId(999);
    let mut saved = owner(expected.clone(), 8, 64 << 20);
    assert_eq!(
        saved
            .add(
                &prefix.source,
                &prefix.bytes,
                OpponentKind::Own,
                "以前の自分"
            )
            .unwrap(),
        0
    );
    assert_eq!(
        saved
            .add(
                &complete.source,
                &complete.bytes,
                OpponentKind::Other,
                " Other "
            )
            .unwrap(),
        1
    );
    assert_eq!(saved.expected_header(), &expected);
    assert_eq!(
        saved.encoded_bytes(),
        prefix.bytes.len() + complete.bytes.len()
    );
    saved.advance_to(ts(HIT_OPERATION - 1)).unwrap();
    assert!(saved
        .opponents()
        .iter()
        .all(|ghost| ghost.score() == &ScoreSummary::default()));
    saved.advance_to(ts(HIT_OPERATION)).unwrap();
    let mut expected_hit = ScoreSummary::default();
    expected_hit.observe(&prefix.events).unwrap();
    assert_eq!(expected_hit.hits, 1);
    for ghost in saved.opponents() {
        assert_eq!(ghost.score(), &expected_hit);
    }
    assert_eq!(saved.opponents()[0].kind(), OpponentKind::Own);
    assert_eq!(saved.opponents()[1].kind(), OpponentKind::Other);
    assert_eq!(saved.opponents()[1].label(), " Other ");
    saved.advance_to(ts(3_000_000_000)).unwrap();
    let mut expected_complete = ScoreSummary::default();
    expected_complete.observe(&complete.events).unwrap();
    assert_eq!((expected_complete.hits, expected_complete.misses), (1, 1));
    assert_eq!(saved.opponents()[0].score(), &expected_hit);
    assert_eq!(saved.opponents()[1].score(), &expected_complete);
}

#[test]
fn empty_and_prefix_recordings_never_fabricate_tail_misses_even_when_loaded_after_progress() {
    let prefix = fixture(false);
    let empty = encode_replay(
        &ReplayFile::new(prefix.file.header.clone(), Vec::new()),
        limits(),
    )
    .unwrap();
    let mut saved = owner(prefix.file.header.clone(), 3, 1 << 20);
    saved
        .add(&prefix.source, &empty, OpponentKind::Own, "empty capture")
        .unwrap();
    saved.advance_to(ts(-100_000_000)).unwrap();
    assert_eq!(saved.opponents()[0].recorded_until(), None);
    saved.advance_to(ts(604_800_000_000_000)).unwrap();
    saved
        .add(
            &prefix.source,
            &prefix.bytes,
            OpponentKind::Other,
            "late prefix",
        )
        .unwrap();
    assert_eq!(saved.opponents()[1].score().hits, 1);
    assert_eq!(
        saved.opponents()[1].recorded_until(),
        Some(ts(HIT_OPERATION))
    );
    saved.advance_to(ts(i64::MAX)).unwrap();
    assert_eq!(saved.opponents()[0].score(), &ScoreSummary::default());
    assert_eq!(
        (
            saved.opponents()[1].score().hits,
            saved.opponents()[1].score().misses
        ),
        (1, 0)
    );
    assert_eq!(saved.opponents()[1].song_time(), Some(ts(i64::MAX)));
    assert_eq!(
        saved.opponents()[1].recorded_until(),
        Some(ts(HIT_OPERATION))
    );
    assert_eq!(saved.encoded_bytes(), empty.len() + prefix.bytes.len());
}

#[test]
fn malformed_or_incompatible_admission_preserves_membership_display_and_charged_bytes() {
    let valid = fixture(false);
    let mut saved = owner(valid.file.header.clone(), 8, 1 << 20);
    saved
        .add(&valid.source, &valid.bytes, OpponentKind::Own, "kept")
        .unwrap();
    saved.advance_to(ts(HIT_OPERATION)).unwrap();
    let before = view(&saved);
    for bytes in [
        &b""[..],
        &b"not a replay"[..],
        &valid.bytes[..valid.bytes.len() - 1],
    ] {
        assert!(matches!(
            saved.add(&valid.source, bytes, OpponentKind::Other, "rejected"),
            Err(SavedOpponentsError::Codec(_))
        ));
        assert_eq!(view(&saved), before);
    }
    let different_profile = recording(CHART, 20, 7, 11, false);
    let different_seed = recording(CHART, 20, 8, 10, false);
    let different_chart = recording(
        &CHART.replace("#00011:00010001", "#00012:00010001"),
        20,
        7,
        10,
        false,
    );
    for candidate in [&different_profile, &different_seed, &different_chart] {
        assert!(matches!(
            saved.add(
                &candidate.source,
                &candidate.bytes,
                OpponentKind::Other,
                "different setup"
            ),
            Err(SavedOpponentsError::Competition(_))
        ));
        assert_eq!(view(&saved), before);
    }
    // Same encoded header still has to reconstruct against the actual supplied source.
    assert!(matches!(
        saved.add(
            &different_chart.source,
            &valid.bytes,
            OpponentKind::Other,
            "wrong source"
        ),
        Err(SavedOpponentsError::Competition(_))
    ));
    assert_eq!(view(&saved), before);
    let mut changed_runtime = valid.file.clone();
    changed_runtime.runtime_version.push_str("-different");
    let bytes = encode_replay(&changed_runtime, limits()).unwrap();
    assert!(matches!(
        saved.add(&valid.source, &bytes, OpponentKind::Other, "wrong runtime"),
        Err(SavedOpponentsError::Competition(_))
    ));
    assert_eq!(view(&saved), before);
    assert_eq!(
        saved
            .add(
                &valid.source,
                &valid.bytes,
                OpponentKind::Other,
                "still usable"
            )
            .unwrap(),
        1
    );
}

#[test]
fn policy_label_and_codec_bounds_reject_without_charging_or_consuming_a_slot() {
    let valid = fixture(false);
    for (count, bytes) in [
        (0, 1),
        (9, 1),
        (1, 0),
        (1, (64 << 20) + 1),
        (usize::MAX, usize::MAX),
    ] {
        assert!(matches!(
            SavedOpponents::new(valid.file.header.clone(), limits(), count, bytes),
            Err(SavedOpponentsError::InvalidLimits)
        ));
    }
    assert!(SavedOpponents::new(valid.file.header.clone(), limits(), 1, 1).is_ok());
    assert!(SavedOpponents::new(valid.file.header.clone(), limits(), 8, 64 << 20).is_ok());
    let mut saved = owner(valid.file.header.clone(), 8, 1 << 20);
    for label in [
        "",
        "line\nfeed",
        "tab\t",
        "nul\0",
        "delete\u{7f}",
        "next\u{85}",
    ] {
        assert!(matches!(
            saved.add(&valid.source, b"malformed", OpponentKind::Own, label),
            Err(SavedOpponentsError::InvalidLabel)
        ));
        assert_eq!(saved.count(), 0);
        assert_eq!(saved.encoded_bytes(), 0);
    }
    let exactly = "é".repeat(128);
    assert_eq!(exactly.len(), 256);
    assert_eq!(
        saved
            .add(&valid.source, &valid.bytes, OpponentKind::Own, &exactly)
            .unwrap(),
        0
    );
    let before = view(&saved);
    assert!(matches!(
        saved.add(
            &valid.source,
            &valid.bytes,
            OpponentKind::Own,
            &(exactly + "x")
        ),
        Err(SavedOpponentsError::InvalidLabel)
    ));
    assert_eq!(view(&saved), before);
    // A tighter caller codec remains authoritative even under a generous owner budget.
    let tight =
        ReplayCodecLimits::new(1 << 20, 1, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let mut bounded = SavedOpponents::new(valid.file.header.clone(), tight, 8, 1 << 20).unwrap();
    assert!(matches!(
        bounded.add(
            &valid.source,
            &valid.bytes,
            OpponentKind::Own,
            "two operations"
        ),
        Err(SavedOpponentsError::Codec(_))
    ));
    assert_eq!(bounded.count(), 0);
    assert_eq!(bounded.encoded_bytes(), 0);
}

#[test]
fn exact_count_and_aggregate_caps_keep_preexisting_recordings_and_only_charge_success() {
    let valid = fixture(false);
    let mut capped = owner(valid.file.header.clone(), 1, 1 << 20);
    capped
        .add(&valid.source, &valid.bytes, OpponentKind::Own, "first")
        .unwrap();
    let before = view(&capped);
    assert!(matches!(
        capped.add(&valid.source, b"malformed", OpponentKind::Other, "second"),
        Err(SavedOpponentsError::Capacity)
    ));
    assert_eq!(view(&capped), before);
    let mut exact = owner(valid.file.header.clone(), 8, valid.bytes.len() * 2);
    exact
        .add(&valid.source, &valid.bytes, OpponentKind::Own, "one")
        .unwrap();
    assert!(matches!(
        exact.add(&valid.source, b"bad", OpponentKind::Other, "bad"),
        Err(SavedOpponentsError::Codec(_))
    ));
    assert_eq!(exact.encoded_bytes(), valid.bytes.len());
    exact
        .add(&valid.source, &valid.bytes, OpponentKind::Other, "two")
        .unwrap();
    let before = view(&exact);
    assert_eq!(exact.encoded_bytes(), valid.bytes.len() * 2);
    assert!(matches!(
        exact.add(&valid.source, &valid.bytes, OpponentKind::Other, "three"),
        Err(SavedOpponentsError::ByteLimit)
    ));
    assert_eq!(view(&exact), before);
    let mut short = owner(valid.file.header.clone(), 8, valid.bytes.len() - 1);
    assert!(matches!(
        short.add(&valid.source, &valid.bytes, OpponentKind::Own, "too large"),
        Err(SavedOpponentsError::ByteLimit)
    ));
    assert_eq!((short.count(), short.encoded_bytes()), (0, 0));
    let mut eight = owner(valid.file.header.clone(), 8, valid.bytes.len() * 9);
    for index in 0..8 {
        assert_eq!(
            eight
                .add(
                    &valid.source,
                    &valid.bytes,
                    OpponentKind::Other,
                    "same metadata is allowed"
                )
                .unwrap(),
            index
        );
    }
    assert!(matches!(
        eight.add(&valid.source, &valid.bytes, OpponentKind::Own, "ninth"),
        Err(SavedOpponentsError::Capacity)
    ));
    assert_eq!(
        (eight.count(), eight.encoded_bytes()),
        (8, valid.bytes.len() * 8)
    );
}

#[test]
fn frontier_regression_is_atomic_and_reset_replays_existing_prefixes_without_recharging() {
    let prefix = fixture(false);
    let complete = fixture(true);
    let mut saved = owner(
        prefix.file.header.clone(),
        2,
        prefix.bytes.len() + complete.bytes.len(),
    );
    saved
        .add(&prefix.source, &prefix.bytes, OpponentKind::Own, "prefix")
        .unwrap();
    saved
        .add(
            &complete.source,
            &complete.bytes,
            OpponentKind::Other,
            "complete",
        )
        .unwrap();
    saved.advance_to(ts(3_000_000_000)).unwrap();
    let completed = view(&saved);
    saved.advance_to(ts(3_000_000_000)).unwrap();
    assert_eq!(view(&saved), completed);
    assert!(matches!(
        saved.advance_to(ts(HIT_OPERATION)),
        Err(SavedOpponentsError::Competition(
            CompetitionError::TimeRegression
        ))
    ));
    assert_eq!(view(&saved), completed);
    saved.reset();
    assert_eq!(saved.song_time(), None);
    assert_eq!(saved.count(), completed.count);
    assert_eq!(saved.encoded_bytes(), completed.bytes);
    for ghost in saved.opponents() {
        assert_eq!(ghost.song_time(), None);
        assert_eq!(ghost.score(), &ScoreSummary::default());
    }
    assert_eq!(
        saved.opponents()[0].recorded_until(),
        Some(ts(HIT_OPERATION))
    );
    assert_eq!(
        saved.opponents()[1].recorded_until(),
        Some(ts(3_000_000_000))
    );
    saved.advance_to(ts(-100_000_000)).unwrap();
    saved.advance_to(ts(HIT_OPERATION)).unwrap();
    assert!(saved
        .opponents()
        .iter()
        .all(|ghost| ghost.score().hits == 1 && ghost.score().misses == 0));
    saved.advance_to(ts(3_000_000_000)).unwrap();
    assert_eq!(view(&saved), completed);
}

fn step_config(domain: u32) -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(domain, 1_000_000_000),
        output_origin: point(domain + 1, 0),
        preroll: Duration::from_nanos(100_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 10,
        command_capacity: 8,
        bgm_pending: 2,
        bgm_lookahead: Duration::from_nanos(500_000_000),
        telemetry_capacity: 8,
    }
}
fn step(config: StepGameplayConfig, seed: u64) -> StepGameplay {
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [4_u32, 8] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2_u16, 16] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4_u32.to_le_bytes());
    for value in [8192_i16, -4096] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", CHART.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    let source = files.scope("pack/chart.bms").unwrap();
    let prepared = prepare_from_source(
        CHART.as_bytes(),
        &source,
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        seed,
        None,
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    StepGameplay::new(prepared, config, bindings).unwrap().0
}
fn clocks(config: StepGameplayConfig) -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: config.host_origin,
            target: config.output_origin,
        },
        ClockInterval {
            start: config.host_origin.timestamp,
            end: config
                .host_origin
                .timestamp
                .checked_add(Duration::from_nanos(10_000_000_000))
                .unwrap(),
        },
    )
    .unwrap()
}

#[test]
fn pristine_step_header_matches_native_capture_identity_without_enabling_capture() {
    let remote = fixture(false);
    for domain in [11, 501] {
        let config = step_config(domain);
        let mut game = step(config, 7);
        let expected = LiveReplayCapture::new_at_with_chart_seed(
            game.judge(),
            config.host_origin.domain,
            limits(),
            Timestamp::ZERO,
            7,
        )
        .unwrap()
        .header()
        .clone();
        let header = game.competition_header(limits(), 7).unwrap();
        assert_eq!(header, expected);
        let identity = game.competition_identity(limits(), 7).unwrap();
        assert_eq!(
            identity,
            competition_identity(&header, env!("CARGO_PKG_VERSION"), limits()).unwrap()
        );
        let decoded_identity = decode_replay(&identity, limits()).unwrap();
        let mut normalized_header = header.clone();
        normalized_header.normalized_clock = ClockDomainId(0);
        assert_eq!(decoded_identity.header, normalized_header);
        assert!(decoded_identity.records.is_empty());
        let mut saved = owner(header, 8, 64 << 20);
        saved
            .add(
                &remote.source,
                &remote.bytes,
                OpponentKind::Own,
                "native prefix",
            )
            .unwrap();
        saved.advance_to(ts(HIT_OPERATION)).unwrap();
        assert_eq!(saved.opponents()[0].score().hits, 1);
        assert_eq!(game.score(), &ScoreSummary::default());
        assert!(game.judge().effective_song_time().is_none());
        game.fail();
        assert!(game.take_replay().unwrap().is_none());
    }
}

#[test]
fn header_queries_preserve_optional_capture_and_setup_failures_leave_live_owner_usable() {
    let config = step_config(11);
    let mut game = step(config, 7);
    let too_small =
        ReplayCodecLimits::new(65536, 8, 1, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    assert!(matches!(
        game.competition_header(too_small, 7),
        Err(StepGameplayError::Capture {
            error: CaptureError::Codec(ReplayCodecError::HeaderTooLarge),
            report: None
        })
    ));
    assert!(!game.failed());
    let expected = game.competition_header(limits(), 7).unwrap();
    game.configure_capture(limits(), 7).unwrap();
    game.activate(config.host_origin).unwrap();
    assert_eq!(game.competition_header(limits(), 7).unwrap(), expected);
    game.advance_to(config.host_origin, &clocks(config), config.output_origin)
        .unwrap();
    assert!(matches!(
        game.competition_header(limits(), 7),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!game.failed());
    assert_eq!(game.score(), &ScoreSummary::default());
    game.fail();
    assert!(matches!(
        game.competition_header(limits(), 7),
        Err(StepGameplayError::Failed)
    ));
    let captured = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(captured.header, expected);
    assert_eq!(captured.records.len(), 1);
    assert_eq!(captured.records[0].song_time, ts(-100_000_000));
    assert!(game.take_replay().unwrap().is_none());
}
