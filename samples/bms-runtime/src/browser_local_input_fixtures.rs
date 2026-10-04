//! Deferred portable local input admission with actual bindings, codec and runtime owners.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    browser_input::{LocalPhysicalInputSetup, decode_input},
    local_players::PlayerId,
    local_runtime::InputResult,
    replay_playback::{decode_section_setup, reconstruct_section},
    step_gameplay::{StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleId, command_queue},
    input::{
        BackendId, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId, DeviceSelector,
        EventMeta, GameControlId, NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
        Position2, TouchEvent, TouchPhase, VendorNamespaceId, encode_event,
    },
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, decode_replay},
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use beatkernel_bms::BmsInputMode;

const HOST: ClockDomainId = ClockDomainId(0x5749_4e);
const OUTPUT: ClockDomainId = ClockDomainId(22);
const ORIGIN: i64 = 604_800_000_000_017;
const PLAYERS: [u32; 4] = [u32::MAX, 9, 3, 1];
const SOURCES: [u64; 4] = [u64::MAX, 0, 9_007_199_254_740_993, 0x8877_6655_4433_2211];
fn point(domain: ClockDomainId, ns: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn exact_plan(player: u32, source: u64) -> [u32; 4] {
    [player, 1, source as u32, (source >> 32) as u32]
}
fn row(player: u32, lane: u32, source: u64, control: [u32; 3]) -> [u32; 8] {
    [
        player,
        lane,
        1,
        source as u32,
        (source >> 32) as u32,
        control[0],
        control[1],
        control[2],
    ]
}
fn meta(source: u64, ns: i64, sequence: u64) -> EventMeta {
    let mut meta = EventMeta::new(DeviceId(source), point(HOST, ns), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(u32::MAX),
        code: Some(0x8000_0001),
        timestamp: Some(point(ClockDomainId(91), -123)),
    });
    meta.original_clock_point = Some(point(ClockDomainId(92), i64::MAX));
    meta
}
fn button(source: u64, physical: PhysicalControlId, ns: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(source, ns, sequence),
        control: physical,
        state: ButtonState::Down,
    })
}
fn touch(source: u64, ns: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(source, ns, sequence),
        control: PhysicalControlId::Native {
            backend: BackendId(0x5754_4f55),
            code: 0,
        },
        contact: ContactId(u64::MAX),
        phase: TouchPhase::Down,
        position: Position2 {
            x: -123.5,
            y: 4096.25,
        },
        pressure: Some(0.75),
    })
}
fn setup(plan: &[u32], bindings: &[u32], lanes: &[u8]) -> Result<LocalPhysicalInputSetup, String> {
    LocalPhysicalInputSetup::new(plan, bindings, lanes, 4096, 1024)
}

#[test]
fn member_order_and_all_source_control_bits_survive_owned_numeric_setup_and_actual_codec_mapping() {
    let controls = [
        ([0, 7, 4], PhysicalControlId::keyboard(4)),
        (
            [1, 0x5754_4f55, 0],
            PhysicalControlId::Native {
                backend: BackendId(0x5754_4f55),
                code: 0,
            },
        ),
        (
            [0, u16::MAX as u32, u16::MAX as u32],
            PhysicalControlId::HidUsage {
                usage_page: u16::MAX,
                usage: u16::MAX,
            },
        ),
        (
            [2, u32::MAX, 0x8000_0000],
            PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(u32::MAX),
                code: 0x8000_0000,
            },
        ),
    ];
    let mut plan_words = (0..4)
        .flat_map(|index| exact_plan(PLAYERS[index], SOURCES[index]))
        .collect::<Vec<_>>();
    let mut binding_words = Vec::new();
    for index in [2, 0, 3, 1] {
        binding_words.extend(row(PLAYERS[index], 0x11, SOURCES[index], controls[index].0));
        binding_words.extend(row(
            PLAYERS[index],
            0x12,
            SOURCES[index],
            [1, u32::MAX, u32::MAX],
        ));
    }
    let retained = setup(&plan_words, &binding_words, &[0x11, 0x12]).unwrap();
    plan_words.fill(0);
    binding_words.fill(0);
    assert_eq!(
        retained.plan.members(),
        &(0..4)
            .map(|index| (PlayerId(PLAYERS[index]), Some(DeviceId(SOURCES[index]))))
            .collect::<Vec<_>>()
    );
    assert_eq!(retained.bindings.len(), 4);
    assert_eq!(
        (
            retained.limits.max_encoded_bytes(),
            retained.limits.max_payload_bytes()
        ),
        (4096, 1024)
    );
    for index in 0..4 {
        let map = &retained.bindings[index];
        assert_eq!(
            map.bindings()[0].device,
            DeviceSelector::Exact(DeviceId(SOURCES[index]))
        );
        assert_eq!(map.bindings()[0].physical, controls[index].1);
        assert_eq!(
            map.bindings()[1].physical,
            PhysicalControlId::Native {
                backend: BackendId(u32::MAX),
                code: u32::MAX
            }
        );
        let original = if index == 1 {
            touch(SOURCES[index], ORIGIN, u64::MAX)
        } else {
            button(SOURCES[index], controls[index].1, ORIGIN, u64::MAX)
        };
        let bytes = encode_event(&original, retained.limits).unwrap();
        let decoded = decode_input(&bytes, retained.limits, HOST).unwrap();
        let bound = map.map(&decoded).collect::<Vec<_>>();
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].game_control, GameControlId(0x11));
        assert_eq!(bound[0].physical, original);
        let mut foreign = original.clone();
        foreign.meta_mut().source = DeviceId(SOURCES[(index + 1) % 4]);
        assert_eq!(
            map.map(&foreign).count(),
            0,
            "one member's map cannot borrow another assigned source"
        );
    }
}

