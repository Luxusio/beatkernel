//! Deferred native presentation over actual portable Runtime and replay reports.
//! Malformed rows below test only the publication boundary, not new judge outcomes.
use crate::{
    competition::ScoreSummary,
    gauge::{BmsGauge, GaugeFailure, GaugeProfile, GaugeSnapshot, GradeDelta},
    local_players::PlayerId,
    local_runtime::{InputResult, MemberConfig, PlayerReport, RuntimeGroup},
    mine_damage::MineDamageSummary,
    native_judge::{NativeJudgeConfig, prepare_capture_for_source},
    player::{self, PauseState, PlayerSnapshot, PlayerStatus, PlayerViewer},
    replay_visual::ReplayVisual,
};
use beatkernel::{
    audio::{CommandConsumer, QueuePushError, SampleId, VoiceId, command_queue},
    chart::CompiledChart,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, GameInputEvent, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{HazardOutcome, JudgeGrade, JudgeStage},
    replay::codec::{ReplayCodecLimits, ReplayFile},
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, parse};
use std::sync::Arc;

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn level(units: u64, failure: Option<GaugeFailure>) -> GaugeSnapshot {
    GaugeSnapshot {
        level_units: units,
        failure,
    }
}
fn summary(triggered: u64, avoided: u64, damage: u64, instant_death: bool) -> MineDamageSummary {
    MineDamageSummary {
        triggered,
        avoided,
        half_percent_damage: damage,
        instant_death,
    }
}
fn source(text: &str) -> (BmsChart, CompiledChart) {
    let source = parse(text, Default::default()).unwrap();
    let chart = source.compile().unwrap().chart;
    (source, chart)
}
fn judge(source: &BmsChart, chart: &CompiledChart) -> beatkernel::judge::JudgeEngine {
    NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(2),
        end: None,
    }
    .judge(source, chart.clone())
    .unwrap()
}
fn bindings(device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([Binding {
        device,
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn button(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, song), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
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
fn runtime(
    source: &BmsChart,
    chart: &CompiledChart,
    capacity: usize,
) -> (Runtime, CommandConsumer) {
    let sounds = source
        .notes
        .iter()
        .map(|note| {
            let object = chart
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
                sample: SampleId(1),
                voice: VoiceId(note.object.0),
                gain: 1.0,
            }
        })
        .collect();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(0), ts(0), Rate::NORMAL),
        bindings(DeviceSelector::Any),
        judge(source, chart),
        producer,
        sounds,
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}
fn group(
    source: &BmsChart,
    chart: &CompiledChart,
    players: &[PlayerId],
) -> (RuntimeGroup, CommandConsumer) {
    let members = players
        .iter()
        .enumerate()
        .map(|(index, &player)| {
            let device = DeviceId(3 + index as u64);
            MemberConfig {
                player,
                device: Some(device),
                bindings: bindings(DeviceSelector::Exact(device)),
                judge: judge(source, chart),
                sounds: vec![],
            }
        })
        .collect();
    let (producer, consumer) = command_queue(8).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(0), ts(0), Rate::NORMAL),
        producer,
        members,
        0,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}
fn processed(result: InputResult) -> Vec<PlayerReport> {
    let InputResult::Processed(reports) = result else {
        panic!("assigned actual source")
    };
    reports
}
// Public pause acknowledgements force publication without sleeps/private locks.
fn snapshot(viewer: &PlayerViewer) -> PlayerSnapshot {
    player::publish_pause(PauseState::Paused);
    player::publish_pause(PauseState::Running);
    viewer.take_latest().unwrap()
}
fn unchanged(before: &PlayerSnapshot, after: &PlayerSnapshot) {
    assert_eq!(after.status, before.status);
    assert_eq!(after.cancelled, before.cancelled);
    assert_eq!(after.pause, before.pause);
    assert_eq!(after.completed_end, before.completed_end);
    assert_eq!(after.song_time, before.song_time);
    assert_eq!(after.score, before.score);
    assert_eq!(after.mine_damage, before.mine_damage);
    assert_eq!(after.gauge, before.gauge);
    assert_eq!(after.last_judge, before.last_judge);
    assert_eq!(after.recent_results, before.recent_results);
    assert_eq!(after.pressed_lanes, before.pressed_lanes);
    assert_eq!(after.players.len(), before.players.len());
    match (&before.chart, &after.chart) {
        (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b)),
        (None, None) => {}
        _ => panic!("chart changed"),
    }
    for (a, b) in before.players.iter().zip(&after.players) {
        assert_eq!(a.player, b.player);
        assert_eq!(a.song_time, b.song_time);
        assert_eq!(a.score, b.score);
        assert_eq!(a.mine_damage, b.mine_damage);
        assert_eq!(a.gauge, b.gauge);
        assert_eq!(a.pressed_lanes, b.pressed_lanes);
        assert_eq!(a.last_judge, b.last_judge);
        assert_eq!(a.recent_results, b.recent_results);
        assert_eq!(a.competition, b.competition);
        match (&a.chart, &b.chart) {
            (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b)),
            (None, None) => {}
            _ => panic!("member chart changed"),
        }
        match (&a.note_progress, &b.note_progress) {
            (Some(a), Some(b)) => {
                for index in 0..3 {
                    assert_eq!(a.state(index), b.state(index));
                }
                assert_eq!(a.last_miss(), b.last_miss());
            }
            (None, None) => {}
            _ => panic!("note progress changed"),
        }
    }
}

