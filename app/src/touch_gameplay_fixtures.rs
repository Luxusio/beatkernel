//! Deferred portable region setup and real shared solo/cohort/stepped owners.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    browser_input::TouchInputSetup,
    local_players::PlayerId,
    local_runtime::{FailureKind, InputResult, MemberConfig, RuntimeGroup, SoloRuntime},
    replay_playback::{decode_section_setup, reconstruct_section},
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, command_queue},
    input::*,
    interaction::InteractionState,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay},
    },
    runtime::{RuntimeError, RuntimeProcessingClock},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsInputMode, parse_seeded};

const CHART: &str = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00052:00010100\n";
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn pos(x: f32) -> Position2 {
    Position2 { x, y: 2.0 }
}
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(17),
        code: 1,
    }
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 604_800_000_000_017),
        preroll: Duration::from_nanos(250_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(4_000_000_000),
        telemetry_capacity: 8,
    }
}
fn transport() -> Transport {
    Transport::new(
        config().host_origin.timestamp,
        ts(-250_000_000),
        Rate::NORMAL,
    )
}
fn mapper() -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: config().host_origin,
            target: config().output_origin,
        },
        ClockInterval {
            start: config().host_origin.timestamp,
            end: ts(30_000_000_000),
        },
    )
    .unwrap()
}
fn host(song: i64) -> ClockPoint {
    point(11, 10_250_000_000 + song)
}
fn output(song: i64) -> ClockPoint {
    point(
        22,
        config().output_origin.timestamp.as_nanos() + 250_000_000 + song,
    )
}
fn input(song: i64, seq: u64, device: u64, contact: u64, phase: TouchPhase) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(device), host(song), seq);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(1),
        timestamp: Some(point(91, -123)),
    });
    meta.original_clock_point = Some(point(92, 9_007_199_254_740_993));
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(),
        contact: ContactId(contact),
        phase,
        position: Position2 { x: 960.0, y: 720.0 },
        pressure: Some(12.5),
    })
}
fn bindings(selector: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([0x11, 0x12].map(|lane| Binding {
        device: selector,
        physical: surface(),
        game_control: GameControlId(lane),
    }))
    .unwrap()
}
fn router(selector: DeviceSelector) -> TouchRouter {
    TouchRouter::new(
        [0x11, 0x12]
            .map(|lane| TouchRegion {
                device: selector,
                physical: surface(),
                game_control: GameControlId(lane),
                min: Position2 {
                    x: (lane - 0x11) as f32 * 10.0,
                    y: 0.0,
                },
                max: Position2 {
                    x: (lane - 0x10) as f32 * 10.0,
                    y: 10.0,
                },
            })
            .to_vec(),
        4,
    )
    .unwrap()
}
fn judge() -> JudgeEngine {
    let source = parse_seeded(CHART, Default::default(), u64::MAX).unwrap();
    JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules_with_input_mode(BmsInputMode::ButtonOrContact),
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
fn prepared() -> PreparedBms {
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    for n in [1u16, 1] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    for n in [4u32, 8] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    for n in [2u16, 16] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4u32.to_le_bytes());
    for n in [8192i16, -4096] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", CHART.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    prepare_from_source(
        CHART.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        u64::MAX,
        None,
    )
    .unwrap()
}
fn game(mode: BmsInputMode, end: Option<i64>) -> StepGameplay {
    StepGameplay::new_section_with_input_mode(
        prepared(),
        config(),
        bindings(DeviceSelector::Any),
        Timestamp::ZERO,
        end.map(ts),
        mode,
    )
    .unwrap()
    .0
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn recording(owner: &mut StepGameplay) -> ReplayFile {
    owner.fail();
    decode_replay(&owner.take_replay().unwrap().unwrap(), limits()).unwrap()
}

#[test]
fn wire_regions_keep_lossless_identity_allow_adjacent_same_lane_and_require_only_prepared_destinations()
 {
    let exact = [0x11, 1, 0x89ab_cdef, 0xfedc_ba98, 1, 17, 1];
    let vendor = [0x12, 0, 0, 0, 2, u32::MAX, u32::MAX];
    let words = [exact, vendor].concat();
    let bounds = [0.0, 0.0, 10.0, 10.0, 10.0, 0.0, 20.0, 10.0];
    let setup = TouchInputSetup::new(&words, &bounds, &[0x11, 0x12, 0x13], 4096).unwrap();
    let regions = setup.router.regions();
    assert_eq!(
        regions[0].device,
        DeviceSelector::Exact(DeviceId(0xfedc_ba98_89ab_cdef))
    );
    assert_eq!(regions[0].physical, surface());
    assert_eq!(
        regions[1].physical,
        PhysicalControlId::Vendor {
            namespace: VendorNamespaceId(u32::MAX),
            code: u32::MAX
        }
    );
    assert_eq!(setup.router.max_contacts(), 4096);
    assert!(
        TouchInputSetup::new(&[], &[], &[0x11, 0x12], 1)
            .unwrap()
            .router
            .regions()
            .is_empty()
    );
    let mut adjacent = TouchInputSetup::new(&[exact, exact].concat(), &bounds, &[0x11], 2)
        .unwrap()
        .router;
    let event = input(0, 1, 0xfedc_ba98_89ab_cdef, u64::MAX, TouchPhase::Down);
    assert!(matches!(
        adjacent.route_at(&event, pos(15.0)).unwrap(),
        TouchRoute::Bound(GameInputEvent {
            game_control: GameControlId(0x11),
            ..
        })
    ));
    assert!(
        TouchInputSetup::new(
            &[exact, exact].concat(),
            &[0.0, 0.0, 10.0, 10.0, 9.0, 0.0, 20.0, 10.0],
            &[0x11],
            2
        )
        .is_err()
    );
    for (index, value) in [(0, 0x13), (0, 0xff), (1, 2), (4, 3)] {
        let mut bad = exact;
        bad[index] = value;
        assert!(TouchInputSetup::new(&bad, &bounds[..4], &[0x11], 1).is_err());
    }
    let mut any = exact;
    any[1] = 0;
    assert!(TouchInputSetup::new(&any, &bounds[..4], &[0x11], 1).is_err());
    for (page, usage) in [(65_536, 1), (1, 65_536)] {
        let row = [0x11, 0, 0, 0, 0, page, usage];
        assert!(TouchInputSetup::new(&row, &bounds[..4], &[0x11], 1).is_err());
    }
    let hid = TouchInputSetup::new(
        &[0x11, 1, 0, 0, 0, 65_535, 65_535],
        &bounds[..4],
        &[0x11],
        1,
    )
    .unwrap();
    assert_eq!(
        hid.router.regions()[0].physical,
        PhysicalControlId::HidUsage {
            usage_page: u16::MAX,
            usage: u16::MAX
        }
    );
    for cap in [0, 4097, u32::MAX] {
        assert!(TouchInputSetup::new(&exact, &bounds[..4], &[0x11], cap).is_err());
    }
    for lanes in [vec![], vec![0xff], vec![0x11; 19]] {
        assert!(TouchInputSetup::new(&exact, &bounds[..4], &lanes, 1).is_err());
    }
    for length in 0..7 {
        assert!(TouchInputSetup::new(&exact[..length], &bounds[..4], &[0x11], 1).is_err());
    }
    assert!(TouchInputSetup::new(&exact, &bounds[..3], &[0x11], 1).is_err());
    assert!(TouchInputSetup::new(&exact, &[0.0, 0.0, f32::NAN, 10.0], &[0x11], 1).is_err());
    let many = (0..256).flat_map(|_| exact).collect::<Vec<_>>();
    let extents = (0..256)
        .flat_map(|index| [index as f32, 0.0, index as f32 + 1.0, 1.0])
        .collect::<Vec<_>>();
    assert_eq!(
        TouchInputSetup::new(&many, &extents, &[0x11], 1)
            .unwrap()
            .router
            .regions()
            .len(),
        256
    );
    let mut excess = many;
    excess.extend(exact);
    assert!(TouchInputSetup::new(&excess, &extents, &[0x11], 1).is_err());
}

#[test]
fn actual_solo_and_cohort_share_routing_while_exact_device_setup_and_failure_guards_remain_enforced()
 {
    let (producer, _consumer) = command_queue(16).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(11),
        ClockDomainId(22),
        transport(),
        bindings(DeviceSelector::Any),
        judge(),
        producer,
        vec![],
        0,
    )
    .unwrap();
    solo.set_processing_clock(RuntimeProcessingClock::Disabled);
    solo.configure_touch_router(router(DeviceSelector::Any))
        .unwrap();
    assert!(
        solo.configure_touch_router(router(DeviceSelector::Any))
            .is_err()
    );
    let original = input(0, 1, 999, 7, TouchPhase::Down);
    let report = solo
        .process_input_at(original.clone(), pos(2.0), &mapper(), output(0))
        .unwrap();
    assert_eq!(
        report.bound_inputs,
        [GameInputEvent {
            game_control: GameControlId(0x11),
            physical: original
        }]
    );
    assert_eq!(report.judge_events.len(), 1);
    assert!(
        solo.configure_touch_router(router(DeviceSelector::Any))
            .is_err()
    );
    let (producer, _consumer) = command_queue(16).unwrap();
    let configs = [(1, 10), (2, 20)]
        .map(|(player, device)| MemberConfig {
            player: PlayerId(player),
            device: Some(DeviceId(device)),
            bindings: bindings(DeviceSelector::Exact(DeviceId(device))),
            judge: judge(),
            sounds: vec![],
        })
        .into_iter()
        .collect();
    let mut group = RuntimeGroup::new(
        ClockDomainId(11),
        ClockDomainId(22),
        transport(),
        producer,
        configs,
        0,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    assert!(
        group
            .configure_touch_router(PlayerId(3), router(DeviceSelector::Exact(DeviceId(10))))
            .is_err()
    );
    assert!(
        group
            .configure_touch_router(PlayerId(1), router(DeviceSelector::Any))
            .is_err()
    );
    assert!(
        group
            .configure_touch_router(PlayerId(1), router(DeviceSelector::Exact(DeviceId(20))))
            .is_err()
    );
    group
        .configure_touch_router(PlayerId(1), router(DeviceSelector::Exact(DeviceId(10))))
        .unwrap();
    assert!(matches!(
        group
            .process_input_at(
                input(0, 1, 99, 7, TouchPhase::Down),
                pos(2.0),
                &mapper(),
                output(0)
            )
            .unwrap(),
        InputResult::Ignored {
            device: DeviceId(99)
        }
    ));
    group
        .configure_touch_router(PlayerId(2), router(DeviceSelector::Exact(DeviceId(20))))
        .unwrap();
    for (player, device) in [(1, 10), (2, 20)] {
        let result = group
            .process_input_at(
                input(0, 1, device, 7, TouchPhase::Down),
                pos(2.0),
                &mapper(),
                output(0),
            )
            .unwrap();
        let InputResult::Processed(reports) = result else {
            panic!("assigned source must reach its real member")
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, PlayerId(player));
        assert_eq!(reports[0].report.judge_events.len(), 1);
        assert_eq!(
            reports[0].report.bound_inputs[0].physical.meta().source,
            DeviceId(device)
        );
    }
    assert!(!group.poisoned());
    assert!(
        group
            .configure_touch_router(PlayerId(1), router(DeviceSelector::Exact(DeviceId(10))))
            .is_err()
    );
    let error = group
        .process_input_at(
            input(1, 2, 10, 7, TouchPhase::Up),
            pos(f32::NAN),
            &mapper(),
            output(1),
        )
        .unwrap_err();
    assert_eq!(error.failed_player, Some(PlayerId(1)));
    assert!(matches!(
        error.kind,
        FailureKind::Core(RuntimeError::TouchRouting(
            TouchRoutingError::NonFiniteSample
        ))
    ));
    assert!(error.completed_reports.is_empty());
    assert!(group.poisoned());
    assert!(
        group
            .configure_touch_router(PlayerId(2), router(DeviceSelector::Exact(DeviceId(20))))
            .is_err()
    );
    assert!(matches!(
        group
            .process_input_at(
                input(1, 2, 20, 7, TouchPhase::Up),
                pos(2.0),
                &mapper(),
                output(1)
            )
            .unwrap_err()
            .kind,
        FailureKind::Poisoned
    ));
}

#[test]
fn real_stepped_region_capture_replays_original_touch_events_and_preserves_the_finite_input_boundary()
 {
    let mut owner = game(BmsInputMode::ButtonOrContact, Some(3_000_000_000));
    owner
        .configure_touch_router(router(DeviceSelector::Any))
        .unwrap();
    owner.configure_capture(limits(), u64::MAX).unwrap();
    owner.activate(config().host_origin).unwrap();
    let mut expected = Vec::new();
    let mut events = Vec::new();
    for (song, sequence, contact, phase, projected, lane) in [
        (0, 1, 7, TouchPhase::Down, 2.0, 0x11),
        (0, 2, 7, TouchPhase::Move, 12.0, 0x11),
        (1_000_000_000, 3, 8, TouchPhase::Down, 12.0, 0x12),
        (1_500_000_000, 4, 8, TouchPhase::Move, 2.0, 0x12),
        (2_000_000_000, 5, 8, TouchPhase::Up, -1.0, 0x12),
    ] {
        let original = input(song, sequence, 77, contact, phase);
        let report = owner
            .process_input_at(original.clone(), pos(projected), &mapper(), output(song))
            .unwrap();
        assert_eq!(
            report.bound_inputs,
            [GameInputEvent {
                game_control: GameControlId(lane),
                physical: original.clone()
            }]
        );
        expected.push((ts(song), report.bound_inputs[0].clone()));
        events.extend(report.judge_events);
    }
    let end = owner
        .process_input_at(
            input(3_000_000_000, 6, 77, 9, TouchPhase::Down),
            pos(f32::NAN),
            &mapper(),
            output(3_000_000_000),
        )
        .unwrap();
    assert!(end.song_end_reached && end.bound_inputs.is_empty() && end.input.is_none());
    assert_eq!(end.song_time, ts(3_000_000_000));
    events.extend(end.judge_events);
    assert_eq!(
        events.iter().map(|event| event.stage).collect::<Vec<_>>(),
        [
            JudgeStage::Instant,
            JudgeStage::HoldHead,
            JudgeStage::HoldTail
        ]
    );
    assert_eq!((owner.score().hits, owner.score().misses), (3, 0));
    let hash = owner.judge().stable_hash().unwrap();
    let file = recording(&mut owner);
    let setup = decode_section_setup(&file.header.options).unwrap();
    assert_eq!(setup.input_mode, BmsInputMode::ButtonOrContact);
    assert_eq!(setup.end, Some(ts(3_000_000_000)));
    assert_eq!(file.records.len(), expected.len() + 1);
    for (record, (song, event)) in file.records.iter().zip(&expected) {
        assert_eq!(record.song_time, *song);
        assert_eq!(record.operation, ReplayOperation::Input(event.clone()));
    }
    assert_eq!(
        file.records.last().unwrap().operation,
        ReplayOperation::Advance
    );
    let source = parse_seeded(CHART, Default::default(), u64::MAX).unwrap();
    let replay = reconstruct_section(&source, file, limits()).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
}

#[test]
fn stepped_setup_locks_are_recoverable_and_projected_input_failures_fence_only_after_the_committed_prefix()
 {
    let mut buttons = game(BmsInputMode::ButtonOnly, None);
    assert!(matches!(
        buttons.configure_touch_router(router(DeviceSelector::Any)),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!buttons.failed());
    let physical = PhysicalInputEvent::Button(ButtonEvent {
        meta: *input(0, 1, 77, 1, TouchPhase::Down).meta(),
        control: surface(),
        state: ButtonState::Down,
    });
    assert_eq!(
        buttons
            .process_input(physical, &mapper(), output(0))
            .unwrap()
            .judge_events
            .len(),
        1
    );
    let mut activated = game(BmsInputMode::ButtonOrContact, None);
    activated.activate(config().host_origin).unwrap();
    assert!(matches!(
        activated.configure_touch_router(router(DeviceSelector::Any)),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!activated.failed());
    let mut started = game(BmsInputMode::ButtonOrContact, None);
    started
        .advance_to(config().host_origin, &mapper(), config().output_origin)
        .unwrap();
    assert!(matches!(
        started.configure_touch_router(router(DeviceSelector::Any)),
        Err(StepGameplayError::InvalidConfiguration(_))
    ));
    assert!(!started.failed());
    let mut owner = game(BmsInputMode::ButtonOrContact, None);
    owner
        .configure_touch_router(router(DeviceSelector::Any))
        .unwrap();
    assert!(matches!(
        owner.configure_touch_router(router(DeviceSelector::Any)),
        Err(StepGameplayError::Setup(_))
    ));
    assert!(!owner.failed());
    owner.configure_capture(limits(), u64::MAX).unwrap();
    let original = input(0, 1, u64::MAX, u64::MAX, TouchPhase::Down);
    let report = owner
        .process_input_at(original.clone(), pos(2.0), &mapper(), output(0))
        .unwrap();
    assert_eq!(report.bound_inputs[0].physical, original);
    assert_eq!(report.judge_events.len(), 1);
    let hash = owner.judge().stable_hash().unwrap();
    let score = owner.score().clone();
    let error = owner
        .process_input_at(
            input(1, 2, u64::MAX, u64::MAX, TouchPhase::Up),
            pos(f32::NAN),
            &mapper(),
            output(1),
        )
        .unwrap_err();
    assert!(
        matches!(error, StepGameplayError::Runtime(ref group) if matches!(group.kind, FailureKind::Core(RuntimeError::TouchRouting(TouchRoutingError::NonFiniteSample))))
    );
    assert!(owner.failed());
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    assert_eq!(owner.score(), &score);
    assert!(matches!(
        owner.configure_touch_router(router(DeviceSelector::Any)),
        Err(StepGameplayError::Failed)
    ));
    let file = recording(&mut owner);
    assert_eq!(file.records.len(), 1);
    let ReplayOperation::Input(captured) = &file.records[0].operation else {
        panic!("one actual admitted input is retained")
    };
    assert_eq!(captured.physical, original);
    let source = parse_seeded(CHART, Default::default(), u64::MAX).unwrap();
    let replay = reconstruct_section(&source, file, limits()).unwrap();
    assert_eq!(replay.results(), report.judge_events);
    let hold = source
        .compile()
        .unwrap()
        .chart
        .objects()
        .iter()
        .find(|object| object.time.end.is_some())
        .unwrap()
        .id;
    assert_eq!(replay.engine().state(hold), Some(InteractionState::Pending));
}
