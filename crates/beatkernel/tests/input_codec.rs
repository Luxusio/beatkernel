use beatkernel::{input::*, time::*};

fn limits() -> CodecLimits {
    CodecLimits::new(4096, 1024).unwrap()
}
fn simple_meta() -> EventMeta {
    EventMeta::new(
        DeviceId(1),
        ClockPoint {
            domain: ClockDomainId(3),
            timestamp: Timestamp::from_nanos(-1),
        },
        2,
    )
}
fn full_meta() -> EventMeta {
    EventMeta {
        source: DeviceId(u64::MAX),
        timestamp: Timestamp::MIN,
        clock_domain: ClockDomainId(u32::MAX),
        sequence: u64::MAX,
        native: Some(NativeEventMeta {
            backend: BackendId(u32::MAX),
            code: Some(u32::MAX),
            timestamp: Some(ClockPoint {
                domain: ClockDomainId(22),
                timestamp: Timestamp::MAX,
            }),
        }),
        original_clock_point: Some(ClockPoint {
            domain: ClockDomainId(77),
            timestamp: Timestamp::from_nanos(-99),
        }),
    }
}
fn button(meta: EventMeta, control: PhysicalControlId, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control,
        state,
    })
}

#[test]
fn literal_button_wire_schema_is_stable_and_little_endian() {
    let event = button(
        simple_meta(),
        PhysicalControlId::keyboard(4),
        ButtonState::Down,
    );
    let literal: [u8; 43] = [
        b'B', b'K', b'P', b'I', 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255,
        255, 3, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0, 4, 0, 0,
    ];
    assert_eq!(encode_event(&event, limits()).unwrap(), literal);
    assert_eq!(decode_event(&literal, limits()).unwrap(), event);
}

fn fixtures() -> Vec<PhysicalInputEvent> {
    let meta = full_meta();
    let hid = PhysicalControlId::HidUsage {
        usage_page: u16::MAX,
        usage: u16::MAX,
    };
    let native = PhysicalControlId::Native {
        backend: BackendId(u32::MAX),
        code: u32::MAX,
    };
    let vendor = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(33),
        code: 44,
    };
    let mut result = Vec::new();
    for state in [ButtonState::Down, ButtonState::Up, ButtonState::Repeat] {
        for control in [hid, native, vendor] {
            result.push(button(meta, control, state));
        }
    }
    for mode in [AxisMode::Absolute, AxisMode::Relative] {
        result.push(PhysicalInputEvent::Axis(AxisEvent {
            meta,
            control: native,
            value: f32::from_bits(0x80000000),
            mode,
        }));
    }
    for phase in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Up,
        TouchPhase::Cancel,
    ] {
        for pressure in [None, Some(0.75), Some(f32::from_bits(0x7fc12345))] {
            result.push(PhysicalInputEvent::Touch(TouchEvent {
                meta,
                control: vendor,
                contact: ContactId(u64::MAX),
                phase,
                position: Position2 {
                    x: f32::INFINITY,
                    y: f32::NEG_INFINITY,
                },
                pressure,
            }));
        }
    }
    for mode in [PointerMode::Absolute, PointerMode::Relative] {
        result.push(PhysicalInputEvent::Pointer(PointerEvent {
            meta,
            control: hid,
            position: Position2 {
                x: -0.0,
                y: f32::from_bits(0xffc12345),
            },
            mode,
        }));
    }
    result.push(PhysicalInputEvent::Pose(PoseEvent {
        meta,
        control: vendor,
        position: Position3 {
            x: f32::MIN,
            y: f32::MAX,
            z: f32::from_bits(1),
        },
        orientation: Quaternion {
            x: f32::from_bits(0x7fa12345),
            y: -0.0,
            z: f32::INFINITY,
            w: f32::NEG_INFINITY,
        },
    }));
    for report_id in [None, Some(0), Some(255)] {
        for data in [vec![], vec![0, 1, 255, 128], (0..=255).collect::<Vec<_>>()] {
            result.push(PhysicalInputEvent::RawHidReport(RawHidReportEvent {
                meta,
                report_id,
                data,
            }));
        }
    }
    result.push(PhysicalInputEvent::Custom(CustomInputEvent {
        meta,
        namespace: VendorNamespaceId(u32::MAX),
        type_id: u32::MAX,
        payload: vec![1, 0, 2, 0, 255],
    }));
    result
}

#[test]
fn all_variants_control_forms_and_provenance_round_trip_exactly() {
    for event in fixtures() {
        let encoded = encode_event(&event, limits()).unwrap();
        let decoded = decode_event(&encoded, limits()).unwrap();
        assert_eq!(decoded.meta(), event.meta());
        // Byte equality checks NaN payloads which cannot use f32 PartialEq.
        assert_eq!(encode_event(&decoded, limits()).unwrap(), encoded);
    }
}

#[test]
fn every_metadata_option_combination_remains_distinct() {
    for native_present in [false, true] {
        for code in [None, Some(0), Some(u32::MAX)] {
            for timestamp in [
                None,
                Some(ClockPoint {
                    domain: ClockDomainId(3),
                    timestamp: Timestamp::MIN,
                }),
            ] {
                for origin in [
                    None,
                    Some(ClockPoint {
                        domain: ClockDomainId(5),
                        timestamp: Timestamp::MAX,
                    }),
                ] {
                    let mut meta = simple_meta();
                    meta.native = native_present.then_some(NativeEventMeta {
                        backend: BackendId(7),
                        code,
                        timestamp,
                    });
                    meta.original_clock_point = origin;
                    let event = button(meta, PhysicalControlId::keyboard(4), ButtonState::Up);
                    let decoded =
                        decode_event(&encode_event(&event, limits()).unwrap(), limits()).unwrap();
                    assert_eq!(decoded, event);
                }
            }
        }
    }
}

