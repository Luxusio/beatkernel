// Actual legacy record continuation: display failure must not rewrite the judge.
use super::*;
use beatkernel::{
    audio::command_queue,
    input::*,
    interaction::InputOwner,
    judge::{JudgeGrade, JudgeProfile, JudgeWindow},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration},
    transport::{Rate, Transport},
};
use beatkernel_bms::BmsInputMode;

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
fn button(ns: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(1, ns), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
    })
}
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(u32::MAX),
        code: 3,
    }
}
fn touch(ns: i64, sequence: u64, id: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(u64::MAX - 1), point(1, ns), sequence),
        control: surface(),
        contact: ContactId(id),
        phase: TouchPhase::Down,
        position: Position2 { x: 4.0, y: 5.0 },
        pressure: None,
    })
}

#[test]
fn replay_failure_hides_all_owners_without_dropping_later_legacy_judge_operations() {
    const SECOND: i64 = 1_000_000_000;
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 key\n#00011:01000100\n#00012:01\n#000D1:00ZZ0000\n",
        Default::default(),
    )
    .unwrap();
    let judge = crate::mine_plan::prepare_judge(
        &source,
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
        16,
    )
    .unwrap();
    let limits =
        ReplayCodecLimits::new(65536, 32, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let mut capture = crate::replay_capture::LiveReplayCapture::new_with_input_sounds(
        &judge,
        ClockDomainId(1),
        limits,
        ts(0),
        0,
        None,
        BmsInputMode::ButtonOrContact,
        crate::input_sounds::InputSoundIdentity::from_source(&source).unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([
        Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(91u16),
            game_control: GameControlId(0x11),
        },
        Binding {
            device: DeviceSelector::Any,
            physical: surface(),
            game_control: GameControlId(0x12),
        },
    ])
    .unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(0), ts(0), Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    for event in [button(0, 1, ButtonState::Down), touch(0, 1, u64::MAX)] {
        capture
            .record_report(&runtime.process_input(event, &Exact, point(2, 0)).unwrap())
            .unwrap();
    }
    capture
        .record_report(
            &runtime
                .advance_to(point(1, SECOND), &Exact, point(2, SECOND))
                .unwrap(),
        )
        .unwrap();
    let failure_hash = runtime.judge().stable_hash().unwrap();
    // These are genuine old-style operations after failure, not a new capture
    // made by an automatically fenced StepGameplay/native owner.
    for event in [
        button(2 * SECOND, 2, ButtonState::Up),
        button(2 * SECOND, 3, ButtonState::Down),
        touch(2 * SECOND, 2, u64::MAX - 1),
    ] {
        capture
            .record_report(
                &runtime
                    .process_input(event, &Exact, point(2, 2 * SECOND))
                    .unwrap(),
            )
            .unwrap();
    }
    capture
        .record_report(
            &runtime
                .advance_to(point(1, 3 * SECOND), &Exact, point(2, 3 * SECOND))
                .unwrap(),
        )
        .unwrap();
    let hash = runtime.judge().stable_hash().unwrap();
    let file = capture.into_file();
    assert_eq!(file.records.len(), 7);
    let mut reconstructed = reconstruct(&source, file.clone(), limits).unwrap();
    reconstructed.seek_cursor(7).unwrap();
    assert_eq!(reconstructed.engine().stable_hash().unwrap(), hash);
    assert_eq!(reconstructed.results().len(), 3);

    let mut visual = ReplayVisual::new(&source, &file, limits).unwrap();
    assert_eq!(visual.advance_to(ts(0)).unwrap().len(), 2);
    assert_eq!(visual.pressed_lanes(), 3);
    visual.advance_to(ts(SECOND)).unwrap();
    assert_eq!(visual.pressed_lanes(), 0);
    assert_eq!(visual.engine.stable_hash().unwrap(), failure_hash);
    assert!(visual.engine.is_held(InputOwner {
        source: DeviceId(u64::MAX),
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(0x11)
    }));
    let failed_gauge = visual.gauge().clone();
    assert!(failed_gauge.snapshot().failure.is_some());
    assert_eq!(visual.advance_to(ts(2 * SECOND)).unwrap().len(), 1);
    assert_eq!(visual.pressed_lanes(), 0);
    assert!(visual.advance_to(ts(2 * SECOND)).unwrap().is_empty());
    assert_eq!(visual.gauge(), &failed_gauge);
    visual.advance_to(ts(3 * SECOND)).unwrap();
    assert!(visual.finished());
    assert_eq!(visual.pressed_lanes(), 0);
    assert_eq!(visual.engine.stable_hash().unwrap(), hash);
    assert!(visual.advance_to(ts(SECOND)).is_err());
    assert_eq!(visual.engine.stable_hash().unwrap(), hash);
    assert_eq!(visual.gauge(), &failed_gauge);
}
