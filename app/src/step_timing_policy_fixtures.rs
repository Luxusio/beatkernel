//! Actual portable solo/local owners retaining immutable staged policy identity.
use super::*;
use crate::play_policy::{ResolvedPlayPolicy, TimingPresetSelection};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    input::{
        Binding, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId,
    },
    judge::{JudgeGrade, JudgeOutcome, JudgeStage},
    replay::codec::decode_replay,
    time::ClockMappingQuality,
};
use beatkernel_bms::{BmsGaugeKind, BmsRankPrecedence, BmsTimingPreset};
const CHART: &str = "#BPM 60\n#WAV01 note.wav\n#RANK 3\n#TOTAL 320\n#LNOBJ 02\n#00011:0102\n#00016:0102";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn prepared() -> PreparedBms {
    let source = beatkernel_bms::parse(CHART, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let bank = SampleBank::new(
        AudioFormat::new(1000, 1).unwrap(),
        PcmLimits::new(64, 256, 1).unwrap(),
    )
    .unwrap();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds: vec![],
        bgm_commands: vec![],
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, 0),
        output_origin: point(2, 0),
        preroll: Duration::ZERO,
        // Explicit policies must replace these legacy timing values.
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
    }
}
fn selected(prepared: &PreparedBms, kind: BmsGaugeKind) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms_with_timing(
        &prepared.source,
        kind,
        TimingPresetSelection {
            preset: BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
            precedence: BmsRankPrecedence::RankFirst,
        },
        0,
    )
    .unwrap()
}
fn bindings(source: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings(
        [0x11, 0x16]
            .into_iter()
            .enumerate()
            .map(|(index, lane)| Binding {
                device: source,
                physical: PhysicalControlId::keyboard(4 + index as u16),
                game_control: GameControlId(lane),
            }),
    )
    .unwrap()
}
fn input(device: u64, key: u16, at: i64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, at), 0),
        control: PhysicalControlId::keyboard(key),
        state,
    })
}
struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn solo(kind: BmsGaugeKind) -> (StepGameplay, SampleBank, beatkernel_bms::BmsChart) {
    let data = prepared();
    let source = data.source.clone();
    let policy = selected(&data, kind);
    let (game, bank) = StepGameplay::new_section_with_policy(
        data,
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
        policy,
    )
    .unwrap();
    (game, bank, source)
}

#[test]
fn actual_solo_stages_classes_capture_replay_and_seek_keep_selected_windows() {
    let (mut game, _, source) = solo(BmsGaugeKind::Groove);
    let header = game.competition_header(limits(), 0).unwrap();
    game.configure_capture(limits(), 0).unwrap();
    assert_eq!(game.capture.as_ref().unwrap().header(), &header);
    let mut results = Vec::new();
    for (key, at, state, stage, grade) in [
        (4, 25_000_000, ButtonState::Down, JudgeStage::HoldHead, 2),
        (5, 25_000_000, ButtonState::Down, JudgeStage::HoldHead, 1),
        (4, 2_125_000_000, ButtonState::Up, JudgeStage::HoldTail, 2),
        (5, 2_125_000_000, ButtonState::Up, JudgeStage::HoldTail, 1),
    ] {
        let report = game
            .process_input(input(7, key, at, state), &SameDomain, point(2, at))
            .unwrap();
        assert_eq!(report.judge_events.len(), 1);
        assert_eq!(report.judge_events[0].stage, stage);
        assert!(
            matches!(report.judge_events[0].outcome, JudgeOutcome::Hit { grade: actual, .. } if actual == JudgeGrade(grade))
        );
        results.extend(report.judge_events);
    }
    let class_score = game
        .play_policy()
        .unwrap()
        .judgments()
        .unwrap()
        .project(game.score())
        .unwrap();
    assert_eq!(
        (class_score.pgreat, class_score.great, class_score.ex_score),
        (2, 2, 6)
    );
    let hash = game.judge().stable_hash().unwrap();
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.header, header);
    let mut replay = crate::replay_playback::reconstruct_section(&source, file, limits()).unwrap();
    let cursor = replay.records().len();
    replay.seek_cursor(cursor).unwrap();
    assert_eq!(replay.results(), results);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    replay.seek_cursor(2).unwrap();
    replay.checkpoint().unwrap();
    replay.seek_cursor(cursor).unwrap();
    assert_eq!(replay.results(), results);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
}

