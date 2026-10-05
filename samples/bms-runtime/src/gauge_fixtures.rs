//! Deferred fixed-point gauge policy and actual committed-owner observations.
//! Typed preparation does not bypass the playable mine-file admission guard.
use crate::{
    PreparedBms,
    gauge::{
        BmsGauge, GaugeError, GaugeFailure, GaugeProfile, GaugeSnapshot, GradeDelta,
        GAUGE_UNITS_PER_PERCENT, MAX_GAUGE_GRADES, MAX_GAUGE_UNITS,
    },
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    replay_visual::ReplayVisual,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError, StepLocalGameplay},
    step_replay::{StepReplay, StepReplayConfig},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    chart::ObjectId,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent, Position2,
        TouchEvent, TouchPhase,
    },
    judge::{
        HazardEvent, HazardId, HazardOutcome, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage,
        MissReason,
    },
    replay::codec::{ReplayCodecLimits, decode_replay},
    runtime::SoundBinding,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode, parse};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn snapshot(level_units: u64, failure: Option<GaugeFailure>) -> GaugeSnapshot {
    GaugeSnapshot {
        level_units,
        failure,
    }
}
fn hit(grade: u32, stage: JudgeStage) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(u64::MAX),
        stage,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(grade),
            delta: Duration::from_nanos(i64::MIN),
        },
        at: ts(i64::MAX),
        input: None,
    }
}
fn miss(stage: JudgeStage) -> JudgeEvent {
    JudgeEvent {
        outcome: JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout,
        },
        ..hit(0, stage)
    }
}
fn hazard(value: u64, outcome: HazardOutcome) -> HazardEvent {
    HazardEvent {
        id: HazardId(u64::MAX),
        at: ts(i64::MIN),
        control: GameControlId(u32::MAX),
        value,
        outcome,
        input: None,
    }
}

