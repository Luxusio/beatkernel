use beatkernel::{
    input::{
        BackendId, Binding, BindingMap, ButtonState, DeviceId, DeviceSelector, GameControlId,
        PhysicalControlId, PhysicalInputEvent,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
};
use beatkernel_platform::raw_input::*;

const OUT: ClockDomainId = ClockDomainId(1);

struct NoMap;
impl ClockMapper for NoMap {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: OUT,
        timestamp: Timestamp::from_nanos(ns),
    }
}

fn packet(layout: RawInputLayout, kind: u32, handle: u64, code: u64, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&kind.to_le_bytes());
    bytes.extend_from_slice(&((layout.header_size() + body.len()) as u32).to_le_bytes());
    match layout {
        RawInputLayout::Win32 => {
            bytes.extend_from_slice(&(handle as u32).to_le_bytes());
            bytes.extend_from_slice(&(code as u32).to_le_bytes());
        }
        RawInputLayout::Win64 => {
            bytes.extend_from_slice(&handle.to_le_bytes());
            bytes.extend_from_slice(&code.to_le_bytes());
        }
    }
    bytes.extend_from_slice(body);
    bytes
}

fn keyboard(handle: u64, make: u16, flags: u16, vkey: u16) -> Vec<u8> {
    let mut body = Vec::new();
    for field in [make, flags, 0xf00d, vkey] {
        body.extend_from_slice(&field.to_le_bytes());
    }
    body.extend_from_slice(&0x1020_3040u32.to_le_bytes());
    body.extend_from_slice(&0xfedc_ba98u32.to_le_bytes());
    packet(RawInputLayout::Win64, 1, handle, 1, &body)
}

fn hid(handle: u64, size: u32, count: u32, data: &[u8]) -> Vec<u8> {
    let mut body = size.to_le_bytes().to_vec();
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(data);
    packet(RawInputLayout::Win64, 2, handle, 0, &body)
}

fn process(
    processor: &mut RawInputProcessor,
    bytes: &[u8],
    ns: i64,
) -> Result<InputBatch, RawInputError> {
    processor.process(
        &RawInputPacket::parse(bytes, RawInputLayout::Win64)?,
        point(ns),
        &NoMap,
    )
}

fn button(batch: &InputBatch) -> (DeviceId, PhysicalControlId, ButtonState) {
    assert_eq!(batch.status, RawInputStatus::Events);
    assert_eq!(batch.events.len(), 1);
    let PhysicalInputEvent::Button(event) = &batch.events[0] else {
        panic!("expected button")
    };
    (event.meta.source, event.control, event.state)
}

#[test]
fn headers_and_every_keyboard_field_are_lossless_for_both_layouts() {
    let body = &keyboard(1, 0xabcd, 0xfe01, 0x1234)[24..];
    for layout in [RawInputLayout::Win32, RawInputLayout::Win64] {
        let (handle, code) = match layout {
            RawInputLayout::Win32 => (0xfedc_ba98, 0x89ab_cdef),
            RawInputLayout::Win64 => (0xfedc_ba98_7654_3210, 0x89ab_cdef_0123_4567),
        };
        let bytes = packet(layout, 1, handle, code, body);
        let decoded = RawInputPacket::parse(&bytes, layout).unwrap();
        assert_eq!(
            decoded.header(),
            RawInputHeader {
                kind: 1,
                size: (layout.header_size() + 16) as u32,
                device_handle: handle,
                input_code: code,
            }
        );
        let RawInputData::Keyboard(key) = decoded.data() else {
            panic!("keyboard")
        };
        assert_eq!(
            key,
            RawKeyboard {
                make_code: 0xabcd,
                flags: 0xfe01,
                reserved: 0xf00d,
                virtual_key: 0x1234,
                message: 0x1020_3040,
                extra_information: 0xfedc_ba98,
            }
        );
        assert_eq!(key.native_code(), 0xfe01_abcd);
    }
}

