//! Deferred portable browser admission fixtures using actual core bindings and BKPI codecs.
use crate::browser_input::{PhysicalInputSetup, decode_input};
use beatkernel::{
    input::{
        AxisEvent, AxisMode, BackendId, ButtonEvent, ButtonState, CodecLimits, ContactId,
        CustomInputEvent, DeviceId, DeviceSelector, EventMeta, GameControlId, NativeEventMeta,
        PhysicalControlId, PhysicalInputEvent, PointerEvent, PointerMode, PoseEvent, Position2,
        Position3, Quaternion, RawHidReportEvent, TouchEvent, TouchPhase, VendorNamespaceId,
        decode_event, encode_event,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};

const HOST: ClockDomainId = ClockDomainId(0x57494e);
const KEY: [u32; 7] = [0x11, 0, 0, 0, 0, 7, 4];

fn meta(device: u64, nanos: i64) -> EventMeta {
    EventMeta::new(
        DeviceId(device),
        ClockPoint {
            domain: HOST,
            timestamp: Timestamp::from_nanos(nanos),
        },
        u64::MAX,
    )
}

fn button(device: u64, control: PhysicalControlId) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(device, 604_800_000_000_001),
        control,
        state: ButtonState::Down,
    })
}

#[test]
fn binding_words_preserve_full_device_and_control_identity_with_common_exact_precedence() {
    let words = [
        KEY,
        [0x12, 1, 1, 0x20_0000, 0, 7, 4],
        [0x29, 1, u32::MAX, u32::MAX, 1, u32::MAX, u32::MAX],
        [0x21, 1, 0, 0, 2, u32::MAX, 0x8000_0000],
        [0x13, 1, 1, 0x20_0000, 0, 7, 4],
    ]
    .concat();
    let setup =
        PhysicalInputSetup::new(&words, &[0x11, 0x12, 0x13, 0x21, 0x29], 4096, 1024).unwrap();
    assert_eq!(setup.bindings.bindings()[0].device, DeviceSelector::Any);
    assert_eq!(
        setup.bindings.bindings()[1].device,
        DeviceSelector::Exact(DeviceId(9_007_199_254_740_993))
    );
    assert_eq!(
        setup.bindings.bindings()[2].device,
        DeviceSelector::Exact(DeviceId(u64::MAX))
    );
    assert_eq!(
        setup.bindings.bindings()[3].device,
        DeviceSelector::Exact(DeviceId(0))
    );
    for (event, expected) in [
        (
            button(9_007_199_254_740_993, PhysicalControlId::keyboard(4)),
            vec![0x12, 0x13],
        ),
        (
            button(9_007_199_254_740_994, PhysicalControlId::keyboard(4)),
            vec![0x11],
        ),
        (button(1, PhysicalControlId::keyboard(4)), vec![0x11]),
        (
            button(
                u64::MAX,
                PhysicalControlId::Native {
                    backend: BackendId(u32::MAX),
                    code: u32::MAX,
                },
            ),
            vec![0x29],
        ),
        (
            button(
                0,
                PhysicalControlId::Vendor {
                    namespace: VendorNamespaceId(u32::MAX),
                    code: 0x8000_0000,
                },
            ),
            vec![0x21],
        ),
    ] {
        let bytes = encode_event(&event, setup.limits).unwrap();
        let decoded = decode_input(&bytes, setup.limits, HOST).unwrap();
        let mapped: Vec<_> = setup.bindings.map(&decoded).collect();
        assert_eq!(
            mapped
                .iter()
                .map(|event| event.game_control.0)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(mapped.iter().all(|bound| bound.physical == event));
    }
}

#[test]
fn setup_refuses_ambiguous_rows_and_uncovered_lanes_without_losing_valid_fanout_or_capacity_edges()
{
    for (index, value) in [
        (0, 0x10),
        (0, 0x1a),
        (0, 0x20),
        (0, 0x2a),
        (0, 0x111),
        (1, 2),
        (2, 1),
        (3, 1),
        (4, 3),
        (5, 65_536),
        (6, 65_536),
    ] {
        let mut row = KEY;
        row[index] = value;
        assert!(PhysicalInputSetup::new(&row, &[0x11], 4096, 1024).is_err());
    }
    assert!(
        PhysicalInputSetup::new(&[], &[], 4096, 1024)
            .unwrap()
            .bindings
            .bindings()
            .is_empty()
    );
    assert!(PhysicalInputSetup::new(&[], &[0x11], 4096, 1024).is_err());
    assert!(PhysicalInputSetup::new(&[], &[], 5, 0).is_err());
    assert!(PhysicalInputSetup::new(&[], &[], 4096, 4097).is_err());
    assert!(PhysicalInputSetup::new(&KEY[..6], &[0x11], 4096, 1024).is_err());
    let mut trailing = KEY.to_vec();
    trailing.push(0);
    assert!(PhysicalInputSetup::new(&trailing, &[0x11], 4096, 1024).is_err());
    assert!(PhysicalInputSetup::new(&[KEY, KEY].concat(), &[0x11], 4096, 1024).is_err());
    for lanes in [&[0x12][..], &[0x10][..], &[0x11; 19][..]] {
        assert!(PhysicalInputSetup::new(&KEY, lanes, 4096, 1024).is_err());
    }
    let mut words = Vec::new();
    for code in 0..256 {
        words.extend_from_slice(&[0x11, 0, 0, 0, 1, u32::MAX, code]);
    }
    let setup = PhysicalInputSetup::new(&words, &[0x11], 4096, 1024).unwrap();
    assert_eq!(setup.bindings.bindings().len(), 256);
    assert_eq!(
        setup.bindings.bindings().last().unwrap().physical,
        PhysicalControlId::Native {
            backend: BackendId(u32::MAX),
            code: 255
        }
    );
    words.extend_from_slice(&[0x11, 0, 0, 0, 1, u32::MAX, 256]);
    assert!(PhysicalInputSetup::new(&words, &[0x11], 4096, 1024).is_err());
    assert_eq!(setup.bindings.bindings().len(), 256);
    assert!(PhysicalInputSetup::new(&KEY, &[], 4096, 1024).is_ok());
    let all_lanes: Vec<u8> = (0x11..=0x19).chain(0x21..=0x29).collect();
    let all_words: Vec<u32> = all_lanes
        .iter()
        .flat_map(|lane| [u32::from(*lane), 0, 0, 0, 0, 7, 4])
        .collect();
    let fanout = PhysicalInputSetup::new(&all_words, &all_lanes, 4096, 1024).unwrap();
    let event = button(0, PhysicalControlId::keyboard(4));
    assert_eq!(
        fanout
            .bindings
            .map(&event)
            .map(|event| event.game_control)
            .collect::<Vec<_>>(),
        all_lanes
            .iter()
            .map(|lane| GameControlId(u32::from(*lane)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn independent_encoded_and_payload_budgets_apply_at_exact_boundaries_without_fallback() {
    for (encoded, payload) in [
        (0, 0),
        (5, 0),
        (6, 7),
        (1_048_577, 0),
        (u32::MAX, 0),
        (4096, u32::MAX),
    ] {
        assert!(PhysicalInputSetup::new(&KEY, &[0x11], encoded, payload).is_err());
    }
    for (encoded, payload) in [(6, 0), (6, 6), (1_048_576, 1_048_576)] {
        let setup = PhysicalInputSetup::new(&KEY, &[0x11], encoded, payload).unwrap();
        assert_eq!(setup.limits.max_encoded_bytes(), encoded as usize);
        assert_eq!(setup.limits.max_payload_bytes(), payload as usize);
    }
    let event = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta: meta(u64::MAX, 0),
        report_id: Some(0xff),
        data: vec![0, 0x80, 0xff],
    });
    let bytes = encode_event(&event, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    let exact = PhysicalInputSetup::new(&KEY, &[0x11], bytes.len() as u32, 3).unwrap();
    assert_eq!(decode_input(&bytes, exact.limits, HOST).unwrap(), event);
    let small_encoded = PhysicalInputSetup::new(&KEY, &[0x11], bytes.len() as u32 - 1, 3).unwrap();
    let small_payload = PhysicalInputSetup::new(&KEY, &[0x11], bytes.len() as u32, 2).unwrap();
    assert!(decode_input(&bytes, small_encoded.limits, HOST).is_err());
    assert!(decode_input(&bytes, small_payload.limits, HOST).is_err());
    assert_eq!(decode_input(&bytes, exact.limits, HOST).unwrap(), event);
    let empty = PhysicalInputEvent::Custom(CustomInputEvent {
        meta: meta(0, 0),
        namespace: VendorNamespaceId(u32::MAX),
        type_id: u32::MAX,
        payload: vec![],
    });
    let zero_payload = PhysicalInputSetup::new(&KEY, &[0x11], 4096, 0).unwrap();
    let bytes = encode_event(&empty, zero_payload.limits).unwrap();
    assert_eq!(
        decode_input(&bytes, zero_payload.limits, HOST).unwrap(),
        empty
    );
}

#[test]
fn every_physical_variant_and_original_provenance_survive_browser_admission_without_keyboard_conversion()
 {
    let setup = PhysicalInputSetup::new(
        &[
            [0x11, 0, 0, 0, 0, 65_535, 65_535],
            [0x12, 0, 0, 0, 1, u32::MAX, u32::MAX],
            [0x13, 0, 0, 0, 2, 0x8000_0000, u32::MAX],
        ]
        .concat(),
        &[0x11, 0x12, 0x13],
        4096,
        1024,
    )
    .unwrap();
    let provenance = EventMeta {
        native: Some(NativeEventMeta {
            backend: BackendId(u32::MAX),
            code: Some(u32::MAX),
            timestamp: Some(ClockPoint {
                domain: ClockDomainId(u32::MAX),
                timestamp: Timestamp::MIN,
            }),
        }),
        original_clock_point: Some(ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(-99),
        }),
        ..meta(u64::MAX, i64::MAX)
    };
    let hid = PhysicalControlId::HidUsage {
        usage_page: u16::MAX,
        usage: u16::MAX,
    };
    let native = PhysicalControlId::Native {
        backend: BackendId(u32::MAX),
        code: u32::MAX,
    };
    let vendor = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(0x8000_0000),
        code: u32::MAX,
    };
    let mut events = Vec::new();
    for state in [ButtonState::Down, ButtonState::Up, ButtonState::Repeat] {
        events.push(PhysicalInputEvent::Button(ButtonEvent {
            meta: provenance,
            control: hid,
            state,
        }));
    }
    for mode in [AxisMode::Absolute, AxisMode::Relative] {
        events.push(PhysicalInputEvent::Axis(AxisEvent {
            meta: provenance,
            control: native,
            value: f32::from_bits(0x8000_0000),
            mode,
        }));
    }
    for phase in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Up,
        TouchPhase::Cancel,
    ] {
        events.push(PhysicalInputEvent::Touch(TouchEvent {
            meta: provenance,
            control: vendor,
            contact: ContactId(u64::MAX),
            phase,
            position: Position2 {
                x: -32.5,
                y: f32::from_bits(1),
            },
            pressure: Some(f32::from_bits(0x7fc0_1234)),
        }));
    }
    for mode in [PointerMode::Absolute, PointerMode::Relative] {
        events.push(PhysicalInputEvent::Pointer(PointerEvent {
            meta: provenance,
            control: native,
            position: Position2 {
                x: 65536.5,
                y: -0.0,
            },
            mode,
        }));
    }
    events.push(PhysicalInputEvent::Pose(PoseEvent {
        meta: provenance,
        control: vendor,
        position: Position3 {
            x: -1.0,
            y: 2.0,
            z: 3.0,
        },
        orientation: Quaternion {
            x: 0.0,
            y: -0.0,
            z: 8.0,
            w: -2.0,
        },
    }));
    events.push(PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta: provenance,
        report_id: Some(0),
        data: vec![0, 1, 0xfe, 0xff],
    }));
    events.push(PhysicalInputEvent::Custom(CustomInputEvent {
        meta: provenance,
        namespace: VendorNamespaceId(u32::MAX),
        type_id: u32::MAX,
        payload: vec![0xff, 0, 0xff],
    }));
    for event in events {
        let bytes = encode_event(&event, setup.limits).unwrap();
        let decoded = decode_input(&bytes, setup.limits, HOST).unwrap();
        assert_eq!(decoded.meta(), &provenance);
        assert_eq!(
            std::mem::discriminant(&decoded),
            std::mem::discriminant(&event)
        );
        assert_eq!(
            encode_event(&decoded, setup.limits).unwrap(),
            bytes,
            "all coordinate, pressure, quaternion and opaque payload bits remain unchanged"
        );
        let mapped: Vec<_> = setup.bindings.map(&decoded).collect();
        if matches!(
            event,
            PhysicalInputEvent::RawHidReport(_) | PhysicalInputEvent::Custom(_)
        ) {
            assert!(
                mapped.is_empty(),
                "opaque reports do not become fabricated key bindings"
            );
        } else {
            assert_eq!(mapped.len(), 1);
            assert_eq!(
                encode_event(&mapped[0].physical, setup.limits).unwrap(),
                bytes
            );
        }
    }
}