#[test]
fn automatic_solo_preserves_any_exact_precedence_real_fanout_and_empty_chart_compatibility() {
    let plan = [17, 0, 0, 0];
    let any = [17, 0x11, 0, 0, 0, 0, 7, 4];
    let words = [
        any,
        row(17, 0x12, u64::MAX, [0, 7, 4]),
        row(17, 0x13, u64::MAX, [0, 7, 4]),
    ]
    .concat();
    let retained = setup(&plan, &words, &[0x11, 0x12, 0x13]).unwrap();
    assert_eq!(retained.plan.members(), &[(PlayerId(17), None)]);
    for (source, expected) in [
        (u64::MAX, vec![0x12, 0x13]),
        (0, vec![0x11]),
        (9_007_199_254_740_993, vec![0x11]),
    ] {
        let event = button(source, PhysicalControlId::keyboard(4), ORIGIN, 1);
        let bound = retained.bindings[0].map(&event).collect::<Vec<_>>();
        assert_eq!(
            bound
                .iter()
                .map(|input| input.game_control.0)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(bound.iter().all(|input| input.physical == event));
    }
    assert!(
        setup(&plan, &[any, any].concat(), &[0x11]).is_err(),
        "identical triples are still rejected by core BindingMap"
    );
    let exact = setup(&exact_plan(17, 0), &row(17, 0x11, 0, [0, 7, 4]), &[0x11]).unwrap();
    assert_eq!(
        exact.bindings[0].bindings()[0].device,
        DeviceSelector::Exact(DeviceId(0))
    );
    let empty = LocalPhysicalInputSetup::new(&plan, &[], &[], 6, 0).unwrap();
    assert_eq!(empty.bindings.len(), 1);
    assert!(empty.bindings[0].bindings().is_empty());
    assert!(setup(&plan, &[], &[0x11]).is_err());
    assert!(
        setup(&plan, &words, &[0x11, 0x11, 0x12, 0x13]).is_ok(),
        "existing lane coverage semantics remain unchanged"
    );
}

#[test]
fn malformed_routes_rows_source_leaks_and_incomplete_member_coverage_refuse_the_whole_setup() {
    let plan = [exact_plan(7, 0), exact_plan(2, u64::MAX)].concat();
    let first = row(7, 0x11, 0, [0, 7, 4]);
    let second = row(2, 0x11, u64::MAX, [0, 7, 4]);
    let words = [first, second].concat();
    let retained = setup(&plan, &words, &[0x11]).unwrap();
    for bad in [
        vec![],
        vec![7, 1, 0],
        vec![0, 1, 0, 0],
        vec![7, 2, 0, 0],
        vec![7, 0, 1, 0],
        vec![7, 0, 0, 1],
        vec![7, 0, 0, 0, 2, 1, 3, 0],
        vec![7, 1, 0, 0, 7, 1, 1, 0],
        vec![7, 1, 0, 0, 2, 1, 0, 0],
    ] {
        assert!(setup(&bad, &words, &[0x11]).is_err());
    }
    assert!(setup(&plan, &words[..15], &[0x11]).is_err());
    let mut tail = words.clone();
    tail.push(0);
    assert!(setup(&plan, &tail, &[0x11]).is_err());
    assert!(setup(&plan, &[first, second, second].concat(), &[0x11]).is_err());
    assert!(
        setup(&plan, &first, &[0x11]).is_err(),
        "global lane coverage cannot stand in for missing second-member coverage"
    );
    for (index, value) in [
        (0, 99),
        (1, 0x10),
        (1, 0x1a),
        (1, 0x10011),
        (2, 0),
        (2, 2),
        (3, 1),
        (4, 1),
        (5, 3),
        (6, 65_536),
        (7, 65_536),
    ] {
        let mut bad = words.clone();
        bad[index] = value;
        assert!(
            setup(&plan, &bad, &[0x11]).is_err(),
            "invalid eight-word identity field {index}"
        );
    }
    let split = [first, row(2, 0x12, u64::MAX, [0, 7, 4])].concat();
    assert!(setup(&plan, &split, &[0x11, 0x12]).is_err());
    for lanes in [vec![0x12], vec![0x20], vec![0x11; 19]] {
        assert!(setup(&plan, &words, &lanes).is_err());
    }
    for (encoded, payload) in [(0, 0), (5, 0), (4096, 4097), (1_048_577, 0)] {
        assert!(LocalPhysicalInputSetup::new(&plan, &words, &[0x11], encoded, payload).is_err());
    }
    assert_eq!(retained.plan.to_words(), plan);
    assert_eq!(
        retained
            .bindings
            .iter()
            .map(|map| map.bindings().len())
            .collect::<Vec<_>>(),
        [1, 1]
    );
}

#[test]
fn row_budget_is_shared_across_members_while_member_and_codec_boundaries_remain_independent() {
    let plan = (0..4)
        .flat_map(|index| exact_plan(PLAYERS[index], SOURCES[index]))
        .collect::<Vec<_>>();
    let mut words = Vec::new();
    for index in 0..4 {
        for code in 0..64 {
            words.extend(row(
                PLAYERS[index],
                0x11,
                SOURCES[index],
                [1, u32::MAX, code],
            ));
        }
    }
    let retained =
        LocalPhysicalInputSetup::new(&plan, &words, &[0x11], 1_048_576, 1_048_576).unwrap();
    assert_eq!(
        retained
            .bindings
            .iter()
            .map(|map| map.bindings().len())
            .collect::<Vec<_>>(),
        [64; 4]
    );
    words.extend(row(PLAYERS[0], 0x11, SOURCES[0], [1, u32::MAX, 64]));
    assert!(
        setup(&plan, &words, &[0x11]).is_err(),
        "257 total rows refuse even though every individual map remains below 256"
    );
    let mut maximal_plan = Vec::new();
    let mut maximal_bindings = Vec::new();
    for index in 0..64_u32 {
        maximal_plan.extend(exact_plan(index + 1, u64::from(index)));
        maximal_bindings.extend(row(index + 1, 0x11, u64::from(index), [0, 7, 4]));
    }
    let maximum = setup(&maximal_plan, &maximal_bindings, &[0x11]).unwrap();
    assert_eq!(maximum.plan.members().len(), 64);
    assert_eq!(maximum.bindings.len(), 64);
    let empty = setup(&maximal_plan, &[], &[]).unwrap();
    assert!(empty.bindings.iter().all(|map| map.bindings().is_empty()));
    maximal_plan.extend(exact_plan(65, 64));
    assert!(setup(&maximal_plan, &maximal_bindings, &[]).is_err());
    let lanes = (0x11..=0x19).chain(0x21..=0x29).collect::<Vec<u8>>();
    let eighteen = lanes
        .iter()
        .flat_map(|lane| row(1, u32::from(*lane), 0, [1, 9, u32::from(*lane)]))
        .collect::<Vec<_>>();
    let maps = setup(&exact_plan(1, 0), &eighteen, &lanes).unwrap();
    assert_eq!(maps.bindings[0].bindings().len(), 18);
}

struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn prepared() -> PreparedBms {
    let chart = b"#BPM 60\n#WAV01 key.wav\n#00011:01\n#00012:00010000\n";
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [4_u32, 8] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2_u16, 16] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4_u32.to_le_bytes());
    for value in [8192_i16, 4096] {
        wav.extend_from_slice(&value.to_le_bytes());
    }
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files.insert("pack/chart.bms", chart.to_vec()).unwrap();
    files.insert("pack/key.wav", wav).unwrap();
    prepare_from_source(
        chart,
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
fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65_536, 64, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}

