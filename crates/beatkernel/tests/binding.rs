use std::collections::HashSet;

use beatkernel::input::*;
use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp};

fn meta(source: u64) -> EventMeta {
    let mut meta = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: Timestamp::from_nanos(-50),
        },
        u64::MAX,
    );
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(u32::MAX),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(3),
            timestamp: Timestamp::from_nanos(-90),
        }),
    });
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::from_nanos(-70),
    });
    meta
}

fn button(source: u64, control: PhysicalControlId, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(source),
        control,
        state,
    })
}

fn rule(device: DeviceSelector, physical: PhysicalControlId, target: u32) -> Binding {
    Binding {
        device,
        physical,
        game_control: GameControlId(target),
    }
}

fn destinations(map: &BindingMap, input: &PhysicalInputEvent) -> Vec<u32> {
    map.map(input).map(|output| output.game_control.0).collect()
}

fn bits(a: f32, b: f32) {
    assert_eq!(a.to_bits(), b.to_bits());
}

fn position(a: Position2, b: Position2) {
    bits(a.x, b.x);
    bits(a.y, b.y);
}

fn unchanged(actual: &PhysicalInputEvent, expected: &PhysicalInputEvent) {
    assert_eq!(actual.meta(), expected.meta());
    match (actual, expected) {
        (PhysicalInputEvent::Button(a), PhysicalInputEvent::Button(b)) => assert_eq!(a, b),
        (PhysicalInputEvent::Axis(a), PhysicalInputEvent::Axis(b)) => {
            assert_eq!((a.control, a.mode), (b.control, b.mode));
            bits(a.value, b.value);
        }
        (PhysicalInputEvent::Touch(a), PhysicalInputEvent::Touch(b)) => {
            assert_eq!(
                (a.control, a.contact, a.phase),
                (b.control, b.contact, b.phase)
            );
            position(a.position, b.position);
            assert_eq!(a.pressure.map(f32::to_bits), b.pressure.map(f32::to_bits));
        }
        (PhysicalInputEvent::Pointer(a), PhysicalInputEvent::Pointer(b)) => {
            assert_eq!((a.control, a.mode), (b.control, b.mode));
            position(a.position, b.position);
        }
        (PhysicalInputEvent::Pose(a), PhysicalInputEvent::Pose(b)) => {
            assert_eq!(a.control, b.control);
            for (x, y) in [
                (a.position.x, b.position.x),
                (a.position.y, b.position.y),
                (a.position.z, b.position.z),
                (a.orientation.x, b.orientation.x),
                (a.orientation.y, b.orientation.y),
                (a.orientation.z, b.orientation.z),
                (a.orientation.w, b.orientation.w),
            ] {
                bits(x, y);
            }
        }
        _ => panic!("semantic variant changed"),
    }
}

#[test]
fn identities_and_empty_construction_use_the_public_contract() {
    assert_eq!(
        std::mem::size_of::<GameControlId>(),
        std::mem::size_of::<u32>()
    );
    assert_eq!(
        HashSet::from([GameControlId(0), GameControlId(u32::MAX), GameControlId(0)]).len(),
        2
    );
    let key = PhysicalControlId::keyboard(4);
    let a = rule(DeviceSelector::Any, key, 0);
    let differing = [
        a,
        rule(DeviceSelector::Exact(DeviceId(0)), key, 0),
        rule(DeviceSelector::Any, PhysicalControlId::keyboard(5), 0),
        rule(DeviceSelector::Any, key, u32::MAX),
    ];
    assert_eq!(HashSet::from(differing).len(), 4);
    for map in [
        BindingMap::new(),
        BindingMap::default(),
        BindingMap::from_bindings([]).unwrap(),
    ] {
        assert!(map.bindings().is_empty());
        assert_eq!(map.map(&button(0, key, ButtonState::Down)).count(), 0);
    }
    let map = BindingMap::from_bindings(differing).unwrap();
    assert_eq!(map.bindings(), &differing);
}

#[test]
fn duplicate_add_is_atomic_and_constructor_returns_the_exact_error() {
    let a = rule(DeviceSelector::Any, PhysicalControlId::keyboard(4), 1);
    let b = rule(DeviceSelector::Any, PhysicalControlId::keyboard(4), 2);
    let mut map = BindingMap::new();
    map.add(a).unwrap();
    map.add(b).unwrap();
    assert_eq!(map.add(a), Err(BindingError::Duplicate(a)));
    assert_eq!(map.bindings(), &[a, b]);
    assert_eq!(
        BindingMap::from_bindings([a, b, a]).unwrap_err(),
        BindingError::Duplicate(a)
    );
    assert_eq!(
        BindingMap::from_bindings([a, b]).unwrap().bindings(),
        &[a, b]
    );
    let error = BindingError::Duplicate(a);
    let object: &dyn std::error::Error = &error;
    assert!(object.source().is_none());
    let message = object.to_string();
    assert!(message.contains("duplicate"));
    assert!(message.contains("GameControlId(1)"));
}