#[test]
fn canonical_host_button_and_malformed_extents_use_the_actual_versioned_codec() {
    let setup = PhysicalInputSetup::new(&KEY, &[0x11], 4096, 1024).unwrap();
    let literal: [u8; 43] = [
        b'B', b'K', b'P', b'I', 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0x4e,
        0x49, 0x57, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0, 4, 0, 0,
    ];
    let expected = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta {
            sequence: 2,
            ..meta(1, 5)
        },
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    });
    assert_eq!(encode_event(&expected, setup.limits).unwrap(), literal);
    assert_eq!(decode_event(&literal, setup.limits).unwrap(), expected);
    assert_eq!(
        decode_input(&literal, setup.limits, HOST).unwrap(),
        expected
    );
    for length in 0..literal.len() {
        assert!(decode_input(&literal[..length], setup.limits, HOST).is_err());
    }
    for index in [0, 4, 6, 35, 36, 37, 42] {
        let mut invalid = literal;
        invalid[index] = 0xff;
        assert!(decode_input(&invalid, setup.limits, HOST).is_err());
    }
    let mut trailing = literal.to_vec();
    trailing.push(0);
    assert!(decode_input(&trailing, setup.limits, HOST).is_err());
    assert!(decode_input(&[literal, literal].concat(), setup.limits, HOST).is_err());
    assert_eq!(
        decode_input(&literal, setup.limits, HOST).unwrap(),
        expected
    );
}

