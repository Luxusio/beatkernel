//! Deferred shared profile decoding and real registry/runtime composition; no device I/O.
use std::sync::Arc;
use beatkernel::{
    audio::{
        AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample, SampleBank, SampleId,
        VoiceId, command_queue,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::*,
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_platform::input::{
    AdapterId, AdapterRegistryError, AdapterRegistryLimits, AdapterRoute, DeviceAdapterRegistry,
    hid_profile::{
        HidBitOrder, HidFieldKind, HidFieldSpec, HidProfile, HidProfileAdapter, HidProfileError,
        HidProfileMatch, HidReportSpec,
    },
};

fn control(usage: u16) -> PhysicalControlId {
    PhysicalControlId::HidUsage {
        usage_page: 9,
        usage,
    }
}
fn field(
    usage: u16,
    offset: u32,
    width: u8,
    order: HidBitOrder,
    kind: HidFieldKind,
) -> HidFieldSpec {
    HidFieldSpec {
        control: control(usage),
        offset_bits: offset,
        width_bits: width,
        order,
        kind,
    }
}
fn axis(signed: bool, mode: AxisMode, scale: f32, offset: f32) -> HidFieldKind {
    HidFieldKind::Axis {
        signed,
        mode,
        scale,
        offset,
    }
}
fn spec(id: Option<u8>, bytes: usize, fields: Vec<HidFieldSpec>) -> HidReportSpec {
    HidReportSpec {
        report_id: id,
        payload_bytes: bytes,
        fields,
    }
}
fn matcher() -> HidProfileMatch {
    HidProfileMatch {
        vendor_id: Some(0x1234),
        product_id: Some(0xabcd),
    }
}
fn profile(reports: Vec<HidReportSpec>) -> Arc<HidProfile> {
    Arc::new(HidProfile::new(matcher(), reports).unwrap())
}
fn raw(source: u64, id: Option<u8>, sequence: u64, nanos: i64, data: &[u8]) -> RawHidReportEvent {
    let host = ClockPoint {
        domain: ClockDomainId(9),
        timestamp: Timestamp::from_nanos(nanos),
    };
    let mut meta = EventMeta::new(DeviceId(source), host, sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(u32::MAX),
        code: Some(0xfeed_abcd),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(91),
            timestamp: Timestamp::from_nanos(-123),
        }),
    });
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(92),
        timestamp: Timestamp::from_nanos(9_007_199_254_740_993),
    });
    RawHidReportEvent {
        meta,
        report_id: id,
        data: data.to_vec(),
    }
}
fn descriptor(source: u64) -> DeviceDescriptor {
    DeviceDescriptor {
        runtime_id: DeviceId(source),
        vendor_id: Some(0x1234),
        product_id: Some(0xabcd),
        serial: None,
        name: Some("declared report device".into()),
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            raw_hid: true,
            ..Default::default()
        },
    }
}
fn button_event(report: &RawHidReportEvent, usage: u16, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: report.meta,
        control: control(usage),
        state,
    })
}
fn axis_event(
    report: &RawHidReportEvent,
    usage: u16,
    value: f32,
    mode: AxisMode,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Axis(AxisEvent {
        meta: report.meta,
        control: control(usage),
        value,
        mode,
    })
}
fn routed(route: AdapterRoute) -> Vec<PhysicalInputEvent> {
    let AdapterRoute::Handled { adapter, events } = route else {
        panic!("declared profile must handle this device")
    };
    assert_eq!(adapter, AdapterId(7));
    events
}
fn registry(profile: Arc<HidProfile>) -> DeviceAdapterRegistry {
    let mut registry =
        DeviceAdapterRegistry::new(AdapterRegistryLimits::new(1, 4, 8, 256, 1024, 128).unwrap());
    registry
        .register(
            AdapterId(7),
            Box::new(move || Box::new(HidProfileAdapter::new(profile.clone()))),
        )
        .unwrap();
    registry
}

