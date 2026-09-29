//! Canonical-input contract tests derived from REQ__canonical-input.md.

use std::cell::Cell;
use std::collections::HashSet;

use beatkernel::input::{
    AxisEvent, AxisMode, BackendId, ButtonEvent, ButtonState, ContactId, CustomInputEvent,
    DeviceAdapter, DeviceCapabilities, DeviceDescriptor, DeviceId, DeviceTransport, EventMeta,
    NativeEventMeta, PhysicalControlId, PhysicalInputEvent, PhysicalInputSink, PointerEvent,
    PointerMode, PoseEvent, Position2, Position3, Quaternion, RawHidReportEvent, TouchEvent,
    TouchPhase, VendorNamespaceId, VirtualInputBackend, VirtualInputError, KEYBOARD_USAGE_PAGE,
};
use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp};

const OUTPUT: ClockDomainId = ClockDomainId(1);
const INPUT: ClockDomainId = ClockDomainId(2);
const DEEP_NATIVE: ClockDomainId = ClockDomainId(3);

fn point(domain: ClockDomainId, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn meta(device: u64, nanos: i64, sequence: u64) -> EventMeta {
    EventMeta::new(DeviceId(device), point(OUTPUT, nanos), sequence)
}

fn descriptor(id: u64) -> DeviceDescriptor {
    DeviceDescriptor {
        runtime_id: DeviceId(id),
        vendor_id: Some(0x1234),
        product_id: Some(0x5678),
        serial: Some("same-serial".into()),
        name: Some("fixture keyboard".into()),
        transport: DeviceTransport::Usb,
        capabilities: DeviceCapabilities {
            button: true,
            ..DeviceCapabilities::default()
        },
    }
}

fn button(device: u64, nanos: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(device, nanos, sequence),
        control: PhysicalControlId::keyboard(0x04),
        state: ButtonState::Down,
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

struct OffsetMapper {
    offset: i64,
    calls: Cell<u32>,
}

impl OffsetMapper {
    fn new(offset: i64) -> Self {
        Self {
            offset,
            calls: Cell::new(0),
        }
    }
}

impl ClockMapper for OffsetMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        self.calls.set(self.calls.get() + 1);
        if from.domain != INPUT || to != OUTPUT {
            return None;
        }
        from.timestamp
            .as_nanos()
            .checked_add(self.offset)
            .map(Timestamp::from_nanos)
    }

    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

struct MustNotMap;

impl ClockMapper for MustNotMap {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        panic!("same-domain input must bypass clock mapping")
    }

    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn all_payloads() -> Vec<PhysicalInputEvent> {
    let vendor_control = PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(42),
        code: 9,
    };
    let native_control = PhysicalControlId::Native {
        backend: BackendId(17),
        code: 0xfedc_ba98,
    };
    let common = meta(1, -50, 8);
    let mut events = Vec::new();
    for state in [ButtonState::Down, ButtonState::Repeat, ButtonState::Up] {
        events.push(PhysicalInputEvent::Button(ButtonEvent {
            meta: common,
            control: PhysicalControlId::keyboard(0x04),
            state,
        }));
    }
    for mode in [AxisMode::Absolute, AxisMode::Relative] {
        events.push(PhysicalInputEvent::Axis(AxisEvent {
            meta: common,
            control: vendor_control,
            value: -2048.75,
            mode,
        }));
    }
    events.push(PhysicalInputEvent::Touch(TouchEvent {
        meta: common,
        control: vendor_control,
        contact: ContactId(99),
        phase: TouchPhase::Move,
        position: Position2 { x: -4.5, y: 80.25 },
        pressure: Some(12.0),
    }));
    for mode in [PointerMode::Absolute, PointerMode::Relative] {
        events.push(PhysicalInputEvent::Pointer(PointerEvent {
            meta: common,
            control: native_control,
            position: Position2 { x: -0.0, y: 999.5 },
            mode,
        }));
    }
    events.push(PhysicalInputEvent::Pose(PoseEvent {
        meta: common,
        control: vendor_control,
        position: Position3 {
            x: -1.0,
            y: 2.5,
            z: 700.0,
        },
        // Deliberately not a unit quaternion: acquisition must not normalize it.
        orientation: Quaternion {
            x: 2.0,
            y: -3.0,
            z: 0.0,
            w: 4.0,
        },
    }));
    for report_id in [None, Some(0x77)] {
        events.push(PhysicalInputEvent::RawHidReport(RawHidReportEvent {
            meta: common,
            report_id,
            data: vec![0, 0xff, 0x80, 2],
        }));
    }
    events.push(PhysicalInputEvent::Custom(CustomInputEvent {
        meta: common,
        namespace: VendorNamespaceId(42),
        type_id: u32::MAX,
        payload: vec![0xff, 0, 1, 0x80],
    }));
    events
}

