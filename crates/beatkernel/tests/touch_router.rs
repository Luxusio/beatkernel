//! Deferred region routing over real physical events, press judging and replay.
use beatkernel::{
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
        compile,
    },
    input::*,
    interaction::{InteractionState, PressHoldEvaluator, PressInstantEvaluator},
    judge::{
        JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, MissReason,
        Rule,
    },
    replay::{REPLAY_VERSION, ReplayHeader, ReplayOperation, ReplaySession},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn position(x: f32, y: f32) -> Position2 {
    Position2 { x, y }
}
fn surface(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x544f_5543),
        code,
    }
}
fn region(
    device: DeviceSelector,
    physical: PhysicalControlId,
    lane: u32,
    min: Position2,
    max: Position2,
) -> TouchRegion {
    TouchRegion {
        device,
        physical,
        game_control: GameControlId(lane),
        min,
        max,
    }
}
fn lanes() -> Vec<TouchRegion> {
    vec![
        region(
            DeviceSelector::Any,
            surface(1),
            1,
            position(0.0, 0.0),
            position(10.0, 10.0),
        ),
        region(
            DeviceSelector::Any,
            surface(1),
            2,
            position(10.0, 0.0),
            position(20.0, 10.0),
        ),
    ]
}
fn meta(device: u64) -> EventMeta {
    EventMeta {
        source: DeviceId(device),
        timestamp: ts(9_007_199_254_740_993),
        clock_domain: ClockDomainId(7),
        sequence: u64::MAX,
        native: Some(NativeEventMeta {
            backend: BackendId(u32::MAX),
            code: Some(0x8000_0001),
            timestamp: Some(ClockPoint {
                domain: ClockDomainId(8),
                timestamp: ts(i64::MIN),
            }),
        }),
        original_clock_point: Some(ClockPoint {
            domain: ClockDomainId(9),
            timestamp: ts(-17),
        }),
    }
}
fn touch(
    device: u64,
    physical: PhysicalControlId,
    contact: u64,
    phase: TouchPhase,
    x: f32,
    y: f32,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(device),
        control: physical,
        contact: ContactId(contact),
        phase,
        position: position(x, y),
        pressure: Some(12.5),
    })
}
fn bound(router: &mut TouchRouter, event: &PhysicalInputEvent, lane: u32) -> GameInputEvent {
    let result = router.route(event).unwrap();
    assert_eq!(
        result,
        TouchRoute::Bound(GameInputEvent {
            game_control: GameControlId(lane),
            physical: event.clone()
        })
    );
    let TouchRoute::Bound(event) = result else {
        unreachable!()
    };
    event
}
fn judge() -> JudgeEngine {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    chart.objects = vec![
        SourceObject {
            id: ObjectId(1),
            start: Beat::new(100).unwrap(),
            end: Some(Beat::new(200).unwrap()),
            interaction: InteractionId(1),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        },
        SourceObject {
            id: ObjectId(2),
            start: Beat::new(150).unwrap(),
            end: None,
            interaction: InteractionId(2),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        },
    ];
    JudgeEngine::new(
        compile(&chart).unwrap(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(PressHoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(2),
                evaluator: Box::new(PressInstantEvaluator),
            },
        ],
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
    .unwrap()
}

#[test]
fn configuration_is_bounded_nonoverlapping_and_half_open_with_exact_surface_override() {
    assert!(
        TouchRouter::new(Vec::new(), 1)
            .unwrap()
            .regions()
            .is_empty()
    );
    for cap in [0, 4097, usize::MAX] {
        assert!(
            matches!(TouchRouter::new(lanes(), cap), Err(TouchRoutingError::InvalidContactLimit { max_contacts }) if max_contacts == cap)
        );
    }
    let many = (0..256)
        .map(|code| {
            region(
                DeviceSelector::Any,
                surface(code),
                code,
                position(0.0, 0.0),
                position(1.0, 1.0),
            )
        })
        .collect::<Vec<_>>();
    let router = TouchRouter::new(many.clone(), 4096).unwrap();
    assert_eq!(router.regions(), many);
    assert_eq!(router.max_contacts(), 4096);
    let mut excess = many;
    excess.push(lanes()[0]);
    assert!(matches!(
        TouchRouter::new(excess, 1),
        Err(TouchRoutingError::InvalidRegionLimit { count: 257 })
    ));
    for (min, max) in [
        (position(0.0, 0.0), position(0.0, 1.0)),
        (position(0.0, 0.0), position(1.0, 0.0)),
        (position(2.0, 0.0), position(1.0, 1.0)),
        (position(0.0, 2.0), position(1.0, 1.0)),
        (position(f32::NAN, 0.0), position(1.0, 1.0)),
        (position(0.0, f32::NEG_INFINITY), position(1.0, 1.0)),
        (position(0.0, 0.0), position(f32::INFINITY, 1.0)),
        (position(0.0, 0.0), position(1.0, f32::NAN)),
    ] {
        assert!(matches!(
            TouchRouter::new(
                vec![region(DeviceSelector::Any, surface(1), 1, min, max)],
                1
            ),
            Err(TouchRoutingError::InvalidRegion { index: 0 })
        ));
    }
    for same_destination in [false, true] {
        let a = lanes()[0];
        let b = region(
            a.device,
            a.physical,
            if same_destination { 1 } else { 2 },
            position(9.0, 0.0),
            position(20.0, 10.0),
        );
        assert!(matches!(
            TouchRouter::new(vec![a, b], 2),
            Err(TouchRoutingError::OverlappingRegions {
                first: 0,
                second: 1
            })
        ));
        assert!(matches!(
            TouchRouter::new(vec![a, a], 2),
            Err(TouchRoutingError::OverlappingRegions {
                first: 0,
                second: 1
            })
        ));
    }
    let mut regions = lanes();
    regions.push(region(
        DeviceSelector::Exact(DeviceId(u64::MAX)),
        surface(1),
        3,
        position(4.0, 2.0),
        position(6.0, 8.0),
    ));
    regions.push(region(
        DeviceSelector::Exact(DeviceId(0)),
        surface(1),
        4,
        position(4.0, 2.0),
        position(6.0, 8.0),
    ));
    let mut router = TouchRouter::new(regions.clone(), 16).unwrap();
    assert_eq!(router.regions(), regions);
    for (id, device, x, y, expected) in [
        (1, 1, 0.0, 0.0, Some(1)),
        (2, 1, 10.0, 0.0, Some(2)),
        (3, 1, 20.0, 5.0, None),
        (4, 1, 5.0, 10.0, None),
        (5, 1, -0.25, 5.0, None),
        (6, u64::MAX, 4.0, 2.0, Some(3)),
        (7, u64::MAX, 6.0, 5.0, None),
        (8, u64::MAX, 5.0, 8.0, None),
        (9, u64::MAX, 1.0, 1.0, None),
        (10, 0, 5.0, 3.0, Some(4)),
    ] {
        let event = touch(device, surface(1), id, TouchPhase::Down, x, y);
        if let Some(lane) = expected {
            bound(&mut router, &event, lane);
        } else {
            assert_eq!(router.route(&event).unwrap(), TouchRoute::Ignored);
        }
    }
    assert_eq!(
        router.active_contacts(),
        10,
        "outside Down is still owned; exact-selector gaps do not fall back"
    );
    assert_eq!(
        router
            .route(&touch(u64::MAX, surface(2), 11, TouchPhase::Down, 5.0, 3.0))
            .unwrap(),
        TouchRoute::Unconfigured
    );
    assert_eq!(router.active_contacts(), 10);
}

#[test]
fn first_down_locks_the_destination_and_outside_contacts_cannot_slide_or_repeat_into_a_lane() {
    let mut router = TouchRouter::new(lanes(), 2).unwrap();
    let down = touch(1, surface(1), 7, TouchPhase::Down, 2.0, 2.0);
    bound(&mut router, &down, 1);
    for (phase, x, y) in [
        (TouchPhase::Move, 12.0, 2.0),
        (TouchPhase::Down, 12.0, 2.0),
        (TouchPhase::Move, -100.0, 99.0),
    ] {
        bound(&mut router, &touch(1, surface(1), 7, phase, x, y), 1);
        assert_eq!(router.active_contacts(), 1);
    }
    bound(
        &mut router,
        &touch(1, surface(1), 7, TouchPhase::Up, 100.0, 100.0),
        1,
    );
    assert_eq!(router.active_contacts(), 0);
    assert_eq!(
        router
            .route(&touch(1, surface(1), 7, TouchPhase::Move, 12.0, 2.0))
            .unwrap(),
        TouchRoute::Ignored
    );
    bound(
        &mut router,
        &touch(1, surface(1), 7, TouchPhase::Down, 12.0, 2.0),
        2,
    );
    bound(
        &mut router,
        &touch(1, surface(1), 7, TouchPhase::Cancel, 2.0, 2.0),
        2,
    );
    assert_eq!(router.active_contacts(), 0);

    for phase in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Down,
        TouchPhase::Up,
    ] {
        let x = if phase == TouchPhase::Down && router.active_contacts() == 0 {
            -1.0
        } else {
            2.0
        };
        assert_eq!(
            router
                .route(&touch(1, surface(1), 8, phase, x, 2.0))
                .unwrap(),
            TouchRoute::Ignored
        );
        assert_eq!(
            router.active_contacts(),
            usize::from(phase != TouchPhase::Up)
        );
    }
    bound(
        &mut router,
        &touch(1, surface(1), 8, TouchPhase::Down, 2.0, 2.0),
        1,
    );
}