#[test]
fn declared_stream_orders_decode_cross_byte_signed_scaled_and_full_width_fields() {
    use HidBitOrder::{LeastSignificantFirst as Lsf, MostSignificantFirst as Msf};
    let declared = profile(vec![spec(
        Some(7),
        2,
        vec![
            field(1, 3, 9, Lsf, axis(false, AxisMode::Absolute, 2.0, -1.0)),
            field(2, 3, 9, Msf, axis(false, AxisMode::Absolute, 0.25, 0.5)),
            field(3, 1, 9, Lsf, axis(true, AxisMode::Absolute, 0.5, 1.0)),
            field(4, 0, 9, Msf, axis(true, AxisMode::Absolute, 0.5, 1.0)),
        ],
    )]);
    let original = raw(u64::MAX, Some(7), u64::MAX, i64::MAX, &[0xad, 0x72]);
    let retained = original.clone();
    let mut adapter = HidProfileAdapter::new(declared);
    let mut events = Vec::new();
    assert_eq!(adapter.decode_report(&original, &mut events).unwrap(), 4);
    // Literal stream values are 85, 215, -170 and -166 before declared units.
    assert_eq!(
        events,
        vec![
            axis_event(&original, 1, 169.0, AxisMode::Absolute),
            axis_event(&original, 2, 54.25, AxisMode::Absolute),
            axis_event(&original, 3, -84.0, AxisMode::Absolute),
            axis_event(&original, 4, -82.0, AxisMode::Absolute)
        ]
    );
    assert_eq!(original, retained);
    for order in [Lsf, Msf] {
        for width in 1..=64 {
            let mut adapter = HidProfileAdapter::new(profile(vec![spec(
                None,
                9,
                vec![
                    field(
                        1,
                        3,
                        width,
                        order,
                        axis(false, AxisMode::Absolute, 1.0, 0.0),
                    ),
                    field(2, 3, width, order, axis(true, AxisMode::Absolute, 1.0, 0.0)),
                ],
            )]));
            let report = raw(9_007_199_254_740_993, None, 1, 1, &[0xff; 9]);
            let mut output = Vec::new();
            assert_eq!(adapter.decode_report(&report, &mut output).unwrap(), 2);
            let magnitude = if width == 64 {
                u64::MAX
            } else {
                (1u64 << width) - 1
            };
            assert_eq!(
                output,
                [
                    axis_event(&report, 1, magnitude as f32, AxisMode::Absolute),
                    axis_event(&report, 2, -1.0, AxisMode::Absolute)
                ]
            );
        }
        let mut adapter = HidProfileAdapter::new(profile(vec![spec(
            None,
            8,
            vec![field(
                1,
                0,
                64,
                order,
                axis(true, AxisMode::Absolute, 1.0, 0.0),
            )],
        )]));
        let bytes = if order == Lsf {
            [0, 0, 0, 0, 0, 0, 0, 0x80]
        } else {
            [0x80, 0, 0, 0, 0, 0, 0, 0]
        };
        let report = raw(7, None, 1, 0, &bytes);
        let mut output = Vec::new();
        adapter.decode_report(&report, &mut output).unwrap();
        assert_eq!(
            output,
            [axis_event(&report, 1, i64::MIN as f32, AxisMode::Absolute)]
        );
    }
}