#[test]
fn actual_three_and_four_member_owners_route_identical_controls_independently_with_original_capture_and_shared_pcm()
 {
    for count in [3, 4] {
        let plan = (0..count)
            .flat_map(|index| exact_plan(PLAYERS[index], SOURCES[index]))
            .collect::<Vec<_>>();
        let mut words = Vec::new();
        // Wire row order differs from member order without changing either source assignment.
        for index in (0..count).rev() {
            words.extend(row(PLAYERS[index], 0x11, SOURCES[index], [0, 7, 4]));
            words.extend(row(
                PLAYERS[index],
                0x12,
                SOURCES[index],
                [1, 0x5754_4f55, 0],
            ));
        }
        let admitted = setup(&plan, &words, &[0x11, 0x12]).unwrap();
        let input_limits = admitted.limits;
        let original = prepared();
        let source = original.source.clone();
        let chosen = StepGameplayConfig {
            host_origin: point(HOST, ORIGIN),
            output_origin: point(OUTPUT, 17),
            preroll: Duration::ZERO,
            early_ns: 0,
            late_ns: 0,
            offset_ns: 0,
            command_capacity: 32,
            bgm_pending: 2,
            bgm_lookahead: Duration::from_nanos(2_000_000_000),
            telemetry_capacity: 8,
        };
        let (mut owner, bank) = StepLocalGameplay::new_section(
            original,
            chosen,
            admitted.plan,
            admitted.bindings,
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOrContact,
        )
        .unwrap();
        assert_eq!(
            owner.players(),
            &PLAYERS[..count]
                .iter()
                .map(|id| PlayerId(*id))
                .collect::<Vec<_>>()
        );
        assert_eq!(bank.len(), 1);
        assert_eq!(bank.get(SampleId(1)).unwrap().samples(), &[0.25, 0.125]);
        let unknown = button(91, PhysicalControlId::keyboard(4), ORIGIN, 1);
        let encoded = encode_event(&unknown, input_limits).unwrap();
        assert!(matches!(
            owner
                .process_input(
                    decode_input(&encoded, input_limits, HOST).unwrap(),
                    &SameDomain,
                    chosen.output_origin
                )
                .unwrap(),
            InputResult::Ignored {
                device: DeviceId(91)
            }
        ));
        assert!(owner.input_setup_available());
        for player in &PLAYERS[..count] {
            owner
                .configure_capture(PlayerId(*player), replay_limits(), u64::MAX)
                .unwrap();
        }
        owner.activate(chosen.host_origin).unwrap();
        let mut originals = vec![Vec::new(); count];
        let mut judgments = vec![Vec::new(); count];
        for phase in 0..2 {
            for index in 0..count {
                let input = if phase == 0 {
                    button(SOURCES[index], PhysicalControlId::keyboard(4), ORIGIN, 1)
                } else {
                    touch(SOURCES[index], ORIGIN + 1_000_000_000, 2)
                };
                let bytes = encode_event(&input, input_limits).unwrap();
                let decoded = decode_input(&bytes, input_limits, HOST).unwrap();
                assert_eq!(decoded, input);
                let InputResult::Processed(rows) = owner
                    .process_input(
                        decoded,
                        &SameDomain,
                        point(OUTPUT, 17 + phase * 1_000_000_000),
                    )
                    .unwrap()
                else {
                    panic!("assigned source must reach its actual member")
                };
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].player, PlayerId(PLAYERS[index]));
                assert_eq!(rows[0].report.input.as_ref(), Some(&input));
                assert_eq!(rows[0].report.bound_inputs.len(), 1);
                assert_eq!(rows[0].report.bound_inputs[0].physical, input);
                assert_eq!(
                    rows[0].report.bound_inputs[0].game_control,
                    GameControlId(if phase == 0 { 0x11 } else { 0x12 })
                );
                assert_eq!(rows[0].report.judge_events.len(), 1);
                assert_eq!(rows[0].report.judge_events[0].input, Some(*input.meta()));
                assert_eq!(
                    owner.score(PlayerId(PLAYERS[index])).unwrap().hits,
                    phase as u64 + 1
                );
                originals[index].push(input);
                judgments[index].extend(rows[0].report.judge_events.clone());
            }
        }
        let rows = owner
            .advance_to(
                point(HOST, ORIGIN + 2_000_000_000),
                &SameDomain,
                point(OUTPUT, 2_000_000_017),
            )
            .unwrap();
        assert_eq!(rows.len(), count);
        assert!(rows.iter().all(|row| row.report.judge_events.is_empty()));
        let (mut producer, consumer) = command_queue(32).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                bank.format(),
                OUTPUT,
                chosen.output_origin.timestamp,
                AudioLimits::new(32, 16, 32, 16, 32).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let mut command_count = 0;
        while let Some(batch) = owner.take_commands(32).unwrap() {
            for command in &batch.commands {
                producer.try_push(*command).unwrap();
            }
            command_count += batch.commands.len();
            owner
                .acknowledge(batch.sequence, batch.commands.len(), true)
                .unwrap();
        }
        assert_eq!(command_count, count * 2);
        let mut pcm = [0.0_f32; 8];
        let report = mixer.render(&mut pcm).unwrap();
        assert_eq!(report.counters.commands_applied, count as u64 * 2);
        assert_eq!(report.counters.late_commands, 0);
        assert_eq!(
            pcm,
            [
                count as f32 * 0.25,
                count as f32 * 0.125,
                0.0,
                0.0,
                count as f32 * 0.25,
                count as f32 * 0.125,
                0.0,
                0.0
            ]
        );
        owner.fail();
        for index in 0..count {
            let player = PlayerId(PLAYERS[index]);
            let file = decode_replay(
                &owner.take_replay(player).unwrap().unwrap(),
                replay_limits(),
            )
            .unwrap();
            let setup = decode_section_setup(&file.header.options).unwrap();
            assert_eq!(setup.input_mode, BmsInputMode::ButtonOrContact);
            assert_eq!(setup.chart_seed, u64::MAX);
            assert_eq!(file.records.len(), 3);
            for (record, original) in file.records[..2].iter().zip(&originals[index]) {
                let ReplayOperation::Input(bound) = &record.operation else {
                    panic!("actual captured input required")
                };
                assert_eq!(&bound.physical, original);
            }
            let replay = reconstruct_section(&source, file, replay_limits()).unwrap();
            assert_eq!(replay.results(), judgments[index]);
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                owner.judge(player).unwrap().stable_hash().unwrap()
            );
            assert!(owner.take_replay(player).unwrap().is_none());
        }
    }
}

