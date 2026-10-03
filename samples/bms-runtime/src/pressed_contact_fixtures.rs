//! Deferred contact feedback against actual bound reports and canonical replay.
use crate::{
    ChannelPolicy, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    pressed_keys::PressedKeys,
    replay_playback::{decode_section_setup, reconstruct_section},
    replay_visual::ReplayVisual,
    step_gameplay::{StepGameplay, StepGameplayConfig},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits},
    input::*,
    judge::{JudgeOutcome, JudgeStage},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, decode_replay},
    },
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
};
use beatkernel_bms::{BmsInputMode, parse_seeded};

const CHART: &str = "#BPM 60\n#WAV01 key.wav\n#00011:01\n#00052:00010100\n";
const DEVICE: u64 = 0xfedc_ba98_7654_3210;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn surface(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(u32::MAX),
        code,
    }
}
fn contact(device: u64, code: u32, lane: u32, id: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(lane),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: EventMeta::new(DeviceId(device), point(11, 10_250_000_000), 1),
            control: surface(code),
            contact: ContactId(id),
            phase,
            position: Position2 {
                x: 960.0,
                y: -720.0,
            },
            pressure: Some(0.75),
        }),
    }
}
fn button(device: u64, code: u32, lane: u32, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(lane),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: *contact(device, code, lane, 0, TouchPhase::Down)
                .physical
                .meta(),
            control: surface(code),
            state,
        }),
    }
}
fn changed(mut input: GameInputEvent, phase: TouchPhase) -> GameInputEvent {
    let PhysicalInputEvent::Touch(touch) = &mut input.physical else {
        panic!("contact fixture")
    };
    touch.phase = phase;
    input
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
fn host(song: i64) -> ClockPoint {
    point(11, 10_250_000_000 + song)
}
fn output(song: i64) -> ClockPoint {
    point(
        22,
        config().output_origin.timestamp.as_nanos() + 250_000_000 + song,
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
fn acquisition(mut input: GameInputEvent, song: i64, sequence: u64) -> PhysicalInputEvent {
    let meta = input.physical.meta_mut();
    meta.timestamp = host(song).timestamp;
    meta.sequence = sequence;
    meta.native = Some(NativeEventMeta {
        backend: BackendId(91),
        code: Some(u32::MAX),
        timestamp: Some(point(91, -123)),
    });
    meta.original_clock_point = Some(point(92, 9_007_199_254_740_993));
    input.physical
}
fn game() -> StepGameplay {
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
        .insert("pack/chart.bms", CHART.as_bytes().to_vec())
        .unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    let prepared = prepare_from_source(
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
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: surface(u32::MAX),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (mut game, _bank) = StepGameplay::new_section_with_input_mode(
        prepared,
        config(),
        bindings,
        Timestamp::ZERO,
        Some(ts(3_000_000_000)),
        BmsInputMode::ButtonOrContact,
    )
    .unwrap();
    let router = TouchRouter::new(
        [0x11, 0x12]
            .into_iter()
            .map(|lane| TouchRegion {
                device: DeviceSelector::Any,
                physical: surface(u32::MAX),
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
            .collect(),
        8,
    )
    .unwrap();
    game.configure_touch_router(router).unwrap();
    game
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}

#[test]
fn contact_owners_keep_full_identity_and_share_a_lane_without_releasing_buttons_or_other_contacts()
{
    let mut pressed = PressedKeys::default();
    let primary = contact(u64::MAX, u32::MAX, 0x11, u64::MAX, TouchPhase::Down);
    let siblings = [
        contact(u32::MAX as u64, u32::MAX, 0x11, u64::MAX, TouchPhase::Down),
        contact(u64::MAX, u16::MAX as u32, 0x11, u64::MAX, TouchPhase::Down),
        contact(u64::MAX, u32::MAX, 0x11, u32::MAX as u64, TouchPhase::Down),
        contact(u64::MAX, u32::MAX, 0x29, u64::MAX, TouchPhase::Down),
    ];
    pressed
        .apply(&[
            changed(primary.clone(), TouchPhase::Move),
            changed(primary.clone(), TouchPhase::Up),
            changed(primary.clone(), TouchPhase::Cancel),
            button(u64::MAX, u32::MAX, 0x11, ButtonState::Repeat),
        ])
        .unwrap();
    assert_eq!(pressed.mask(), 0);
    pressed.apply(std::slice::from_ref(&primary)).unwrap();
    pressed.apply(std::slice::from_ref(&primary)).unwrap();
    pressed
        .apply(&[changed(primary.clone(), TouchPhase::Cancel)])
        .unwrap();
    assert_eq!(
        pressed.mask(),
        0,
        "duplicate Down must not leave a second owner after one Cancel"
    );
    pressed.apply(std::slice::from_ref(&primary)).unwrap();
    pressed.apply(&siblings).unwrap();
    pressed
        .apply(&[button(u64::MAX, u32::MAX, 0x11, ButtonState::Down)])
        .unwrap();
    assert_eq!(pressed.mask(), 1 | (1 << 17));
    pressed
        .apply(&[changed(primary.clone(), TouchPhase::Up)])
        .unwrap();
    assert_eq!(pressed.mask(), 1 | (1 << 17));
    for sibling in &siblings[..3] {
        pressed
            .apply(&[changed(sibling.clone(), TouchPhase::Cancel)])
            .unwrap();
        assert_eq!(
            pressed.mask(),
            1 | (1 << 17),
            "the same-source button still owns lane 11"
        );
    }
    pressed
        .apply(&[button(u64::MAX, u32::MAX, 0x11, ButtonState::Up)])
        .unwrap();
    assert_eq!(
        pressed.mask(),
        1 << 17,
        "a release for game control 11 cannot release control 29"
    );
    let mut relocated = changed(siblings[3].clone(), TouchPhase::Move);
    let PhysicalInputEvent::Touch(touch) = &mut relocated.physical else {
        unreachable!()
    };
    touch.position = Position2 {
        x: f32::INFINITY,
        y: f32::NAN,
    };
    pressed
        .apply(&[relocated, contact(1, 1, u32::MAX, 1, TouchPhase::Down)])
        .unwrap();
    assert_eq!(
        pressed.mask(),
        1 << 17,
        "feedback consumes admitted controls; it never re-hit-tests position"
    );
    pressed
        .apply(&[changed(siblings[3].clone(), TouchPhase::Up)])
        .unwrap();
    assert_eq!(pressed.mask(), 0);
    let vendor = GameInputEvent {
        game_control: GameControlId(0x11),
        physical: PhysicalInputEvent::Axis(AxisEvent {
            meta: *primary.physical.meta(),
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(u32::MAX),
                code: u32::MAX,
            },
            value: 1.0,
            mode: AxisMode::Absolute,
        }),
    };
    pressed.apply(&[vendor]).unwrap();
    assert_eq!(pressed.mask(), 0);
}

#[test]
fn mixed_capacity_refusal_keeps_the_entire_previous_prefix_and_prepare_commit_remains_atomic() {
    let contacts: Vec<_> = (0..4094)
        .map(|id| contact(DEVICE, 7, 0x11, id, TouchPhase::Down))
        .collect();
    let mut full = contacts.clone();
    full.extend([
        button(1, 3, 0x29, ButtonState::Down),
        button(1, 4, 0x29, ButtonState::Down),
    ]);
    let mut pressed = PressedKeys::default();
    pressed.apply(&full).unwrap();
    let mask = 1 | (1 << 17);
    assert_eq!(pressed.mask(), mask);
    let extra = contact(DEVICE, 7, 0x12, u64::MAX, TouchPhase::Down);
    let overflow = [
        button(1, 3, 0x29, ButtonState::Up),
        extra.clone(),
        button(9, 9, 0x13, ButtonState::Down),
    ];
    assert!(pressed.prepare(&overflow).is_err());
    assert_eq!(pressed.mask(), mask);
    assert!(pressed.apply(&overflow).is_err());
    assert_eq!(pressed.mask(), mask);
    assert!(pressed.apply(std::slice::from_ref(&extra)).is_err());
    pressed
        .apply(&[
            changed(extra.clone(), TouchPhase::Cancel),
            changed(contacts[0].clone(), TouchPhase::Move),
            contacts[0].clone(),
            button(1, 3, 0x29, ButtonState::Repeat),
        ])
        .unwrap();
    pressed
        .apply(&[button(1, 4, 0x29, ButtonState::Up)])
        .unwrap();
    assert_eq!(
        pressed.mask(),
        mask,
        "the release staged in the rejected batch did not commit"
    );
    let replacement = [button(1, 3, 0x29, ButtonState::Up), extra.clone()];
    let candidate = pressed.prepare(&replacement).unwrap().unwrap();
    assert_eq!(candidate, 1 | 2);
    assert_eq!(
        pressed.mask(),
        mask,
        "prepared output does not replace visible state before commit"
    );
    pressed.commit(candidate);
    assert_eq!(pressed.mask(), 3);
    let releases = contacts
        .iter()
        .cloned()
        .map(|input| changed(input, TouchPhase::Up))
        .collect::<Vec<_>>();
    pressed.apply(&releases).unwrap();
    assert_eq!(pressed.mask(), 2);
    pressed.apply(&[changed(extra, TouchPhase::Up)]).unwrap();
    assert_eq!(pressed.mask(), 0);
    pressed.apply(&full).unwrap();
    pressed.clear();
    assert_eq!(pressed.mask(), 0);
    pressed
        .apply(&[
            changed(contacts[0].clone(), TouchPhase::Up),
            button(1, 3, 0x29, ButtonState::Up),
        ])
        .unwrap();
    pressed
        .apply(&[contact(DEVICE, 7, 0x21, u64::MAX, TouchPhase::Down)])
        .unwrap();
    assert_eq!(pressed.mask(), 1 << 9);
}

#[test]
fn actual_projected_runtime_reports_drive_contact_feedback_with_original_payload_and_shared_buttons()
 {
    let mut game = game();
    game.activate(config().host_origin).unwrap();
    let mut pressed = PressedKeys::default();
    let mut emitted = Vec::new();
    let operations = [
        (
            0,
            contact(DEVICE, u32::MAX, 0x11, 1, TouchPhase::Down),
            2.0,
            Some(0x11),
            1,
        ),
        (
            1,
            button(DEVICE, u32::MAX, 0x11, ButtonState::Down),
            999.0,
            Some(0x11),
            1,
        ),
        (
            2,
            contact(DEVICE, u32::MAX, 0x11, 1, TouchPhase::Move),
            12.0,
            Some(0x11),
            1,
        ),
        (
            3,
            contact(DEVICE, u32::MAX, 0x11, 1, TouchPhase::Cancel),
            -1.0,
            Some(0x11),
            1,
        ),
        (
            4,
            button(DEVICE, u32::MAX, 0x11, ButtonState::Up),
            -1.0,
            Some(0x11),
            0,
        ),
        (
            5,
            contact(DEVICE, u32::MAX, 0x11, 99, TouchPhase::Move),
            2.0,
            None,
            0,
        ),
        (
            6,
            contact(DEVICE, u32::MAX, 0x11, 2, TouchPhase::Down),
            30.0,
            None,
            0,
        ),
        (
            7,
            contact(DEVICE, u32::MAX, 0x11, 2, TouchPhase::Move),
            2.0,
            None,
            0,
        ),
        (
            8,
            contact(DEVICE, u32::MAX, 0x11, 2, TouchPhase::Up),
            2.0,
            None,
            0,
        ),
        (
            1_000_000_000,
            contact(DEVICE, u32::MAX, 0x12, 3, TouchPhase::Down),
            12.0,
            Some(0x12),
            2,
        ),
        (
            1_500_000_000,
            contact(DEVICE, u32::MAX, 0x12, 3, TouchPhase::Move),
            999.0,
            Some(0x12),
            2,
        ),
        (
            1_750_000_000,
            contact(DEVICE, u32::MAX, 0x12, 3, TouchPhase::Cancel),
            -100.0,
            Some(0x12),
            0,
        ),
    ];
    for (sequence, (song, input, x, lane, mask)) in operations.into_iter().enumerate() {
        let original = acquisition(input, song, sequence as u64 + 1);
        let report = game
            .process_input_at(
                original.clone(),
                Position2 { x, y: 2.0 },
                &mapper(),
                output(song),
            )
            .unwrap();
        match lane {
            Some(lane) => assert_eq!(
                report.bound_inputs,
                [GameInputEvent {
                    game_control: GameControlId(lane),
                    physical: original
                }]
            ),
            None => {
                assert!(report.bound_inputs.is_empty());
                assert!(report.judge_events.is_empty());
            }
        }
        pressed.apply(&report.bound_inputs).unwrap();
        assert_eq!(pressed.mask(), mask);
        emitted.extend(report.judge_events);
    }
    assert_eq!(
        emitted.iter().map(|event| event.stage).collect::<Vec<_>>(),
        [
            JudgeStage::Instant,
            JudgeStage::HoldHead,
            JudgeStage::HoldTail
        ]
    );
    assert!(matches!(emitted[2].outcome, JudgeOutcome::Miss { .. }));
    assert_eq!((game.score().hits, game.score().misses), (2, 1));
    assert_eq!(pressed.mask(), 0);
}

#[test]
fn typed_captured_contact_prefix_replays_the_same_masks_without_synthetic_release_or_future_judgements()
 {
    let mut game = game();
    game.configure_capture(limits(), u64::MAX).unwrap();
    game.activate(config().host_origin).unwrap();
    let mut pressed = PressedKeys::default();
    let mut states = Vec::new();
    let mut accepted = Vec::new();
    let mut events = Vec::new();
    for (sequence, (song, input, x, expected)) in [
        (
            0,
            contact(DEVICE, u32::MAX, 0x11, u64::MAX, TouchPhase::Down),
            2.0,
            1,
        ),
        (
            100_000_000,
            button(DEVICE, u32::MAX, 0x11, ButtonState::Down),
            2.0,
            1,
        ),
        (
            200_000_000,
            contact(DEVICE, u32::MAX, 0x11, u64::MAX, TouchPhase::Move),
            12.0,
            1,
        ),
        (
            300_000_000,
            contact(DEVICE, u32::MAX, 0x11, u64::MAX, TouchPhase::Cancel),
            -2.0,
            1,
        ),
        (
            400_000_000,
            button(DEVICE, u32::MAX, 0x11, ButtonState::Up),
            2.0,
            0,
        ),
        (
            500_000_000,
            contact(DEVICE, u32::MAX, 0x11, 9, TouchPhase::Move),
            2.0,
            0,
        ),
        (
            1_000_000_000,
            contact(DEVICE, u32::MAX, 0x12, 2, TouchPhase::Down),
            12.0,
            2,
        ),
        (
            1_500_000_000,
            contact(DEVICE, u32::MAX, 0x12, 2, TouchPhase::Move),
            2.0,
            2,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let original = acquisition(input, song, sequence as u64 + 1);
        let report = game
            .process_input_at(original, Position2 { x, y: 2.0 }, &mapper(), output(song))
            .unwrap();
        pressed.apply(&report.bound_inputs).unwrap();
        assert_eq!(pressed.mask(), expected);
        states.push((ts(song), expected, report.judge_events.clone()));
        accepted.extend(
            report
                .bound_inputs
                .into_iter()
                .map(|input| (report.song_time, input)),
        );
        events.extend(report.judge_events);
    }
    let hash = game.judge().stable_hash().unwrap();
    game.fail();
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    let setup = decode_section_setup(&file.header.options).unwrap();
    assert_eq!(setup.input_mode, BmsInputMode::ButtonOrContact);
    assert_eq!(setup.end, Some(ts(3_000_000_000)));
    assert_eq!(setup.chart_seed, u64::MAX);
    let recorded = file
        .records
        .iter()
        .filter_map(|record| match &record.operation {
            ReplayOperation::Input(input) => Some((record.song_time, input.clone())),
            ReplayOperation::Advance => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(recorded, accepted);
    assert_eq!(
        recorded.len(),
        7,
        "unknown Move has no bound operation to invent in the log"
    );
    let source = parse_seeded(CHART, Default::default(), u64::MAX).unwrap();
    let replay = reconstruct_section(&source, file.clone(), limits()).unwrap();
    assert_eq!(replay.results(), events);
    assert_eq!(replay.engine().stable_hash().unwrap(), hash);
    let mut visual = ReplayVisual::new_section(&source, &file, limits()).unwrap();
    assert_eq!(visual.pressed_lanes(), 0);
    assert!(visual.advance_to(ts(-1)).unwrap().is_empty());
    for (at, mask, expected_events) in states {
        assert_eq!(visual.advance_to(at).unwrap(), expected_events);
        assert_eq!(visual.pressed_lanes(), mask);
        assert!(visual.advance_to(at).unwrap().is_empty());
        assert_eq!(visual.pressed_lanes(), mask);
    }
    assert!(visual.finished());
    assert_eq!(visual.recorded_until(), Some(ts(1_500_000_000)));
    assert!(visual.advance_to(ts(3_000_000_000)).unwrap().is_empty());
    assert_eq!(
        visual.pressed_lanes(),
        2,
        "an ended recording prefix cannot fabricate the missing hold release"
    );
    assert!(visual.advance_to(ts(1_500_000_000)).is_err());
    assert_eq!(visual.pressed_lanes(), 2);
}