#[test]
fn normalized_host_time_is_strict_while_original_clock_and_acquisition_identity_remain_lossless() {
    let setup = PhysicalInputSetup::new(&KEY, &[0x11], 4096, 1024).unwrap();
    for nanos in [0, 1, 604_800_000_000_001, 9_007_199_254_740_993, i64::MAX] {
        let mut event = button(u64::MAX, PhysicalControlId::keyboard(4));
        *event.meta_mut() = EventMeta {
            native: Some(NativeEventMeta {
                backend: BackendId(55),
                code: None,
                timestamp: Some(ClockPoint {
                    domain: ClockDomainId(0),
                    timestamp: Timestamp::MIN,
                }),
            }),
            original_clock_point: Some(ClockPoint {
                domain: ClockDomainId(u32::MAX),
                timestamp: Timestamp::from_nanos(-1),
            }),
            ..meta(u64::MAX, nanos)
        };
        let bytes = encode_event(&event, setup.limits).unwrap();
        assert_eq!(decode_input(&bytes, setup.limits, HOST).unwrap(), event);
        assert!(decode_input(&bytes, setup.limits, ClockDomainId(HOST.0 + 1)).is_err());
    }
    for nanos in [-1, i64::MIN] {
        let mut event = button(0, PhysicalControlId::keyboard(4));
        event.meta_mut().timestamp = Timestamp::from_nanos(nanos);
        let bytes = encode_event(&event, setup.limits).unwrap();
        assert_eq!(
            decode_event(&bytes, setup.limits).unwrap(),
            event,
            "the canonical codec retains signed time; browser acquisition imposes the host boundary"
        );
        assert!(decode_input(&bytes, setup.limits, HOST).is_err());
    }
    for domain in [ClockDomainId(0), ClockDomainId(u32::MAX)] {
        let mut event = button(0, PhysicalControlId::keyboard(4));
        event.meta_mut().clock_domain = domain;
        event.meta_mut().timestamp = Timestamp::ZERO;
        event.meta_mut().sequence = 0;
        let bytes = encode_event(&event, setup.limits).unwrap();
        assert!(decode_input(&bytes, setup.limits, HOST).is_err());
        assert_eq!(
            decode_input(&bytes, setup.limits, domain).unwrap(),
            event,
            "admission follows the explicit configured domain without inventing a clock mapping"
        );
    }
}