#[test]
fn contact_identity_includes_all_device_surface_and_contact_bits_and_releases_only_its_exact_tuple()
{
    let vendor = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(0x544f_5543),
        code: 1,
    };
    let regions = vec![
        lanes()[0],
        region(
            DeviceSelector::Any,
            surface(2),
            2,
            position(0.0, 0.0),
            position(10.0, 10.0),
        ),
        region(
            DeviceSelector::Any,
            vendor,
            3,
            position(0.0, 0.0),
            position(10.0, 10.0),
        ),
    ];
    let mut router = TouchRouter::new(regions, 4).unwrap();
    let owners = [
        (u64::MAX, surface(1), u64::MAX, 1),
        (0, surface(1), u64::MAX, 1),
        (u64::MAX, surface(2), u64::MAX, 2),
        (u64::MAX, vendor, u64::MAX, 3),
    ];
    for &(device, physical, contact, lane) in &owners {
        bound(
            &mut router,
            &touch(device, physical, contact, TouchPhase::Down, 1.0, 1.0),
            lane,
        );
    }
    assert_eq!(router.active_contacts(), 4);
    for (device, physical, contact) in [
        (u64::MAX - 1, surface(1), u64::MAX),
        (u64::MAX, surface(1), 0),
        (0, surface(2), u64::MAX),
    ] {
        for phase in [TouchPhase::Move, TouchPhase::Up, TouchPhase::Cancel] {
            assert_eq!(
                router
                    .route(&touch(device, physical, contact, phase, 1.0, 1.0))
                    .unwrap(),
                TouchRoute::Ignored
            );
            assert_eq!(router.active_contacts(), 4);
        }
    }
    for (index, &(device, physical, contact, lane)) in owners.iter().enumerate() {
        let phase = if index % 2 == 0 {
            TouchPhase::Cancel
        } else {
            TouchPhase::Up
        };
        bound(
            &mut router,
            &touch(device, physical, contact, phase, 99.0, -99.0),
            lane,
        );
        assert_eq!(router.active_contacts(), 3 - index);
        assert_eq!(
            router
                .route(&touch(device, physical, contact, phase, 1.0, 1.0))
                .unwrap(),
            TouchRoute::Ignored
        );
        for &(other_device, other_physical, other_contact, other_lane) in &owners[index + 1..] {
            bound(
                &mut router,
                &touch(
                    other_device,
                    other_physical,
                    other_contact,
                    TouchPhase::Move,
                    -1.0,
                    -1.0,
                ),
                other_lane,
            );
        }
    }
    bound(
        &mut router,
        &touch(0, surface(1), 0, TouchPhase::Down, 1.0, 1.0),
        1,
    );
}

