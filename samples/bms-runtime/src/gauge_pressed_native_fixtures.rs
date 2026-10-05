// Actual native publication over portable runtime reports. No device is opened.
use super::*;
use beatkernel::{
    audio::{CommandConsumer, SampleId, VoiceId, command_queue},
    input::*,
    judge::{JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration},
    transport::{Rate, Transport},
};
use crate::local_runtime::{InputResult, MemberConfig, RuntimeGroup};
use beatkernel_bms::{BmsChart, BmsInputMode};

const SECOND: i64 = 1_000_000_000;
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
struct Exact;
impl ClockMapper for Exact {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn source(damage: &str) -> BmsChart {
    beatkernel_bms::parse(
        &format!("#BPM 60\n#WAV01 key\n#00011:01010000\n#00012:01\n#000D1:00{damage}0000\n"),
        Default::default(),
    )
    .unwrap()
}
fn judge(source: &BmsChart) -> beatkernel::judge::JudgeEngine {
    crate::mine_plan::prepare_judge(
        source,
        source.compile().unwrap().chart,
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
        BmsInputMode::ButtonOrContact,
        100,
    )
    .unwrap()
}
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(u32::MAX),
        code: 7,
    }
}
fn bindings(device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([
        Binding {
            device,
            physical: PhysicalControlId::keyboard(91u16),
            game_control: GameControlId(0x11),
        },
        Binding {
            device,
            physical: surface(),
            game_control: GameControlId(0x12),
        },
    ])
    .unwrap()
}
fn button(device: u64, ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, ns), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn touch(
    device: u64,
    ns: i64,
    sequence: u64,
    contact: u64,
    phase: TouchPhase,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(device), point(1, ns), sequence),
        control: surface(),
        contact: ContactId(contact),
        phase,
        position: Position2 { x: 5.0, y: 6.0 },
        pressure: Some(0.5),
    })
}
fn runtime(source: &BmsChart) -> (Runtime, CommandConsumer) {
    let (producer, consumer) = command_queue(1).unwrap();
    let sounds = source
        .notes
        .iter()
        .filter(|note| note.lane.control() == GameControlId(0x11))
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(note.object.0),
            gain: 1.0,
        })
        .collect();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(0), ts(0), Rate::NORMAL),
        bindings(DeviceSelector::Any),
        judge(source),
        producer,
        sounds,
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}
fn latest(viewer: &PlayerViewer) -> PlayerSnapshot {
    publish_pause(PauseState::Paused);
    publish_pause(PauseState::Running);
    viewer.take_latest().unwrap()
}

