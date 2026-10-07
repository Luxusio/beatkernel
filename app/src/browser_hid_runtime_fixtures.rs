//! Deferred numeric HID setup and actual portable gameplay composition; no WASM or devices.
use crate::{
    ChannelPolicy, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    browser_hid_input::BrowserHidSetup,
    browser_input::PhysicalInputSetup,
    pressed_keys::PressedKeys,
    replay_playback::{decode_section_setup, reconstruct_section},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue},
    input::*,
    judge::JudgeOutcome,
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, decode_replay},
    },
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::parse_seeded;
use beatkernel_platform::input::{
    AdapterId, AdapterRegistryError, AdapterRegistryLimits, AdapterRoute, DeviceAdapterRegistry,
    validate_report_order,
    hid_profile::{
        HidBitOrder, HidFieldKind, HidFieldSpec, HidProfile, HidProfileAdapter, HidProfileMatch,
        HidReportSpec,
    },
};
use std::sync::Arc;

const SOURCE: u64 = 0xfedc_ba98_7654_3210;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn devices(source: u64) -> [u32; 6] {
    [source as u32, (source >> 32) as u32, 1, 0xffff, 1, 0xabcd]
}
fn field(device: u32, usage: u32, bit: u32) -> [u32; 13] {
    [device, 1, 1, 1, 0, 9, usage, bit, 1, 0, 0, 0, 0]
}
fn empty(device: u32, id: u32) -> [u32; 13] {
    [device, 1, id, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0]
}
fn physical(count: u32) -> PhysicalInputSetup {
    let rows = (0..count)
        .flat_map(|index| {
            [
                0x11 + index,
                1,
                SOURCE as u32,
                (SOURCE >> 32) as u32,
                0,
                9,
                index + 1,
            ]
        })
        .collect::<Vec<_>>();
    let lanes = (0..count)
        .map(|index| 0x11 + index as u8)
        .collect::<Vec<_>>();
    PhysicalInputSetup::new(&rows, &lanes, 4096, 1024).unwrap()
}
fn raw(source: u64, id: Option<u8>, seq: u64, at: i64, bytes: &[u8]) -> RawHidReportEvent {
    let mut meta = EventMeta::new(DeviceId(source), point(11, at), seq);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x57484944),
        code: Some(1),
        timestamp: Some(point(99, -123)),
    });
    meta.original_clock_point = Some(point(98, 9_007_199_254_740_993));
    RawHidReportEvent {
        meta,
        report_id: id,
        data: bytes.to_vec(),
    }
}
fn buttons(count: u32, bindings: &[Binding]) -> BrowserHidSetup {
    let fields = (0..count)
        .flat_map(|index| field(0, index + 1, index))
        .collect::<Vec<_>>();
    BrowserHidSetup::new(
        &devices(SOURCE),
        &fields,
        &vec![0.0; count as usize * 2],
        bindings,
    )
    .unwrap()
}
fn typed_button(report: &RawHidReportEvent, usage: u16, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: report.meta,
        control: PhysicalControlId::HidUsage {
            usage_page: 9,
            usage,
        },
        state,
    })
}
fn descriptor() -> DeviceDescriptor {
    DeviceDescriptor {
        runtime_id: DeviceId(SOURCE),
        vendor_id: Some(0xffff),
        product_id: Some(0xabcd),
        serial: None,
        name: None,
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            raw_hid: true,
            ..Default::default()
        },
    }
}
fn registry() -> DeviceAdapterRegistry {
    let profile = Arc::new(
        HidProfile::new(
            HidProfileMatch {
                vendor_id: Some(0xffff),
                product_id: Some(0xabcd),
            },
            vec![HidReportSpec {
                report_id: Some(1),
                payload_bytes: 1,
                fields: vec![HidFieldSpec {
                    control: PhysicalControlId::HidUsage {
                        usage_page: 9,
                        usage: 1,
                    },
                    offset_bits: 0,
                    width_bits: 1,
                    order: HidBitOrder::LeastSignificantFirst,
                    kind: HidFieldKind::Button { invert: false },
                }],
            }],
        )
        .unwrap(),
    );
    let mut registry =
        DeviceAdapterRegistry::new(AdapterRegistryLimits::new(1, 1, 2, 8, 1024, 128).unwrap());
    registry
        .register(
            AdapterId(1),
            Box::new(move || Box::new(HidProfileAdapter::new(profile.clone()))),
        )
        .unwrap();
    registry.attach(descriptor()).unwrap();
    registry
}
fn config(capacity: usize) -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 100),
        preroll: Duration::from_nanos(250_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: capacity,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(4_000_000_000),
        telemetry_capacity: 8,
    }
}
fn host(song: i64) -> ClockPoint {
    point(11, 10_250_000_000 + song)
}
fn audio(song: i64) -> ClockPoint {
    point(22, 250_000_100 + song)
}
fn clocks() -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: config(8).host_origin,
            target: config(8).output_origin,
        },
        ClockInterval {
            start: config(8).host_origin.timestamp,
            end: ts(30_000_000_000),
        },
    )
    .unwrap()
}
fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn prepared_game(count: u32, bgm: bool, capacity: usize) -> (StepGameplay, SampleBank, String) {
    let mut chart = String::from("#BPM 60\n#WAV01 key.wav\n");
    for lane in 0x11..0x11 + count {
        chart.push_str(&format!("#000{lane:02X}:01\n"));
    }
    if bgm {
        chart.push_str("#00001:01\n");
    }
    let mut wav = b"RIFF".to_vec();
    wav.extend(40u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    for value in [1u16, 1] {
        wav.extend(value.to_le_bytes());
    }
    for value in [4u32, 8] {
        wav.extend(value.to_le_bytes());
    }
    for value in [2u16, 16] {
        wav.extend(value.to_le_bytes());
    }
    wav.extend(b"data");
    wav.extend(4u32.to_le_bytes());
    for value in [8192i16, -4096] {
        wav.extend(value.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", chart.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    let prepared = prepare_from_source(
        chart.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        u64::MAX,
        None,
    )
    .unwrap();
    let (game, bank) =
        StepGameplay::new(prepared, config(capacity), physical(count).bindings).unwrap();
    (game, bank, chart)
}

#[test]
fn numeric_setup_preserves_distinct_full_sources_and_actual_constructor_control_identities() {
    let words = [
        [
            0x11,
            1,
            u32::MAX,
            u32::MAX,
            0,
            u16::MAX as u32,
            u16::MAX as u32,
        ],
        [0x12, 0, 0, 0, 1, u32::MAX, u32::MAX],
        [0x13, 1, u32::MAX, u32::MAX, 2, u32::MAX, u32::MAX],
        [0x14, 1, 3, 0, 0, 1, 48],
    ]
    .concat();
    let constructor =
        PhysicalInputSetup::new(&words, &[0x11, 0x12, 0x13, 0x14], 4096, 1024).unwrap();
    let device_words = [devices(u64::MAX), devices(3)].concat();
    let rows = [
        [0, 1, 1, 2, 0, 65535, 65535, 0, 1, 0, 0, 0, 0],
        [0, 1, 1, 2, 2, u32::MAX, u32::MAX, 8, 8, 0, 1, 1, 1],
        [1, 0, 0, 1, 1, u32::MAX, u32::MAX, 0, 1, 0, 0, 1, 0],
        [1, 1, 2, 1, 0, 1, 48, 0, 8, 0, 1, 0, 0],
        empty(0, 2),
    ]
    .concat();
    let params = [0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 2.0, -1.0, 0.0, 0.0];
    let mut setup = BrowserHidSetup::new(
        &device_words,
        &rows,
        &params,
        constructor.bindings.bindings(),
    )
    .unwrap();
    assert_eq!(setup.source_count(), 2);
    let first = raw(u64::MAX, Some(1), u64::MAX, i64::MAX, &[1, 0xfe]);
    let retained = first.clone();
    let mut events = Vec::new();
    assert_eq!(setup.decode_report(&first, &mut events).unwrap(), 2);
    assert_eq!(
        events,
        [
            PhysicalInputEvent::Button(ButtonEvent {
                meta: first.meta,
                control: PhysicalControlId::HidUsage {
                    usage_page: u16::MAX,
                    usage: u16::MAX
                },
                state: ButtonState::Down
            }),
            PhysicalInputEvent::Axis(AxisEvent {
                meta: first.meta,
                control: PhysicalControlId::Vendor {
                    namespace: VendorNamespaceId(u32::MAX),
                    code: u32::MAX
                },
                value: -1.0,
                mode: AxisMode::Relative
            })
        ]
    );
    assert_eq!(first, retained);
    for event in &events {
        assert_eq!(constructor.bindings.map(event).count(), 1);
    }
    events.clear();
    assert_eq!(setup.decode_report(&first, &mut events).unwrap(), 1);
    assert!(
        matches!(&events[0], PhysicalInputEvent::Axis(axis) if axis.mode == AxisMode::Relative)
    );
    let inactive = raw(3, None, 1, 1, &[1]);
    events.clear();
    assert_eq!(setup.decode_report(&inactive, &mut events).unwrap(), 0);
    let down = raw(3, None, 2, 2, &[0]);
    setup.decode_report(&down, &mut events).unwrap();
    assert_eq!(
        events,
        [PhysicalInputEvent::Button(ButtonEvent {
            meta: down.meta,
            control: PhysicalControlId::Native {
                backend: BackendId(u32::MAX),
                code: u32::MAX
            },
            state: ButtonState::Down
        })]
    );
    events.clear();
    let axis = raw(3, Some(2), 3, 3, &[3]);
    setup.decode_report(&axis, &mut events).unwrap();
    assert!(
        matches!(&events[0], PhysicalInputEvent::Axis(value) if value.value == 5.0 && value.meta == axis.meta && value.mode == AxisMode::Absolute)
    );
    let mut empty_report = first.clone();
    empty_report.report_id = Some(2);
    empty_report.data.clear();
    events.clear();
    assert_eq!(setup.decode_report(&empty_report, &mut events).unwrap(), 0);
    assert!(events.is_empty());
    assert_eq!(
        BrowserHidSetup::new(&[], &[], &[], &[])
            .unwrap()
            .source_count(),
        0
    );
}

#[test]
fn numeric_admission_rejects_inconsistent_rows_tags_limits_or_unbound_controls_before_an_owner_exists()
 {
    let constructor = physical(2);
    let bindings = constructor.bindings.bindings();
    let dev = devices(SOURCE);
    let row = field(0, 1, 0);
    for bad in [
        vec![0; 5],
        [dev, dev].concat(),
        (0..17).flat_map(|index| devices(index + 3)).collect(),
    ] {
        assert!(BrowserHidSetup::new(&bad, &row, &[0.0, 0.0], bindings).is_err());
    }
    for (index, value) in [(0, 2), (2, 2), (3, 65536), (4, 2), (5, 65536)] {
        let mut bad = dev;
        bad[index] = value;
        if index == 0 {
            bad[1] = 0;
        }
        assert!(BrowserHidSetup::new(&bad, &row, &[0.0, 0.0], bindings).is_err());
    }
    let mut absent = dev;
    absent[2] = 0;
    assert!(BrowserHidSetup::new(&absent, &row, &[0.0, 0.0], bindings).is_err());
    assert!(BrowserHidSetup::new(&dev, &row[..12], &[0.0, 0.0], bindings).is_err());
    assert!(BrowserHidSetup::new(&[], &row, &[0.0, 0.0], bindings).is_err());
    for params in [
        vec![],
        vec![0.0],
        vec![0.0; 3],
        vec![1.0, 0.0],
        vec![-0.0, 0.0],
        vec![0.0, f32::NAN],
    ] {
        assert!(BrowserHidSetup::new(&dev, &row, &params, bindings).is_err());
    }
    for (index, value) in [
        (0, 1),
        (1, 2),
        (2, 0),
        (2, 256),
        (3, 1025),
        (4, 3),
        (5, 65536),
        (6, 65536),
        (7, u32::MAX),
        (8, 0),
        (8, 65),
        (9, 2),
        (10, 3),
        (11, 2),
        (12, 1),
    ] {
        let mut bad = row;
        bad[index] = value;
        assert!(
            BrowserHidSetup::new(&dev, &bad, &[0.0, 0.0], bindings).is_err(),
            "field word {index}"
        );
    }
    let mut absent_id = row;
    absent_id[1] = 0;
    assert!(BrowserHidSetup::new(&dev, &absent_id, &[0.0, 0.0], bindings).is_err());
    let mut axis = row;
    axis[10] = 1;
    axis[12] = 2;
    assert!(BrowserHidSetup::new(&dev, &axis, &[1.0, 0.0], bindings).is_err());
    axis[12] = 0;
    assert!(BrowserHidSetup::new(&dev, &axis, &[f32::INFINITY, 0.0], bindings).is_err());
    assert!(BrowserHidSetup::new(&dev, &row, &[0.0, 0.0], &[]).is_err());
    let mut wrong_source = bindings.to_vec();
    for binding in &mut wrong_source {
        binding.device = DeviceSelector::Exact(DeviceId(3));
    }
    assert!(BrowserHidSetup::new(&dev, &row, &[0.0, 0.0], &wrong_source).is_err());
    let mut inconsistent = field(0, 2, 1);
    inconsistent[3] = 2;
    for rows in [
        [row, inconsistent].concat(),
        [row, empty(0, 1)].concat(),
        [empty(0, 2), empty(0, 2), row].concat(),
    ] {
        assert!(
            BrowserHidSetup::new(&dev, &rows, &vec![0.0; rows.len() / 13 * 2], bindings).is_err()
        );
    }
    let mut duplicate = row;
    duplicate[2] = 2;
    assert!(BrowserHidSetup::new(&dev, &[row, duplicate].concat(), &[0.0; 4], bindings).is_err());
    assert!(BrowserHidSetup::new(&dev, &empty(0, 1), &[0.0; 2], bindings).is_err());
    assert!(
        BrowserHidSetup::new(
            &dev,
            &[row, empty(0, 2)].concat(),
            &[0.0, 0.0, 0.0, -0.0],
            bindings
        )
        .is_err()
    );
    let mut invalid_empty = empty(0, 2);
    invalid_empty[4] = 1;
    assert!(
        BrowserHidSetup::new(&dev, &[row, invalid_empty].concat(), &[0.0; 4], bindings).is_err()
    );
    let oversized = vec![0; (16 * 512 + 1) * 13];
    assert!(
        BrowserHidSetup::new(&dev, &oversized, &vec![0.0; (16 * 512 + 1) * 2], bindings).is_err()
    );
    let any = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: bindings[0].physical,
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let device_words = (3..19).flat_map(devices).collect::<Vec<_>>();
    let fields = (0..16)
        .flat_map(|index| field(index, 1, 0))
        .collect::<Vec<_>>();
    assert_eq!(
        BrowserHidSetup::new(&device_words, &fields, &[0.0; 32], any.bindings())
            .unwrap()
            .source_count(),
        16
    );
    let rows = std::iter::once(row)
        .chain((2..=255).map(|id| empty(0, id)))
        .chain(std::iter::once([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0]))
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 256 * 13);
    assert!(BrowserHidSetup::new(&dev, &rows, &[0.0; 512], bindings).is_ok());
}

#[test]
fn shared_order_checks_match_registry_and_browser_refusals_preserve_the_last_successful_report() {
    let first = raw(SOURCE, Some(1), 8, 10, &[1]);
    assert_eq!(validate_report_order(None, first.meta), Ok(()));
    assert_eq!(validate_report_order(Some(first.meta), first.meta), Ok(()));
    let mut cases = Vec::new();
    let mut changed = first.clone();
    changed.meta.sequence = 7;
    cases.push((changed, AdapterRegistryError::ReportSequence));
    let mut changed = first.clone();
    changed.meta.native.as_mut().unwrap().code = Some(2);
    cases.push((changed, AdapterRegistryError::ReportAcquisitionMismatch));
    let mut changed = first.clone();
    changed.meta.sequence = 9;
    changed.meta.clock_domain = ClockDomainId(12);
    cases.push((changed, AdapterRegistryError::ReportChronology));
    let mut changed = first.clone();
    changed.meta.sequence = 9;
    changed.meta.timestamp = ts(9);
    cases.push((changed, AdapterRegistryError::ReportChronology));
    let constructor = physical(1);
    for (bad, expected) in cases {
        assert_eq!(
            validate_report_order(Some(first.meta), bad.meta),
            Err(expected)
        );
        let mut native = registry();
        native.route(DeviceId(SOURCE), &first).unwrap();
        assert_eq!(native.route(DeviceId(SOURCE), &bad), Err(expected));
        let mut browser = buttons(1, constructor.bindings.bindings());
        let mut output = Vec::new();
        browser.decode_report(&first, &mut output).unwrap();
        output.clear();
        assert!(browser.decode_report(&bad, &mut output).is_err());
        assert!(output.is_empty());
        let mut sibling = first.clone();
        sibling.data = vec![0];
        assert_eq!(browser.decode_report(&sibling, &mut output).unwrap(), 1);
        assert_eq!(output, [typed_button(&sibling, 1, ButtonState::Up)]);
    }
    let mut browser = buttons(1, constructor.bindings.bindings());
    let mut output = Vec::new();
    browser.decode_report(&first, &mut output).unwrap();
    output.clear();
    let malformed = raw(SOURCE, Some(1), 20, 20, &[]);
    assert!(browser.decode_report(&malformed, &mut output).is_err());
    let valid = raw(SOURCE, Some(1), 9, 11, &[0]);
    assert_eq!(browser.decode_report(&valid, &mut output).unwrap(), 1);
    assert_eq!(output, [typed_button(&valid, 1, ButtonState::Up)]);
    // The native registry intentionally observes acquisition before its infallible
    // callback; the browser's fallible decoder adopts chronology only on success.
    let mut native = registry();
    native.route(DeviceId(SOURCE), &first).unwrap();
    assert!(
        matches!(native.route(DeviceId(SOURCE), &malformed).unwrap(), AdapterRoute::Handled { events, .. } if events.is_empty())
    );
    assert_eq!(
        native.route(DeviceId(SOURCE), &valid),
        Err(AdapterRegistryError::ReportSequence)
    );
    output.clear();
    let unknown = raw(SOURCE, Some(99), 30, 30, &[]);
    assert_eq!(browser.decode_report(&unknown, &mut output).unwrap(), 0);
    assert!(
        browser
            .decode_report(&raw(SOURCE, Some(1), 29, 29, &[1]), &mut output)
            .is_err()
    );
    assert!(
        browser
            .decode_report(&raw(3, Some(1), 31, 31, &[1]), &mut output)
            .is_err()
    );
    let final_report = raw(SOURCE, Some(1), 31, 31, &[1]);
    assert_eq!(
        browser.decode_report(&final_report, &mut output).unwrap(),
        1
    );
    assert_eq!(output, [typed_button(&final_report, 1, ButtonState::Down)]);
}

#[test]
fn actual_hid_bound_reports_share_step_capture_pressed_feedback_and_real_mixer_commands() {
    let constructor = physical(2);
    let mut decoder = buttons(2, constructor.bindings.bindings());
    let (mut game, bank, chart) = prepared_game(2, false, 8);
    assert!(game.input_setup_available());
    game.configure_capture(replay_limits(), u64::MAX).unwrap();
    assert!(game.input_setup_available());
    game.activate(config(8).host_origin).unwrap();
    assert!(!game.input_setup_available());
    let mut pressed = PressedKeys::default();
    let mut accepted = Vec::new();
    let mut judged = Vec::new();
    for (song, sequence, data, expected_mask) in
        [(-1, 1, 0, 0), (0, 2, 3, 3), (1, 3, 3, 3), (2, 4, 0, 0)]
    {
        let raw = raw(
            SOURCE,
            Some(1),
            sequence,
            host(song).timestamp.as_nanos(),
            &[data],
        );
        let mut events = Vec::new();
        decoder.decode_report(&raw, &mut events).unwrap();
        if events.is_empty() {
            let report = game
                .process_input(
                    PhysicalInputEvent::RawHidReport(raw.clone()),
                    &clocks(),
                    audio(song),
                )
                .unwrap();
            assert_eq!(report.song_time, ts(song));
            assert!(report.bound_inputs.is_empty());
            assert!(report.judge_events.is_empty());
            assert_eq!(report.input, Some(PhysicalInputEvent::RawHidReport(raw)));
            pressed.apply(&report.bound_inputs).unwrap();
        } else {
            for event in events {
                let report = game
                    .process_input(event.clone(), &clocks(), audio(song))
                    .unwrap();
                assert_eq!(report.bound_inputs.len(), 1);
                assert_eq!(report.bound_inputs[0].physical, event);
                assert_eq!(*report.bound_inputs[0].physical.meta(), raw.meta);
                pressed.apply(&report.bound_inputs).unwrap();
                accepted.extend(
                    report
                        .bound_inputs
                        .into_iter()
                        .map(|input| (report.song_time, input)),
                );
                judged.extend(report.judge_events);
            }
        }
        assert_eq!(pressed.mask(), expected_mask);
    }
    assert_eq!(judged.len(), 2);
    assert!(
        judged
            .iter()
            .all(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
    );
    let batch = game.take_commands(8).unwrap().unwrap();
    assert_eq!(batch.commands.len(), 2);
    let format = bank.format();
    let (mut producer, consumer) = command_queue(8).unwrap();
    for command in &batch.commands {
        producer.try_push(command.clone()).unwrap();
    }
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(22),
            ts(100),
            AudioLimits::new(8, 4, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pcm = [99.0; 4];
    let rendered = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.5, -0.25, 0.0]);
    assert_eq!(rendered.counters.commands_applied, 2);
    let hash = game.judge().stable_hash().unwrap();
    game.fail();
    assert!(!game.input_setup_available());
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), replay_limits()).unwrap();
    assert_eq!(
        decode_section_setup(&file.header.options)
            .unwrap()
            .chart_seed,
        u64::MAX
    );
    assert_eq!(
        file.records.len(),
        4,
        "zero-transition raw packets are not an invented typed-input archive"
    );
    for (record, (song, event)) in file.records.iter().zip(accepted) {
        assert_eq!(record.song_time, song);
        assert_eq!(record.operation, ReplayOperation::Input(event));
    }
    let source = parse_seeded(&chart, Default::default(), u64::MAX).unwrap();
    let replay = reconstruct_section(&source, file, replay_limits()).unwrap();
    assert_eq!(replay.results(), judged);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    let (mut unactivated, _, _) = prepared_game(2, false, 8);
    assert!(unactivated.input_setup_available());
    unactivated
        .process_input(
            PhysicalInputEvent::RawHidReport(raw(
                SOURCE,
                Some(99),
                1,
                host(-1).timestamp.as_nanos(),
                &[],
            )),
            &clocks(),
            audio(-1),
        )
        .unwrap();
    assert!(
        !unactivated.input_setup_available(),
        "even a valid zero-emission acquisition closes pristine setup"
    );
}