#[test]
fn capacity_and_nonfinite_refusals_are_atomic_and_bound_samples_keep_exact_codec_provenance() {
    let mut router = TouchRouter::new(lanes(), 2).unwrap();
    let mut down = touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Down, -0.0, 1.0);
    if let PhysicalInputEvent::Touch(sample) = &mut down {
        sample.pressure = Some(-0.0);
    }
    let routed = bound(&mut router, &down, 1);
    let codec = CodecLimits::new(4096, 1024).unwrap();
    assert_eq!(
        encode_event(&routed.physical, codec).unwrap(),
        encode_event(&down, codec).unwrap(),
        "signed zeros and all original integer/provenance bits survive routing"
    );
    assert_eq!(
        router
            .route(&touch(1, surface(1), 1, TouchPhase::Down, 30.0, 1.0))
            .unwrap(),
        TouchRoute::Ignored
    );
    let refused = touch(2, surface(1), 2, TouchPhase::Down, 12.0, 1.0);
    assert_eq!(
        router.route(&refused),
        Err(TouchRoutingError::ContactCapacity)
    );
    assert_eq!(router.active_contacts(), 2);
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Down, 12.0, 1.0),
        1,
    );
    for phase in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Up,
        TouchPhase::Cancel,
    ] {
        for component in 0..3 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = touch(u64::MAX, surface(1), u64::MAX, phase, 1.0, 1.0);
                if let PhysicalInputEvent::Touch(sample) = &mut invalid {
                    match component {
                        0 => sample.position.x = value,
                        1 => sample.position.y = value,
                        _ => sample.pressure = Some(value),
                    }
                }
                assert_eq!(
                    router.route(&invalid),
                    Err(TouchRoutingError::NonFiniteSample)
                );
                assert_eq!(
                    router.active_contacts(),
                    2,
                    "even invalid release keeps the original contact"
                );
            }
        }
    }
    let mut release = touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Up, 12.0, 1.0);
    if let PhysicalInputEvent::Touch(sample) = &mut release {
        sample.pressure = Some(-123.75);
    }
    bound(&mut router, &release, 1);
    assert_eq!(router.active_contacts(), 1);
    bound(&mut router, &refused, 2);
    assert_eq!(
        router.active_contacts(),
        2,
        "failed admission did not reserve a hidden owner or slot"
    );
    assert_eq!(
        router
            .route(&touch(1, surface(1), 1, TouchPhase::Cancel, 1.0, 1.0))
            .unwrap(),
        TouchRoute::Ignored
    );
    assert_eq!(router.active_contacts(), 1);
    let mut without_pressure = touch(2, surface(1), 2, TouchPhase::Move, 1.0, 1.0);
    if let PhysicalInputEvent::Touch(sample) = &mut without_pressure {
        sample.pressure = None;
    }
    bound(&mut router, &without_pressure, 2);

    let mut projected = TouchRouter::new(lanes(), 1).unwrap();
    let original = touch(
        u64::MAX,
        surface(1),
        u64::MAX,
        TouchPhase::Down,
        960.0,
        720.0,
    );
    let routed = projected.route_at(&original, position(12.0, 2.0)).unwrap();
    assert_eq!(
        routed,
        TouchRoute::Bound(GameInputEvent {
            game_control: GameControlId(2),
            physical: original.clone()
        })
    );
    let TouchRoute::Bound(routed) = routed else {
        unreachable!()
    };
    assert_eq!(
        encode_event(&routed.physical, codec).unwrap(),
        encode_event(&original, codec).unwrap()
    );
    assert_eq!(
        projected.route_at(&original, position(2.0, 2.0)).unwrap(),
        TouchRoute::Bound(routed),
        "duplicate Down keeps the projected initial lane without changing the original sample"
    );
    let release = touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Up, 999.0, 721.0);
    for invalid in [position(f32::NAN, 2.0), position(2.0, f32::INFINITY)] {
        assert_eq!(
            projected.route_at(&release, invalid),
            Err(TouchRoutingError::NonFiniteSample)
        );
        assert_eq!(projected.active_contacts(), 1);
    }
    let bad_original = touch(
        u64::MAX,
        surface(1),
        u64::MAX,
        TouchPhase::Up,
        f32::NAN,
        721.0,
    );
    assert_eq!(
        projected.route_at(&bad_original, position(2.0, 2.0)),
        Err(TouchRoutingError::NonFiniteSample)
    );
    assert_eq!(
        projected.active_contacts(),
        1,
        "valid projected coordinates cannot hide a malformed original sample"
    );
    assert_eq!(
        projected.route_at(&release, position(-1.0, -1.0)).unwrap(),
        TouchRoute::Bound(GameInputEvent {
            game_control: GameControlId(2),
            physical: release
        })
    );
    assert_eq!(projected.active_contacts(), 0);
}

