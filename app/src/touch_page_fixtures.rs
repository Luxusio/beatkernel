//! Deferred page routing over genuine local owners, resolved capture and replay.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::prepare_local_members,
    local_runtime::{InputResult, PlayerReport, RuntimeGroup},
    replay_playback::{decode_section_setup, reconstruct_section},
    step_gameplay::{StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, command_queue},
    input::*,
    judge::{JudgeGrade, JudgeProfile, JudgeWindow},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, decode_replay},
    },
    runtime::RuntimeProcessingClock,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::BmsInputMode;

const MEMBERS: [PlayerId; 2] = [PlayerId(7), PlayerId(u32::MAX)];
const SOURCES: [DeviceId; 2] = [DeviceId(2), DeviceId(u64::MAX)];
const CHART: &[u8] =
    b"#BPM 60\n#WAV01 key.wav\n#00051:01000100\n#00012:00010000\n#00011:00000001\n";
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn host(song: i64) -> ClockPoint {
    point(11, 10_100_000_000 + song)
}
fn audio(song: i64) -> ClockPoint {
    point(22, 100_000_000 + song)
}
fn pos(x: f32) -> Position2 {
    Position2 { x, y: 2.0 }
}
struct OriginalClock;
impl ClockMapper for OriginalClock {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn surface() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x5754_4f55),
        code: 0,
    }
}
fn plan() -> ResolvedInputPlan {
    ResolvedInputPlan::new(MEMBERS.into_iter().zip(SOURCES.map(Some)).collect()).unwrap()
}
fn bindings() -> Vec<BindingMap> {
    SOURCES
        .iter()
        .map(|source| {
            BindingMap::from_bindings([0x11, 0x12].map(|lane| Binding {
                device: DeviceSelector::Exact(*source),
                physical: surface(),
                game_control: GameControlId(lane),
            }))
            .unwrap()
        })
        .collect()
}
fn regions(member: usize, shift: f32) -> Vec<TouchRegion> {
    [0x11, 0x12]
        .into_iter()
        .enumerate()
        .map(|(index, lane)| TouchRegion {
            device: DeviceSelector::Exact(SOURCES[member]),
            physical: surface(),
            game_control: GameControlId(lane),
            min: Position2 {
                x: shift + index as f32 * 10.0,
                y: 0.0,
            },
            max: Position2 {
                x: shift + (index + 1) as f32 * 10.0,
                y: 10.0,
            },
        })
        .collect()
}
fn router(member: usize) -> TouchRouter {
    TouchRouter::new(regions(member, 0.0), 8).unwrap()
}
fn event(
    member: usize,
    song: i64,
    sequence: u64,
    contact: u64,
    phase: TouchPhase,
) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(SOURCES[member], host(song), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(0x5754_4f55),
        code: Some(0xffff_fffe),
        timestamp: Some(point(91, -1)),
    });
    meta.original_clock_point = Some(point(92, 9_007_199_254_740_993));
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(),
        contact: ContactId(contact),
        phase,
        position: Position2 {
            x: 959.5,
            y: 719.25,
        },
        pressure: Some(0.375),
    })
}
fn prepared() -> PreparedBms {
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    for n in [1_u16, 1] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    for n in [8_u32, 16] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    for n in [2_u16, 16] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4_u32.to_le_bytes());
    for n in [8192_i16, -4096] {
        wav.extend_from_slice(&n.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files.insert("pack/chart.bms", CHART.to_vec()).unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    prepare_from_source(
        CHART,
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(8, 1).unwrap(),
        PcmLimits::new(256, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        u64::MAX,
        None,
    )
    .unwrap()
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(11, 10_000_000_000),
        output_origin: point(22, 0),
        preroll: Duration::from_nanos(100_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 32,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn accepted(result: InputResult) -> PlayerReport {
    let InputResult::Processed(mut rows) = result else {
        panic!("assigned source must retain its actual report")
    };
    assert_eq!(rows.len(), 1);
    rows.remove(0)
}

#[test]
fn hidden_page_holds_keep_their_destination_and_capture_replays_resolved_inputs_without_spatial_rejudging()
 {
    let original = prepared();
    let source = original.source.clone();
    let (mut game, bank) = StepLocalGameplay::new_section(
        original,
        config(),
        plan(),
        bindings(),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOrContact,
    )
    .unwrap();
    assert_eq!(bank.len(), 1);
    for index in 0..2 {
        game.configure_touch_router(MEMBERS[index], router(index))
            .unwrap();
        game.configure_capture(MEMBERS[index], limits(), u64::MAX)
            .unwrap();
    }
    game.activate(config().host_origin).unwrap();
    let mut reports = [Vec::new(), Vec::new()];
    for member in 0..2 {
        let down = event(member, 0, 1, u64::MAX, TouchPhase::Down);
        let report = accepted(
            game.process_input_at(down.clone(), pos(2.0), &OriginalClock, audio(0))
                .unwrap(),
        );
        assert_eq!(
            report.report.bound_inputs[0],
            GameInputEvent {
                game_control: GameControlId(0x11),
                physical: down
            }
        );
        reports[member].push(report.report);
    }
    let initial_hash = game.judge(MEMBERS[0]).unwrap().stable_hash().unwrap();
    game.set_touch_routing_enabled(MEMBERS[0], false).unwrap();
    game.remap_touch_regions(MEMBERS[0], regions(0, 100.0))
        .unwrap();
    assert_eq!(
        game.judge(MEMBERS[0]).unwrap().stable_hash().unwrap(),
        initial_hash
    );
    // Member 0 is hidden; the same physical surface would statically fan out.
    // Member 1 still has genuine active lane acquisition and independent scoring.
    for (member, contact, x, expected) in [(0, 8, 112.0, 0), (1, 8, 12.0, 1)] {
        let sample = event(member, 1_000_000_000, 2, contact, TouchPhase::Down);
        let report = accepted(
            game.process_input_at(sample.clone(), pos(x), &OriginalClock, audio(1_000_000_000))
                .unwrap(),
        );
        assert_eq!(report.report.bound_inputs.len(), expected);
        assert_eq!(report.report.input.as_ref(), Some(&sample));
        reports[member].push(report.report);
    }
    game.set_touch_routing_enabled(MEMBERS[0], true).unwrap();
    for (member, song, sequence, contact, phase, x, lane) in [
        (0, 1_500_000_000, 3, 8, TouchPhase::Move, 102.0, None),
        (
            0,
            1_500_000_000,
            4,
            u64::MAX,
            TouchPhase::Move,
            112.0,
            Some(0x11),
        ),
        (
            0,
            2_000_000_000,
            5,
            u64::MAX,
            TouchPhase::Up,
            112.0,
            Some(0x11),
        ),
        (
            1,
            2_000_000_000,
            3,
            u64::MAX,
            TouchPhase::Up,
            12.0,
            Some(0x11),
        ),
        (0, 2_500_000_000, 6, 8, TouchPhase::Up, 102.0, None),
        (0, 3_000_000_000, 7, 8, TouchPhase::Down, 102.0, Some(0x11)),
        (1, 3_000_000_000, 4, 9, TouchPhase::Down, 2.0, Some(0x11)),
    ] {
        let sample = event(member, song, sequence, contact, phase);
        let report = accepted(
            game.process_input_at(sample.clone(), pos(x), &OriginalClock, audio(song))
                .unwrap(),
        );
        assert_eq!(report.player, MEMBERS[member]);
        assert_eq!(
            report.report.bound_inputs,
            lane.map(|lane| GameInputEvent {
                game_control: GameControlId(lane),
                physical: sample
            })
            .into_iter()
            .collect::<Vec<_>>()
        );
        reports[member].push(report.report);
    }
    assert_eq!(
        (
            game.score(MEMBERS[0]).unwrap().hits,
            game.score(MEMBERS[0]).unwrap().misses
        ),
        (3, 1)
    );
    assert_eq!(
        (
            game.score(MEMBERS[1]).unwrap().hits,
            game.score(MEMBERS[1]).unwrap().misses
        ),
        (4, 0)
    );
    let hashes = MEMBERS.map(|member| game.judge(member).unwrap().stable_hash().unwrap());
    game.fail();
    for member in 0..2 {
        let bytes = game.take_replay(MEMBERS[member]).unwrap().unwrap();
        let file = decode_replay(&bytes, limits()).unwrap();
        assert_eq!(
            decode_section_setup(&file.header.options)
                .unwrap()
                .input_mode,
            BmsInputMode::ButtonOrContact
        );
        let recorded: Vec<_> = file
            .records
            .iter()
            .filter_map(|row| match &row.operation {
                ReplayOperation::Input(input) => Some(input.clone()),
                ReplayOperation::Advance => None,
            })
            .collect();
        let resolved: Vec<_> = reports[member]
            .iter()
            .flat_map(|report| report.bound_inputs.clone())
            .collect();
        assert_eq!(recorded, resolved);
        assert!(
            recorded.iter().all(|row| match &row.physical {
                PhysicalInputEvent::Touch(touch) =>
                    touch.position
                        == Position2 {
                            x: 959.5,
                            y: 719.25
                        },
                _ => false,
            }),
            "recording stores the original CSS position, which is outside both logical page layouts"
        );
        let replay = reconstruct_section(&source, file, limits()).unwrap();
        assert_eq!(replay.engine().stable_hash().unwrap(), hashes[member]);
        assert_eq!(
            replay.results(),
            reports[member]
                .iter()
                .flat_map(|report| report.judge_events.clone())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn live_group_page_controls_are_recoverable_before_input_but_refuse_absent_unknown_and_poisoned_owners()
 {
    let prepared = prepared();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap();
    let members = prepare_local_members(
        &prepared,
        &plan(),
        bindings(),
        profile,
        BmsInputMode::ButtonOrContact,
    )
    .unwrap();
    let (producer, _consumer) = command_queue(32).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(11),
        ClockDomainId(22),
        Transport::new(
            config().host_origin.timestamp,
            ts(-100_000_000),
            Rate::NORMAL,
        ),
        producer,
        members.configs,
        0,
        &members.reserved,
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    assert!(group.set_touch_routing_enabled(MEMBERS[0], false).is_err());
    assert!(
        group
            .remap_touch_regions(MEMBERS[0], regions(0, 100.0))
            .is_err()
    );
    group.configure_touch_router(MEMBERS[0], router(0)).unwrap();
    group.configure_touch_router(MEMBERS[1], router(1)).unwrap();
    assert!(
        group
            .set_touch_routing_enabled(PlayerId(91), false)
            .is_err()
    );
    assert!(
        group
            .remap_touch_regions(PlayerId(91), regions(0, 100.0))
            .is_err()
    );
    let before = group
        .member_judge(MEMBERS[0])
        .unwrap()
        .stable_hash()
        .unwrap();
    assert!(
        group
            .remap_touch_regions(MEMBERS[0], regions(1, 100.0))
            .is_err()
    );
    let mut overlap = regions(0, 100.0);
    overlap[1].min.x = 109.0;
    assert!(group.remap_touch_regions(MEMBERS[0], overlap).is_err());
    assert!(!group.poisoned());
    assert_eq!(
        group
            .member_judge(MEMBERS[0])
            .unwrap()
            .stable_hash()
            .unwrap(),
        before
    );
    let down = accepted(
        group
            .process_input_at(
                event(0, 0, 1, 1, TouchPhase::Down),
                pos(2.0),
                &OriginalClock,
                audio(0),
            )
            .unwrap(),
    );
    assert_eq!(
        down.report.bound_inputs[0].game_control,
        GameControlId(0x11)
    );
    group
        .remap_touch_regions(MEMBERS[0], regions(0, 100.0))
        .unwrap();
    group.set_touch_routing_enabled(MEMBERS[0], false).unwrap();
    let released = accepted(
        group
            .process_input_at(
                event(0, 2_000_000_000, 2, 1, TouchPhase::Up),
                pos(112.0),
                &OriginalClock,
                audio(2_000_000_000),
            )
            .unwrap(),
    );
    assert_eq!(
        released.report.bound_inputs[0].game_control,
        GameControlId(0x11)
    );
    assert!(
        released
            .report
            .judge_events
            .iter()
            .any(|event| event.stage == beatkernel::judge::JudgeStage::HoldTail)
    );
    // Actual malformed acquisition poisons the group; page controls cannot
    // restore a failed owner or change a sibling's routing state afterwards.
    assert!(
        group
            .process_input_at(
                event(0, 2_000_000_001, 3, 2, TouchPhase::Down),
                pos(f32::NAN),
                &OriginalClock,
                audio(2_000_000_001)
            )
            .is_err()
    );
    assert!(group.poisoned());
    assert!(group.set_touch_routing_enabled(MEMBERS[1], false).is_err());
    assert!(
        group
            .remap_touch_regions(MEMBERS[1], regions(1, 100.0))
            .is_err()
    );
}