#[test]
fn every_short_header_and_keyboard_body_is_rejected_without_a_cast() {
    for layout in [RawInputLayout::Win32, RawInputLayout::Win64] {
        for length in 0..layout.header_size() {
            assert_eq!(
                RawInputPacket::parse(&vec![0; length], layout),
                Err(RawInputError::Truncated)
            );
        }
        for length in 0..16 {
            let bytes = packet(layout, 1, 1, 0, &vec![0; length]);
            assert_eq!(
                RawInputPacket::parse(&bytes, layout),
                Err(RawInputError::Truncated)
            );
        }
        let mut bytes = packet(layout, 1, 1, 0, &[0; 16]);
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            RawInputPacket::parse(&bytes, layout),
            Err(RawInputError::InvalidPacketSize)
        );
        for kind in [0, 3, u32::MAX] {
            let bytes = packet(layout, kind, 1, 0, &[0; 24]);
            assert_eq!(
                RawInputPacket::parse(&bytes, layout),
                Err(RawInputError::UnsupportedType(kind))
            );
        }
        let bytes = packet(layout, 1, 1, 0, &[0; 24]);
        assert!(matches!(
            RawInputPacket::parse(&bytes, layout).unwrap().data(),
            RawInputData::Keyboard(_)
        ));
    }
    assert_eq!(
        RawInputPacket::parse(&vec![0; MAX_RAW_INPUT_BYTES + 1], RawInputLayout::Win64),
        Err(RawInputError::PacketTooLarge)
    );
    let bytes = packet(
        RawInputLayout::Win64,
        1,
        1,
        0,
        &vec![0; MAX_RAW_INPUT_BYTES - 24],
    );
    assert!(RawInputPacket::parse(&bytes, RawInputLayout::Win64).is_ok());
}

#[test]
fn hid_sizes_and_output_amplification_are_bounded_before_state_change() {
    for (size, count) in [(0, 1), (1, 0), (0, 0)] {
        assert_eq!(
            RawInputPacket::parse(&hid(1, size, count, &[]), RawInputLayout::Win64),
            Err(RawInputError::InvalidHidSize)
        );
    }
    for length in 0..8 {
        let bytes = packet(RawInputLayout::Win64, 2, 1, 0, &vec![0; length]);
        assert_eq!(
            RawInputPacket::parse(&bytes, RawInputLayout::Win64),
            Err(RawInputError::Truncated)
        );
    }
    for count in [MAX_HID_REPORTS as u32 + 1, u32::MAX] {
        assert_eq!(
            RawInputPacket::parse(&hid(1, 1, count, &[]), RawInputLayout::Win64),
            Err(RawInputError::TooManyReports(count as usize))
        );
    }
    assert_eq!(
        RawInputPacket::parse(&hid(1, u32::MAX, 1, &[]), RawInputLayout::Win64),
        Err(RawInputError::Truncated)
    );
    assert_eq!(
        RawInputPacket::parse(&hid(1, 3, 2, &[1, 2, 3, 4, 5]), RawInputLayout::Win64),
        Err(RawInputError::Truncated)
    );
    let mut processor = RawInputProcessor::new(OUT);
    processor
        .register_device(1, RawDeviceInfo::new(RawDeviceKind::Hid))
        .unwrap();
    assert_eq!(
        process(&mut processor, &hid(1, 1, 4097, &[]), 0),
        Err(RawInputError::TooManyReports(4097))
    );
    let batch = process(&mut processor, &hid(1, 1, 4096, &vec![7; 4096]), 0).unwrap();
    assert_eq!(batch.sequence, 1);
    assert_eq!(batch.events.len(), 4096);
    assert!(batch.events.iter().all(|event| event.meta().sequence == 1));
}