#[test]
fn two_devices_with_the_same_physical_key_have_distinct_exact_controls() {
    let key = PhysicalControlId::keyboard(4);
    let map = BindingMap::from_bindings([
        rule(DeviceSelector::Exact(DeviceId(1)), key, 10),
        rule(DeviceSelector::Exact(DeviceId(2)), key, 20),
    ])
    .unwrap();
    assert_eq!(destinations(&map, &button(1, key, ButtonState::Down)), [10]);
    assert_eq!(destinations(&map, &button(2, key, ButtonState::Down)), [20]);
    assert!(destinations(&map, &button(3, key, ButtonState::Down)).is_empty());
}

#[test]
fn exact_override_and_any_fanout_are_independent_of_rule_placement() {
    let key = PhysicalControlId::keyboard(4);
    let any1 = rule(DeviceSelector::Any, key, 1);
    let any2 = rule(DeviceSelector::Any, key, 2);
    let exact1 = rule(DeviceSelector::Exact(DeviceId(1)), key, 10);
    let exact2 = rule(DeviceSelector::Exact(DeviceId(1)), key, 20);
    for rules in [[any1, exact1, any2, exact2], [exact1, any1, exact2, any2]] {
        let map = BindingMap::from_bindings(rules).unwrap();
        assert_eq!(
            destinations(&map, &button(1, key, ButtonState::Down)),
            [10, 20]
        );
        assert_eq!(
            destinations(&map, &button(2, key, ButtonState::Down)),
            [1, 2]
        );
    }
}

#[test]
fn unrelated_exact_rules_never_suppress_any_and_nonmatches_are_empty() {
    let key = PhysicalControlId::keyboard(4);
    let map = BindingMap::from_bindings([
        rule(DeviceSelector::Any, key, 1),
        rule(DeviceSelector::Exact(DeviceId(2)), key, 2),
        rule(
            DeviceSelector::Exact(DeviceId(1)),
            PhysicalControlId::keyboard(5),
            3,
        ),
        rule(
            DeviceSelector::Exact(DeviceId(1)),
            PhysicalControlId::HidUsage {
                usage_page: 8,
                usage: 4,
            },
            4,
        ),
        rule(
            DeviceSelector::Exact(DeviceId(1)),
            PhysicalControlId::Native {
                backend: BackendId(7),
                code: 4,
            },
            5,
        ),
    ])
    .unwrap();
    assert_eq!(destinations(&map, &button(1, key, ButtonState::Down)), [1]);
    assert!(destinations(
        &map,
        &button(1, PhysicalControlId::keyboard(99), ButtonState::Down)
    )
    .is_empty());
}

#[test]
fn removal_preserves_fanout_order_readd_appends_and_last_exact_restores_fallback() {
    let key = PhysicalControlId::keyboard(4);
    let any = rule(DeviceSelector::Any, key, 1);
    let a = rule(DeviceSelector::Exact(DeviceId(1)), key, 10);
    let b = rule(DeviceSelector::Exact(DeviceId(1)), key, 20);
    let c = rule(DeviceSelector::Exact(DeviceId(1)), key, 30);
    let input = button(1, key, ButtonState::Down);
    let mut map = BindingMap::from_bindings([any, a, b, c]).unwrap();
    assert!(map.remove(&b));
    assert_eq!(map.bindings(), &[any, a, c]);
    assert!(!map.remove(&b));
    assert_eq!(map.bindings(), &[any, a, c]);
    assert_eq!(destinations(&map, &input), [10, 30]);
    map.add(b).unwrap();
    assert_eq!(destinations(&map, &input), [10, 30, 20]);
    assert!(map.remove(&a));
    assert!(map.remove(&c));
    assert!(map.remove(&b));
    assert_eq!(destinations(&map, &input), [1]);
}

#[test]
fn all_physical_namespaces_keep_complete_identity() {
    let controls = [
        PhysicalControlId::HidUsage {
            usage_page: 7,
            usage: 4,
        },
        PhysicalControlId::HidUsage {
            usage_page: 8,
            usage: 4,
        },
        PhysicalControlId::Native {
            backend: BackendId(1),
            code: 4,
        },
        PhysicalControlId::Native {
            backend: BackendId(2),
            code: 4,
        },
        PhysicalControlId::Vendor {
            namespace: VendorNamespaceId(1),
            code: 4,
        },
        PhysicalControlId::Vendor {
            namespace: VendorNamespaceId(2),
            code: 4,
        },
    ];
    let map = BindingMap::from_bindings(
        controls
            .into_iter()
            .enumerate()
            .map(|(i, c)| rule(DeviceSelector::Any, c, i as u32)),
    )
    .unwrap();
    for (i, control) in controls.into_iter().enumerate() {
        assert_eq!(
            destinations(&map, &button(1, control, ButtonState::Down)),
            [i as u32]
        );
    }
}