#[test]
fn actual_hold_and_mine_reports_publish_gauge_once_even_when_later_audio_admission_fails() {
    let (source, chart) = source(
        "#BPM 60\n#LNTYPE 1\n#WAV01 head\n#00051:01000100\n#00011:00000001\n#000D1:00010001",
    );
    let (mut runtime, _consumer) = runtime(&source, &chart, 1);
    let (publisher, viewer) = player::channel();
    let result = player::with_publisher(publisher, || {
        player::publish_chart(&source, &chart).unwrap();
        let mut actual = BmsGauge::default();
        let head = runtime
            .process_input(
                button(u64::MAX, 0, 0, ButtonState::Down),
                &NoMapping,
                point(2, 0),
            )
            .unwrap();
        assert_eq!(head.judge_events[0].stage, JudgeStage::HoldHead);
        assert_eq!(head.audio_commands.len(), 1);
        actual
            .observe(&head.judge_events, &head.hazard_events)
            .unwrap();
        player::publish_report(&head).unwrap();
        let start = snapshot(&viewer);
        assert_eq!(start.gauge.snapshot(), &level(21_000_000, None));
        assert_eq!(start.gauge, actual);
        assert_eq!(start.players[0].gauge, start.gauge);
        assert_eq!(start.score.hits, 1);
        let duplicate = runtime
            .process_input(
                button(u64::MAX, 0, 1, ButtonState::Down),
                &NoMapping,
                point(2, 0),
            )
            .unwrap();
        assert!(duplicate.judge_events.is_empty() && duplicate.hazard_events.is_empty());
        player::publish_report(&duplicate).unwrap();
        unchanged(&start, &snapshot(&viewer));
        let held = runtime
            .advance_to(point(1, 1_000_000_000), &NoMapping, point(2, 1_000_000_000))
            .unwrap();
        assert!(held.judge_events.is_empty());
        assert_eq!(held.hazard_events[0].outcome, HazardOutcome::Triggered);
        actual
            .observe(&held.judge_events, &held.hazard_events)
            .unwrap();
        player::publish_report(&held).unwrap();
        assert_eq!(snapshot(&viewer).gauge.snapshot(), &level(20_500_000, None));
        let tail = runtime
            .process_input(
                button(u64::MAX, 2_000_000_000, 2, ButtonState::Up),
                &NoMapping,
                point(2, 2_000_000_000),
            )
            .unwrap();
        assert_eq!(tail.judge_events[0].stage, JudgeStage::HoldTail);
        assert!(tail.audio_commands.is_empty());
        actual
            .observe(&tail.judge_events, &tail.hazard_events)
            .unwrap();
        player::publish_report(&tail).unwrap();
        assert_eq!(snapshot(&viewer).gauge.snapshot(), &level(21_500_000, None));
        let last = runtime
            .process_input(
                button(u64::MAX, 3_000_000_000, 3, ButtonState::Down),
                &NoMapping,
                point(2, 3_000_000_000),
            )
            .unwrap();
        assert_eq!(
            (
                last.judge_events.len(),
                last.hazard_events.len(),
                last.audio_failures.len()
            ),
            (1, 1, 1)
        );
        assert_eq!(last.audio_failures[0].reason, QueuePushError::Full);
        assert!(last.audio_commands.is_empty());
        actual
            .observe(&last.judge_events, &last.hazard_events)
            .unwrap();
        player::publish_report(&last).unwrap();
        let committed = snapshot(&viewer);
        assert_eq!(committed.gauge.snapshot(), &level(22_000_000, None));
        assert_eq!(committed.gauge, actual);
        assert_eq!(committed.mine_damage, summary(2, 0, 2, false));
        assert_eq!(
            (
                committed.score.hits,
                committed.score.misses,
                committed.recent_results.len()
            ),
            (3, 0, 3)
        );
        for pause in [
            PauseState::Pausing,
            PauseState::Paused,
            PauseState::Resuming,
        ] {
            player::publish_pause(pause);
            let paused = viewer.take_latest().unwrap();
            assert_eq!(paused.gauge, actual);
            assert_eq!(paused.players[0].gauge, actual);
            assert_eq!(paused.pressed_lanes, 0);
        }
        player::publish_pause(PauseState::Running);
        let empty = runtime
            .advance_to(point(1, 3_000_000_001), &NoMapping, point(2, 3_000_000_001))
            .unwrap();
        assert!(empty.judge_events.is_empty() && empty.hazard_events.is_empty());
        player::publish_report(&empty).unwrap();
        assert_eq!(snapshot(&viewer).gauge, actual);
        Err::<(), _>(format!(
            "actual command admission: {:?}",
            last.audio_failures[0].reason
        ))
    });
    assert!(result.is_err());
    let terminal = viewer.take_latest().unwrap();
    assert!(matches!(terminal.status, PlayerStatus::Failed(_)));
    assert_eq!(terminal.gauge.snapshot(), &level(22_000_000, None));
    assert_eq!(terminal.players[0].gauge, terminal.gauge);
    assert_eq!(terminal.pressed_lanes, 0);
    assert_eq!(terminal.score.hits, 3);
}