#[test]
fn profiles_validate_bounds_and_opaque_grades_with_exact_signed_clamping_and_clear_readiness() {
    assert_eq!(GAUGE_UNITS_PER_PERCENT, 1_000_000);
    assert_eq!(MAX_GAUGE_UNITS, 100_000_000);
    assert_eq!(MAX_GAUGE_GRADES, 64);
    let documented =
        GaugeProfile::new(20_000_000, 80_000_000, 1_000_000, -6_000_000, false, vec![]).unwrap();
    assert_eq!(GaugeProfile::default(), documented);
    for (initial, clear) in [(100_000_001, 0), (0, 100_000_001), (u64::MAX, u64::MAX)] {
        assert!(matches!(
            GaugeProfile::new(initial, clear, 0, 0, false, vec![]),
            Err(GaugeError::InvalidConfiguration(_))
        ));
    }
    let overrides: Vec<_> = (0..64)
        .rev()
        .map(|grade| GradeDelta {
            grade: JudgeGrade(grade),
            delta: i64::from(grade),
        })
        .collect();
    let profile = GaugeProfile::new(0, 100_000_000, 77, -1, false, overrides.clone()).unwrap();
    let mut exact = BmsGauge::new(profile.clone());
    assert_eq!(exact.profile(), &profile);
    exact
        .observe(
            &[
                hit(0, JudgeStage::Instant),
                hit(63, JudgeStage::HoldHead),
                hit(u32::MAX, JudgeStage::HoldTail),
            ],
            &[],
        )
        .unwrap();
    assert_eq!(exact.snapshot(), &snapshot(140, None));
    let mut too_many = overrides;
    too_many.push(GradeDelta {
        grade: JudgeGrade(u32::MAX),
        delta: 0,
    });
    assert_eq!(
        GaugeProfile::new(0, 0, 0, 0, false, too_many),
        Err(GaugeError::GradeCapacity)
    );
    assert_eq!(
        GaugeProfile::new(
            0,
            0,
            0,
            0,
            false,
            vec![
                GradeDelta {
                    grade: JudgeGrade(42),
                    delta: i64::MAX
                },
                GradeDelta {
                    grade: JudgeGrade(42),
                    delta: i64::MIN
                },
            ]
        ),
        Err(GaugeError::DuplicateGrade {
            grade: JudgeGrade(42)
        })
    );

    let mut wide = BmsGauge::new(
        GaugeProfile::new(
            50_000_000,
            80_000_000,
            i64::MAX,
            i64::MIN,
            false,
            vec![
                GradeDelta {
                    grade: JudgeGrade(42),
                    delta: -7_000_000,
                },
                GradeDelta {
                    grade: JudgeGrade(1),
                    delta: 3_000_000,
                },
            ],
        )
        .unwrap(),
    );
    for (event, expected) in [
        (hit(u32::MAX, JudgeStage::Instant), 100_000_000),
        (hit(42, JudgeStage::HoldHead), 93_000_000),
        (hit(1, JudgeStage::HoldTail), 96_000_000),
        (miss(JudgeStage::Custom(7)), 0),
        (hit(0, JudgeStage::Custom(8)), 100_000_000),
    ] {
        wide.observe(&[event], &[]).unwrap();
        assert_eq!(wide.snapshot(), &snapshot(expected, None));
    }
    let mut reverse = BmsGauge::new(
        GaugeProfile::new(100_000_000, 1, i64::MIN, i64::MAX, false, vec![]).unwrap(),
    );
    reverse
        .observe(&[hit(1, JudgeStage::Instant)], &[])
        .unwrap();
    assert_eq!(reverse.snapshot(), &snapshot(0, None));
    reverse.observe(&[miss(JudgeStage::Instant)], &[]).unwrap();
    assert_eq!(reverse.snapshot(), &snapshot(100_000_000, None));

    let mut gauge = BmsGauge::default();
    assert_eq!(gauge.snapshot(), &snapshot(20_000_000, None));
    assert!(!gauge.can_clear());
    // Grade values have no ordinal meaning, and both real held stages count.
    gauge
        .observe(
            &[
                hit(u32::MAX, JudgeStage::HoldHead),
                hit(0, JudgeStage::HoldTail),
            ],
            &[],
        )
        .unwrap();
    assert_eq!(gauge.snapshot(), &snapshot(22_000_000, None));
    gauge
        .observe(&vec![hit(7, JudgeStage::Instant); 57], &[])
        .unwrap();
    assert!(!gauge.can_clear());
    gauge.observe(&[hit(7, JudgeStage::Instant)], &[]).unwrap();
    assert_eq!(gauge.snapshot(), &snapshot(80_000_000, None));
    assert!(gauge.can_clear());
    gauge
        .observe(&vec![hit(7, JudgeStage::Instant); 30], &[])
        .unwrap();
    assert_eq!(gauge.snapshot(), &snapshot(100_000_000, None));
    // No song-completion evidence was supplied to this pure level predicate.
    assert!(gauge.can_clear());
}