#[test]
fn report_specific_buttons_and_absolute_levels_do_not_suppress_relative_deltas_or_repeat_initial_false()
 {
    use HidBitOrder::LeastSignificantFirst as Lsf;
    let mut adapter = HidProfileAdapter::new(profile(vec![
        spec(
            Some(1),
            1,
            vec![
                field(1, 0, 1, Lsf, HidFieldKind::Button { invert: false }),
                field(2, 1, 1, Lsf, HidFieldKind::Button { invert: true }),
                field(3, 2, 2, Lsf, axis(false, AxisMode::Absolute, 0.5, 0.0)),
                field(4, 4, 4, Lsf, axis(true, AxisMode::Relative, 1.0, 0.0)),
            ],
        ),
        spec(
            Some(2),
            1,
            vec![field(5, 0, 8, Lsf, HidFieldKind::Button { invert: false })],
        ),
    ]));
    let mut output = Vec::new();
    let zero = raw(u64::MAX, Some(1), 1, 0, &[0]);
    assert_eq!(adapter.decode_report(&zero, &mut output).unwrap(), 3);
    assert_eq!(
        output,
        [
            button_event(&zero, 2, ButtonState::Down),
            axis_event(&zero, 3, 0.0, AxisMode::Absolute),
            axis_event(&zero, 4, 0.0, AxisMode::Relative)
        ]
    );
    output.clear();
    assert_eq!(adapter.decode_report(&zero, &mut output).unwrap(), 1);
    assert_eq!(output, [axis_event(&zero, 4, 0.0, AxisMode::Relative)]);
    let changed = raw(u64::MAX, Some(1), 2, 1, &[0xf7]);
    output.clear();
    adapter.decode_report(&changed, &mut output).unwrap();
    assert_eq!(
        output,
        [
            button_event(&changed, 1, ButtonState::Down),
            button_event(&changed, 2, ButtonState::Up),
            axis_event(&changed, 3, 0.5, AxisMode::Absolute),
            axis_event(&changed, 4, -1.0, AxisMode::Relative)
        ]
    );
    let other = raw(u64::MAX, Some(2), 3, 2, &[2]);
    output.clear();
    adapter.decode_report(&other, &mut output).unwrap();
    assert_eq!(
        output,
        [button_event(&other, 5, ButtonState::Down)],
        "any nonzero declared button field is pressed"
    );
    output.clear();
    adapter.decode_report(&changed, &mut output).unwrap();
    assert_eq!(output, [axis_event(&changed, 4, -1.0, AxisMode::Relative)]);
    let release = raw(u64::MAX, Some(1), 4, 3, &[2]);
    output.clear();
    adapter.decode_report(&release, &mut output).unwrap();
    assert_eq!(
        output,
        [
            button_event(&release, 1, ButtonState::Up),
            axis_event(&release, 3, 0.0, AxisMode::Absolute),
            axis_event(&release, 4, 0.0, AxisMode::Relative)
        ]
    );
    let release_other = raw(u64::MAX, Some(2), 5, 4, &[0]);
    output.clear();
    adapter.decode_report(&release_other, &mut output).unwrap();
    assert_eq!(output, [button_event(&release_other, 5, ButtonState::Up)]);
    output.clear();
    adapter.reset();
    assert!(output.is_empty(), "reset synthesizes no input releases");
    let fresh = raw(3, Some(1), 0, -5, &[0]);
    adapter.decode_report(&fresh, &mut output).unwrap();
    assert_eq!(output.len(), 3);
    assert!(output.iter().all(|event| *event.meta() == fresh.meta));
    let mut zeros = HidProfileAdapter::new(profile(vec![spec(
        None,
        1,
        vec![field(
            1,
            0,
            1,
            Lsf,
            axis(true, AxisMode::Absolute, 0.0, -0.0),
        )],
    )]));
    let mut zero_events = Vec::new();
    for bits in [1, 0, 0] {
        zeros
            .decode_report(&raw(1, None, 1, 0, &[bits]), &mut zero_events)
            .unwrap();
    }
    assert_eq!(
        zero_events.len(),
        2,
        "absolute levels compare the resulting f32 bits, including signed zero"
    );
    let zeros = zero_events
        .iter()
        .map(|event| match event {
            PhysicalInputEvent::Axis(axis) => axis.value.to_bits(),
            _ => panic!("axis field"),
        })
        .collect::<Vec<_>>();
    assert_eq!(zeros, [(-0.0f32).to_bits(), 0.0f32.to_bits()]);
}