#[test]
fn routed_contacts_drive_the_actual_press_judge_and_replay_without_rewriting_or_cross_lane_fanout()
{
    let mut router = TouchRouter::new(lanes(), 4).unwrap();
    let header = ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"touch-region/chart".to_vec(),
        rules_identity: b"touch-region/press".to_vec(),
        options: vec![],
        seed: u64::MAX,
        normalized_clock: ClockDomainId(7),
    };
    let mut replay = ReplaySession::new(header.clone(), judge()).unwrap();
    let mut admitted = Vec::new();
    for (at, contact, phase, x, lane) in [
        (100, 7, TouchPhase::Down, 2.0, 1),
        (150, 7, TouchPhase::Move, 12.0, 1),
        (150, 7, TouchPhase::Down, 12.0, 1),
        (150, 8, TouchPhase::Down, 12.0, 2),
        (200, 7, TouchPhase::Up, -20.0, 1),
        (200, 8, TouchPhase::Cancel, 2.0, 2),
    ] {
        let input = touch(77, surface(1), contact, phase, x, 2.0);
        let event = bound(&mut router, &input, lane);
        admitted.push((ts(at), event.clone()));
        replay.push_input(event, ts(at)).unwrap();
    }
    assert_eq!(router.active_contacts(), 0);
    let results = replay.results().to_vec();
    assert_eq!(
        results
            .iter()
            .map(|event| (event.object, event.stage, event.at))
            .collect::<Vec<_>>(),
        [
            (ObjectId(1), JudgeStage::HoldHead, ts(100)),
            (ObjectId(2), JudgeStage::Instant, ts(150)),
            (ObjectId(1), JudgeStage::HoldTail, ts(200))
        ]
    );
    assert!(results.iter().all(|event| event.outcome
        == JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: Duration::ZERO
        }));
    for (record, (at, event)) in replay.records().iter().zip(&admitted) {
        assert_eq!(record.song_time, *at);
        assert_eq!(record.operation, ReplayOperation::Input(event.clone()));
    }
    assert_eq!(replay.records().len(), admitted.len());
    let reconstructed =
        ReplaySession::from_records(header, judge(), replay.records().iter().cloned()).unwrap();
    assert_eq!(reconstructed.results(), results);
    assert_eq!(
        reconstructed.stable_hash().unwrap(),
        replay.stable_hash().unwrap()
    );
    assert_eq!(
        reconstructed.engine().state(ObjectId(1)),
        Some(InteractionState::Completed)
    );
    assert_eq!(
        reconstructed.engine().state(ObjectId(2)),
        Some(InteractionState::Completed)
    );
}