#[test]
fn hid_batches_preserve_opaque_ids_padding_and_owned_lifetimes() {
    let mut processor = RawInputProcessor::new(OUT);
    let id = processor
        .register_device(44, RawDeviceInfo::new(RawDeviceKind::Hid))
        .unwrap();
    let batch = {
        let bytes = hid(44, 3, 2, &[9, 0xaa, 0xbb, 0, 0xcc, 0xdd, 0xee, 0xff]);
        let decoded = RawInputPacket::parse(&bytes, RawInputLayout::Win64).unwrap();
        let RawInputData::Hid(reports) = decoded.data() else {
            panic!("HID")
        };
        assert_eq!(reports.report_count(), 2);
        assert_eq!(reports.report_size(), 3);
        assert_eq!(
            reports.reports().collect::<Vec<_>>(),
            [&[9, 0xaa, 0xbb][..], &[0, 0xcc, 0xdd][..]]
        );
        process(&mut processor, &bytes, -20).unwrap()
    };
    processor.unregister_device(44).unwrap();
    for (event, wire) in batch.events.iter().zip([[9, 0xaa, 0xbb], [0, 0xcc, 0xdd]]) {
        let PhysicalInputEvent::RawHidReport(report) = event else {
            panic!("report")
        };
        assert_eq!(report.report_id, None);
        assert_eq!(report.data, wire);
        assert_eq!(report.meta.source, id);
        assert_eq!(report.meta.sequence, 1);
        assert_eq!(report.meta.timestamp, Timestamp::from_nanos(-20));
        assert_eq!(report.meta.native.unwrap().timestamp, Some(point(-20)));
        assert_eq!(report.meta.native.unwrap().code, None);
    }
    assert_eq!(processor.devices().count(), 0);
}

#[test]
fn identical_descriptors_stay_distinct_and_reconnect_resets_state() {
    let mut processor = RawInputProcessor::new(OUT);
    let mut info = RawDeviceInfo::new(RawDeviceKind::Keyboard);
    info.vendor_id = Some(0x1234);
    info.product_id = Some(0xabcd);
    info.serial = Some("same optional serial".into());
    info.name = Some("same keyboard".into());
    let first = processor.register_device(11, info.clone()).unwrap();
    let second = processor.register_device(22, info.clone()).unwrap();
    assert_eq!((first, second), (DeviceId(1), DeviceId(2)));
    assert_eq!(processor.register_device(11, info.clone()).unwrap(), first);
    assert_eq!(
        processor.register_device(0, info.clone()),
        Err(RawInputError::InvalidDeviceHandle)
    );
    assert_eq!(
        processor.register_device(11, RawDeviceInfo::new(RawDeviceKind::Hid)),
        Err(RawInputError::DeviceKindMismatch)
    );
    assert_eq!(processor.devices().count(), 2);
    assert!(processor.device(11).unwrap().capabilities.button);
    assert!(!processor.device(11).unwrap().capabilities.raw_hid);
    assert_eq!(processor.device(11).unwrap().serial, info.serial);
    process(&mut processor, &keyboard(11, 0x1e, 0, 0x41), 0).unwrap();
    assert_eq!(processor.unregister_device(11).unwrap().runtime_id, first);
    assert_eq!(processor.unregister_device(11), None);
    assert_eq!(processor.device(11), None);
    assert_eq!(
        process(&mut processor, &keyboard(11, 0x1e, 0, 0x41), 0),
        Err(RawInputError::UnknownDevice(11))
    );
    let third = processor.register_device(11, info).unwrap();
    assert_eq!(third, DeviceId(3));
    let batch = process(&mut processor, &keyboard(11, 0x1e, 0, 0x41), 0).unwrap();
    assert_eq!(
        button(&batch),
        (third, PhysicalControlId::keyboard(4), ButtonState::Down)
    );
    assert_eq!(batch.sequence, 1);
    assert_eq!(
        process(&mut processor, &keyboard(0, 0x1e, 0, 0x41), 0),
        Err(RawInputError::InvalidDeviceHandle)
    );
    assert_eq!(
        process(&mut processor, &hid(22, 1, 1, &[0]), 0),
        Err(RawInputError::DeviceKindMismatch)
    );
}