#[test]
fn all_six_gauges_start_from_selected_profile_and_survive_actual_perfect_stages() {
    for kind in BmsGaugeKind::ALL {
        let (mut game, _, _) = solo(kind);
        assert_eq!(game.gauge().profile(), game.play_policy().unwrap().gauge());
        assert_eq!(
            game.gauge().snapshot(),
            BmsGauge::new(game.play_policy().unwrap().gauge().try_copy().unwrap()).snapshot()
        );
        for (key, at, state) in [
            (4, 0, ButtonState::Down),
            (5, 0, ButtonState::Down),
            (4, 2_000_000_000, ButtonState::Up),
            (5, 2_000_000_000, ButtonState::Up),
        ] {
            assert_eq!(
                game.process_input(input(7, key, at, state), &SameDomain, point(2, at))
                    .unwrap()
                    .judge_events
                    .len(),
                1
            );
        }
        assert_eq!(game.score().hits, 4);
        assert_eq!(game.score().misses, 0);
        assert!(game.gauge().snapshot().failure.is_none());
    }
}

#[test]
fn local_members_keep_independent_actual_stages_and_identical_recorded_policy() {
    let data = prepared();
    let source = data.source.clone();
    let policy = selected(&data, BmsGaugeKind::Groove);
    let plan = ResolvedInputPlan::new(vec![
        (PlayerId(7), Some(DeviceId(71))),
        (PlayerId(u32::MAX), Some(DeviceId(91))),
    ])
    .unwrap();
    let (mut game, _) = StepLocalGameplay::new_section_with_policy(
        data,
        config(),
        plan,
        [71, 91]
            .into_iter()
            .map(|device| bindings(DeviceSelector::Exact(DeviceId(device))))
            .collect(),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
        policy,
    )
    .unwrap();
    let mut headers = Vec::new();
    for player in [PlayerId(7), PlayerId(u32::MAX)] {
        headers.push(game.competition_header(player, limits(), 0).unwrap());
        assert_eq!(
            game.gauge(player).unwrap().profile(),
            game.play_policy().unwrap().gauge()
        );
        game.configure_capture(player, limits(), 0).unwrap();
    }
    assert_eq!(headers[0], headers[1]);
    for (key, at, state) in [
        (4, 25_000_000, ButtonState::Down),
        (5, 25_000_000, ButtonState::Down),
        (4, 2_125_000_000, ButtonState::Up),
        (5, 2_125_000_000, ButtonState::Up),
    ] {
        for device in [71, 91] {
            game.process_input(input(device, key, at, state), &SameDomain, point(2, at))
                .unwrap();
        }
    }
    let hashes = [PlayerId(7), PlayerId(u32::MAX)].map(|player| {
        let score = game
            .play_policy()
            .unwrap()
            .judgments()
            .unwrap()
            .project(game.score(player).unwrap())
            .unwrap();
        assert_eq!((score.pgreat, score.great), (2, 2));
        game.judge(player).unwrap().stable_hash().unwrap()
    });
    game.fail();
    for (index, player) in [PlayerId(7), PlayerId(u32::MAX)].into_iter().enumerate() {
        let file = decode_replay(&game.take_replay(player).unwrap().unwrap(), limits()).unwrap();
        assert_eq!(file.header, headers[index]);
        let mut replay =
            crate::replay_playback::reconstruct_section(&source, file, limits()).unwrap();
        replay.seek_cursor(replay.records().len()).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), hashes[index]);
        assert_eq!(replay.results().len(), 4);
    }
}

