//! Deferred shared geometry and the genuine cross-language canonical touch packet.
use crate::{
    browser_input::{TouchInputSetup, decode_input},
    playfield_layout::{LOGICAL_EXTENT, DEFAULT_BOUNDS, default_touch_bounds, partition_lane},
};
use beatkernel::{
    input::*,
    interaction::InteractionState,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::{BmsInputMode, parse};

fn physical(contact: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(
            DeviceId(2),
            ClockPoint {
                domain: ClockDomainId(0x57494e),
                timestamp: Timestamp::from_nanos(100),
            },
            contact,
        ),
        control: PhysicalControlId::Native {
            backend: BackendId(0x57544f55),
            code: 0,
        },
        contact: ContactId(contact),
        phase: TouchPhase::Down,
        position: Position2 {
            x: 1400.0,
            y: 900.0,
        },
        pressure: Some(0.5),
    })
}

#[test]
fn actual_default_geometry_selects_rendered_bms_lanes_without_rewriting_physical_coordinates() {
    assert_eq!(LOGICAL_EXTENT, [960, 720]);
    assert_eq!(DEFAULT_BOUNDS, [80, 106, 640, 528]);
    let lanes = [0x13, 0x11, 0x12];
    let bounds = default_touch_bounds(&lanes).unwrap();
    assert_eq!(
        bounds,
        [
            80.0, 110.0, 293.0, 634.0, 293.0, 110.0, 506.0, 634.0, 506.0, 110.0, 720.0, 634.0
        ]
    );
    assert_eq!(
        (0..3)
            .map(|index| partition_lane(index, 3, 80, 640))
            .collect::<Vec<_>>(),
        [(80, 293), (293, 506), (506, 720)]
    );
    assert_eq!(
        default_touch_bounds(&[0x11]).unwrap(),
        [80.0, 110.0, 720.0, 634.0]
    );
    assert!(default_touch_bounds(&[]).unwrap().is_empty());
    for invalid in [vec![0x11, 0x11], vec![0x10], vec![0xff], vec![0x11; 19]] {
        assert!(default_touch_bounds(&invalid).is_err());
    }
    let words = lanes
        .into_iter()
        .flat_map(|lane| [u32::from(lane), 0, 0, 0, 1, 0x57544f55, 0])
        .collect::<Vec<_>>();
    let mut router = TouchInputSetup::new(&words, &bounds, &lanes, 256)
        .unwrap()
        .router;
    let chart = parse(
        "#BPM 120\n#WAV01 key.wav\n#00011:01\n#00012:01\n#00013:01\n",
        Default::default(),
    )
    .unwrap();
    let mut judge = JudgeEngine::new(
        chart.compile().unwrap().chart,
        chart.rules_with_input_mode(BmsInputMode::ButtonOrContact),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    for (index, x) in [80.0, 293.0, 506.0].into_iter().enumerate() {
        let original = physical(index as u64);
        let TouchRoute::Bound(bound) = router
            .route_at(&original, Position2 { x, y: 110.0 })
            .unwrap()
        else {
            panic!("visible lane must own its left edge")
        };
        assert_eq!(bound.game_control, GameControlId(u32::from(lanes[index])));
        assert_eq!(bound.physical, original);
        let events = judge.push_input(&bound, Timestamp::ZERO).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].input, Some(*original.meta()));
        assert_eq!(
            judge.state(events[0].object),
            Some(InteractionState::Completed)
        );
    }
    for (contact, x, y) in [
        (10, 720.0, 110.0),
        (11, 80.0, 634.0),
        (12, 79.0, 110.0),
        (13, 80.0, 109.0),
    ] {
        assert_eq!(
            router
                .route_at(&physical(contact), Position2 { x, y })
                .unwrap(),
            TouchRoute::Ignored
        );
    }
}

#[test]
fn javascript_touch_golden_decodes_through_the_actual_core_with_full_contact_native_and_float_fields()
 {
    // Same independent 90-byte golden in physical-input.test.mjs.
    let literal = [
        66, 75, 80, 73, 1, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0x4e, 0x49, 0x57,
        0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 1, 0x55, 0x4f, 0x54, 0x57, 1, 0xfe,
        0xff, 0xff, 0xff, 1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 0x55, 0x4f, 0x54,
        0x57, 0, 0, 0, 0, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0, 0, 0, 0xc0, 0x3f, 0,
        0, 0x10, 0xc0, 1, 0, 0, 0, 0x3f,
    ];
    let host = ClockPoint {
        domain: ClockDomainId(0x57494e),
        timestamp: Timestamp::from_nanos(0x0102030405060708),
    };
    let expected = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta {
            source: DeviceId(2),
            timestamp: host.timestamp,
            clock_domain: host.domain,
            sequence: 0x8877665544332211,
            native: Some(NativeEventMeta {
                backend: BackendId(0x57544f55),
                code: Some(0xffff_fffe),
                timestamp: Some(host),
            }),
            original_clock_point: None,
        },
        control: PhysicalControlId::Native {
            backend: BackendId(0x57544f55),
            code: 0,
        },
        contact: ContactId(0xfedcba9876543210),
        phase: TouchPhase::Down,
        position: Position2 { x: 1.5, y: -2.25 },
        pressure: Some(0.5),
    });
    let limits = CodecLimits::new(4096, 1024).unwrap();
    assert_eq!(literal.len(), 90);
    assert_eq!(
        decode_input(&literal, limits, host.domain).unwrap(),
        expected
    );
    assert_eq!(encode_event(&expected, limits).unwrap(), literal);
    for length in [68, 76, 85, 89] {
        assert!(decode_input(&literal[..length], limits, host.domain).is_err());
    }
    assert!(decode_input(&literal, limits, ClockDomainId(1)).is_err());
}