#[test]
fn make_repeat_break_are_per_source_and_native_break_identity_is_retained() {
    let mut processor = RawInputProcessor::new(OUT);
    for handle in [11, 22] {
        processor
            .register_device(handle, RawDeviceInfo::new(RawDeviceKind::Keyboard))
            .unwrap();
    }
    for (handle, flags, expected, sequence) in [
        (11, 0, ButtonState::Down, 1),
        (22, 0, ButtonState::Down, 1),
        (11, 0, ButtonState::Repeat, 2),
        (11, 1, ButtonState::Up, 3),
        (11, 1, ButtonState::Up, 4),
        (11, 0, ButtonState::Down, 5),
    ] {
        let batch = process(
            &mut processor,
            &keyboard(handle, 0x1e, flags, 0x41),
            100 - sequence,
        )
        .unwrap();
        assert_eq!(button(&batch).1, PhysicalControlId::keyboard(4));
        assert_eq!(button(&batch).2, expected);
        assert_eq!(batch.sequence, sequence as u64);
        let meta = batch.events[0].meta();
        assert_eq!(meta.native.unwrap().backend, BackendId(4));
        assert_eq!(
            meta.native.unwrap().code,
            Some((u32::from(flags) << 16) | 0x1e)
        );
        assert_eq!(meta.original_clock_point, None);
    }
}

#[test]
fn unsupported_flags_prefixes_and_scan_zero_have_lossless_distinct_fallbacks() {
    let mut processor = RawInputProcessor::new(OUT);
    processor
        .register_device(1, RawDeviceInfo::new(RawDeviceKind::Keyboard))
        .unwrap();
    for (make, flags, vkey, expected) in [
        (0x1c, 2, 0x0d, PhysicalControlId::keyboard(0x58)),
        (0x1d, 2, 0x11, PhysicalControlId::keyboard(0xe4)),
        (
            0x8888,
            0,
            1,
            PhysicalControlId::Native {
                backend: BackendId(1),
                code: 0x8888,
            },
        ),
        (
            0x1e,
            6,
            0x41,
            PhysicalControlId::Native {
                backend: BackendId(4),
                code: 0x0006_001e,
            },
        ),
        (
            0x1e,
            0x8000,
            0x41,
            PhysicalControlId::Native {
                backend: BackendId(4),
                code: 0x8000_001e,
            },
        ),
        (
            0,
            0,
            0xb0,
            PhysicalControlId::Native {
                backend: BackendId(5),
                code: 0xb0,
            },
        ),
        (
            0,
            2,
            0xb0,
            PhysicalControlId::Native {
                backend: BackendId(5),
                code: 0x0002_00b0,
            },
        ),
        (
            0,
            0,
            0xb1,
            PhysicalControlId::Native {
                backend: BackendId(5),
                code: 0xb1,
            },
        ),
    ] {
        let down = process(&mut processor, &keyboard(1, make, flags, vkey), 0).unwrap();
        let up = process(&mut processor, &keyboard(1, make, flags | 1, vkey), 0).unwrap();
        assert_eq!(button(&down).1, expected);
        assert_eq!(button(&up).1, expected);
        assert_eq!(button(&down).2, ButtonState::Down);
        assert_eq!(button(&up).2, ButtonState::Up);
    }
}

#[test]
fn pause_assembly_is_source_scoped_completion_timed_and_never_synthesizes_up() {
    let mut processor = RawInputProcessor::new(OUT);
    for handle in [11, 22] {
        processor
            .register_device(handle, RawDeviceInfo::new(RawDeviceKind::Keyboard))
            .unwrap();
    }
    for pulse in 0..2 {
        let header = process(&mut processor, &keyboard(11, 0x1d, 4, 0x13), 10 + pulse).unwrap();
        assert!(header.events.is_empty());
        assert_eq!(header.status, RawInputStatus::PendingPause);
        let other = process(&mut processor, &keyboard(22, 0x45, 0, 0x90), 20).unwrap();
        assert_eq!(button(&other).1, PhysicalControlId::keyboard(0x53));
        let completed = process(&mut processor, &keyboard(11, 0x45, 0, 0x13), 30 + pulse).unwrap();
        assert_eq!(
            button(&completed),
            (
                DeviceId(1),
                PhysicalControlId::keyboard(0x48),
                ButtonState::Down
            )
        );
        assert_eq!(completed.sequence, (pulse * 2 + 2) as u64);
        assert_eq!(completed.events[0].meta().timestamp.as_nanos(), 30 + pulse);
        assert_eq!(completed.events[0].meta().native.unwrap().code, Some(0x45));
    }
    for make in [0x45, 0x1d45] {
        let down = process(&mut processor, &keyboard(11, make, 4, 0x13), 40).unwrap();
        assert_eq!(button(&down).2, ButtonState::Down);
        assert_eq!(button(&down).1, PhysicalControlId::keyboard(0x48));
        let up = process(&mut processor, &keyboard(11, make, 5, 0x13), 41).unwrap();
        assert_eq!(button(&up).2, ButtonState::Up);
    }
}