#[test]
fn mine_units_failure_order_and_entire_batch_validation_are_atomic_even_after_failure() {
    for raw in 1..=1295 {
        let mut gauge = BmsGauge::default();
        gauge
            .observe(&[], &[hazard(raw, HazardOutcome::Avoided)])
            .unwrap();
        assert_eq!(gauge.snapshot(), &snapshot(20_000_000, None));
    }
    for (raw, remaining) in [
        (1, 19_500_000),
        (39, 500_000),
        (40, 0),
        (41, 0),
        (200, 0),
        (201, 0),
        (1294, 0),
    ] {
        let mut gauge = BmsGauge::default();
        gauge
            .observe(&[], &[hazard(raw, HazardOutcome::Triggered)])
            .unwrap();
        assert_eq!(gauge.snapshot(), &snapshot(remaining, None));
    }
    let mut recovery = BmsGauge::default();
    recovery
        .observe(&[], &[hazard(50, HazardOutcome::Triggered)])
        .unwrap();
    assert_eq!(recovery.snapshot(), &snapshot(0, None));
    recovery
        .observe(
            &[hit(9, JudgeStage::Instant)],
            &[hazard(1, HazardOutcome::Triggered)],
        )
        .unwrap();
    assert_eq!(
        recovery.snapshot(),
        &snapshot(500_000, None),
        "normal stages precede mine damage within one report"
    );
    recovery
        .observe(&[], &[hazard(1295, HazardOutcome::Triggered)])
        .unwrap();
    assert_eq!(
        recovery.snapshot(),
        &snapshot(0, Some(GaugeFailure::InstantDeath))
    );
    recovery
        .observe(
            &[hit(9, JudgeStage::Instant); 200],
            &[hazard(1, HazardOutcome::Triggered)],
        )
        .unwrap();
    assert_eq!(
        recovery.snapshot(),
        &snapshot(0, Some(GaugeFailure::InstantDeath))
    );
    assert!(!recovery.can_clear());

    let fail_empty = GaugeProfile::new(1_000_000, 0, 1_000_000, -6_000_000, true, vec![]).unwrap();
    let mut depleted = BmsGauge::new(fail_empty);
    depleted
        .observe(
            &[miss(JudgeStage::Instant)],
            &[hazard(1295, HazardOutcome::Triggered)],
        )
        .unwrap();
    assert_eq!(
        depleted.snapshot(),
        &snapshot(0, Some(GaugeFailure::Depleted))
    );
    assert!(!depleted.can_clear());
    depleted
        .observe(&[hit(1, JudgeStage::Instant)], &[])
        .unwrap();
    assert_eq!(
        depleted.snapshot(),
        &snapshot(0, Some(GaugeFailure::Depleted))
    );
    let initial_failure =
        BmsGauge::new(GaugeProfile::new(0, 0, i64::MAX, 0, true, vec![]).unwrap());
    assert_eq!(
        initial_failure.snapshot(),
        &snapshot(0, Some(GaugeFailure::Depleted))
    );
    assert!(!initial_failure.can_clear());
    let initial_zero = BmsGauge::new(GaugeProfile::new(0, 0, 0, 0, false, vec![]).unwrap());
    assert!(initial_zero.can_clear());
    for (values, failure) in [
        ([2, 1295], GaugeFailure::Depleted),
        ([1295, 2], GaugeFailure::InstantDeath),
    ] {
        let mut ordered =
            BmsGauge::new(GaugeProfile::new(1_000_000, 0, 0, 0, true, vec![]).unwrap());
        ordered
            .observe(
                &[],
                &values.map(|value| hazard(value, HazardOutcome::Triggered)),
            )
            .unwrap();
        assert_eq!(ordered.snapshot(), &snapshot(0, Some(failure)));
    }

    for initial in [BmsGauge::default(), recovery, depleted, initial_failure] {
        for value in [0, 1296, u64::MAX] {
            for outcome in [HazardOutcome::Triggered, HazardOutcome::Avoided] {
                let mut gauge = initial.clone();
                let before = gauge.clone();
                assert_eq!(
                    gauge.observe(
                        &[hit(1, JudgeStage::Instant)],
                        &[
                            hazard(1295, HazardOutcome::Triggered),
                            hazard(value, outcome),
                        ]
                    ),
                    Err(GaugeError::InvalidDamage { value })
                );
                assert_eq!(
                    gauge, before,
                    "all hazards validate before normal or failure state commits"
                );
            }
        }
        let mut empty = initial.clone();
        empty.observe(&[], &[]).unwrap();
        assert_eq!(empty, initial);
    }
}