#[test]
fn uninterpreted_axis_values_preserve_all_selected_ieee_bits() {
    for bits in [
        0, 0x80000000, 1, 0x007fffff, 0x7f7fffff, 0xff7fffff, 0x7f800000, 0xff800000, 0x7fc12345,
        0x7fa12345, 0xffa12345,
    ] {
        let event = PhysicalInputEvent::Axis(AxisEvent {
            meta: simple_meta(),
            control: PhysicalControlId::keyboard(4),
            value: f32::from_bits(bits),
            mode: AxisMode::Relative,
        });
        let PhysicalInputEvent::Axis(decoded) =
            decode_event(&encode_event(&event, limits()).unwrap(), limits()).unwrap()
        else {
            panic!("axis variant lost");
        };
        assert_eq!(decoded.value.to_bits(), bits);
    }
}

#[test]
fn every_truncated_prefix_fails_and_extra_bytes_are_rejected() {
    for event in fixtures() {
        let bytes = encode_event(&event, limits()).unwrap();
        for end in 0..bytes.len() {
            assert!(
                decode_event(&bytes[..end], limits()).is_err(),
                "accepted prefix {end}"
            );
        }
        let mut extra = bytes;
        extra.push(0);
        assert_eq!(
            decode_event(&extra, limits()),
            Err(InputCodecError::TrailingBytes)
        );
    }
}

#[test]
fn unknown_versions_magic_and_strict_discriminants_do_not_fallback() {
    let canonical = encode_event(
        &button(
            simple_meta(),
            PhysicalControlId::keyboard(4),
            ButtonState::Down,
        ),
        limits(),
    )
    .unwrap();
    let mut changed = canonical.clone();
    changed[0] ^= 1;
    assert_eq!(
        decode_event(&changed, limits()),
        Err(InputCodecError::InvalidMagic)
    );
    changed = canonical.clone();
    changed[4..6].copy_from_slice(&65535u16.to_le_bytes());
    assert_eq!(
        decode_event(&changed, limits()),
        Err(InputCodecError::UnsupportedVersion(65535))
    );
    for (index, field) in [
        (6, "event"),
        (35, "option"),
        (36, "option"),
        (37, "control"),
        (42, "button state"),
    ] {
        changed = canonical.clone();
        changed[index] = 255;
        assert_eq!(
            decode_event(&changed, limits()),
            Err(InputCodecError::InvalidTag { field, tag: 255 })
        );
    }
    for event in [
        PhysicalInputEvent::Axis(AxisEvent {
            meta: simple_meta(),
            control: PhysicalControlId::keyboard(4),
            value: 0.0,
            mode: AxisMode::Absolute,
        }),
        PhysicalInputEvent::Pointer(PointerEvent {
            meta: simple_meta(),
            control: PhysicalControlId::keyboard(4),
            position: Position2 { x: 0.0, y: 0.0 },
            mode: PointerMode::Absolute,
        }),
    ] {
        let mut bytes = encode_event(&event, limits()).unwrap();
        *bytes.last_mut().unwrap() = 2;
        assert!(matches!(
            decode_event(&bytes, limits()),
            Err(InputCodecError::InvalidTag { tag: 2, .. })
        ));
    }
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: simple_meta(),
        control: PhysicalControlId::keyboard(4),
        contact: ContactId(0),
        phase: TouchPhase::Down,
        position: Position2 { x: 0.0, y: 0.0 },
        pressure: None,
    });
    let mut bytes = encode_event(&touch, limits()).unwrap();
    bytes[50] = 4;
    assert_eq!(
        decode_event(&bytes, limits()),
        Err(InputCodecError::InvalidTag {
            field: "touch phase",
            tag: 4
        })
    );
}

#[test]
fn hostile_payload_lengths_are_bounded_before_allocating() {
    let event = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta: simple_meta(),
        report_id: None,
        data: vec![1, 2],
    });
    let canonical = encode_event(&event, limits()).unwrap();
    let mut changed = canonical.clone();
    changed[38..46].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        decode_event(&changed, limits()),
        Err(InputCodecError::PayloadTooLarge | InputCodecError::LengthOverflow)
    ));
    changed = canonical.clone();
    changed[38..46].copy_from_slice(&1000u64.to_le_bytes());
    assert_eq!(
        decode_event(&changed, limits()),
        Err(InputCodecError::Truncated)
    );
    assert_eq!(
        encode_event(&event, CodecLimits::new(100, 1).unwrap()),
        Err(InputCodecError::PayloadTooLarge)
    );
    assert_eq!(
        decode_event(&canonical, CodecLimits::new(100, 1).unwrap()),
        Err(InputCodecError::PayloadTooLarge)
    );
    assert_eq!(
        encode_event(&event, CodecLimits::new(46, 2).unwrap()),
        Err(InputCodecError::EncodedTooLarge)
    );
    assert_eq!(
        decode_event(&canonical, CodecLimits::new(46, 2).unwrap()),
        Err(InputCodecError::EncodedTooLarge)
    );
    let exact = CodecLimits::new(canonical.len(), 2).unwrap();
    assert_eq!(encode_event(&event, exact).unwrap(), canonical);
    assert_eq!(decode_event(&canonical, exact).unwrap(), event);
    assert_eq!(CodecLimits::new(5, 0), Err(InputCodecError::InvalidLimits));
    assert_eq!(
        CodecLimits::new(10, 11),
        Err(InputCodecError::InvalidLimits)
    );
}