#[test]
fn interrupted_filtered_repeated_and_retired_pause_headers_follow_the_contract() {
    let mut processor = RawInputProcessor::new(OUT);
    processor
        .register_device(11, RawDeviceInfo::new(RawDeviceKind::Keyboard))
        .unwrap();
    let head = keyboard(11, 0x1d, 4, 0x13);
    process(&mut processor, &head, 0).unwrap();
    let unrelated = process(&mut processor, &keyboard(11, 0x45, 0, 0x90), 1).unwrap();
    assert_eq!(button(&unrelated).1, PhysicalControlId::keyboard(0x53));
    let tail = process(&mut processor, &keyboard(11, 0x45, 0, 0x13), 2).unwrap();
    assert_eq!(button(&tail).1, PhysicalControlId::keyboard(0x53));
    process(&mut processor, &head, 3).unwrap();
    process(&mut processor, &head, 4).unwrap();
    assert_eq!(
        button(&process(&mut processor, &keyboard(11, 0x45, 4, 0x13), 5).unwrap()).1,
        PhysicalControlId::keyboard(0x48)
    );
    process(&mut processor, &head, 6).unwrap();
    let filtered = process(&mut processor, &keyboard(11, 0x2a, 2, 255), 7).unwrap();
    assert_eq!(filtered.status, RawInputStatus::FilteredKeyboard);
    assert!(filtered.events.is_empty());
    assert_eq!(
        button(&process(&mut processor, &keyboard(11, 0x45, 0, 0x13), 8).unwrap()).1,
        PhysicalControlId::keyboard(0x53)
    );
    process(&mut processor, &head, 9).unwrap();
    processor.unregister_device(11).unwrap();
    processor
        .register_device(11, RawDeviceInfo::new(RawDeviceKind::Keyboard))
        .unwrap();
    let tail = process(&mut processor, &keyboard(11, 0x45, 0, 0x13), 10).unwrap();
    assert_eq!(button(&tail).1, PhysicalControlId::keyboard(0x53));
    assert_eq!(tail.sequence, 1);
}

#[test]
fn rejected_clock_and_overrun_leave_sequence_held_and_assembly_state_unchanged() {
    let mut processor = RawInputProcessor::new(OUT);
    processor
        .register_device(11, RawDeviceInfo::new(RawDeviceKind::Keyboard))
        .unwrap();
    let key = keyboard(11, 0x1e, 0, 0x41);
    process(&mut processor, &key, 0).unwrap();
    let decoded = RawInputPacket::parse(&key, RawInputLayout::Win64).unwrap();
    assert_eq!(
        processor.process(
            &decoded,
            ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::ZERO
            },
            &NoMap
        ),
        Err(RawInputError::UnmappedClock)
    );
    let retry = process(&mut processor, &key, 1).unwrap();
    assert_eq!(button(&retry).2, ButtonState::Repeat);
    assert_eq!(retry.sequence, 2);
    process(&mut processor, &keyboard(11, 0x1d, 4, 0x13), 2).unwrap();
    assert_eq!(
        process(&mut processor, &keyboard(11, 0xff, 0, 0), 3),
        Err(RawInputError::KeyboardOverrun)
    );
    let completed = process(&mut processor, &keyboard(11, 0x45, 0, 0x13), 4).unwrap();
    assert_eq!(completed.sequence, 4);
    assert_eq!(button(&completed).1, PhysicalControlId::keyboard(0x48));
}