const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 20_000_000_000;
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, HOST),
        output_origin: point(2, OUTPUT),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 8,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn bindings(selector: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings(
        [(91u16, 0x11), (92u16, 0x12)].map(|(code, control)| Binding {
            device: selector,
            physical: PhysicalControlId::keyboard(code),
            game_control: GameControlId(control),
        }),
    )
    .unwrap()
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    assert!(compiled.bgm.is_empty());
    let format = AudioFormat::new(10, 1).unwrap();
    let bounds = PcmLimits::new(64, 128, 4).unwrap();
    let mut bank = SampleBank::new(format, bounds).unwrap();
    if source.samples.contains_key(&1) {
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, -0.25], bounds).unwrap(),
        )
        .unwrap();
    }
    let sounds = source
        .notes
        .iter()
        .map(|note| {
            let object = compiled
                .chart
                .objects()
                .iter()
                .find(|object| object.id == note.object)
                .unwrap();
            SoundBinding {
                object: note.object,
                stage: if object.time.end.is_some() {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                },
                sample: note.sample,
                voice: VoiceId(note.object.0),
                gain: 1.0,
            }
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    }
}
fn button(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, HOST + song), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn touch(device: u64, song: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(device), point(1, HOST + song), sequence),
        control: PhysicalControlId::keyboard(91u16),
        contact: ContactId(u64::MAX),
        phase,
        position: Position2 { x: 1.0, y: 2.0 },
        pressure: None,
    })
}
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