#[test]
fn identical_keyboard_descriptions_preserve_distinct_runtime_identity() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    let first = descriptor(1);
    let second = descriptor(2);
    let mut second_as_first = second.clone();
    second_as_first.runtime_id = DeviceId(1);
    assert_eq!(first, second_as_first);
    backend.register_device(first.clone()).unwrap();
    backend.register_device(second.clone()).unwrap();
    assert_eq!(backend.device(DeviceId(1)), Some(&first));
    assert_eq!(backend.device(DeviceId(2)), Some(&second));
    let ids: HashSet<_> = backend.devices().map(|device| device.runtime_id).collect();
    assert_eq!(ids, HashSet::from([DeviceId(1), DeviceId(2)]));

    let expected = vec![button(1, 1, 0), button(2, 1, 0)];
    for event in &expected {
        backend.push(event.clone(), &NoMapping).unwrap();
    }
    assert_eq!(backend.drain_events().collect::<Vec<_>>(), expected);
}

#[test]
fn physical_control_namespaces_do_not_collapse_equal_numeric_codes() {
    assert_eq!(KEYBOARD_USAGE_PAGE, 0x07);
    assert_eq!(
        PhysicalControlId::keyboard(4),
        PhysicalControlId::HidUsage {
            usage_page: 0x07,
            usage: 4,
        }
    );
    let identities = [
        PhysicalControlId::keyboard(4),
        PhysicalControlId::HidUsage {
            usage_page: 0x09,
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
    assert_eq!(identities.into_iter().collect::<HashSet<_>>().len(), 6);
}

#[test]
fn descriptors_retain_optional_fields_transport_and_capability_flags() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    for (index, transport) in [
        DeviceTransport::Usb,
        DeviceTransport::Bluetooth,
        DeviceTransport::Virtual,
        DeviceTransport::Unknown,
    ]
    .into_iter()
    .enumerate()
    {
        let device = DeviceDescriptor {
            runtime_id: DeviceId(index as u64),
            vendor_id: None,
            product_id: None,
            serial: None,
            name: None,
            transport,
            capabilities: DeviceCapabilities {
                button: true,
                axis: true,
                touch: true,
                pointer: true,
                pose: true,
                raw_hid: true,
                custom: true,
            },
        };
        backend.register_device(device.clone()).unwrap();
        assert_eq!(backend.device(device.runtime_id), Some(&device));
    }
    assert_eq!(backend.devices().count(), 4);
}

#[test]
fn every_typed_payload_round_trips_without_coordinate_or_byte_changes() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    backend.register_device(descriptor(1)).unwrap();
    let expected = all_payloads();
    for event in &expected {
        backend.push(event.clone(), &MustNotMap).unwrap();
    }
    assert_eq!(backend.pending_len(), expected.len());
    let received = backend.drain_events().collect::<Vec<_>>();
    assert_eq!(received, expected);
    for event in received {
        if let PhysicalInputEvent::Pointer(event) = event {
            assert_eq!(event.position.x.to_bits(), (-0.0_f32).to_bits());
        }
    }
    assert_eq!(backend.pending_len(), 0);
    assert_eq!(backend.pop(), None);
    assert_eq!(backend.drain_events().count(), 0);
}