#[test]
fn native_solo_failed_feedback_clears_buttons_and_contacts_even_on_audio_error() {
    for (damage, failed, mask) in [("ZZ", true, 0), ("1E", false, 3)] {
        let source = source(damage);
        let (mut runtime, mut consumer) = runtime(&source);
        let (publisher, viewer) = channel();
        with_publisher(publisher, || {
            publish_chart(&source, &source.compile().unwrap().chart).unwrap();
            for event in [
                button(u64::MAX, 0, 1, ButtonState::Down),
                touch(u64::MAX - 1, 0, 1, u64::MAX, TouchPhase::Down),
            ] {
                let report = runtime.process_input(event, &Exact, point(2, 0)).unwrap();
                publish_report(&report).unwrap();
            }
            assert_eq!(latest(&viewer).pressed_lanes, 3);
            let fatal = runtime
                .process_input(
                    button(u64::MAX - 2, SECOND, 1, ButtonState::Down),
                    &Exact,
                    point(2, SECOND),
                )
                .unwrap();
            assert_eq!(fatal.judge_events.len(), 1);
            assert_eq!(fatal.hazard_events.len(), 1);
            assert_eq!(fatal.audio_failures.len(), 1); // Original head command still occupies the queue.
            let hash = runtime.judge().stable_hash().unwrap();
            publish_report(&fatal).unwrap();
            assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
            let snapshot = latest(&viewer);
            assert_eq!(snapshot.gauge.snapshot().level_units, 0);
            assert_eq!(snapshot.gauge.snapshot().failure.is_some(), failed);
            assert_eq!(snapshot.pressed_lanes, mask);
            assert_eq!(snapshot.players[0].pressed_lanes, mask);
            assert_eq!(snapshot.score.hits, 3);
            let later = runtime
                .process_input(
                    touch(u64::MAX - 1, 2 * SECOND, 2, u64::MAX - 1, TouchPhase::Down),
                    &Exact,
                    point(2, 2 * SECOND),
                )
                .unwrap();
            assert_eq!(later.bound_inputs.len(), 1); // Legacy/unfenced reports can still carry genuine Down.
            publish_report(&later).unwrap();
            assert_eq!(latest(&viewer).pressed_lanes, mask);
            let current = latest(&viewer);
            publish_replay_prefix_with_gauge(
                ts(2 * SECOND),
                &[],
                3,
                current.mine_damage,
                &current.gauge,
            )
            .unwrap();
            assert_eq!(latest(&viewer).pressed_lanes, mask); // Stale absolute mask cannot revive a failed display.
            assert!(
                publish_replay_prefix_with_gauge(
                    ts(2 * SECOND),
                    &[],
                    1 << 31,
                    current.mine_damage,
                    &current.gauge
                )
                .is_err()
            );
            assert_eq!(latest(&viewer).pressed_lanes, mask);
            assert!(consumer.try_pop().is_ok()); // Presentation never flushes admitted audio.
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn native_local_feedback_is_atomic_and_preserves_exact_healthy_contact_owners() {
    let source = source("ZZ");
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let devices = [u64::MAX, u64::MAX - 1];
    let configs = ids
        .iter()
        .zip(devices)
        .map(|(&player, device)| MemberConfig {
            player,
            device: Some(DeviceId(device)),
            bindings: bindings(DeviceSelector::Exact(DeviceId(device))),
            judge: judge(&source),
            sounds: vec![],
        })
        .collect();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(0), ts(0), Rate::NORMAL),
        producer,
        configs,
        0,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_local_chart(&source, &source.compile().unwrap().chart, &ids).unwrap();
        for device in devices {
            for event in [
                button(device, 0, 1, ButtonState::Down),
                touch(device, 0, 2, u64::MAX, TouchPhase::Down),
            ] {
                let InputResult::Processed(reports) =
                    group.process_input(event, &Exact, point(2, 0)).unwrap()
                else {
                    panic!("admitted source");
                };
                publish_local_reports(&reports).unwrap();
            }
        }
        let InputResult::Processed(reports) = group
            .process_input(
                button(devices[1], SECOND / 2, 3, ButtonState::Up),
                &Exact,
                point(2, SECOND / 2),
            )
            .unwrap()
        else {
            panic!("healthy button release");
        };
        publish_local_reports(&reports).unwrap();
        let before = latest(&viewer);
        assert_eq!(
            before
                .players
                .iter()
                .map(|p| p.pressed_lanes)
                .collect::<Vec<_>>(),
            [3, 2]
        );
        let reports = group
            .advance_to(point(1, SECOND), &Exact, point(2, SECOND))
            .unwrap();
        for variant in 0..3 {
            let mut bad = reports.clone();
            match variant {
                0 => bad[1].report.hazard_events[0].value = 0,
                1 => bad[1].player = PlayerId(23),
                _ => bad[1].player = ids[0],
            }
            assert!(publish_local_reports(&bad).is_err());
            let after = latest(&viewer);
            for (old, new) in before.players.iter().zip(&after.players) {
                assert_eq!(new.pressed_lanes, old.pressed_lanes);
                assert_eq!(new.gauge, old.gauge);
                assert_eq!(new.mine_damage, old.mine_damage);
                assert_eq!(new.score, old.score);
                assert_eq!(new.song_time, old.song_time);
            }
        }
        publish_local_reports(&reports).unwrap();
        let after = latest(&viewer);
        assert_eq!(
            after
                .players
                .iter()
                .map(|p| p.pressed_lanes)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        assert!(after.players[0].gauge.snapshot().failure.is_some());
        assert!(after.players[1].gauge.snapshot().failure.is_none());
        for (device, sequence, contact, phase, expected) in [
            (devices[0], 3, u64::MAX - 1, TouchPhase::Down, [0, 2]),
            (devices[1], 4, u32::MAX as u64, TouchPhase::Cancel, [0, 2]),
            (devices[1], 5, u64::MAX, TouchPhase::Cancel, [0, 0]),
        ] {
            let InputResult::Processed(reports) = group
                .process_input(
                    touch(device, 2 * SECOND, sequence, contact, phase),
                    &Exact,
                    point(2, 2 * SECOND),
                )
                .unwrap()
            else {
                panic!("exact retained owner");
            };
            publish_local_reports(&reports).unwrap();
            assert_eq!(
                latest(&viewer)
                    .players
                    .iter()
                    .map(|p| p.pressed_lanes)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        Ok(())
    })
    .unwrap();
}