#[test]
fn actual_solo_reports_preserve_gauge_on_queue_failure_and_numeric_fatal_fences_gameplay() {
    let source = parse(
        "#BPM 60\n#WAV01 head\n#00011:01010100\n#000D1:01010100",
        Default::default(),
    )
    .unwrap();
    let mut cfg = config();
    cfg.command_capacity = 2;
    let (mut game, _) =
        StepGameplay::new(prepared(source.clone()), cfg, bindings(DeviceSelector::Any)).unwrap();
    game.configure_capture(limits(), 0).unwrap();
    game.activate(cfg.host_origin).unwrap();
    for (song, sequence, expected) in [(0, 0, 20_500_000), (1_000_000_000, 3, 21_000_000)] {
        let report = game
            .process_input(
                button(3, song, sequence, ButtonState::Down),
                &NoMapping,
                point(2, OUTPUT + song),
            )
            .unwrap();
        assert_eq!(
            (
                report.judge_events.len(),
                report.hazard_events.len(),
                report.audio_commands.len()
            ),
            (1, 1, 1)
        );
        assert_eq!(game.gauge().snapshot(), &snapshot(expected, None));
        let duplicate = game
            .process_input(
                button(3, song, sequence + 1, ButtonState::Down),
                &NoMapping,
                point(2, OUTPUT + song),
            )
            .unwrap();
        assert!(duplicate.judge_events.is_empty() && duplicate.hazard_events.is_empty());
        game.process_input(
            button(3, song + 500_000_000, sequence + 2, ButtonState::Up),
            &NoMapping,
            point(2, OUTPUT + song + 500_000_000),
        )
        .unwrap();
        assert_eq!(game.gauge().snapshot(), &snapshot(expected, None));
    }
    let error = game
        .process_input(
            button(3, 2_000_000_000, 6, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap_err();
    let StepGameplayError::Report {
        report,
        score_error,
        capture_error,
    } = error
    else {
        panic!("actual third command exceeds the undrained queue")
    };
    assert!(score_error.is_none() && capture_error.is_none() && report.judge_error.is_none());
    assert_eq!(
        (
            report.judge_events.len(),
            report.hazard_events.len(),
            report.audio_failures.len()
        ),
        (1, 1, 1)
    );
    assert_eq!(report.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert!(report.audio_commands.is_empty());
    assert_eq!(game.gauge().snapshot(), &snapshot(21_500_000, None));
    assert_eq!((game.score().hits, game.score().misses), (3, 0));
    assert!(game.failed());
    assert!(matches!(
        game.advance_to(
            point(1, HOST + 3_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 3_000_000_000)
        ),
        Err(StepGameplayError::Failed)
    ));
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    let mut replay = ReplayVisual::new(&source, &file, limits()).unwrap();
    replay.advance_to(ts(2_000_000_000)).unwrap();
    assert_eq!(replay.gauge(), game.gauge());

    let fatal_source = parse(
        "#BPM 60\n#WAV01 head\n#00011:0001\n#000D1:ZZ",
        Default::default(),
    )
    .unwrap();
    let (mut fatal, _) = StepGameplay::new(
        prepared(fatal_source),
        config(),
        bindings(DeviceSelector::Any),
    )
    .unwrap();
    fatal.activate(config().host_origin).unwrap();
    let first = fatal
        .process_input(
            button(3, 0, 0, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert!(first.judge_events.is_empty());
    assert_eq!(first.hazard_events[0].value, 1295);
    assert_eq!(
        fatal.gauge().snapshot(),
        &snapshot(0, Some(GaugeFailure::InstantDeath))
    );
    assert!(!fatal.failed());
    assert_eq!(fatal.gameplay_fence(), Some(ts(0)));
    let fatal_hash = fatal.judge().stable_hash().unwrap();
    fatal
        .process_input(
            button(3, 1_000_000_000, 1, ButtonState::Up),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    let later = fatal
        .process_input(
            button(3, 2_000_000_000, 2, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(
        (later.judge_events.len(), later.audio_commands.len()),
        (0, 0)
    );
    assert!(later.bound_inputs.is_empty() && later.hazard_events.is_empty());
    assert_eq!(later.song_time, ts(0));
    assert_eq!(fatal.judge().stable_hash().unwrap(), fatal_hash);
    assert_eq!(fatal.score().hits, 0);
    assert!(!fatal.failed());
    assert_eq!(
        fatal.gauge().snapshot(),
        &snapshot(0, Some(GaugeFailure::InstantDeath))
    );

    let (mut rejected, _) =
        StepGameplay::new(prepared(source), config(), bindings(DeviceSelector::Any)).unwrap();
    rejected.activate(config().host_origin).unwrap();
    let input = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(3), point(99, HOST), 0),
        control: PhysicalControlId::keyboard(91u16),
        state: ButtonState::Down,
    });
    assert!(matches!(
        rejected.process_input(input, &NoMapping, point(2, OUTPUT)),
        Err(StepGameplayError::Runtime(_))
    ));
    assert_eq!(rejected.gauge(), &BmsGauge::default());
}

#[test]
fn actual_local_contacts_keep_member_gauges_independent_and_fatal_is_not_a_cohort_failure() {
    let source = parse(
        "#BPM 60\n#WAV01 head\n#00011:01\n#000D1:01ZZ",
        Default::default(),
    )
    .unwrap();
    let players = [PlayerId(9), PlayerId(u32::MAX)];
    let plan = ResolvedInputPlan::new(vec![
        (players[0], Some(DeviceId(3))),
        (players[1], Some(DeviceId(u64::MAX))),
    ])
    .unwrap();
    let maps = [3, u64::MAX]
        .map(|device| bindings(DeviceSelector::Exact(DeviceId(device))))
        .into();
    let (mut game, bank) = StepLocalGameplay::new_section(
        prepared(source),
        config(),
        plan,
        maps,
        ts(0),
        None,
        BmsInputMode::ButtonOrContact,
    )
    .unwrap();
    assert_eq!(bank.len(), 1);
    assert_eq!(game.gauge(PlayerId(8)), None);
    game.activate(config().host_origin).unwrap();
    assert!(matches!(
        game.process_input(
            touch(8, 0, 0, TouchPhase::Down),
            &NoMapping,
            point(2, OUTPUT)
        )
        .unwrap(),
        InputResult::Ignored {
            device: DeviceId(8)
        }
    ));
    for player in players {
        assert_eq!(game.gauge(player), Some(&BmsGauge::default()));
    }
    let InputResult::Processed(first) = game
        .process_input(
            touch(3, 0, 1, TouchPhase::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap()
    else {
        panic!("assigned first contact")
    };
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].player, players[0]);
    assert_eq!(
        game.gauge(players[0]).unwrap().snapshot(),
        &snapshot(20_500_000, None)
    );
    assert_eq!(
        game.gauge(players[1]).unwrap().snapshot(),
        &snapshot(20_000_000, None)
    );
    let InputResult::Processed(second) = game
        .process_input(
            touch(u64::MAX, 1_000_000_000, 1, TouchPhase::Down),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap()
    else {
        panic!("assigned second contact")
    };
    assert_eq!(
        second[0].report.hazard_events[0].outcome,
        HazardOutcome::Avoided
    );
    assert_eq!(second[0].report.judge_events.len(), 1);
    assert!(matches!(
        second[0].report.judge_events[0].outcome,
        JudgeOutcome::Miss { .. }
    ));
    assert_eq!(
        game.gauge(players[1]).unwrap().snapshot(),
        &snapshot(14_000_000, None)
    );
    game.process_input(
        touch(3, 2_000_000_000, 2, TouchPhase::Cancel),
        &NoMapping,
        point(2, OUTPUT + 2_000_000_000),
    )
    .unwrap();
    let reports = game
        .advance_to(
            point(1, HOST + 2_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports[0].report.hazard_events.is_empty());
    assert_eq!(
        reports[1].report.hazard_events[0].outcome,
        HazardOutcome::Triggered
    );
    assert_eq!(
        game.gauge(players[0]).unwrap().snapshot(),
        &snapshot(20_500_000, None)
    );
    assert_eq!(
        game.gauge(players[1]).unwrap().snapshot(),
        &snapshot(0, Some(GaugeFailure::InstantDeath))
    );
    assert_eq!(
        (
            game.score(players[0]).unwrap().hits,
            game.score(players[0]).unwrap().misses
        ),
        (1, 0)
    );
    assert_eq!(
        (
            game.score(players[1]).unwrap().hits,
            game.score(players[1]).unwrap().misses
        ),
        (0, 1)
    );
    assert!(!game.failed());
    game.advance_to(
        point(1, HOST + 2_000_000_000),
        &NoMapping,
        point(2, OUTPUT + 2_000_000_000),
    )
    .unwrap();
    game.fail();
    assert_eq!(
        game.gauge(players[0]).unwrap().snapshot(),
        &snapshot(20_500_000, None)
    );
    assert_eq!(
        game.gauge(players[1]).unwrap().snapshot(),
        &snapshot(0, Some(GaugeFailure::InstantDeath))
    );
}

#[test]
fn captured_hold_miss_and_mines_replay_each_committed_operation_once_without_display_time_judging()
{
    let source = parse("#BPM 60\n#LNTYPE 1\n#WAV01 head\n#00051:01000100\n#00012:00010000\n#00011:00000001\n#000D1:00010001\n#001D1:ZZ", Default::default()).unwrap();
    let (mut live, _) = StepGameplay::new(
        prepared(source.clone()),
        config(),
        bindings(DeviceSelector::Any),
    )
    .unwrap();
    live.configure_capture(limits(), 0).unwrap();
    live.activate(config().host_origin).unwrap();
    let head = live
        .process_input(
            button(3, 0, 0, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert_eq!(head.judge_events[0].stage, JudgeStage::HoldHead);
    assert_eq!(live.gauge().snapshot(), &snapshot(21_000_000, None));
    let duplicate = live
        .process_input(
            button(3, 0, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert!(duplicate.judge_events.is_empty());
    let held = live
        .advance_to(
            point(1, HOST + 1_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    assert!(held.judge_events.is_empty());
    assert_eq!(held.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(live.gauge().snapshot(), &snapshot(20_500_000, None));
    let timed_out = live
        .advance_to(
            point(1, HOST + 1_000_000_001),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_001),
        )
        .unwrap();
    assert_eq!(timed_out.judge_events.len(), 1);
    assert!(matches!(
        timed_out.judge_events[0].outcome,
        JudgeOutcome::Miss { .. }
    ));
    assert_eq!(live.gauge().snapshot(), &snapshot(14_500_000, None));
    let tail = live
        .process_input(
            button(3, 2_000_000_000, 2, ButtonState::Up),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(tail.judge_events[0].stage, JudgeStage::HoldTail);
    assert_eq!(live.gauge().snapshot(), &snapshot(15_500_000, None));
    let last = live
        .process_input(
            button(3, 3_000_000_000, 3, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + 3_000_000_000),
        )
        .unwrap();
    assert_eq!((last.judge_events.len(), last.hazard_events.len()), (1, 1));
    let cleared = live
        .advance_to(
            point(1, HOST + 3_000_000_001),
            &NoMapping,
            point(2, OUTPUT + 3_000_000_001),
        )
        .unwrap();
    assert!(cleared.judge_events.is_empty() && cleared.hazard_events.is_empty());
    assert_eq!(live.gauge().snapshot(), &snapshot(16_000_000, None));
    assert_eq!((live.score().hits, live.score().misses), (3, 1));
    let gauge = live.gauge().clone();
    let judge_hash = live.judge().stable_hash().unwrap();
    live.fail();
    let file = decode_replay(&live.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.records.len(), 7);
    assert_eq!(
        crate::replay_playback::reconstruct(&source, file.clone(), limits())
            .unwrap()
            .engine()
            .stable_hash()
            .unwrap(),
        judge_hash
    );
    let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
    assert!(visual.advance_to(ts(-1)).unwrap().is_empty());
    assert_eq!(visual.gauge(), &BmsGauge::default());
    for (target, expected) in [
        (0, 21_000_000),
        (1_000_000_000, 20_500_000),
        (1_000_000_001, 14_500_000),
        (2_000_000_000, 15_500_000),
        (3_000_000_001, 16_000_000),
    ] {
        visual.advance_to(ts(target)).unwrap();
        assert_eq!(visual.gauge().snapshot(), &snapshot(expected, None));
        assert!(visual.advance_to(ts(target)).unwrap().is_empty());
        assert_eq!(visual.gauge().snapshot(), &snapshot(expected, None));
    }
    let mut whole = ReplayVisual::new(&source, &file, limits()).unwrap();
    assert_eq!(whole.advance_to(ts(9_000_000_000)).unwrap().len(), 4);
    assert_eq!(
        whole.gauge(),
        &gauge,
        "the unrecorded four-second fatal mine must not be observed"
    );
    assert!(whole.advance_to(ts(8_000_000_000)).is_err());
    assert_eq!(whole.gauge(), &gauge);
    assert!(whole.advance_to(ts(10_000_000_000)).unwrap().is_empty());
    assert_eq!(whole.gauge(), &gauge);

    let (mut replay, bank) = StepReplay::new(
        prepared(source),
        file,
        limits(),
        StepReplayConfig {
            output_origin: config().output_origin,
            preroll: Duration::ZERO,
            lookahead: Duration::from_nanos(1_000_000_000),
            max_pending: 8,
        },
    )
    .unwrap();
    assert_eq!(bank.len(), 1);
    replay.observe_output(None, None).unwrap();
    assert_eq!(replay.gauge(), &BmsGauge::default());
    // Exercise the presentation API only; no render/drain evidence is fabricated.
    replay
        .observe_output(None, Some(point(2, OUTPUT + 3_000_000_001)))
        .unwrap();
    assert_eq!(replay.gauge(), &gauge);
    assert_eq!(replay.drain_events().len(), 4);
    replay
        .observe_output(None, Some(point(2, OUTPUT + 3_000_000_001)))
        .unwrap();
    replay.observe_output(None, None).unwrap();
    replay
        .observe_output(None, Some(point(2, OUTPUT + 9_000_000_000)))
        .unwrap();
    assert!(replay.drain_events().is_empty());
    assert_eq!(replay.gauge(), &gauge);
    assert_eq!((replay.score().hits, replay.score().misses), (3, 1));
}