#[test]
fn two_contacts_keep_independent_lifecycle_surface_and_pressure_data() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    backend.register_device(descriptor(1)).unwrap();
    backend.register_device(descriptor(2)).unwrap();
    let mut expected = Vec::new();
    for (sequence, phase) in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Up,
        TouchPhase::Cancel,
    ]
    .into_iter()
    .enumerate()
    {
        for (device, surface, contact) in [(1, 2, 70), (1, 2, 71), (1, 3, 70), (2, 2, 70)] {
            let event = PhysicalInputEvent::Touch(TouchEvent {
                meta: meta(device, sequence as i64, sequence as u64),
                control: PhysicalControlId::Vendor {
                    namespace: VendorNamespaceId(4),
                    code: surface,
                },
                contact: ContactId(contact),
                phase,
                position: Position2 {
                    x: contact as f32,
                    y: sequence as f32 - 100.0,
                },
                pressure: if contact == 70 { None } else { Some(-2.5) },
            });
            backend.push(event.clone(), &NoMapping).unwrap();
            expected.push(event);
        }
    }
    // Cancel after Up is also preserved: interaction policy owns validity.
    assert_eq!(backend.drain_events().collect::<Vec<_>>(), expected);
}

#[test]
fn fifo_keeps_interleaved_sources_equal_sequences_and_regressing_timestamps() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    backend.register_device(descriptor(1)).unwrap();
    backend.register_device(descriptor(2)).unwrap();
    let expected = vec![
        button(1, 1000, 0),
        button(2, -100, 100),
        button(1, -200, 0),
        button(2, -100, 100),
        button(1, i64::MIN, 1_000_000),
        button(2, 500, u64::MAX),
        button(2, 499, u64::MAX),
    ];
    for event in &expected {
        backend.push(event.clone(), &NoMapping).unwrap();
    }
    assert_eq!(backend.pop(), Some(expected[0].clone()));
    assert_eq!(backend.drain_events().collect::<Vec<_>>(), expected[1..]);
}

#[test]
fn retiring_a_device_keeps_accepted_input_and_prevents_id_reuse_forever() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    backend.register_device(descriptor(1)).unwrap();
    backend.register_device(descriptor(2)).unwrap();
    assert_eq!(
        backend.register_device(descriptor(1)),
        Err(VirtualInputError::DuplicateDevice(DeviceId(1)))
    );
    let first = button(1, 10, 1);
    let second = button(2, 20, 1);
    backend.push(first.clone(), &NoMapping).unwrap();
    assert!(backend.unregister_device(DeviceId(1)));
    assert!(!backend.unregister_device(DeviceId(1)));
    assert!(!backend.unregister_device(DeviceId(999)));
    assert_eq!(backend.device(DeviceId(1)), None);
    assert_eq!(backend.devices().count(), 1);
    assert_eq!(
        backend.push(button(1, 30, 2), &NoMapping),
        Err(VirtualInputError::UnknownDevice(DeviceId(1)))
    );
    let mut changed_descriptor = descriptor(1);
    changed_descriptor.serial = Some("replacement".into());
    assert_eq!(
        backend.register_device(changed_descriptor),
        Err(VirtualInputError::DuplicateDevice(DeviceId(1)))
    );
    backend.push(second.clone(), &NoMapping).unwrap();
    assert_eq!(backend.drain_events().collect::<Vec<_>>(), [first, second]);
    assert_eq!(
        backend.register_device(descriptor(1)),
        Err(VirtualInputError::DuplicateDevice(DeviceId(1)))
    );
    backend.register_device(descriptor(3)).unwrap();
    assert_eq!(backend.devices().count(), 2);
}