#[test]
fn committed_audio_failure_retains_the_actual_typed_prefix_and_fences_unprocessed_fanout_without_retry()
 {
    let constructor = physical(3);
    let mut decoder = buttons(3, constructor.bindings.bindings());
    let (mut game, _bank, chart) = prepared_game(3, true, 2);
    game.configure_capture(replay_limits(), u64::MAX).unwrap();
    let raw = raw(SOURCE, Some(1), 1, host(0).timestamp.as_nanos(), &[7]);
    let mut typed = Vec::new();
    assert_eq!(decoder.decode_report(&raw, &mut typed).unwrap(), 3);
    let first = game
        .process_input(typed[0].clone(), &clocks(), audio(0))
        .unwrap();
    assert_eq!(first.audio_commands.len(), 1);
    assert_eq!(first.judge_events.len(), 1);
    let error = game
        .process_input(typed[1].clone(), &clocks(), audio(0))
        .unwrap_err();
    let StepGameplayError::Report {
        report,
        score_error,
        capture_error,
    } = error
    else {
        panic!("second typed event must retain its committed report")
    };
    assert!(score_error.is_none() && capture_error.is_none());
    assert_eq!(report.bound_inputs.len(), 1);
    assert_eq!(report.bound_inputs[0].physical, typed[1]);
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.audio_failures.len(), 1);
    assert!(report.audio_commands.is_empty());
    assert_eq!((game.score().hits, game.score().misses), (2, 0));
    assert!(game.failed());
    assert!(!game.input_setup_available());
    let hash = game.judge().stable_hash().unwrap();
    assert!(matches!(
        game.process_input(typed[2].clone(), &clocks(), audio(0)),
        Err(StepGameplayError::Failed)
    ));
    assert!(matches!(
        game.take_commands(2),
        Err(StepGameplayError::Failed)
    ));
    assert_eq!(game.judge().stable_hash().unwrap(), hash);
    let mut repeated = Vec::new();
    assert_eq!(decoder.decode_report(&raw, &mut repeated).unwrap(), 0);
    let mut pressed = PressedKeys::default();
    pressed.apply(&first.bound_inputs).unwrap();
    pressed.apply(&report.bound_inputs).unwrap();
    assert_eq!(
        pressed.mask(),
        3,
        "only genuinely bound prefix owners affect display, never the discarded third control"
    );
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), replay_limits()).unwrap();
    assert_eq!(file.records.len(), 2);
    let source = parse_seeded(&chart, Default::default(), u64::MAX).unwrap();
    let replay = reconstruct_section(&source, file, replay_limits()).unwrap();
    let mut expected = first.judge_events;
    expected.extend(report.judge_events);
    assert_eq!(replay.results(), expected);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
}
