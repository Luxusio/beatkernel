//! Deferred committed-damage fixtures. Typed preparation exercises portable
//! owners while the playable-source mine admission guard remains in force.
use crate::{
    PreparedBms,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
    mine_damage::{MineDamageError, MineDamageSummary},
    replay_visual::ReplayVisual,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError, StepLocalGameplay},
    step_replay::{StepReplay, StepReplayConfig},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleBank, SampleId, VoiceId},
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent, Position2,
        TouchEvent, TouchPhase,
    },
    judge::{HazardEvent, HazardId, HazardOutcome, JudgeStage},
    replay::codec::{ReplayCodecLimits, ReplayFile, decode_replay},
    runtime::SoundBinding,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsChart, BmsInputMode, parse};

fn event(value: u64, outcome: HazardOutcome) -> HazardEvent {
    HazardEvent {
        id: HazardId(u64::MAX),
        at: Timestamp::from_nanos(i64::MAX),
        control: GameControlId(u32::MAX),
        value,
        outcome,
        input: None,
    }
}
fn summary(triggered: u64, avoided: u64, damage: u64, fatal: bool) -> MineDamageSummary {
    MineDamageSummary {
        triggered,
        avoided,
        half_percent_damage: damage,
        instant_death: fatal,
    }
}

#[test]
fn exact_raw_range_avoids_numeric_fatal_damage_and_requires_no_unique_event_ids() {
    let mut damage = MineDamageSummary::default();
    let avoided: Vec<_> = (1..=1295)
        .map(|raw| event(raw, HazardOutcome::Avoided))
        .collect();
    damage.observe(&avoided).unwrap();
    assert_eq!(damage, summary(0, 1295, 0, false));
    let triggered: Vec<_> = (1..=1294)
        .map(|raw| event(raw, HazardOutcome::Triggered))
        .collect();
    damage.observe(&triggered).unwrap();
    assert_eq!(damage, summary(1294, 1295, 837_865, false));
    damage
        .observe(&[event(1295, HazardOutcome::Triggered)])
        .unwrap();
    assert_eq!(damage, summary(1295, 1295, 837_865, true));
    damage
        .observe(&[event(1295, HazardOutcome::Avoided)])
        .unwrap();
    assert_eq!(damage, summary(1295, 1296, 837_865, true));
    // This accumulator trusts committed delivery; it does not silently dedupe
    // a caller's repeated id, timestamp or complete event.
    damage
        .observe(&[event(50, HazardOutcome::Triggered); 2])
        .unwrap();
    assert_eq!(damage, summary(1297, 1296, 837_965, true));
    let mut wide = summary(0, 0, 9_007_199_254_740_993, false);
    wide.observe(&[event(1294, HazardOutcome::Triggered)])
        .unwrap();
    assert_eq!(wide, summary(1, 0, 9_007_199_254_742_287, false));
}

#[test]
fn invalid_values_and_each_counter_overflow_leave_the_entire_batch_uncommitted() {
    let initial = summary(7, 11, 29, false);
    for value in [0, 1296, u64::MAX] {
        for outcome in [HazardOutcome::Triggered, HazardOutcome::Avoided] {
            let mut damage = initial;
            assert_eq!(
                damage.observe(&[
                    event(1295, HazardOutcome::Triggered),
                    event(50, HazardOutcome::Triggered),
                    event(value, outcome),
                ]),
                Err(MineDamageError::InvalidDamage { value })
            );
            assert_eq!(damage, initial);
        }
    }
    for (initial, batch) in [
        (
            summary(u64::MAX, 3, 5, false),
            vec![
                event(1, HazardOutcome::Avoided),
                event(1295, HazardOutcome::Triggered),
            ],
        ),
        (
            summary(2, u64::MAX, 5, false),
            vec![
                event(1295, HazardOutcome::Triggered),
                event(1, HazardOutcome::Avoided),
            ],
        ),
        (
            summary(2, 3, u64::MAX - 50, false),
            vec![
                event(25, HazardOutcome::Triggered),
                event(26, HazardOutcome::Triggered),
            ],
        ),
        (
            summary(2, 3, u64::MAX - 1, false),
            vec![
                event(1295, HazardOutcome::Triggered),
                event(2, HazardOutcome::Triggered),
            ],
        ),
    ] {
        let mut damage = initial;
        assert_eq!(
            damage.observe(&batch),
            Err(MineDamageError::CounterOverflow)
        );
        assert_eq!(damage, initial);
    }
    let mut exact = summary(u64::MAX - 1, u64::MAX - 1, u64::MAX - 1, false);
    exact
        .observe(&[
            event(1, HazardOutcome::Triggered),
            event(1295, HazardOutcome::Avoided),
        ])
        .unwrap();
    assert_eq!(exact, summary(u64::MAX, u64::MAX, u64::MAX, false));
    exact.observe(&[]).unwrap();
    assert_eq!(exact, summary(u64::MAX, u64::MAX, u64::MAX, false));
}

