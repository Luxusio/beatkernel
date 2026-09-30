use beatkernel::{input::*, time::*};
use beatkernel_platform::input::*;
fn descriptor(id: u64, vendor: u16) -> DeviceDescriptor {
    DeviceDescriptor {
        runtime_id: DeviceId(id),
        vendor_id: Some(vendor),
        product_id: Some(2),
        serial: None,
        name: None,
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            raw_hid: true,
            ..Default::default()
        },
    }
}
fn report(id: u64, sequence: u64, time: i64, data: Vec<u8>) -> RawHidReportEvent {
    let mut meta = EventMeta::new(
        DeviceId(id),
        ClockPoint {
            domain: ClockDomainId(9),
            timestamp: Timestamp::from_nanos(time),
        },
        sequence,
    );
    meta.native = Some(NativeEventMeta {
        backend: BackendId(3),
        code: Some(8),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(10),
            timestamp: Timestamp::from_nanos(time - 1),
        }),
    });
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(11),
        timestamp: Timestamp::from_nanos(time - 2),
    });
    RawHidReportEvent {
        meta,
        report_id: Some(1),
        data,
    }
}
struct Decoder {
    calls: u32,
}
impl DeviceAdapter for Decoder {
    fn accepts(&self, descriptor: &DeviceDescriptor) -> bool {
        descriptor.vendor_id == Some(1)
    }
    fn on_report(&mut self, report: &RawHidReportEvent, out: &mut dyn PhysicalInputSink) {
        self.calls += 1;
        if report.report_id != Some(1) || !(1..=2).contains(&report.data.len()) {
            return;
        }
        out.push(PhysicalInputEvent::Button(ButtonEvent {
            meta: report.meta,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(1),
                code: self.calls,
            },
            state: if report.data[0] & 1 == 1 {
                ButtonState::Down
            } else {
                ButtonState::Up
            },
        }));
        if report.data.len() == 2 {
            out.push(PhysicalInputEvent::Axis(AxisEvent {
                meta: report.meta,
                control: PhysicalControlId::Vendor {
                    namespace: VendorNamespaceId(1),
                    code: 100,
                },
                value: f32::from(report.data[1] as i8),
                mode: AxisMode::Absolute,
            }));
        }
    }
}
fn registry(emissions: usize) -> DeviceAdapterRegistry {
    let mut registry =
        DeviceAdapterRegistry::new(AdapterRegistryLimits::new(4, 4, 8, emissions, 16, 16).unwrap());
    registry
        .register(AdapterId(7), Box::new(|| Box::new(Decoder { calls: 0 })))
        .unwrap();
    registry
}
fn events(route: AdapterRoute) -> Vec<PhysicalInputEvent> {
    match route {
        AdapterRoute::Handled { events, .. } => events,
        AdapterRoute::Unhandled => panic!("expected handled"),
    }
}
fn call_number(event: &PhysicalInputEvent) -> u32 {
    match event {
        PhysicalInputEvent::Button(ButtonEvent {
            control: PhysicalControlId::Vendor { code, .. },
            ..
        }) => *code,
        _ => panic!("expected vendor button"),
    }
}
#[test]
fn actual_decoding_retains_provenance_and_binds_canonical_control() {
    let mut registry = registry(2);
    assert_eq!(
        registry.attach(descriptor(50, 1)).unwrap(),
        Some(AdapterId(7))
    );
    let raw = report(50, 1, 100, vec![1, 0xfe]);
    let emitted = events(registry.route(DeviceId(50), &raw).unwrap());
    assert_eq!(emitted.len(), 2);
    assert!(emitted.iter().all(|event| *event.meta() == raw.meta));
    assert!(matches!(&emitted[1], PhysicalInputEvent::Axis(e) if e.value == -2.0));
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(50)),
        physical: PhysicalControlId::Vendor {
            namespace: VendorNamespaceId(1),
            code: 1,
        },
        game_control: GameControlId(4),
    }])
    .unwrap();
    let bound: Vec<_> = bindings.map(&emitted[0]).collect();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].physical, emitted[0]);
    assert_eq!(raw.data, [1, 0xfe]);
}
#[test]
fn devices_interleave_arbitrary_ids_and_native_equal_meta_fanout() {
    let mut registry = registry(2);
    registry.attach(descriptor(90, 1)).unwrap();
    registry.attach(descriptor(2, 1)).unwrap(); // no global monotonic-ID assumption
    let first = report(90, 8, 100, vec![1, 2]);
    let a = events(registry.route(DeviceId(90), &first).unwrap());
    let b = events(
        registry
            .route(DeviceId(2), &report(2, 1, 20, vec![0, 3]))
            .unwrap(),
    );
    assert_eq!(call_number(&a[0]), 1);
    assert_eq!(call_number(&b[0]), 1); // independent per-device decoder state
    let mut sibling = first.clone();
    sibling.data = vec![0, 9];
    let c = events(registry.route(DeviceId(90), &sibling).unwrap());
    assert_eq!(call_number(&c[0]), 2);
    assert_eq!(*c[0].meta(), first.meta);
    assert!(matches!(&c[1], PhysicalInputEvent::Axis(e) if e.value == 9.0));
    let mut bad = first.clone();
    bad.meta.native.as_mut().unwrap().code = Some(99);
    assert_eq!(
        registry.route(DeviceId(90), &bad),
        Err(AdapterRegistryError::ReportAcquisitionMismatch)
    );
    assert_eq!(
        registry.route(DeviceId(90), &report(90, 7, 100, vec![0, 0])),
        Err(AdapterRegistryError::ReportSequence)
    );
    assert_eq!(
        registry.route(DeviceId(90), &report(90, 9, 99, vec![0, 0])),
        Err(AdapterRegistryError::ReportChronology)
    );
}
#[test]
fn retirement_resets_owned_decoder_and_rejects_identity_reuse() {
    let mut registry = registry(2);
    registry.attach(descriptor(5, 1)).unwrap();
    registry
        .route(DeviceId(5), &report(5, 1, 1, vec![1, 0]))
        .unwrap();
    registry.remove(DeviceId(5)).unwrap();
    assert_eq!(
        registry.attach(descriptor(5, 1)),
        Err(AdapterRegistryError::RetiredDevice)
    );
    registry.attach(descriptor(1, 1)).unwrap();
    let fresh = events(
        registry
            .route(DeviceId(1), &report(1, 1, 1, vec![1, 0]))
            .unwrap(),
    );
    assert_eq!(call_number(&fresh[0]), 1);
    assert_eq!(
        registry.route(DeviceId(5), &report(5, 2, 2, vec![0, 0])),
        Err(AdapterRegistryError::UnknownDevice)
    );
}
#[test]
fn ambiguity_and_unhandled_raw_are_explicit() {
    let mut registry = registry(2);
    registry
        .register(AdapterId(8), Box::new(|| Box::new(Decoder { calls: 0 })))
        .unwrap();
    assert_eq!(
        registry.attach(descriptor(1, 1)),
        Err(AdapterRegistryError::AmbiguousAdapters {
            first: AdapterId(7),
            second: AdapterId(8)
        })
    );
    assert!(registry.device(DeviceId(1)).is_none());
    registry.attach(descriptor(1, 99)).unwrap();
    let raw = report(1, 1, 1, vec![5, 6]);
    assert_eq!(
        registry.route(DeviceId(1), &raw).unwrap(),
        AdapterRoute::Unhandled
    );
    assert_eq!(raw.data, [5, 6]);
    assert_eq!(
        registry.unregister(AdapterId(7)),
        Err(AdapterRegistryError::DevicesConnected)
    );
    assert_eq!(
        registry.route(DeviceId(2), &raw),
        Err(AdapterRegistryError::ReportDeviceMismatch)
    );
}
#[test]
fn whole_batch_withheld_on_capacity_but_callback_state_not_rolled_back() {
    let mut registry = registry(1);
    registry.attach(descriptor(1, 1)).unwrap();
    assert_eq!(
        registry.route(DeviceId(1), &report(1, 1, 1, vec![1, 0])),
        Err(AdapterRegistryError::InvalidEmission {
            index: 1,
            reason: EmissionError::EmissionCapacity
        })
    );
    // Same acquisition can represent packet fanout: rerouting is not deduplicated.
    assert_eq!(
        registry.route(DeviceId(1), &report(1, 1, 1, vec![0, 0])),
        Err(AdapterRegistryError::InvalidEmission {
            index: 1,
            reason: EmissionError::EmissionCapacity
        })
    );
    let next = events(
        registry
            .route(DeviceId(1), &report(1, 2, 2, vec![0]))
            .unwrap(),
    );
    assert_eq!(call_number(&next[0]), 3); // both rejected callbacks really mutated state
}
struct InvalidDecoder {
    kind: u8,
}
impl DeviceAdapter for InvalidDecoder {
    fn accepts(&self, _: &DeviceDescriptor) -> bool {
        true
    }
    fn on_report(&mut self, report: &RawHidReportEvent, out: &mut dyn PhysicalInputSink) {
        let mut meta = report.meta;
        if self.kind == 0 {
            meta.sequence += 1;
        }
        if self.kind == 2 {
            out.push(PhysicalInputEvent::Custom(CustomInputEvent {
                meta,
                namespace: VendorNamespaceId(1),
                type_id: 1,
                payload: vec![0; 17],
            }));
        } else {
            out.push(PhysicalInputEvent::Axis(AxisEvent {
                meta,
                control: PhysicalControlId::keyboard(1),
                value: if self.kind == 1 { f32::NAN } else { 0.0 },
                mode: AxisMode::Absolute,
            }));
        }
    }
}
#[test]
fn invalid_metadata_numeric_and_payload_are_rejected_without_rewrite() {
    for (kind, reason) in [
        (0, EmissionError::MetadataMismatch),
        (1, EmissionError::NonFinite),
        (2, EmissionError::PayloadCapacity),
    ] {
        let mut registry =
            DeviceAdapterRegistry::new(AdapterRegistryLimits::new(1, 1, 2, 2, 16, 16).unwrap());
        registry
            .register(
                AdapterId(1),
                Box::new(move || Box::new(InvalidDecoder { kind })),
            )
            .unwrap();
        registry.attach(descriptor(1, 1)).unwrap();
        let raw = report(1, 1, 1, vec![1, 0]);
        assert_eq!(
            registry.route(DeviceId(1), &raw),
            Err(AdapterRegistryError::InvalidEmission { index: 0, reason })
        );
        assert_eq!(raw.meta.sequence, 1);
    }
}
#[test]
fn identity_capacity_reserved_at_attach_makes_removal_unconditional() {
    let mut registry =
        DeviceAdapterRegistry::new(AdapterRegistryLimits::new(1, 1, 2, 1, 16, 16).unwrap());
    registry.attach(descriptor(8, 99)).unwrap();
    registry.remove(DeviceId(8)).unwrap();
    registry.attach(descriptor(3, 99)).unwrap();
    registry.remove(DeviceId(3)).unwrap();
    assert_eq!(
        registry.attach(descriptor(2, 99)),
        Err(AdapterRegistryError::IdentityCapacity)
    );
    assert!(AdapterRegistryLimits::new(0, 1, 1, 1, 1, 1).is_err());
    assert!(AdapterRegistryLimits::new(1, 2, 1, 1, 1, 1).is_err());
}
