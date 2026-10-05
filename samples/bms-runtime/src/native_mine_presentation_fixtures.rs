//! Deferred public player-channel fixtures. Reports come from portable owners;
//! malformed report/summary injections exercise only the presentation boundary.
use crate::{
    competition::ScoreSummary,
    local_players::PlayerId,
    local_runtime::{InputResult, MemberConfig, PlayerReport, RuntimeGroup},
    mine_damage::MineDamageSummary,
    native_judge::NativeJudgeConfig,
    player::{self, PauseState, PlayerSnapshot, PlayerStatus, PlayerViewer},
};
use beatkernel::{
    audio::{CommandConsumer, command_queue},
    chart::CompiledChart,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, GameInputEvent, PhysicalControlId, PhysicalInputEvent,
    },
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, parse};
use std::sync::Arc;

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
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
fn source() -> (BmsChart, CompiledChart) {
    let source = parse(
        "#BPM 60\n#WAV01 head\n#00011:01\n#000D1:1EZZ\n#000D2:01",
        Default::default(),
    )
    .unwrap();
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
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }])
    .unwrap()
}
fn button(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, song), sequence),
        control: PhysicalControlId::keyboard(91),
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
fn runtime(source: &BmsChart, chart: &CompiledChart) -> (Runtime, CommandConsumer) {
    let (producer, consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(0), ts(0), Rate::NORMAL),
        bindings(DeviceSelector::Any),
        judge(source, chart),
        producer,
        vec![],
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
        panic!("assigned source must produce actual reports")
    };
    reports
}
// Actual public pause acknowledgements force the latest-state handoff without
// sleeping, altering private state or depending on its eight-ms coalescing timer.
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
    assert_eq!(after.last_judge, before.last_judge);
    assert_eq!(after.recent_results, before.recent_results);
    assert_eq!(after.pressed_lanes, before.pressed_lanes);
    assert_eq!(after.players.len(), before.players.len());
    for (old, new) in before.players.iter().zip(&after.players) {
        assert_eq!(new.player, old.player);
        assert_eq!(new.song_time, old.song_time);
        assert_eq!(new.score, old.score);
        assert_eq!(new.mine_damage, old.mine_damage);
        assert_eq!(new.last_judge, old.last_judge);
        assert_eq!(new.recent_results, old.recent_results);
        assert_eq!(new.pressed_lanes, old.pressed_lanes);
        assert_eq!(new.competition, old.competition);
        match (&old.chart, &new.chart) {
            (Some(old), Some(new)) => assert!(Arc::ptr_eq(old, new)),
            (None, None) => {}
            _ => panic!("chart ownership changed on rejected publication"),
        }
        match (&old.note_progress, &new.note_progress) {
            (Some(old), Some(new)) => {
                assert_eq!(new.state(0), old.state(0));
                assert_eq!(new.last_miss(), old.last_miss());
            }
            (None, None) => {}
            _ => panic!("note progress appeared or disappeared on refusal"),
        }
    }
}

#[test]
fn actual_solo_reports_keep_damage_through_empty_pause_cancel_and_final_publication() {
    let (source, chart) = source();
    let (mut runtime, _consumer) = runtime(&source, &chart);
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &chart).unwrap();
        let first = runtime
            .process_input(
                button(u64::MAX, 0, 0, ButtonState::Down),
                &NoMapping,
                point(2, 0),
            )
            .unwrap();
        assert_eq!(first.hazard_events.len(), 2);
        assert_eq!(first.judge_events.len(), 1);
        player::publish_report(&first).unwrap();
        let initial = snapshot(&viewer);
        assert_eq!(initial.mine_damage, summary(1, 1, 50, false));
        assert_eq!(initial.players[0].mine_damage, initial.mine_damage);
        assert_eq!((initial.score.hits, initial.score.misses), (1, 0));
        assert_eq!(initial.recent_results, first.judge_events);
        assert_eq!(initial.pressed_lanes, 1);
        let duplicate = runtime
            .process_input(
                button(u64::MAX, 0, 1, ButtonState::Down),
                &NoMapping,
                point(2, 0),
            )
            .unwrap();
        assert!(duplicate.hazard_events.is_empty());
        player::publish_report(&duplicate).unwrap();
        unchanged(&initial, &snapshot(&viewer));
        for pause in [
            PauseState::Pausing,
            PauseState::Paused,
            PauseState::Resuming,
        ] {
            player::publish_pause(pause);
            let paused = viewer.take_latest().unwrap();
            assert_eq!(paused.mine_damage, initial.mine_damage);
            assert_eq!(paused.players[0].mine_damage, initial.mine_damage);
            assert_eq!(paused.pressed_lanes, 0);
        }
        player::publish_pause(PauseState::Running);
        assert_eq!(viewer.take_latest().unwrap().pressed_lanes, 1);
        let fatal = runtime
            .advance_to(point(1, 2_000_000_000), &NoMapping, point(2, 2_000_000_000))
            .unwrap();
        player::publish_report(&fatal).unwrap();
        let empty = runtime
            .advance_to(point(1, 3_000_000_000), &NoMapping, point(2, 3_000_000_000))
            .unwrap();
        assert!(empty.hazard_events.is_empty());
        player::publish_report(&empty).unwrap();
        let complete_prefix = snapshot(&viewer);
        assert_eq!(complete_prefix.mine_damage, summary(2, 1, 50, true));
        assert_eq!(
            (complete_prefix.score.hits, complete_prefix.score.misses),
            (1, 0)
        );
        assert_eq!(complete_prefix.recent_results, first.judge_events);
        assert_eq!(initial.mine_damage, summary(1, 1, 50, false));
        viewer.cancel();
        player::publish_pause(PauseState::Unavailable);
        let stopping = viewer.take_latest().unwrap();
        assert_eq!(stopping.status, PlayerStatus::Stopping);
        assert_eq!(stopping.mine_damage, complete_prefix.mine_damage);
        assert_eq!(stopping.pressed_lanes, 0);
        Ok(())
    })
    .unwrap();
    let terminal = viewer.take_latest().unwrap();
    assert_eq!(terminal.status, PlayerStatus::Finished);
    assert!(terminal.cancelled);
    assert_eq!(terminal.mine_damage, summary(2, 1, 50, true));
    assert_eq!(terminal.players[0].mine_damage, terminal.mine_damage);
}