#[test]
fn unconfigured_samples_keep_stateless_binding_compatibility_and_clear_only_discards_router_ownership()
 {
    let mut router = TouchRouter::new(lanes(), 2).unwrap();
    let bindings = BindingMap::from_bindings([1, 2].map(|lane| Binding {
        device: DeviceSelector::Any,
        physical: surface(1),
        game_control: GameControlId(lane),
    }))
    .unwrap();
    let samples = [
        PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(1),
            control: surface(1),
            state: ButtonState::Down,
        }),
        PhysicalInputEvent::Pointer(PointerEvent {
            meta: meta(1),
            control: surface(1),
            position: position(12.0, 2.0),
            mode: PointerMode::Absolute,
        }),
        PhysicalInputEvent::Axis(AxisEvent {
            meta: meta(1),
            control: surface(1),
            value: 12.5,
            mode: AxisMode::Absolute,
        }),
        PhysicalInputEvent::RawHidReport(RawHidReportEvent {
            meta: meta(1),
            report_id: Some(0xff),
            data: vec![0xff, 0, 1],
        }),
    ];
    for sample in &samples {
        assert_eq!(router.route(sample).unwrap(), TouchRoute::Unconfigured);
        assert_eq!(
            router
                .route_at(sample, position(f32::NAN, f32::INFINITY))
                .unwrap(),
            TouchRoute::Unconfigured
        );
        let expected = if matches!(sample, PhysicalInputEvent::RawHidReport(_)) {
            vec![]
        } else {
            vec![1, 2]
        };
        let mapped = bindings.map(sample).collect::<Vec<_>>();
        assert_eq!(
            mapped
                .iter()
                .map(|event| event.game_control.0)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(mapped.iter().all(|event| &event.physical == sample));
        assert_eq!(router.active_contacts(), 0);
    }
    let malformed_unconfigured =
        touch(1, surface(99), 1, TouchPhase::Down, f32::NAN, f32::INFINITY);
    assert_eq!(
        router.route(&malformed_unconfigured).unwrap(),
        TouchRoute::Unconfigured
    );
    assert_eq!(
        router
            .route_at(&malformed_unconfigured, position(f32::NAN, f32::NAN))
            .unwrap(),
        TouchRoute::Unconfigured
    );
    let outside = touch(1, surface(1), 10, TouchPhase::Down, 99.0, 2.0);
    assert_eq!(
        bindings.map(&outside).count(),
        2,
        "unchanged static bindings would fan out; Ignored explicitly prevents that fallback"
    );
    assert_eq!(router.route(&outside).unwrap(), TouchRoute::Ignored);
    let mut judge = judge();
    let down = touch(1, surface(1), 11, TouchPhase::Down, 2.0, 2.0);
    let routed = bound(&mut router, &down, 1);
    let head = judge.push_input(&routed, ts(100)).unwrap();
    assert_eq!(head.len(), 1);
    assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Active));
    let before = judge.stable_hash().unwrap();
    let regions = router.regions().to_vec();
    assert_eq!(
        router.clear(),
        2,
        "clear includes outside contacts as well as bound contacts"
    );
    assert_eq!(router.clear(), 0);
    assert_eq!(router.active_contacts(), 0);
    assert_eq!(router.max_contacts(), 2);
    assert_eq!(router.regions(), regions);
    assert_eq!(
        judge.stable_hash().unwrap(),
        before,
        "clear is not a synthesized cancel or gameplay reset"
    );
    assert_eq!(
        router
            .route(&touch(1, surface(1), 11, TouchPhase::Up, 2.0, 2.0))
            .unwrap(),
        TouchRoute::Ignored
    );
    assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Active));
    let missed = judge.advance_to(ts(201)).unwrap();
    assert!(missed.iter().any(|event| event.object == ObjectId(1)
        && event.stage == JudgeStage::HoldTail
        && event.outcome
            == JudgeOutcome::Miss {
                reason: MissReason::TailTimeout
            }));
    bound(
        &mut router,
        &touch(1, surface(1), 11, TouchPhase::Down, 12.0, 2.0),
        2,
    );
}