#[test]
fn normalization_preserves_all_payloads_native_metadata_and_deeper_origin() {
    let mapper = OffsetMapper::new(100);
    for native in [
        None,
        Some(NativeEventMeta {
            backend: BackendId(10),
            code: None,
            timestamp: None,
        }),
        Some(NativeEventMeta {
            backend: BackendId(11),
            code: Some(u32::MAX),
            timestamp: Some(point(DEEP_NATIVE, -900)),
        }),
    ] {
        for origin in [None, Some(point(DEEP_NATIVE, -800))] {
            let mut backend = VirtualInputBackend::new(OUTPUT);
            backend.register_device(descriptor(1)).unwrap();
            let mut expected = Vec::new();
            for mut event in all_payloads() {
                let incoming = point(INPUT, -50);
                event.meta_mut().clock_domain = incoming.domain;
                event.meta_mut().native = native;
                event.meta_mut().original_clock_point = origin;
                let mut converted = event.clone();
                converted.meta_mut().timestamp = Timestamp::from_nanos(50);
                converted.meta_mut().clock_domain = OUTPUT;
                converted.meta_mut().original_clock_point = origin.or(Some(incoming));
                backend.push(event, &mapper).unwrap();
                expected.push(converted);
            }
            assert_eq!(backend.drain_events().collect::<Vec<_>>(), expected);
        }
    }
    assert_eq!(mapper.calls.get(), (all_payloads().len() * 6) as u32);
}

#[test]
fn same_domain_input_adds_no_origin_and_does_not_consult_mapper() {
    for origin in [None, Some(point(DEEP_NATIVE, 700))] {
        let mut backend = VirtualInputBackend::new(OUTPUT);
        backend.register_device(descriptor(1)).unwrap();
        let expected: Vec<_> = all_payloads()
            .into_iter()
            .map(|mut event| {
                event.meta_mut().original_clock_point = origin;
                event.meta_mut().native = Some(NativeEventMeta {
                    backend: BackendId(9),
                    code: Some(0x8000_0001),
                    timestamp: Some(point(DEEP_NATIVE, 400)),
                });
                event
            })
            .collect();
        for event in &expected {
            backend.push(event.clone(), &MustNotMap).unwrap();
        }
        assert_eq!(backend.drain_events().collect::<Vec<_>>(), expected);
    }
}

#[test]
fn failed_validation_does_not_advance_sequence_or_change_pending_events() {
    let mut backend = VirtualInputBackend::new(OUTPUT);
    backend.register_device(descriptor(1)).unwrap();
    let accepted = button(1, 0, 10);
    backend.push(accepted.clone(), &NoMapping).unwrap();
    assert_eq!(
        backend.push(button(2, 0, u64::MAX), &NoMapping),
        Err(VirtualInputError::UnknownDevice(DeviceId(2)))
    );
    assert_eq!(backend.pending_len(), 1);
    assert_eq!(
        backend.push(button(1, 0, 9), &NoMapping),
        Err(VirtualInputError::SequenceRegression {
            device: DeviceId(1),
            last: 10,
            received: 9,
        })
    );
    assert_eq!(backend.pending_len(), 1);
    let mut unmapped = button(1, 0, 11);
    unmapped.meta_mut().clock_domain = INPUT;
    assert_eq!(
        backend.push(unmapped, &NoMapping),
        Err(VirtualInputError::UnmappedClock {
            from: INPUT,
            to: OUTPUT,
        })
    );
    assert_eq!(backend.pending_len(), 1);
    let mut overflowing = button(1, i64::MAX, 11);
    overflowing.meta_mut().clock_domain = INPUT;
    assert_eq!(
        backend.push(overflowing, &OffsetMapper::new(1)),
        Err(VirtualInputError::UnmappedClock {
            from: INPUT,
            to: OUTPUT,
        })
    );
    assert_eq!(backend.pending_len(), 1);
    // A rejected sequence of 11 must not poison this valid equal-sequence retry.
    let retry = button(1, -10, 10);
    backend.push(retry.clone(), &NoMapping).unwrap();
    backend.register_device(descriptor(2)).unwrap();
    let other = button(2, 0, 0);
    backend.push(other.clone(), &NoMapping).unwrap();
    let mut mapped_retry = button(1, 20, 11);
    mapped_retry.meta_mut().clock_domain = INPUT;
    backend
        .push(mapped_retry.clone(), &OffsetMapper::new(-10))
        .unwrap();
    mapped_retry.meta_mut().clock_domain = OUTPUT;
    mapped_retry.meta_mut().timestamp = Timestamp::from_nanos(10);
    mapped_retry.meta_mut().original_clock_point = Some(point(INPUT, 20));
    assert_eq!(
        backend.drain_events().collect::<Vec<_>>(),
        [accepted, retry, other, mapped_retry]
    );
}