const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 20_000_000_000;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
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
    BindingMap::from_bindings([Binding {
        device: selector,
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn prepared(source: BmsChart) -> PreparedBms {
    let compiled = source.compile().unwrap();
    assert!(compiled.bgm.is_empty());
    let format = AudioFormat::new(10, 1).unwrap();
    let bounds = PcmLimits::new(64, 128, 4).unwrap();
    let mut bank = SampleBank::new(format, bounds).unwrap();
    if !source.notes.is_empty() {
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, -0.25], bounds).unwrap(),
        )
        .unwrap();
    }
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: 1.0,
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
fn meta(device: u64, song: i64, sequence: u64) -> EventMeta {
    EventMeta::new(DeviceId(device), point(1, HOST + song), sequence)
}
fn button(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(device, song, sequence),
        control: PhysicalControlId::keyboard(91),
        state,
    })
}
fn touch(device: u64, song: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(device, song, sequence),
        control: PhysicalControlId::keyboard(91),
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
fn live_damage_survives_committed_audio_failure_and_empty_reports_without_becoming_ordinary_score()
{
    let source = parse(
        "#BPM 60\n#WAV01 head\n#00011:01010100\n#000D1:1EZZ",
        Default::default(),
    )
    .unwrap();
    let mut cfg = config();
    cfg.command_capacity = 2;
    let (mut game, _bank) =
        StepGameplay::new(prepared(source.clone()), cfg, bindings(DeviceSelector::Any)).unwrap();
    game.configure_capture(limits(), 0).unwrap();
    game.activate(cfg.host_origin).unwrap();
    assert_eq!(game.mine_damage(), &MineDamageSummary::default());
    let first = game
        .process_input(
            button(3, 0, 0, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert_eq!(first.hazard_events.len(), 1);
    assert_eq!(first.hazard_events[0].value, 50);
    assert_eq!(first.audio_commands.len(), 1);
    assert_eq!(game.mine_damage(), &summary(1, 0, 50, false));
    let duplicate = game
        .process_input(
            button(3, 0, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert!(duplicate.hazard_events.is_empty() && duplicate.judge_events.is_empty());
    assert_eq!(game.mine_damage(), &summary(1, 0, 50, false));
    let released = game
        .process_input(
            button(3, 500_000_000, 2, ButtonState::Up),
            &NoMapping,
            point(2, OUTPUT + 500_000_000),
        )
        .unwrap();
    assert!(released.hazard_events.is_empty());
    let second = game
        .process_input(
            button(3, 1_000_000_000, 3, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    assert_eq!(second.audio_commands.len(), 1);
    assert!(second.hazard_events.is_empty());
    game.process_input(
        button(3, 1_500_000_000, 4, ButtonState::Up),
        &NoMapping,
        point(2, OUTPUT + 1_500_000_000),
    )
    .unwrap();
    assert_eq!(game.mine_damage(), &summary(1, 0, 50, false));
    let error = game
        .process_input(
            button(3, 2_000_000_000, 5, ButtonState::Down),
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
        panic!("the third actual ordinary keysound must retain its committed queue-failure report")
    };
    assert!(score_error.is_none() && capture_error.is_none() && report.judge_error.is_none());
    assert_eq!(report.hazard_events.len(), 1);
    assert_eq!(report.hazard_events[0].value, 1295);
    assert_eq!(report.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(report.audio_failures.len(), 1);
    assert!(report.audio_commands.is_empty());
    assert_eq!(game.mine_damage(), &summary(2, 0, 50, true));
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
    assert_eq!(game.mine_damage(), &summary(2, 0, 50, true));
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.records.len(), 6);
    let mut replay = ReplayVisual::new(&source, &file, limits()).unwrap();
    assert_eq!(replay.advance_to(ts(2_000_000_000)).unwrap().len(), 3);
    assert_eq!(replay.mine_damage(), game.mine_damage());

    let (mut fresh, _) =
        StepGameplay::new(prepared(source), config(), bindings(DeviceSelector::Any)).unwrap();
    fresh.activate(config().host_origin).unwrap();
    let invalid = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(3), point(99, HOST), 0),
        control: PhysicalControlId::keyboard(91),
        state: ButtonState::Down,
    });
    assert!(matches!(
        fresh.process_input(invalid, &NoMapping, point(2, OUTPUT)),
        Err(StepGameplayError::Runtime(_))
    ));
    assert_eq!(fresh.mine_damage(), &MineDamageSummary::default());
}

#[test]
fn local_contact_owners_keep_separate_damage_and_ignore_unassigned_sources_and_unknown_members() {
    let source = parse("#BPM 60\n#000D1:1EZZ", Default::default()).unwrap();
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
    assert_eq!(bank.len(), 0);
    game.activate(config().host_origin).unwrap();
    assert_eq!(game.mine_damage(PlayerId(8)), None);
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
        assert_eq!(
            game.mine_damage(player),
            Some(&MineDamageSummary::default())
        );
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
        game.mine_damage(players[0]),
        Some(&summary(1, 0, 50, false))
    );
    assert_eq!(
        game.mine_damage(players[1]),
        Some(&MineDamageSummary::default())
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
    assert_eq!(game.mine_damage(players[1]), Some(&summary(0, 1, 0, false)));
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
    assert_eq!(reports[1].report.hazard_events[0].value, 1295);
    assert_eq!(
        game.mine_damage(players[0]),
        Some(&summary(1, 1, 50, false))
    );
    assert_eq!(game.mine_damage(players[1]), Some(&summary(1, 1, 0, true)));
    for player in players {
        assert_eq!(
            (
                game.score(player).unwrap().hits,
                game.score(player).unwrap().misses
            ),
            (0, 0)
        );
    }
    for report in reports {
        assert!(report.report.audio_commands.is_empty() && report.report.audio_failures.is_empty());
    }
    game.advance_to(
        point(1, HOST + 2_000_000_000),
        &NoMapping,
        point(2, OUTPUT + 2_000_000_000),
    )
    .unwrap();
    game.fail();
    assert_eq!(
        game.mine_damage(players[0]),
        Some(&summary(1, 1, 50, false))
    );
    assert_eq!(game.mine_damage(players[1]), Some(&summary(1, 1, 0, true)));
}

fn recorded_prefix(source: &BmsChart) -> (ReplayFile, MineDamageSummary) {
    let (mut game, _) = StepGameplay::new(
        prepared(source.clone()),
        config(),
        bindings(DeviceSelector::Any),
    )
    .unwrap();
    game.configure_capture(limits(), 0).unwrap();
    game.activate(config().host_origin).unwrap();
    for sequence in [0, 1] {
        game.process_input(
            button(3, 0, sequence, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    }
    game.advance_to(
        point(1, HOST + 1_000_000_000),
        &NoMapping,
        point(2, OUTPUT + 1_000_000_000),
    )
    .unwrap();
    game.process_input(
        button(3, 2_000_000_000, 2, ButtonState::Up),
        &NoMapping,
        point(2, OUTPUT + 2_000_000_000),
    )
    .unwrap();
    game.advance_to(
        point(1, HOST + 3_000_000_000),
        &NoMapping,
        point(2, OUTPUT + 3_000_000_000),
    )
    .unwrap();
    // The final empty report must not replace any previously committed totals.
    let cleared = game
        .advance_to(
            point(1, HOST + 3_000_000_001),
            &NoMapping,
            point(2, OUTPUT + 3_000_000_001),
        )
        .unwrap();
    assert!(cleared.hazard_events.is_empty());
    let total = *game.mine_damage();
    game.fail();
    (
        decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap(),
        total,
    )
}

#[test]
fn replay_consumes_every_record_before_a_shared_display_target_and_never_judges_display_time() {
    let source = parse("#BPM 60\n#000D1:1EZZ5L01\n#001D1:1E", Default::default()).unwrap();
    let (file, live) = recorded_prefix(&source);
    assert_eq!(file.records.len(), 6);
    assert_eq!(live, summary(2, 2, 50, true));
    let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
    assert_eq!(visual.mine_damage(), &MineDamageSummary::default());
    assert!(visual.advance_to(ts(-1)).unwrap().is_empty());
    assert_eq!(visual.mine_damage(), &MineDamageSummary::default());
    assert!(visual.advance_to(ts(3_000_000_001)).unwrap().is_empty());
    assert_eq!(visual.mine_damage(), &live);
    for target in [3_000_000_001, 4_000_000_000, 9_000_000_000] {
        assert!(visual.advance_to(ts(target)).unwrap().is_empty());
        assert_eq!(
            visual.mine_damage(),
            &live,
            "the unrecorded four-second mine must not be consumed"
        );
    }
    assert!(visual.advance_to(ts(8_000_000_000)).is_err());
    assert_eq!(visual.mine_damage(), &live);
    let mut incremental = ReplayVisual::new(&source, &file, limits()).unwrap();
    incremental.advance_to(ts(0)).unwrap();
    assert_eq!(incremental.mine_damage(), &summary(1, 0, 50, false));
    incremental.advance_to(ts(500_000_000)).unwrap();
    assert_eq!(incremental.mine_damage(), &summary(1, 0, 50, false));
    incremental.advance_to(ts(3_000_000_001)).unwrap();
    assert_eq!(incremental.mine_damage(), &live);

    let chosen = StepReplayConfig {
        output_origin: config().output_origin,
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(1_000_000_000),
        max_pending: 8,
    };
    let (mut replay, bank) = StepReplay::new(prepared(source), file, limits(), chosen).unwrap();
    assert_eq!(bank.len(), 0);
    assert!(replay.take_commands(8).unwrap().is_none());
    replay.observe_output(None, None).unwrap();
    assert_eq!(replay.mine_damage(), &MineDamageSummary::default());
    // Caller-supplied presentation points exercise the real stepped API; no
    // RenderReport, native output or successful physical drain is fabricated.
    replay
        .observe_output(None, Some(point(2, OUTPUT + 3_000_000_001)))
        .unwrap();
    assert_eq!(replay.mine_damage(), &live);
    assert!(replay.drain_events().is_empty());
    replay
        .observe_output(None, Some(point(2, OUTPUT + 3_000_000_001)))
        .unwrap();
    replay.observe_output(None, None).unwrap();
    replay
        .observe_output(None, Some(point(2, OUTPUT + 9_000_000_000)))
        .unwrap();
    assert_eq!(replay.mine_damage(), &live);
    assert_eq!((replay.score().hits, replay.score().misses), (0, 0));
    assert!(
        replay
            .observe_output(None, Some(point(2, OUTPUT + 8_000_000_000)))
            .is_err()
    );
    assert!(replay.failed());
    assert_eq!(replay.mine_damage(), &live);
}