#[test]
fn qpc_mapping_pins_rounding_origin_negative_delta_domains_and_overflow() {
    let mapping = QpcClockMapping::new(3, 2, OUT).unwrap();
    assert_eq!(mapping.frequency(), 3);
    assert_eq!(mapping.origin_counter(), 2);
    assert_eq!(mapping.point(2).unwrap().timestamp.as_nanos(), 666_666_666);
    assert_eq!(
        mapping
            .map(mapping.point(3).unwrap(), OUT)
            .unwrap()
            .as_nanos(),
        333_333_334
    );
    assert_eq!(
        mapping
            .map(mapping.point(1).unwrap(), OUT)
            .unwrap()
            .as_nanos(),
        -333_333_333
    );
    assert_eq!(
        mapping.map(mapping.point(2).unwrap(), OUT),
        Some(Timestamp::ZERO)
    );
    assert_eq!(
        mapping.map(mapping.point(3).unwrap(), ClockDomainId(2)),
        None
    );
    assert_eq!(mapping.map(point(0), WINDOWS_QPC_CLOCK_DOMAIN), None);
    assert_eq!(
        mapping.map(point(-10), OUT),
        Some(Timestamp::from_nanos(-10))
    );
    assert_eq!(
        mapping.map(
            ClockPoint {
                domain: WINDOWS_QPC_CLOCK_DOMAIN,
                timestamp: Timestamp::from_nanos(i64::MIN)
            },
            OUT
        ),
        None
    );
    assert!(matches!(
        QpcClockMapping::new(0, 0, OUT),
        Err(RawInputError::InvalidQpcFrequency)
    ));
    assert!(matches!(
        QpcClockMapping::new(-1, 0, OUT),
        Err(RawInputError::InvalidQpcFrequency)
    ));
    assert!(matches!(
        QpcClockMapping::new(1, -1, OUT),
        Err(RawInputError::InvalidQpcCounter)
    ));
    assert!(matches!(
        QpcClockMapping::new(1, 0, WINDOWS_QPC_CLOCK_DOMAIN),
        Err(RawInputError::ClockDomainCollision)
    ));
    assert!(matches!(
        QpcClockMapping::new(1, i64::MAX, OUT),
        Err(RawInputError::TimestampOverflow)
    ));
    assert_eq!(mapping.point(-1), Err(RawInputError::InvalidQpcCounter));
    assert_eq!(
        mapping.point(i64::MAX),
        Err(RawInputError::TimestampOverflow)
    );
    let boundary = QpcClockMapping::new(1_000_000_000, 0, OUT).unwrap();
    assert_eq!(
        boundary.point(i64::MAX).unwrap().timestamp.as_nanos(),
        i64::MAX
    );
    assert_eq!(mapping.quality(), ClockMappingQuality::Exact);
}

#[test]
fn production_parser_processor_and_binding_keep_two_keys_and_native_provenance() {
    let mapping = QpcClockMapping::new(1_000_000_000, 1000, OUT).unwrap();
    let mut processor = RawInputProcessor::new(OUT);
    let ids = [11, 22].map(|handle| {
        processor
            .register_device(handle, RawDeviceInfo::new(RawDeviceKind::Keyboard))
            .unwrap()
    });
    let bindings =
        BindingMap::from_bindings(ids.into_iter().zip([10, 20]).map(|(id, game)| Binding {
            device: DeviceSelector::Exact(id),
            physical: PhysicalControlId::keyboard(4),
            game_control: GameControlId(game),
        }))
        .unwrap();
    for (handle, expected) in [(11, 10), (22, 20)] {
        let bytes = keyboard(handle, 0x1e, 0, 0x41);
        let receipt = mapping.point(1100).unwrap();
        let packet = RawInputPacket::parse(&bytes, RawInputLayout::Win64).unwrap();
        let batch = processor.process(&packet, receipt, &mapping).unwrap();
        let mapped = bindings.map(&batch.events[0]).collect::<Vec<_>>();
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].game_control, GameControlId(expected));
        assert_eq!(mapped[0].physical, batch.events[0]);
        let meta = mapped[0].physical.meta();
        assert_eq!(meta.timestamp.as_nanos(), 100);
        assert_eq!(meta.clock_domain, OUT);
        assert_eq!(meta.original_clock_point, Some(receipt));
        assert_eq!(meta.native.unwrap().timestamp, Some(receipt));
        assert_eq!(meta.native.unwrap().code, Some(0x1e));
        assert_eq!(meta.native.unwrap().backend, BackendId(4));
    }
}