#[test]
fn page_remap_preserves_bound_and_unbound_contacts_and_disabled_acquisition_survives_clone() {
    let mut router = TouchRouter::new(lanes(), 4).unwrap();
    assert!(router.new_contacts_enabled());
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Down, 2.0, 2.0),
        1,
    );
    assert_eq!(
        router
            .route(&touch(u64::MAX, surface(1), 2, TouchPhase::Down, -1.0, 2.0))
            .unwrap(),
        TouchRoute::Ignored
    );
    let mut moved = lanes();
    for row in &mut moved {
        row.min.x += 100.0;
        row.max.x += 100.0;
    }
    router.set_new_contacts_enabled(false);
    router.remap_regions(moved.clone()).unwrap();
    assert_eq!(router.regions(), moved);
    assert_eq!(router.active_contacts(), 2);
    // A hidden-page contact gets retained unbound, rather than falling back to
    // the static map or acquiring a lane after the page becomes visible again.
    assert_eq!(
        router
            .route(&touch(
                u64::MAX,
                surface(1),
                3,
                TouchPhase::Down,
                112.0,
                2.0
            ))
            .unwrap(),
        TouchRoute::Ignored
    );
    let mut cloned = router.try_clone().unwrap();
    assert!(!cloned.new_contacts_enabled());
    assert_eq!(cloned.max_contacts(), 4);
    assert_eq!(cloned.active_contacts(), 3);
    assert_eq!(cloned.regions(), moved);
    router.set_new_contacts_enabled(true);
    for (contact, phase) in [
        (2, TouchPhase::Down),
        (2, TouchPhase::Move),
        (3, TouchPhase::Down),
        (3, TouchPhase::Move),
    ] {
        assert_eq!(
            router
                .route(&touch(u64::MAX, surface(1), contact, phase, 102.0, 2.0))
                .unwrap(),
            TouchRoute::Ignored
        );
    }
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Move, 112.0, 2.0),
        1,
    );
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), 4, TouchPhase::Down, 112.0, 2.0),
        2,
    );
    assert_eq!(
        cloned
            .route(&touch(
                u64::MAX,
                surface(1),
                4,
                TouchPhase::Down,
                112.0,
                2.0
            ))
            .unwrap(),
        TouchRoute::Ignored
    );
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), u64::MAX, TouchPhase::Up, -50.0, 2.0),
        1,
    );
    bound(
        &mut cloned,
        &touch(
            u64::MAX,
            surface(1),
            u64::MAX,
            TouchPhase::Cancel,
            112.0,
            2.0,
        ),
        1,
    );
    for contact in [2, 3] {
        assert_eq!(
            router
                .route(&touch(
                    u64::MAX,
                    surface(1),
                    contact,
                    TouchPhase::Up,
                    102.0,
                    2.0
                ))
                .unwrap(),
            TouchRoute::Ignored
        );
    }
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), 4, TouchPhase::Up, 102.0, 2.0),
        2,
    );
    assert_eq!(router.active_contacts(), 0);
    assert_eq!(
        cloned.active_contacts(),
        3,
        "routing a clone does not release another owner's contacts"
    );
    bound(
        &mut router,
        &touch(u64::MAX, surface(1), 3, TouchPhase::Down, 102.0, 2.0),
        1,
    );
}