#[test]
fn profile_limits_and_report_refusals_are_atomic_including_source_adoption_and_finite_conversion() {
    use HidBitOrder::LeastSignificantFirst as Lsf;
    let button = || field(1, 0, 1, Lsf, HidFieldKind::Button { invert: false });
    assert!(matches!(
        HidProfile::new(matcher(), vec![]),
        Err(HidProfileError::InvalidReportCount)
    ));
    assert!(matches!(
        HidProfile::new(matcher(), vec![spec(Some(0), 1, vec![button()])]),
        Err(HidProfileError::InvalidReportId)
    ));
    assert!(matches!(
        HidProfile::new(
            matcher(),
            vec![spec(None, 1, vec![button()]), spec(None, 0, vec![])]
        ),
        Err(HidProfileError::DuplicateReportId)
    ));
    assert!(matches!(
        HidProfile::new(matcher(), vec![spec(None, 1025, vec![button()])]),
        Err(HidProfileError::PayloadCapacity)
    ));
    assert!(matches!(
        HidProfile::new(matcher(), vec![spec(None, 0, vec![])]),
        Err(HidProfileError::InvalidFieldCount)
    ));
    assert!(matches!(
        HidProfile::new(
            matcher(),
            vec![
                spec(None, 1, vec![button()]),
                spec(Some(1), 1, vec![button()])
            ]
        ),
        Err(HidProfileError::DuplicateControl)
    ));
    for (offset, width, bytes) in [
        (0, 0, 1),
        (0, 65, 16),
        (8, 1, 1),
        (u32::MAX, 64, 1024),
        (0, 1, 0),
    ] {
        assert!(matches!(
            HidProfile::new(
                matcher(),
                vec![spec(
                    None,
                    bytes,
                    vec![field(
                        1,
                        offset,
                        width,
                        Lsf,
                        HidFieldKind::Button { invert: false }
                    )]
                )]
            ),
            Err(HidProfileError::InvalidFieldExtent)
        ));
    }
    for (scale, offset) in [
        (f32::NAN, 0.0),
        (f32::INFINITY, 0.0),
        (1.0, f32::NEG_INFINITY),
    ] {
        assert!(matches!(
            HidProfile::new(
                matcher(),
                vec![spec(
                    None,
                    1,
                    vec![field(
                        1,
                        0,
                        8,
                        Lsf,
                        axis(false, AxisMode::Absolute, scale, offset)
                    )]
                )]
            ),
            Err(HidProfileError::NonFiniteParameters)
        ));
    }
    let all_reports = || {
        (0..256)
            .map(|id| {
                spec(
                    (id != 0).then_some(id as u8),
                    1,
                    vec![field(
                        id as u16 + 1,
                        0,
                        1,
                        Lsf,
                        HidFieldKind::Button { invert: false },
                    )],
                )
            })
            .collect::<Vec<_>>()
    };
    let boundary = HidProfile::new(matcher(), all_reports()).unwrap();
    assert_eq!(boundary.reports().len(), 256);
    assert_eq!(boundary.matcher(), matcher());
    let mut excess = all_reports();
    excess.push(spec(None, 0, vec![]));
    assert!(matches!(
        HidProfile::new(matcher(), excess),
        Err(HidProfileError::InvalidReportCount)
    ));
    let many_fields = |count| {
        (1..=count)
            .map(|id| field(id, 0, 1, Lsf, HidFieldKind::Button { invert: false }))
            .collect()
    };
    let mut full = HidProfileAdapter::new(profile(vec![spec(None, 1, many_fields(256))]));
    let mut sink = Vec::new();
    assert_eq!(
        full.decode_report(&raw(1, None, 0, 0, &[1]), &mut sink)
            .unwrap(),
        256
    );
    assert_eq!(sink.len(), 256);
    assert!(matches!(
        HidProfile::new(matcher(), vec![spec(None, 1, many_fields(257))]),
        Err(HidProfileError::InvalidFieldCount)
    ));
    let mut adapter = HidProfileAdapter::new(profile(vec![
        spec(None, 0, vec![]),
        spec(
            Some(1),
            1,
            vec![
                button(),
                field(2, 1, 2, Lsf, axis(false, AxisMode::Absolute, f32::MAX, 0.0)),
            ],
        ),
    ]));
    sink.clear();
    assert_eq!(
        adapter
            .decode_report(&raw(11, Some(99), 1, 0, &[255; 8]), &mut sink)
            .unwrap(),
        0
    );
    assert_eq!(
        adapter.decode_report(&raw(11, Some(1), 1, 0, &[]), &mut sink),
        Err(HidProfileError::ReportLength {
            expected: 1,
            actual: 0
        })
    );
    assert_eq!(
        adapter.decode_report(&raw(11, Some(1), 1, 0, &[5]), &mut sink),
        Err(HidProfileError::NonFiniteValue)
    );
    assert!(sink.is_empty());
    let admitted = raw(u64::MAX, Some(1), 1, 0, &[3]);
    assert_eq!(
        adapter.decode_report(&admitted, &mut sink).unwrap(),
        2,
        "the failed earlier button level and source were not committed"
    );
    assert_eq!(
        sink,
        [
            button_event(&admitted, 1, ButtonState::Down),
            axis_event(&admitted, 2, f32::MAX, AxisMode::Absolute)
        ]
    );
    sink.clear();
    assert_eq!(
        adapter.decode_report(&raw(11, Some(1), 2, 1, &[0]), &mut sink),
        Err(HidProfileError::SourceMismatch {
            expected: DeviceId(u64::MAX),
            actual: DeviceId(11)
        })
    );
    adapter.on_report(&raw(u64::MAX, Some(1), 2, 1, &[5]), &mut sink);
    assert!(
        sink.is_empty(),
        "DeviceAdapter safely refuses the entire malformed batch"
    );
    assert_eq!(adapter.decode_report(&admitted, &mut sink).unwrap(), 0);
    assert_eq!(
        adapter
            .decode_report(&raw(11, Some(99), 2, 1, &[]), &mut sink)
            .unwrap(),
        0
    );
    adapter.reset();
    assert_eq!(
        adapter
            .decode_report(&raw(11, None, 1, 0, &[]), &mut sink)
            .unwrap(),
        0
    );
    assert!(
        matches!(
            adapter.decode_report(&admitted, &mut sink),
            Err(HidProfileError::SourceMismatch {
                expected: DeviceId(11),
                ..
            })
        ),
        "a valid zero-field report still establishes source ownership"
    );
}