#[test]
fn browser_keyboard_javascript_golden_packet_decodes_to_native_acquisition_and_common_binding() {
    // The independent physical-input.test.mjs golden uses these same 69 bytes.
    let literal: [u8; 69] = [
        66, 75, 80, 73, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0x4e, 0x49, 0x57,
        0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 1, 0x59, 0x45, 0x4b, 0x57, 1, 0x34,
        0x12, 0, 0, 1, 0x4e, 0x49, 0x57, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 0x59, 0x45, 0x4b, 0x57,
        0x34, 0x12, 0, 0, 0,
    ];
    let point = ClockPoint {
        domain: HOST,
        timestamp: Timestamp::from_nanos(0x0102_0304_0506_0708),
    };
    let expected = PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta {
            source: DeviceId(1),
            timestamp: point.timestamp,
            clock_domain: HOST,
            sequence: 0x8877_6655_4433_2211,
            native: Some(NativeEventMeta {
                backend: BackendId(0x574b_4559),
                code: Some(0x1234),
                timestamp: Some(point),
            }),
            original_clock_point: None,
        },
        control: PhysicalControlId::Native {
            backend: BackendId(0x574b_4559),
            code: 0x1234,
        },
        state: ButtonState::Down,
    });
    let setup = PhysicalInputSetup::new(
        &[0x11, 0, 0, 0, 1, 0x574b_4559, 0x1234],
        &[0x11],
        4096,
        1024,
    )
    .unwrap();
    assert_eq!(encode_event(&expected, setup.limits).unwrap(), literal);
    let decoded = decode_input(&literal, setup.limits, HOST).unwrap();
    assert_eq!(decoded, expected);
    let bound: Vec<_> = setup.bindings.map(&decoded).collect();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].game_control, GameControlId(0x11));
    assert_eq!(bound[0].physical, expected);
}