#[test]
fn buttons_preserve_states_extreme_metadata_and_optional_provenance() {
    let key = PhysicalControlId::keyboard(4);
    let map = BindingMap::from_bindings([rule(DeviceSelector::Any, key, u32::MAX)]).unwrap();
    for state in [ButtonState::Down, ButtonState::Up, ButtonState::Repeat] {
        for source in [0, u64::MAX] {
            for timestamp in [i64::MIN, -1, i64::MAX] {
                for sequence in [0, u64::MAX] {
                    for native in [None, meta(source).native] {
                        for origin in [None, meta(source).original_clock_point] {
                            let mut input = button(source, key, state);
                            let m = input.meta_mut();
                            m.timestamp = Timestamp::from_nanos(timestamp);
                            m.sequence = sequence;
                            m.native = native;
                            m.original_clock_point = origin;
                            let outputs: Vec<_> = map.map(&input).collect();
                            assert_eq!(outputs.len(), 1);
                            assert_eq!(outputs[0].game_control, GameControlId(u32::MAX));
                            unchanged(&outputs[0].physical, &input);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn axes_pointers_touches_and_poses_keep_modes_contacts_and_float_bits() {
    let control = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(42),
        code: 9,
    };
    let map = BindingMap::from_bindings([
        rule(DeviceSelector::Any, control, 100),
        rule(DeviceSelector::Any, control, 101),
    ])
    .unwrap();
    let nan = f32::from_bits(0x7fc01234);
    let mut inputs = Vec::new();
    for mode in [AxisMode::Absolute, AxisMode::Relative] {
        for value in [-0.0, nan, f32::INFINITY, -17.5] {
            inputs.push(PhysicalInputEvent::Axis(AxisEvent {
                meta: meta(1),
                control,
                value,
                mode,
            }));
        }
    }
    for mode in [PointerMode::Absolute, PointerMode::Relative] {
        inputs.push(PhysicalInputEvent::Pointer(PointerEvent {
            meta: meta(2),
            control,
            position: Position2 { x: -0.0, y: nan },
            mode,
        }));
    }
    for source in [1, 2] {
        for contact in [ContactId(0), ContactId(u64::MAX)] {
            for phase in [
                TouchPhase::Down,
                TouchPhase::Move,
                TouchPhase::Up,
                TouchPhase::Cancel,
            ] {
                for pressure in [None, Some(-0.0), Some(nan), Some(2.0)] {
                    inputs.push(PhysicalInputEvent::Touch(TouchEvent {
                        meta: meta(source),
                        control,
                        contact,
                        phase,
                        position: Position2 { x: nan, y: -0.0 },
                        pressure,
                    }));
                }
            }
        }
    }
    inputs.push(PhysicalInputEvent::Pose(PoseEvent {
        meta: meta(3),
        control,
        position: Position3 {
            x: -0.0,
            y: nan,
            z: f32::NEG_INFINITY,
        },
        orientation: Quaternion {
            x: 2.0,
            y: -0.0,
            z: nan,
            w: 3.0,
        },
    }));
    for input in inputs {
        let output: Vec<_> = map.map(&input).collect();
        assert_eq!(
            output.iter().map(|e| e.game_control.0).collect::<Vec<_>>(),
            [100, 101]
        );
        for mapped in output {
            unchanged(&mapped.physical, &input);
        }
    }
    let other_surface = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(42),
        code: 10,
    };
    let mut touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(1),
        control: other_surface,
        contact: ContactId(0),
        phase: TouchPhase::Down,
        position: Position2 { x: 1.0, y: 2.0 },
        pressure: None,
    });
    assert_eq!(map.map(&touch).count(), 0);
    if let PhysicalInputEvent::Touch(event) = &mut touch {
        event.control = control;
    }
    assert_eq!(map.map(&touch).count(), 2);
}

#[test]
fn raw_and_custom_payloads_produce_no_semantic_outputs_or_changes() {
    let map = BindingMap::from_bindings([
        rule(DeviceSelector::Any, PhysicalControlId::keyboard(4), 1),
        rule(
            DeviceSelector::Exact(DeviceId(1)),
            PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(7),
                code: 4,
            },
            2,
        ),
    ])
    .unwrap();
    for data in [vec![], vec![0, 4, 255]] {
        for report_id in [None, Some(4)] {
            let input = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
                meta: meta(1),
                report_id,
                data: data.clone(),
            });
            let before = input.clone();
            assert_eq!(map.map(&input).count(), 0);
            assert_eq!(input, before);
        }
        let input = PhysicalInputEvent::Custom(CustomInputEvent {
            meta: meta(1),
            namespace: VendorNamespaceId(7),
            type_id: 4,
            payload: data,
        });
        let before = input.clone();
        assert_eq!(map.map(&input).count(), 0);
        assert_eq!(input, before);
    }
}