#[test]
fn page_remap_rejects_identity_or_extent_changes_atomically_without_losing_held_release() {
    let original = lanes();
    let mut router = TouchRouter::new(original.clone(), 2).unwrap();
    bound(
        &mut router,
        &touch(0, surface(1), 9, TouchPhase::Down, 2.0, 2.0),
        1,
    );
    router.set_new_contacts_enabled(false);
    let mut candidates = vec![
        Vec::new(),
        vec![original[0]],
        original.iter().copied().rev().collect(),
    ];
    for changed in [
        region(
            DeviceSelector::Exact(DeviceId(0)),
            surface(1),
            1,
            position(0.0, 0.0),
            position(10.0, 10.0),
        ),
        region(
            DeviceSelector::Any,
            surface(2),
            1,
            position(0.0, 0.0),
            position(10.0, 10.0),
        ),
        region(
            DeviceSelector::Any,
            surface(1),
            3,
            position(0.0, 0.0),
            position(10.0, 10.0),
        ),
    ] {
        let mut rows = original.clone();
        rows[0] = changed;
        candidates.push(rows);
    }
    for rows in candidates {
        assert_eq!(
            router.remap_regions(rows),
            Err(TouchRoutingError::RegionIdentityChanged)
        );
        assert_eq!(router.regions(), original);
        assert_eq!(router.active_contacts(), 1);
        assert!(!router.new_contacts_enabled());
    }
    for (index, maximum) in [(0, position(f32::NAN, 10.0)), (1, position(10.0, 10.0))] {
        let mut rows = original.clone();
        rows[index].max = maximum;
        assert!(matches!(
            router.remap_regions(rows),
            Err(TouchRoutingError::InvalidRegion { .. })
        ));
        assert_eq!(router.regions(), original);
        assert_eq!(router.active_contacts(), 1);
    }
    let mut overlapping = original.clone();
    overlapping[1].min.x = 9.0;
    assert_eq!(
        router.remap_regions(overlapping),
        Err(TouchRoutingError::OverlappingRegions {
            first: 0,
            second: 1
        })
    );
    assert_eq!(router.regions(), original);
    assert!(!router.new_contacts_enabled());
    bound(
        &mut router,
        &touch(0, surface(1), 9, TouchPhase::Up, 19.0, 2.0),
        1,
    );
    assert_eq!(router.active_contacts(), 0);
    let mut empty = TouchRouter::new(Vec::new(), 1).unwrap();
    empty.remap_regions(Vec::new()).unwrap();
    assert_eq!(
        empty
            .route(&touch(0, surface(1), 1, TouchPhase::Down, 2.0, 2.0))
            .unwrap(),
        TouchRoute::Unconfigured
    );
}