#[test]
fn actual_registry_isolates_device_levels_preserves_equal_metadata_fanout_and_retires_identity() {
    use HidBitOrder::LeastSignificantFirst as Lsf;
    let declared = profile(vec![
        spec(
            Some(1),
            1,
            vec![
                field(1, 0, 1, Lsf, HidFieldKind::Button { invert: false }),
                field(2, 0, 8, Lsf, axis(false, AxisMode::Absolute, 1.0, 0.0)),
            ],
        ),
        spec(
            Some(2),
            1,
            vec![field(3, 0, 1, Lsf, HidFieldKind::Button { invert: false })],
        ),
    ]);
    let adapter = HidProfileAdapter::new(declared.clone());
    assert!(adapter.accepts(&descriptor(1)));
    let mut wrong = descriptor(1);
    wrong.vendor_id = None;
    assert!(!adapter.accepts(&wrong));
    wrong = descriptor(1);
    wrong.product_id = Some(0);
    assert!(!adapter.accepts(&wrong));
    wrong = descriptor(1);
    wrong.capabilities.raw_hid = false;
    assert!(!adapter.accepts(&wrong));
    let wildcard = HidProfileAdapter::new(Arc::new(
        HidProfile::new(
            HidProfileMatch {
                vendor_id: None,
                product_id: None,
            },
            vec![spec(
                None,
                1,
                vec![field(1, 0, 1, Lsf, HidFieldKind::Button { invert: false })],
            )],
        )
        .unwrap(),
    ));
    wrong = descriptor(1);
    wrong.vendor_id = None;
    wrong.product_id = None;
    assert!(wildcard.accepts(&wrong));
    let mut registry = registry(declared);
    for id in [u64::MAX, 3] {
        assert_eq!(registry.attach(descriptor(id)).unwrap(), Some(AdapterId(7)));
    }
    let first = raw(u64::MAX, Some(1), u64::MAX, 9_007_199_254_740_993, &[1]);
    let fanout = routed(registry.route(DeviceId(u64::MAX), &first).unwrap());
    assert_eq!(
        fanout,
        [
            button_event(&first, 1, ButtonState::Down),
            axis_event(&first, 2, 1.0, AxisMode::Absolute)
        ]
    );
    assert!(fanout.iter().all(|event| *event.meta() == first.meta));
    assert!(routed(registry.route(DeviceId(u64::MAX), &first).unwrap()).is_empty());
    let other = raw(3, Some(1), 1, 5, &[1]);
    assert_eq!(
        routed(registry.route(DeviceId(3), &other).unwrap()).len(),
        2,
        "another interface begins with fresh levels"
    );
    let mut sibling = first.clone();
    sibling.report_id = Some(2);
    assert_eq!(
        routed(registry.route(DeviceId(u64::MAX), &sibling).unwrap()),
        [button_event(&sibling, 3, ButtonState::Down)]
    );
    let mut changed_meta = sibling.clone();
    changed_meta.meta.native.as_mut().unwrap().code = Some(1);
    assert_eq!(
        registry.route(DeviceId(u64::MAX), &changed_meta),
        Err(AdapterRegistryError::ReportAcquisitionMismatch)
    );
    registry.remove(DeviceId(u64::MAX)).unwrap();
    assert_eq!(
        registry.attach(descriptor(u64::MAX)),
        Err(AdapterRegistryError::RetiredDevice)
    );
    assert_eq!(
        registry.route(DeviceId(u64::MAX), &first),
        Err(AdapterRegistryError::UnknownDevice)
    );
    registry.attach(descriptor(7)).unwrap();
    let fresh = raw(7, Some(1), 0, 0, &[1]);
    assert_eq!(
        routed(registry.route(DeviceId(7), &fresh).unwrap()).len(),
        2
    );
}