#[test]
fn owned_outputs_outlive_inputs_and_partial_iterators_do_not_consume_configuration() {
    let key = PhysicalControlId::keyboard(4);
    let output = {
        let input = button(1, key, ButtonState::Down);
        let map = BindingMap::from_bindings([
            rule(DeviceSelector::Any, key, 1),
            rule(DeviceSelector::Any, key, 2),
        ])
        .unwrap();
        let mut partial = map.map(&input);
        assert_eq!(partial.next().unwrap().game_control, GameControlId(1));
        drop(partial);
        assert_eq!(map.bindings().len(), 2);
        assert_eq!(input, button(1, key, ButtonState::Down));
        assert_eq!(destinations(&map, &input), [1, 2]);
        map.map(&input).collect::<Vec<_>>()
    };
    assert_eq!(
        output.iter().map(|e| e.game_control.0).collect::<Vec<_>>(),
        [1, 2]
    );
    for event in output {
        unchanged(&event.physical, &button(1, key, ButtonState::Down));
    }
}

struct Offset;
impl ClockMapper for Offset {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        if point.domain == ClockDomainId(2) && target == ClockDomainId(1) {
            point
                .timestamp
                .as_nanos()
                .checked_add(1000)
                .map(Timestamp::from_nanos)
        } else {
            None
        }
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

#[test]
fn virtual_fifo_to_binding_keeps_two_keyboards_touch_fanout_and_clock_provenance() {
    let key = PhysicalControlId::keyboard(4);
    let surface = PhysicalControlId::HidUsage {
        usage_page: 0x0d,
        usage: 4,
    };
    let mut backend = VirtualInputBackend::new(ClockDomainId(1));
    for source in [1, 2, 3] {
        backend
            .register_device(DeviceDescriptor {
                runtime_id: DeviceId(source),
                vendor_id: None,
                product_id: None,
                serial: None,
                name: Some("virtual fixture".into()),
                transport: DeviceTransport::Virtual,
                capabilities: DeviceCapabilities {
                    button: true,
                    touch: true,
                    ..Default::default()
                },
            })
            .unwrap();
    }
    let map = BindingMap::from_bindings([
        rule(DeviceSelector::Exact(DeviceId(1)), key, 10),
        rule(DeviceSelector::Exact(DeviceId(2)), key, 20),
        rule(DeviceSelector::Any, surface, 30),
        rule(DeviceSelector::Any, surface, 31),
    ])
    .unwrap();
    let mut samples = vec![
        button(1, key, ButtonState::Down),
        button(2, key, ButtonState::Down),
        button(1, key, ButtonState::Up),
    ];
    for contact in [8, 9] {
        samples.push(PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(3),
            control: surface,
            contact: ContactId(contact),
            phase: TouchPhase::Down,
            position: Position2 {
                x: contact as f32,
                y: -0.0,
            },
            pressure: Some(0.5),
        }));
    }
    let mut expected = Vec::new();
    for (i, mut sample) in samples.into_iter().enumerate() {
        let source = sample.meta().source.0;
        let m = sample.meta_mut();
        m.clock_domain = ClockDomainId(2);
        m.timestamp = Timestamp::from_nanos([100, 50, 150, 100, 100][i]);
        m.sequence = if i == 2 { 2 } else { 1 };
        m.original_clock_point = None;
        backend.push(sample.clone(), &Offset).unwrap();
        let m = sample.meta_mut();
        m.original_clock_point = Some(ClockPoint {
            domain: ClockDomainId(2),
            timestamp: m.timestamp,
        });
        m.timestamp = Timestamp::from_nanos(m.timestamp.as_nanos() + 1000);
        m.clock_domain = ClockDomainId(1);
        let targets: &[u32] = match source {
            1 => &[10],
            2 => &[20],
            3 => &[30, 31],
            _ => unreachable!(),
        };
        for target in targets {
            expected.push(GameInputEvent {
                game_control: GameControlId(*target),
                physical: sample.clone(),
            });
        }
    }
    let actual: Vec<_> = backend
        .drain_events()
        .flat_map(|input| map.map(&input).collect::<Vec<_>>())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(
        actual.iter().map(|e| e.game_control.0).collect::<Vec<_>>(),
        [10, 20, 10, 30, 31, 30, 31]
    );
    assert!(backend.pop().is_none());
}