#[test]
fn selected_completion_uses_actual_late_extent_and_configuration_is_immutable() {
    let (mut game, _, _) = solo(BmsGaugeKind::Hard);
    assert_eq!(
        game.completion.as_ref().unwrap().judge_until(),
        ts(2_290_000_001)
    );
    let header = game.competition_header(limits(), 0).unwrap();
    let same = game.gauge().profile().try_copy().unwrap();
    game.configure_gauge(same).unwrap();
    assert!(game
        .configure_gauge(crate::gauge::GaugeProfile::default())
        .is_err());
    assert_eq!(game.competition_header(limits(), 0).unwrap(), header);
    let data = prepared();
    let policy = selected(&data, BmsGaugeKind::Hard);
    let mut wrong = config();
    wrong.offset_ns = 1;
    assert!(StepGameplay::new_section_with_policy(
        data,
        wrong,
        bindings(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
        policy
    )
    .is_err());
    let mut data = prepared();
    let policy = selected(&data, BmsGaugeKind::Hard);
    data.source.metadata.insert("RANK".into(), "2".into());
    assert!(StepGameplay::new_section_with_policy(
        data,
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
        policy
    )
    .is_err());
}

#[test]
fn absent_policy_and_explicit_builtin_policy_keep_legacy_headers_identical() {
    let (legacy, _) = StepGameplay::new_section(
        prepared(),
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        None,
    )
    .unwrap();
    let policy = ResolvedPlayPolicy::builtin(0, 0, 0).unwrap();
    let (mut explicit, _) = StepGameplay::new_section_with_policy(
        prepared(),
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
        policy,
    )
    .unwrap();
    assert!(legacy.play_policy().is_none());
    let header = legacy.competition_header(limits(), 0).unwrap();
    assert_eq!(explicit.competition_header(limits(), 0).unwrap(), header);
    explicit.configure_capture(limits(), 0).unwrap();
    assert_eq!(explicit.capture.as_ref().unwrap().header(), &header);
}

#[test]
fn policy_audio_authority_constructor_uses_normalized_live_timing_and_capture() {
    use crate::audio_authority::{AudioAuthorityConfig, AudioAuthorityEpoch};
    let data = prepared();
    let policy = selected(&data, BmsGaugeKind::Groove);
    let authority = AudioAuthority::new(
        AudioAuthorityConfig::default(),
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: point(2, 0),
            logical_origin: point(3, 0),
            host_domain: ClockDomainId(1),
        },
    )
    .unwrap();
    let (mut game, _) = StepGameplay::new_audio_section_with_policy(
        data,
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
        authority,
        policy,
    )
    .unwrap();
    let header = game.competition_header(limits(), 0).unwrap();
    assert_eq!(header.normalized_clock, ClockDomainId(3));
    game.configure_capture(limits(), 0).unwrap();
    game.activate_audio().unwrap();
    game.observe_audio_output(
        1,
        ClockPair {
            source: point(2, 0),
            target: point(1, 0),
        },
    )
    .unwrap();
    game.observe_audio_output(
        1,
        ClockPair {
            source: point(2, 50_000_000),
            target: point(1, 25_000_000),
        },
    )
    .unwrap();
    let mut merger = InputMerger::new(ClockDomainId(1), point(1, 0), vec![DeviceId(7)], 8).unwrap();
    merger
        .admit(
            input(7, 4, 12_500_000, ButtonState::Down),
            point(1, 25_000_000),
        )
        .unwrap();
    game.record_audio_prefix(point(1, 25_000_000)).unwrap();
    let report = game
        .process_next_audio_input(
            &mut merger,
            point(1, 25_000_000),
            point(2, 60_000_000),
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, ts(25_000_000));
    assert!(
        matches!(report.judge_events[0].outcome, JudgeOutcome::Hit { grade: JudgeGrade(2), delta } if delta.as_nanos() == 25_000_000)
    );
    assert_eq!(game.capture.as_ref().unwrap().header(), &header);
    assert_eq!(game.capture.as_ref().unwrap().records().len(), 1);
}

#[test]
fn actual_contact_policy_keeps_owner_cancellation_and_recorded_input_mode() {
    use beatkernel::input::{ContactId, TouchEvent, TouchPhase};
    let data = prepared();
    let policy = selected(&data, BmsGaugeKind::Groove);
    let (mut game, _) = StepGameplay::new_section_with_policy(
        data,
        config(),
        bindings(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOrContact,
        policy,
    )
    .unwrap();
    let header = game.competition_header(limits(), 0).unwrap();
    assert_eq!(
        crate::replay_playback::decode_section_setup(&header.options)
            .unwrap()
            .input_mode,
        BmsInputMode::ButtonOrContact
    );
    let touch = |device, contact, at, phase| {
        PhysicalInputEvent::Touch(TouchEvent {
            meta: EventMeta::new(DeviceId(device), point(1, at), 0),
            control: PhysicalControlId::keyboard(4),
            contact: ContactId(contact),
            phase,
            position: Position2 { x: 0.0, y: 0.0 },
            pressure: None,
        })
    };
    assert_eq!(
        game.process_input(
            touch(7, 1, 25_000_000, TouchPhase::Down),
            &SameDomain,
            point(2, 25_000_000)
        )
        .unwrap()
        .judge_events
        .len(),
        1
    );
    assert!(game
        .process_input(
            touch(7, 2, 2_000_000_000, TouchPhase::Cancel),
            &SameDomain,
            point(2, 2_000_000_000)
        )
        .unwrap()
        .judge_events
        .iter().all(|event| event.stage != JudgeStage::HoldTail));
    let report = game
        .process_input(
            touch(7, 1, 2_000_000_000, TouchPhase::Cancel),
            &SameDomain,
            point(2, 2_000_000_000),
        )
        .unwrap();
    assert_eq!(
        report.judge_events[0].outcome,
        JudgeOutcome::Miss {
            reason: beatkernel::judge::MissReason::RejectedInput
        }
    );
}