#[cfg(feature = "graphics")]
#[test]
fn borrowed_local_views_retain_actual_prefix_references_and_match_owned_scene_packets_on_sparse_pages()
 {
    use crate::{
        bga_render::BgaFrame,
        competition::ScoreSummary,
        note_progress::NoteProgress,
        player::LocalPlayerSnapshot,
        player_chart::PlayerChart,
        scene::Scene,
        ui::organisms::{
            LocalPlayerView, local_player_views_with_background, local_players_with_background,
        },
    };
    use beatkernel::{
        input::GameInputEvent,
        judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    };
    use std::sync::Arc;
    let original = prepared();
    let chart =
        Arc::new(PlayerChart::from_compiled(&original.source, &original.compiled.chart).unwrap());
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap();
    let mut players = Vec::new();
    for index in 0..64_u32 {
        let mut judge = JudgeEngine::new(
            original.compiled.chart.clone(),
            original.source.rules(),
            profile.clone(),
        )
        .unwrap();
        let events = if index % 2 == 0 {
            judge
                .push_input(
                    &GameInputEvent {
                        game_control: GameControlId(0x11),
                        physical: button(
                            u64::from(index) + 3,
                            PhysicalControlId::keyboard(4),
                            ORIGIN,
                            1,
                        ),
                    },
                    Timestamp::ZERO,
                )
                .unwrap()
        } else {
            judge.advance_to(Timestamp::from_nanos(1)).unwrap()
        };
        assert_eq!(events.len(), 1);
        let mut score = ScoreSummary::default();
        score.observe(&events).unwrap();
        let mut progress = NoteProgress::new(chart.clone()).unwrap();
        progress.apply(&events);
        players.push(LocalPlayerSnapshot {
            player: PlayerId(if index == 63 {
                u32::MAX
            } else {
                index * 31 + 9
            }),
            chart: Some(chart.clone()),
            song_time: Some(Timestamp::from_nanos(100_000_000)),
            score,
            last_judge: events.last().copied(),
            recent_results: events,
            pressed_lanes: 0,
            note_progress: Some(progress),
            competition: None,
        });
    }
    let views = players
        .iter()
        .map(LocalPlayerView::from)
        .collect::<Vec<_>>();
    for (owner, view) in players.iter().zip(&views) {
        assert!(std::ptr::eq(view.score, &owner.score));
        assert!(std::ptr::eq(
            view.recent_results,
            owner.recent_results.as_slice()
        ));
        assert!(std::ptr::eq(
            view.note_progress.unwrap(),
            owner.note_progress.as_ref().unwrap()
        ));
        assert!(std::ptr::eq(
            view.last_judge.unwrap(),
            owner.last_judge.as_ref().unwrap()
        ));
        assert!(std::ptr::eq(
            view.chart.unwrap(),
            owner.chart.as_deref().unwrap()
        ));
    }
    // Compare the actual retained renderer packets, including painter ordering and
    // GPU note instances, rather than rebuilding either composition algorithm.
    let packet = |scene: &Scene| {
        (
            scene
                .rectangles()
                .iter()
                .map(|rect| {
                    (
                        rect.bounds.map(f32::to_bits),
                        rect.color.map(f32::to_bits),
                        rect.uv.map(f32::to_bits),
                    )
                })
                .collect::<Vec<_>>(),
            scene
                .batches()
                .iter()
                .map(|batch| (batch.texture, batch.first, batch.count, batch.playfield))
                .collect::<Vec<_>>(),
            scene
                .playfields()
                .iter()
                .map(|field| {
                    (
                        field.drift.to_bits(),
                        field.top.to_bits(),
                        field.bottom.to_bits(),
                        field
                            .instances
                            .iter()
                            .map(|note| {
                                (
                                    note.geometry.map(f32::to_bits),
                                    note.appearance.map(f32::to_bits),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let frames = [BgaFrame::default(); 4];
    for (count, page, visible) in [(3, 0, 3), (4, 0, 4), (64, 15, 4)] {
        let mut owned = Scene::new(960, 720);
        let mut borrowed = Scene::new(960, 720);
        local_players_with_background(
            &mut owned,
            &players[..count],
            2_000_000_000,
            page,
            false,
            &frames,
        )
        .unwrap();
        local_player_views_with_background(
            &mut borrowed,
            &views[..count],
            2_000_000_000,
            page,
            false,
            &frames,
        )
        .unwrap();
        owned.status().unwrap();
        borrowed.status().unwrap();
        assert!(!borrowed.rectangles().is_empty());
        assert_eq!(borrowed.playfields().len(), visible);
        assert!(
            borrowed
                .playfields()
                .iter()
                .all(|field| !field.instances.is_empty())
        );
        assert_eq!(packet(&borrowed), packet(&owned));
    }
    let mut retained = Scene::new(960, 720);
    local_player_views_with_background(&mut retained, &views, 2_000_000_000, 15, false, &frames)
        .unwrap();
    let before = packet(&retained);
    let stamp = (
        retained.geometry_stamp().0.clone(),
        retained.geometry_stamp().1,
    );
    let mut duplicate = views.clone();
    duplicate[0].player = duplicate[1].player;
    let mut zero = views.clone();
    zero[63].player = PlayerId(0);
    let mut oversized = views.clone();
    oversized.push(views[0]);
    for invalid in [
        &views[..0],
        duplicate.as_slice(),
        zero.as_slice(),
        oversized.as_slice(),
    ] {
        assert!(
            local_player_views_with_background(
                &mut retained,
                invalid,
                2_000_000_000,
                15,
                false,
                &frames
            )
            .is_err()
        );
        assert_eq!(packet(&retained), before);
        assert_eq!(retained.geometry_stamp().1, stamp.1);
        assert!(Arc::ptr_eq(retained.geometry_stamp().0, &stamp.0));
    }
    assert!(
        local_player_views_with_background(
            &mut retained,
            &views,
            2_000_000_000,
            16,
            false,
            &frames
        )
        .is_err()
    );
    assert!(
        local_players_with_background(&mut retained, &players, 2_000_000_000, 16, false, &frames)
            .is_err()
    );
    assert_eq!(packet(&retained), before);
    assert_eq!(retained.geometry_stamp().1, stamp.1);
    retained.status().unwrap();
}