#[test]
fn local_gauges_stay_independent_and_a_later_invalid_row_cannot_publish_any_candidate() {
    let (source, chart) = source("#BPM 60\n#WAV01 head\n#00011:01\n#000D1:0101");
    for players in [vec![PlayerId(77)], vec![PlayerId(77), PlayerId(u32::MAX)]] {
        let (mut group, _consumer) = group(&source, &chart, &players);
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_local_chart(&source, &chart, &players).unwrap();
            let first = processed(
                group
                    .process_input(button(3, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
                    .unwrap(),
            );
            player::publish_local_reports(&first).unwrap();
            let start = snapshot(&viewer);
            assert_eq!(start.players[0].gauge.snapshot(), &level(20_500_000, None));
            if players.len() == 1 {
                assert_eq!(start.gauge, start.players[0].gauge);
            } else {
                assert_eq!(start.gauge, BmsGauge::default());
                assert_eq!(start.players[1].gauge, BmsGauge::default());
            }
            let reports = group
                .advance_to(point(1, 2_000_000_000), &NoMapping, point(2, 2_000_000_000))
                .unwrap();
            player::publish_local_reports(&reports).unwrap();
            let after = snapshot(&viewer);
            assert_eq!(after.players[0].gauge.snapshot(), &level(20_000_000, None));
            if players.len() == 1 {
                assert_eq!(after.gauge, after.players[0].gauge);
                assert_eq!(after.score, after.players[0].score);
            } else {
                assert_eq!(after.players[1].gauge.snapshot(), &level(14_000_000, None));
                assert_eq!(after.gauge, BmsGauge::default());
                assert_eq!(after.score, ScoreSummary::default());
            }
            player::publish_local_reports(&[]).unwrap();
            unchanged(&after, &snapshot(&viewer));
            Ok(())
        })
        .unwrap();
        let final_state = viewer.take_latest().unwrap();
        assert_eq!(
            final_state.players[0].gauge.snapshot(),
            &level(20_000_000, None)
        );
    }

    let players = [PlayerId(7), PlayerId(42)];
    let (mut group, _consumer) = group(&source, &chart, &players);
    let first = processed(
        group
            .process_input(button(3, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
            .unwrap(),
    )
    .remove(0);
    let second = processed(
        group
            .process_input(button(4, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
            .unwrap(),
    )
    .remove(0);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_local_chart(&source, &chart, &players).unwrap();
        let before = snapshot(&viewer);
        for value in [0, 1296, u64::MAX] {
            let mut bad = second.clone();
            bad.report.hazard_events[0].value = value;
            assert!(player::publish_local_reports(&[first.clone(), bad]).is_err());
            unchanged(&before, &snapshot(&viewer));
        }
        let mut bad = second.clone();
        bad.player = PlayerId(99);
        assert!(player::publish_local_reports(&[first.clone(), bad]).is_err());
        unchanged(&before, &snapshot(&viewer));
        assert!(player::publish_local_reports(&[first.clone(), first.clone()]).is_err());
        unchanged(&before, &snapshot(&viewer));
        let mut bad = second.clone();
        bad.report.bound_inputs = (0..4097)
            .map(|device| GameInputEvent {
                game_control: GameControlId(0x11),
                physical: button(device, 0, 0, ButtonState::Down),
            })
            .collect();
        assert!(player::publish_local_reports(&[first.clone(), bad]).is_err());
        unchanged(&before, &snapshot(&viewer));
        player::publish_local_reports(&[first.clone(), second.clone()]).unwrap();
        let accepted = snapshot(&viewer);
        for member in &accepted.players {
            assert_eq!(member.gauge.snapshot(), &level(20_500_000, None));
            assert_eq!(member.score.hits, 1);
            assert_eq!(member.mine_damage, summary(1, 0, 1, false));
            assert_eq!(member.pressed_lanes, 1);
        }
        assert_eq!(accepted.gauge, BmsGauge::default());
        Ok(())
    })
    .unwrap();
}

fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn captured_recovery(source: &BmsChart, chart: &CompiledChart) -> ReplayFile {
    let (mut runtime, _consumer) = runtime(source, chart, 8);
    let mut capture = prepare_capture_for_source(
        source,
        runtime.judge(),
        ClockDomainId(1),
        ts(0),
        0,
        Some(limits()),
    )
    .unwrap()
    .unwrap();
    let mut observed = BmsGauge::default();
    for report in [
        runtime
            .process_input(button(3, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
            .unwrap(),
        runtime
            .advance_to(point(1, 1_000_000_000), &NoMapping, point(2, 1_000_000_000))
            .unwrap(),
        runtime
            .process_input(
                button(3, 2_000_000_000, 1, ButtonState::Up),
                &NoMapping,
                point(2, 2_000_000_000),
            )
            .unwrap(),
        runtime
            .process_input(
                button(3, 3_000_000_000, 2, ButtonState::Down),
                &NoMapping,
                point(2, 3_000_000_000),
            )
            .unwrap(),
        runtime
            .advance_to(point(1, 3_000_000_001), &NoMapping, point(2, 3_000_000_001))
            .unwrap(),
    ] {
        capture.record_report(&report).unwrap();
        observed
            .observe(&report.judge_events, &report.hazard_events)
            .unwrap();
    }
    assert_eq!(observed.snapshot(), &level(2_000_000, None));
    capture.into_file()
}

#[test]
fn actual_replay_visual_gauge_is_copied_after_recovery_without_reordering_or_double_application() {
    let (source, chart) = source(
        "#BPM 60\n#LNTYPE 1\n#WAV01 head\n#00051:01000100\n#00011:00000001\n#000D1:001E0000\n#001D1:ZZ",
    );
    let file = captured_recovery(&source, &chart);
    assert_eq!(file.records.len(), 5);
    // Publishing the whole recorded prefix at once must retain recovery AFTER
    // the mine, rather than subtracting total damage after all normal hits.
    let mut visual = ReplayVisual::new(&source, &file, limits()).unwrap();
    let events = visual.advance_to(ts(3_000_000_001)).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(visual.gauge().snapshot(), &level(2_000_000, None));
    assert_eq!(*visual.mine_damage(), summary(1, 0, 50, false));
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &chart).unwrap();
        player::publish_replay_prefix_with_gauge(
            ts(3_000_000_001),
            &events,
            visual.pressed_lanes(),
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        let full = snapshot(&viewer);
        assert_eq!(&full.gauge, visual.gauge());
        assert_eq!(full.players[0].gauge, full.gauge);
        assert_eq!((full.score.hits, full.score.misses), (3, 0));
        assert_eq!(full.recent_results, events);
        for _ in 0..2 {
            player::publish_replay_prefix_with_gauge(
                ts(3_000_000_001),
                &[],
                visual.pressed_lanes(),
                *visual.mine_damage(),
                visual.gauge(),
            )
            .unwrap();
            unchanged(&full, &snapshot(&viewer));
        }
        player::publish_pause(PauseState::Paused);
        let paused = viewer.take_latest().unwrap();
        assert_eq!(paused.gauge, full.gauge);
        assert_eq!(paused.pressed_lanes, 0);
        player::publish_replay_prefix_with_gauge(
            ts(3_000_000_001),
            &[],
            visual.pressed_lanes(),
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        assert_eq!(snapshot(&viewer).gauge, full.gauge);
        assert!(visual.advance_to(ts(9_000_000_000)).unwrap().is_empty());
        player::publish_replay_prefix_with_gauge(
            ts(9_000_000_000),
            &[],
            visual.pressed_lanes(),
            *visual.mine_damage(),
            visual.gauge(),
        )
        .unwrap();
        let beyond = snapshot(&viewer);
        assert_eq!(beyond.gauge, full.gauge);
        assert_eq!(beyond.score, full.score);
        assert_eq!(
            beyond.mine_damage, full.mine_damage,
            "unrecorded fatal marker stays unconsumed"
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(
        viewer.take_latest().unwrap().gauge.snapshot(),
        &level(2_000_000, None)
    );

    let mut incremental = ReplayVisual::new(&source, &file, limits()).unwrap();
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &chart).unwrap();
        for (at, units) in [
            (0, 21_000_000),
            (1_000_000_000, 0),
            (2_000_000_000, 1_000_000),
            (3_000_000_001, 2_000_000),
        ] {
            let events = incremental.advance_to(ts(at)).unwrap();
            player::publish_replay_prefix_with_gauge(
                ts(at),
                &events,
                incremental.pressed_lanes(),
                *incremental.mine_damage(),
                incremental.gauge(),
            )
            .unwrap();
            assert_eq!(snapshot(&viewer).gauge.snapshot(), &level(units, None));
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn replay_rejects_nondefault_policy_death_mismatch_and_failure_revival_before_any_publication() {
    let (source, chart) = source("#BPM 60\n#WAV01 head\n#00011:01\n#000D1:ZZ");
    let (mut runtime, _consumer) = runtime(&source, &chart, 8);
    let report = runtime
        .process_input(button(3, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
        .unwrap();
    let mut fatal = BmsGauge::default();
    fatal
        .observe(&report.judge_events, &report.hazard_events)
        .unwrap();
    let mut damage = MineDamageSummary::default();
    damage.observe(&report.hazard_events).unwrap();
    assert_eq!(
        fatal.snapshot(),
        &level(0, Some(GaugeFailure::InstantDeath))
    );
    assert_eq!(damage, summary(1, 0, 0, true));
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &chart).unwrap();
        let before = snapshot(&viewer);
        let custom = [
            GaugeProfile::new(20_000_000, 80_000_000, 2_000_000, -6_000_000, false, vec![])
                .unwrap(),
            GaugeProfile::new(30_000_000, 80_000_000, 1_000_000, -6_000_000, false, vec![])
                .unwrap(),
            GaugeProfile::new(0, 80_000_000, 1_000_000, -6_000_000, true, vec![]).unwrap(),
            GaugeProfile::new(
                20_000_000,
                80_000_000,
                1_000_000,
                -6_000_000,
                false,
                vec![GradeDelta {
                    grade: JudgeGrade(1),
                    delta: 1_000_000,
                }],
            )
            .unwrap(),
        ];
        for profile in custom {
            assert!(
                player::publish_replay_prefix_with_gauge(
                    ts(0),
                    &report.judge_events,
                    1,
                    MineDamageSummary::default(),
                    &BmsGauge::new(profile)
                )
                .is_err()
            );
            unchanged(&before, &snapshot(&viewer));
        }
        for (incoming, mines) in [
            (&fatal, MineDamageSummary::default()),
            (&BmsGauge::default(), damage),
            (&fatal, summary(0, 0, 0, true)),
            (&fatal, summary(1, 0, 1, true)),
        ] {
            assert!(
                player::publish_replay_prefix_with_gauge(
                    ts(0),
                    &report.judge_events,
                    1,
                    mines,
                    incoming
                )
                .is_err()
            );
            unchanged(&before, &snapshot(&viewer));
        }
        assert!(
            player::publish_replay_prefix_with_gauge(
                ts(0),
                &report.judge_events,
                1 << 18,
                damage,
                &fatal
            )
            .is_err()
        );
        unchanged(&before, &snapshot(&viewer));
        player::publish_replay_prefix_with_gauge(ts(0), &report.judge_events, 1, damage, &fatal)
            .unwrap();
        let committed = snapshot(&viewer);
        assert_eq!(committed.gauge, fatal);
        assert_eq!(committed.score.hits, 1);
        for mines in [MineDamageSummary::default(), damage] {
            assert!(
                player::publish_replay_prefix_with_gauge(
                    ts(1),
                    &report.judge_events,
                    0,
                    mines,
                    &BmsGauge::default()
                )
                .is_err()
            );
            unchanged(&committed, &snapshot(&viewer));
        }
        player::publish_replay_prefix_with_gauge(ts(0), &[], 1, damage, &fatal).unwrap();
        unchanged(&committed, &snapshot(&viewer));
        player::publish_replay_prefix_with_pressed(ts(1), &[], 1).unwrap();
        let legacy = snapshot(&viewer);
        assert_eq!(legacy.gauge, fatal);
        assert_eq!(legacy.mine_damage, damage);
        assert_eq!(legacy.score.hits, 1);
        viewer.cancel();
        player::publish_pause(PauseState::Unavailable);
        let cancelled = viewer.take_latest().unwrap();
        assert_eq!(cancelled.status, PlayerStatus::Stopping);
        assert_eq!(cancelled.gauge, fatal);
        Ok(())
    })
    .unwrap();
    let terminal = viewer.take_latest().unwrap();
    assert!(terminal.cancelled);
    assert_eq!(terminal.gauge, fatal);
    assert_eq!(terminal.players[0].gauge, fatal);
    assert_eq!(terminal.pressed_lanes, 0);
}