struct FixtureAdapter;

impl DeviceAdapter for FixtureAdapter {
    fn accepts(&self, device: &DeviceDescriptor) -> bool {
        device.vendor_id == Some(0x1234) && device.product_id == Some(0x5678)
    }

    fn on_report(&mut self, report: &RawHidReportEvent, sink: &mut dyn PhysicalInputSink) {
        let [state, lo, hi, ..] = report.data.as_slice() else {
            return;
        };
        let state = match state {
            0 => ButtonState::Up,
            1 => ButtonState::Down,
            2 => ButtonState::Repeat,
            _ => return,
        };
        sink.push(PhysicalInputEvent::Button(ButtonEvent {
            meta: report.meta,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(0xa001),
                code: 1,
            },
            state,
        }));
        sink.push(PhysicalInputEvent::Axis(AxisEvent {
            meta: report.meta,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(0xa001),
                code: 2,
            },
            value: f32::from(i16::from_le_bytes([*lo, *hi])),
            mode: AxisMode::Relative,
        }));
    }
}

#[test]
fn raw_adapter_safely_parses_fanout_and_queue_accepts_shared_acquisition_sequence() {
    let mut adapter = FixtureAdapter;
    assert!(adapter.accepts(&descriptor(1)));
    let mut unrelated = descriptor(1);
    unrelated.vendor_id = Some(0xffff);
    assert!(!adapter.accepts(&unrelated));
    unrelated.vendor_id = Some(0x1234);
    unrelated.product_id = Some(0xffff);
    assert!(!adapter.accepts(&unrelated));

    let mut acquisition = meta(1, 300, 42);
    acquisition.native = Some(NativeEventMeta {
        backend: BackendId(55),
        code: None,
        timestamp: Some(point(DEEP_NATIVE, 100)),
    });
    acquisition.original_clock_point = Some(point(INPUT, 200));
    let mut sink = Vec::new();
    for data in [vec![], vec![1], vec![1, 0], vec![255, 0, 0]] {
        adapter.on_report(
            &RawHidReportEvent {
                meta: acquisition,
                report_id: Some(5),
                data,
            },
            &mut sink,
        );
        assert!(sink.is_empty());
    }
    adapter.on_report(
        &RawHidReportEvent {
            meta: acquisition,
            report_id: Some(5),
            // Report ID is separate; payload starts with the button state.
            data: vec![2, 0, 0xc0],
        },
        &mut sink,
    );
    let expected = vec![
        PhysicalInputEvent::Button(ButtonEvent {
            meta: acquisition,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(0xa001),
                code: 1,
            },
            state: ButtonState::Repeat,
        }),
        PhysicalInputEvent::Axis(AxisEvent {
            meta: acquisition,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(0xa001),
                code: 2,
            },
            value: -16384.0,
            mode: AxisMode::Relative,
        }),
    ];
    assert_eq!(sink, expected);
    let mut backend = VirtualInputBackend::new(OUTPUT);
    backend.register_device(descriptor(1)).unwrap();
    for event in sink {
        backend.push(event, &MustNotMap).unwrap();
    }
    assert_eq!(backend.drain_events().collect::<Vec<_>>(), expected);
}