#[test]
fn actual_profile_buttons_reach_common_runtime_judging_and_pcm_as_hid_controls_with_original_metadata()
 {
    use HidBitOrder::LeastSignificantFirst as Lsf;
    let mut registry = registry(profile(vec![spec(
        Some(1),
        1,
        vec![
            field(1, 0, 1, Lsf, HidFieldKind::Button { invert: false }),
            field(2, 1, 1, Lsf, HidFieldKind::Button { invert: false }),
        ],
    )]));
    registry.attach(descriptor(u64::MAX)).unwrap();
    let mut chart = SourceChart::new(1, Bpm::new(60, 1).unwrap()).unwrap();
    for id in 1u32..=2 {
        chart.objects.push(SourceObject {
            id: ObjectId(u64::from(id)),
            start: Beat::new(0).unwrap(),
            end: None,
            interaction: InteractionId(id),
            visual: VisualId(id),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let judge = JudgeEngine::new(
        chart.compile().unwrap(),
        (1..=2)
            .map(|id| Rule {
                interaction: InteractionId(id),
                control: GameControlId(id),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
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
    let bindings = BindingMap::from_bindings((1..=2).map(|id| Binding {
        device: DeviceSelector::Exact(DeviceId(u64::MAX)),
        physical: control(id),
        game_control: GameControlId(u32::from(id)),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(4).unwrap();
    let sounds = (1..=2)
        .map(|id| SoundBinding {
            object: ObjectId(id),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(id),
            gain: 1.0,
        })
        .collect();
    let mut runtime = Runtime::new(
        ClockDomainId(9),
        ClockDomainId(10),
        Transport::new(Timestamp::from_nanos(1000), Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        sounds,
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    struct NoMapping;
    impl ClockMapper for NoMapping {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    let report = raw(u64::MAX, Some(1), 9_007_199_254_740_993, 1000, &[3]);
    let typed = routed(registry.route(DeviceId(u64::MAX), &report).unwrap());
    assert_eq!(
        typed,
        [
            button_event(&report, 1, ButtonState::Down),
            button_event(&report, 2, ButtonState::Down)
        ]
    );
    let output = ClockPoint {
        domain: ClockDomainId(10),
        timestamp: Timestamp::ZERO,
    };
    for (index, event) in typed.into_iter().enumerate() {
        let actual = runtime
            .process_input(event.clone(), &NoMapping, output)
            .unwrap();
        assert_eq!(
            actual.bound_inputs,
            [GameInputEvent {
                game_control: GameControlId(index as u32 + 1),
                physical: event
            }]
        );
        assert_eq!(actual.judge_events.len(), 1);
        assert_eq!(actual.judge_events[0].input, Some(report.meta));
        assert!(matches!(
            actual.judge_events[0].outcome,
            JudgeOutcome::Hit { .. }
        ));
        assert_eq!(actual.audio_commands.len(), 1);
    }
    assert!(
        routed(registry.route(DeviceId(u64::MAX), &report).unwrap()).is_empty(),
        "a repeated level cannot synthesize a second fresh press"
    );
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(128, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, -0.25], limits).unwrap(),
    )
    .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(10),
            Timestamp::ZERO,
            AudioLimits::new(4, 2, 4, 8, 4).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pcm = [99.0; 3];
    let rendered = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.5, -0.5, 0.0]);
    assert_eq!(rendered.counters.commands_applied, 2);
    assert_eq!(runtime.telemetry().counters().judge_results, 2);
}
