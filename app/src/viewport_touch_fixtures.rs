//! Deferred actual canonical decoding and core contact ownership, without WASM.
use crate::browser_input::{decode_input, project_touch_on_surface};
use beatkernel::{
    input::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};

const HOST: ClockDomainId = ClockDomainId(0x5749_4e);
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x5754_4f55),
        code: 0,
    }
}
fn sample(sequence: u64, contact: u64, phase: TouchPhase, x: f32, y: f32) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(
        DeviceId(u64::MAX),
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::from_nanos(9_007_199_254_740_993 + sequence as i64),
        },
        sequence,
    );
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x5754_4f55),
        code: Some(u32::MAX),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(-17),
        }),
    });
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(i64::MAX),
    });
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(),
        contact: ContactId(contact),
        phase,
        position: Position2 { x, y },
        pressure: Some(0.375),
    })
}
fn regions(swapped: bool) -> Vec<TouchRegion> {
    [0x11, 0x12]
        .into_iter()
        .enumerate()
        .map(|(index, lane)| {
            let left = if (index == 0) != swapped { 0.0 } else { 480.0 };
            TouchRegion {
                device: DeviceSelector::Exact(DeviceId(u64::MAX)),
                physical: surface(),
                game_control: GameControlId(lane),
                min: Position2 { x: left, y: 0.0 },
                max: Position2 {
                    x: left + 480.0,
                    y: 720.0,
                },
            }
        })
        .collect()
}
fn route(
    router: &mut TouchRouter,
    original: &PhysicalInputEvent,
    css: [f64; 2],
    backing: [u32; 2],
) -> TouchRoute {
    let limits = CodecLimits::new(4096, 1024).unwrap();
    let bytes = encode_event(original, limits).unwrap();
    let decoded = decode_input(&bytes, limits, HOST).unwrap();
    let position = project_touch_on_surface(&decoded, css, backing, [960, 720]).unwrap();
    assert_eq!(decoded, *original);
    assert_eq!(encode_event(&decoded, limits).unwrap(), bytes);
    let routed = router.route_at(&decoded, position).unwrap();
    if let TouchRoute::Bound(bound) = &routed {
        assert_eq!(bound.physical, *original);
    }
    routed
}
fn lane(result: TouchRoute) -> GameControlId {
    let TouchRoute::Bound(bound) = result else {
        panic!("retained contact must reach its original lane")
    };
    bound.game_control
}

#[test]
fn actual_touch_routing_preserves_original_bytes_and_held_destination_across_bars_resize_and_page_remap()
 {
    let mut router = TouchRouter::new(regions(false), 8).unwrap();
    let down = sample(1, u64::MAX, TouchPhase::Down, 200.0, 200.0);
    assert_eq!(
        project_touch_on_surface(&down, [1280.0, 720.0], [2560, 1440], [960, 720]).unwrap(),
        Position2 { x: 40.0, y: 200.0 }
    );
    assert_eq!(
        lane(route(&mut router, &down, [1280.0, 720.0], [2560, 1440])),
        GameControlId(0x11)
    );
    let moved = sample(2, u64::MAX, TouchPhase::Move, 800.0, 500.0);
    assert_eq!(
        project_touch_on_surface(&moved, [1000.0, 1000.0], [1000, 1000], [960, 720]).unwrap(),
        Position2 { x: 768.0, y: 360.0 }
    );
    assert_eq!(
        lane(route(&mut router, &moved, [1000.0, 1000.0], [1000, 1000])),
        GameControlId(0x11)
    );
    router.remap_regions(regions(true)).unwrap();
    router.set_new_contacts_enabled(false);
    let up_in_bar = sample(3, u64::MAX, TouchPhase::Up, -12.5, 50.0);
    assert_eq!(
        lane(route(
            &mut router,
            &up_in_bar,
            [1000.0, 1000.0],
            [1000, 1000]
        )),
        GameControlId(0x11)
    );
    assert_eq!(router.active_contacts(), 0);
    let hidden = sample(4, 5, TouchPhase::Down, 400.0, 200.0);
    assert_eq!(
        route(&mut router, &hidden, [1280.0, 720.0], [1280, 720]),
        TouchRoute::Ignored
    );
    router.set_new_contacts_enabled(true);
    assert_eq!(
        route(
            &mut router,
            &sample(5, 5, TouchPhase::Move, 400.0, 200.0),
            [1280.0, 720.0],
            [1280, 720]
        ),
        TouchRoute::Ignored
    );
    assert_eq!(
        route(
            &mut router,
            &sample(6, 5, TouchPhase::Cancel, 1500.0, -20.0),
            [1280.0, 720.0],
            [1280, 720]
        ),
        TouchRoute::Ignored
    );
    assert_eq!(router.active_contacts(), 0);
    assert_eq!(
        lane(route(
            &mut router,
            &sample(7, 5, TouchPhase::Down, 400.0, 200.0),
            [1280.0, 720.0],
            [1280, 720]
        )),
        GameControlId(0x12)
    );
    assert_eq!(
        lane(route(
            &mut router,
            &sample(8, 5, TouchPhase::Cancel, 1500.0, -20.0),
            [1280.0, 720.0],
            [1280, 720]
        )),
        GameControlId(0x12)
    );
    assert_eq!(router.active_contacts(), 0);
}

#[test]
fn bar_downs_never_fall_through_and_invalid_geometry_cannot_consume_contact_ownership() {
    let mut router = TouchRouter::new(regions(false), 4).unwrap();
    let bar = sample(1, 1, TouchPhase::Down, 100.0, 300.0);
    assert_eq!(
        route(&mut router, &bar, [1280.0, 720.0], [1280, 720]),
        TouchRoute::Ignored
    );
    assert_eq!(
        route(
            &mut router,
            &sample(2, 1, TouchPhase::Move, 700.0, 300.0),
            [1280.0, 720.0],
            [1280, 720]
        ),
        TouchRoute::Ignored
    );
    assert_eq!(
        route(
            &mut router,
            &sample(3, 1, TouchPhase::Up, 700.0, 300.0),
            [1280.0, 720.0],
            [1280, 720]
        ),
        TouchRoute::Ignored
    );
    let down = sample(4, 2, TouchPhase::Down, 200.25, 300.5);
    assert_eq!(
        lane(route(&mut router, &down, [1280.0, 720.0], [1280, 720])),
        GameControlId(0x11)
    );
    let up = sample(5, 2, TouchPhase::Up, 1800.0, -100.0);
    for (css, backing, logical) in [
        ([0.0, 720.0], [1280, 720], [960, 720]),
        ([1280.0, f64::NAN], [1280, 720], [960, 720]),
        ([1280.0, 720.0], [0, 720], [960, 720]),
        ([1280.0, 720.0], [1280, 720], [960, 0]),
        ([f64::MIN_POSITIVE, 720.0], [u32::MAX, 720], [960, 720]),
    ] {
        assert!(project_touch_on_surface(&up, css, backing, logical).is_err());
        assert_eq!(router.active_contacts(), 1);
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let invalid = sample(5, 2, TouchPhase::Up, value, 100.0);
        assert!(
            project_touch_on_surface(&invalid, [1280.0, 720.0], [1280, 720], [960, 720]).is_err()
        );
    }
    assert_eq!(
        lane(route(&mut router, &up, [1280.0, 720.0], [1280, 720])),
        GameControlId(0x11)
    );
    assert_eq!(router.active_contacts(), 0);
}