#[test]
fn actual_local_report_routes_have_independent_damage_with_only_a_sole_member_legacy_mirror() {
    let (source, chart) = source();
    for players in [vec![PlayerId(77)], vec![PlayerId(77), PlayerId(u32::MAX)]] {
        let (mut group, _consumer) = group(&source, &chart, &players);
        let (publisher, viewer) = player::channel();
        player::with_publisher(publisher, || {
            player::publish_local_chart(&source, &chart, &players).unwrap();
            let reports = processed(
                group
                    .process_input(button(3, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
                    .unwrap(),
            );
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, players[0]);
            player::publish_local_reports(&reports).unwrap();
            let reports = group
                .advance_to(point(1, 2_000_000_000), &NoMapping, point(2, 2_000_000_000))
                .unwrap();
            player::publish_local_reports(&reports).unwrap();
            let current = snapshot(&viewer);
            assert_eq!(current.players[0].mine_damage, summary(2, 1, 50, true));
            assert_eq!(current.players[0].score.hits, 1);
            if players.len() == 1 {
                assert_eq!(current.mine_damage, current.players[0].mine_damage);
                assert_eq!(current.score, current.players[0].score);
            } else {
                assert_eq!(current.players[1].mine_damage, summary(0, 3, 0, false));
                assert_eq!(current.players[1].score.misses, 1);
                assert_eq!(current.mine_damage, MineDamageSummary::default());
                assert_eq!(current.score, ScoreSummary::default());
            }
            player::publish_local_reports(&[]).unwrap();
            unchanged(&current, &snapshot(&viewer));
            Ok(())
        })
        .unwrap();
        let terminal = viewer.take_latest().unwrap();
        assert_eq!(terminal.players[0].mine_damage, summary(2, 1, 50, true));
        assert_eq!(
            terminal.mine_damage,
            if players.len() == 1 {
                summary(2, 1, 50, true)
            } else {
                MineDamageSummary::default()
            }
        );
    }
}

#[test]
fn malformed_later_local_rows_preserve_every_score_damage_pressed_and_lifecycle_field() {
    let (source, chart) = source();
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
        let baseline = snapshot(&viewer);
        assert_eq!(baseline.status, PlayerStatus::Loading);
        for invalid_value in [0, 1296, u64::MAX] {
            let mut invalid = second.clone();
            invalid.report.hazard_events[0].value = invalid_value;
            assert!(player::publish_local_reports(&[first.clone(), invalid]).is_err());
            unchanged(&baseline, &snapshot(&viewer));
        }
        let mut unknown = second.clone();
        unknown.player = PlayerId(99);
        assert!(player::publish_local_reports(&[first.clone(), unknown]).is_err());
        unchanged(&baseline, &snapshot(&viewer));
        assert!(player::publish_local_reports(&[first.clone(), first.clone()]).is_err());
        unchanged(&baseline, &snapshot(&viewer));
        let mut invalid_inputs = second.clone();
        // The presentation API's actual bounded ownership parser must refuse
        // the complete row before any earlier member candidate is committed.
        invalid_inputs.report.bound_inputs = (0..4097)
            .map(|device| GameInputEvent {
                game_control: GameControlId(0x11),
                physical: button(device, 0, 0, ButtonState::Down),
            })
            .collect();
        assert!(player::publish_local_reports(&[first.clone(), invalid_inputs]).is_err());
        unchanged(&baseline, &snapshot(&viewer));
        assert!(player::publish_report(&first.report).is_err());
        unchanged(&baseline, &snapshot(&viewer));
        player::publish_local_reports(&[first.clone(), second.clone()]).unwrap();
        let accepted = snapshot(&viewer);
        assert_eq!(accepted.status, PlayerStatus::Playing);
        for member in &accepted.players {
            assert_eq!(member.mine_damage, summary(1, 1, 50, false));
            assert_eq!(member.score.hits, 1);
            assert_eq!(member.recent_results.len(), 1);
            assert_eq!(member.pressed_lanes, 1);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        viewer.take_latest().unwrap().players[1].mine_damage,
        summary(1, 1, 50, false)
    );
}

#[test]
fn absolute_replay_damage_is_idempotent_monotonic_and_preserved_by_legacy_publication() {
    let (source, chart) = source();
    let (mut runtime, _consumer) = runtime(&source, &chart);
    let first = runtime
        .process_input(button(3, 0, 0, ButtonState::Down), &NoMapping, point(2, 0))
        .unwrap();
    let fatal = runtime
        .advance_to(point(1, 2_000_000_000), &NoMapping, point(2, 2_000_000_000))
        .unwrap();
    let mut actual = MineDamageSummary::default();
    actual.observe(&first.hazard_events).unwrap();
    actual.observe(&fatal.hazard_events).unwrap();
    assert_eq!(actual, summary(2, 1, 50, true));
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher, || {
        player::publish_chart(&source, &chart).unwrap();
        let empty = snapshot(&viewer);
        for impossible in [
            summary(0, 0, 0, true),
            summary(0, 0, 1, false),
            summary(1, 0, 1, true),
            summary(1, 0, 1295, false),
        ] {
            assert!(
                player::publish_replay_prefix_with_mines(
                    ts(2_000_000_000),
                    &first.judge_events,
                    1,
                    impossible
                )
                .is_err()
            );
            unchanged(&empty, &snapshot(&viewer));
        }
        player::publish_replay_prefix_with_mines(ts(2_000_000_000), &first.judge_events, 1, actual)
            .unwrap();
        let original = snapshot(&viewer);
        player::publish_replay_prefix_with_mines(ts(2_000_000_000), &[], 1, actual).unwrap();
        unchanged(&original, &snapshot(&viewer));
        assert_eq!(original.score.hits, 1);
        assert_eq!(original.mine_damage, actual);
        for invalid in [
            summary(1, 1, 0, true),
            summary(2, 0, 50, true),
            summary(2, 1, 49, true),
            summary(2, 1, 50, false),
            summary(0, 1, 0, true),
            summary(1, 1, 1, true),
            summary(1, 1, 1295, false),
            summary(2, 1, 1295, true),
            summary(3, 1, 2589, true),
        ] {
            assert!(
                player::publish_replay_prefix_with_mines(
                    ts(3_000_000_000),
                    &first.judge_events,
                    1 << 17,
                    invalid
                )
                .is_err()
            );
            unchanged(&original, &snapshot(&viewer));
        }
        assert!(
            player::publish_replay_prefix_with_mines(
                ts(3_000_000_000),
                &first.judge_events,
                1 << 18,
                actual
            )
            .is_err()
        );
        unchanged(&original, &snapshot(&viewer));
        player::publish_replay_prefix_with_pressed(ts(3_000_000_000), &[], 1 << 17).unwrap();
        let legacy_pressed = snapshot(&viewer);
        assert_eq!(legacy_pressed.mine_damage, actual);
        assert_eq!(legacy_pressed.pressed_lanes, 1 << 17);
        player::publish_replay_prefix(ts(4_000_000_000), &[]).unwrap();
        let legacy = snapshot(&viewer);
        assert_eq!(legacy.mine_damage, actual);
        assert_eq!(legacy.pressed_lanes, 0);
        assert_eq!(legacy.score.hits, 1);
        // Full-width bounds use integer arithmetic; this tests the public
        // cumulative metadata boundary, not a fabricated recorded play history.
        let maximum = summary(u64::MAX, u64::MAX, u64::MAX, true);
        player::publish_replay_prefix_with_mines(ts(5_000_000_000), &[], 0, maximum).unwrap();
        let saturated = snapshot(&viewer);
        assert_eq!(saturated.mine_damage, maximum);
        assert!(player::publish_report(&first).is_err());
        unchanged(&saturated, &snapshot(&viewer));
        player::publish_replay_prefix(ts(6_000_000_000), &[]).unwrap();
        assert_eq!(snapshot(&viewer).mine_damage, maximum);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        viewer.take_latest().unwrap().mine_damage,
        summary(u64::MAX, u64::MAX, u64::MAX, true)
    );
}
