//! Deferred genuine member preparation, shared PCM, and native delegation fixtures.
use crate::{
    ChannelPolicy, PreparedBms, WavDecoder, prepare_from_source,
    asset_paths::AssetPathPolicy,
    asset_source::MemoryFiles,
    competition_live::{CompetitionOptions, NetworkRole},
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::{PreparedLocalMembers, prepare_local_members},
    local_runtime::{InputResult, RuntimeGroup},
    native_cohort_setup::{CohortPreparation, prepare_cohort},
    native_judge::capture_limits,
    replay_capture::LiveReplayCapture,
    replay_playback::decode_section_setup,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleId, VoiceId,
        command_queue,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent, Position2,
        TouchEvent, TouchPhase, VendorNamespaceId,
    },
    judge::{JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
    runtime::RuntimeProcessingClock,
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::BmsInputMode;
use std::{collections::BTreeMap, path::Path};

const CHART: &str =
    "#BPM 60\n#WAV01 key.wav\n#WAV02 bg.wav\n#00011:01\n#00012:00010000\n#00001:02\n";
const HOST: ClockDomainId = ClockDomainId(31);
const OUTPUT: ClockDomainId = ClockDomainId(32);
const ORIGIN: i64 = 604_800_000_000_017;
struct SameDomain;
impl ClockMapper for SameDomain {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn point(domain: ClockDomainId, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain,
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn wav(samples: &[i16]) -> Vec<u8> {
    let size = (samples.len() * 2) as u32;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    for value in [1_u16, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [4_u32, 8] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [2_u16, 16] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
fn prepared() -> PreparedBms {
    let mut files = MemoryFiles::new(Default::default()).unwrap();
    files
        .insert("pack/chart.bms", CHART.as_bytes().to_vec())
        .unwrap();
    files
        .insert("pack/key.wav", wav(&[2048, 4096, -2048, 0]))
        .unwrap();
    files
        .insert("pack/bg.wav", wav(&[1024, 2048, 0, -1024]))
        .unwrap();
    prepare_from_source(
        CHART.as_bytes(),
        &files.scope("pack/chart.bms").unwrap(),
        AudioFormat::new(4, 1).unwrap(),
        PcmLimits::new(128, 1024, 8).unwrap(),
        ChannelPolicy::Exact,
        &WavDecoder,
        AssetPathPolicy::AudioVariants,
        u64::MAX,
        None,
    )
    .unwrap()
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap()
}
fn controls(index: usize) -> [PhysicalControlId; 2] {
    match index {
        0 => [
            PhysicalControlId::keyboard(4),
            PhysicalControlId::keyboard(5),
        ],
        1 => [
            PhysicalControlId::HidUsage {
                usage_page: 9,
                usage: 1,
            },
            PhysicalControlId::HidUsage {
                usage_page: 9,
                usage: 2,
            },
        ],
        2 => [
            PhysicalControlId::Native {
                backend: BackendId(0x5747_5044),
                code: 0x30000,
            },
            PhysicalControlId::Native {
                backend: BackendId(0x5747_5044),
                code: 0x30001,
            },
        ],
        _ => [
            PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(u32::MAX),
                code: u32::MAX,
            },
            PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(u32::MAX),
                code: 0,
            },
        ],
    }
}
fn map(selector: DeviceSelector, physical: [PhysicalControlId; 2]) -> BindingMap {
    BindingMap::from_bindings(
        physical
            .into_iter()
            .zip([0x11, 0x12])
            .map(|(physical, lane)| Binding {
                device: selector,
                physical,
                game_control: GameControlId(lane),
            }),
    )
    .unwrap()
}
fn plan(count: usize) -> ResolvedInputPlan {
    let all = [
        (PlayerId(u32::MAX), Some(DeviceId(0))),
        (PlayerId(7), Some(DeviceId(u64::MAX))),
        (PlayerId(91), Some(DeviceId(0x8877_6655_4433_2211))),
        (PlayerId(2), Some(DeviceId(3))),
    ];
    ResolvedInputPlan::new(all[..count].to_vec()).unwrap()
}
fn maps(plan: &ResolvedInputPlan) -> Vec<BindingMap> {
    plan.members()
        .iter()
        .enumerate()
        .map(|(index, &(_, device))| {
            map(
                device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
                controls(index),
            )
        })
        .collect()
}
fn button(
    device: DeviceId,
    control: PhysicalControlId,
    song: i64,
    sequence: u64,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(device, point(HOST, ORIGIN + song), sequence),
        control,
        state: ButtonState::Down,
    })
}
fn runtime(members: PreparedLocalMembers) -> (RuntimeGroup, beatkernel::audio::CommandConsumer) {
    let (producer, consumer) = command_queue(32).unwrap();
    let mut group = RuntimeGroup::new(
        HOST,
        OUTPUT,
        Transport::new(Timestamp::from_nanos(ORIGIN), Timestamp::ZERO, Rate::NORMAL),
        producer,
        members.configs,
        8,
        &members.reserved,
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}

#[test]
fn three_and_four_prepared_members_use_real_typed_bindings_judges_and_one_original_pcm_bank() {
    for count in [3, 4] {
        let prepared = prepared();
        let chosen = plan(count);
        let sample = prepared.bank.get(SampleId(1)).unwrap();
        let pointer = sample.samples().as_ptr();
        let original_pcm = sample.samples().to_vec();
        let original_sounds = prepared.sounds.clone();
        let members = prepare_local_members(
            &prepared,
            &chosen,
            maps(&chosen),
            profile(),
            BmsInputMode::ButtonOnly,
        )
        .unwrap();
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            pointer
        );
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples(),
            original_pcm
        );
        assert_eq!(prepared.bank.len(), 2);
        assert_eq!(prepared.sounds, original_sounds);
        let (mut group, consumer) = runtime(members);
        for command in &prepared.bgm_commands {
            group.enqueue_audio(*command).unwrap();
        }
        for (index, &(player, device)) in chosen.members().iter().enumerate() {
            let event = button(device.unwrap(), controls(index)[0], 0, u64::MAX - 1);
            let InputResult::Processed(reports) = group
                .process_input(event.clone(), &SameDomain, point(OUTPUT, 0))
                .unwrap()
            else {
                panic!("prepared source was ignored")
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, player);
            assert_eq!(reports[0].report.bound_inputs[0].physical, event);
            assert_eq!(
                reports[0].report.bound_inputs[0].game_control,
                GameControlId(0x11)
            );
            assert!(matches!(
                reports[0].report.judge_events[0].outcome,
                JudgeOutcome::Hit { .. }
            ));
            assert!(matches!(
                reports[0].report.audio_commands[0],
                AudioCommand::Play {
                    sample: SampleId(1),
                    ..
                }
            ));
        }
        let mut mixer = Mixer::new(
            MixerConfig::new(
                AudioFormat::new(4, 1).unwrap(),
                OUTPUT,
                Timestamp::ZERO,
                AudioLimits::new(32, 16, 32, 4, 32).unwrap(),
            ),
            prepared.bank,
            consumer,
        )
        .unwrap();
        let mut pcm = [0.0; 4];
        let report = mixer.render(&mut pcm).unwrap();
        let n = count as f32;
        assert_eq!(
            pcm,
            [
                n * 0.0625 + 0.03125,
                n * 0.125 + 0.0625,
                -n * 0.0625,
                -0.03125
            ]
        );
        assert_eq!(report.counters.commands_applied, count as u64 + 1);
        for (index, &(player, device)) in chosen.members().iter().enumerate() {
            let InputResult::Processed(reports) = group
                .process_input(
                    button(device.unwrap(), controls(index)[1], 1_000_000_000, u64::MAX),
                    &SameDomain,
                    point(OUTPUT, 1_000_000_000),
                )
                .unwrap()
            else {
                panic!("second lane ignored")
            };
            assert_eq!(reports[0].player, player);
            assert_eq!(
                reports[0].report.song_time,
                Timestamp::from_nanos(1_000_000_000)
            );
            assert!(matches!(
                reports[0].report.judge_events[0].outcome,
                JudgeOutcome::Hit { .. }
            ));
        }
        let report = mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [n * 0.0625, n * 0.125, -n * 0.0625, 0.0]);
        assert_eq!(report.counters.commands_applied, count as u64 * 2 + 1);
    }
}

#[test]
fn opt_in_contact_rules_accept_original_touch_while_button_only_keeps_its_existing_semantics() {
    let prepared = prepared();
    let chosen = plan(1);
    let mut hashes = Vec::new();
    for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
        let members =
            prepare_local_members(&prepared, &chosen, maps(&chosen), profile(), mode).unwrap();
        hashes.push(members.configs[0].judge.stable_hash().unwrap());
        let (mut group, mut audio) = runtime(members);
        let touch = PhysicalInputEvent::Touch(TouchEvent {
            meta: EventMeta::new(DeviceId(0), point(HOST, ORIGIN), u64::MAX),
            control: controls(0)[0],
            contact: ContactId(u64::MAX),
            phase: TouchPhase::Down,
            position: Position2 {
                x: -0.25,
                y: 1234.5,
            },
            pressure: Some(0.75),
        });
        let InputResult::Processed(reports) = group
            .process_input(touch.clone(), &SameDomain, point(OUTPUT, 0))
            .unwrap()
        else {
            panic!("contact source ignored before actual judge")
        };
        assert_eq!(reports[0].report.bound_inputs[0].physical, touch);
        let contact = mode == BmsInputMode::ButtonOrContact;
        assert_eq!(reports[0].report.judge_events.len(), usize::from(contact));
        if contact {
            assert!(matches!(
                reports[0].report.judge_events[0].outcome,
                JudgeOutcome::Hit { .. }
            ));
            assert_eq!(reports[0].report.judge_events[0].input, Some(*touch.meta()));
            assert!(matches!(
                audio.try_pop().unwrap(),
                AudioCommand::Play {
                    sample: SampleId(1),
                    ..
                }
            ));
        } else {
            assert!(audio.try_pop().is_err());
        }
        let after = group
            .advance_to(point(HOST, ORIGIN + 1), &SameDomain, point(OUTPUT, 1))
            .unwrap();
        assert_eq!(after[0].report.judge_events.len(), usize::from(!contact));
        if !contact {
            assert!(matches!(
                after[0].report.judge_events[0].outcome,
                JudgeOutcome::Miss { .. }
            ));
        }
    }
    assert_ne!(hashes[0], hashes[1]);
    for selector in [DeviceSelector::Any, DeviceSelector::Exact(DeviceId(0))] {
        let automatic = ResolvedInputPlan::new(vec![(PlayerId(1), None)]).unwrap();
        assert!(
            prepare_local_members(
                &prepared,
                &automatic,
                vec![map(selector, controls(0))],
                profile(),
                BmsInputMode::ButtonOnly
            )
            .is_ok()
        );
    }
}

#[test]
fn failed_member_preflight_preserves_prepared_source_pcm_and_rejects_coverage_gain_and_namespace_errors()
 {
    let mut prepared = prepared();
    let chosen = plan(3);
    let pointer = prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr();
    let pcm = prepared.bank.get(SampleId(1)).unwrap().samples().to_vec();
    let chart = prepared.compiled.chart.clone();
    let notes = prepared.source.notes.clone();
    let original_sounds = prepared.sounds.clone();
    let original_bgm = prepared.bgm_commands.clone();
    for case in 0..8 {
        let mut binding_maps = maps(&chosen);
        match case {
            0 => {
                binding_maps.pop();
            }
            1 => binding_maps.push(map(DeviceSelector::Any, controls(0))),
            2 => binding_maps[1] = map(DeviceSelector::Any, controls(1)),
            3 => binding_maps[1] = map(DeviceSelector::Exact(DeviceId(0)), controls(1)),
            4 => binding_maps[1] = BindingMap::from_bindings([]).unwrap(),
            5 => {
                binding_maps[1] = BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(DeviceId(u64::MAX)),
                    physical: controls(1)[0],
                    game_control: GameControlId(0x11),
                }])
                .unwrap()
            }
            6 => prepared.sounds[0].gain = f32::NAN,
            _ => {
                prepared.bgm_commands[0] = AudioCommand::Stop {
                    voice: VoiceId(3),
                    at: Timestamp::ZERO,
                }
            }
        }
        assert!(
            prepare_local_members(
                &prepared,
                &chosen,
                binding_maps,
                profile(),
                BmsInputMode::ButtonOnly
            )
            .is_err()
        );
        assert_eq!(prepared.compiled.chart, chart);
        assert_eq!(prepared.source.notes, notes);
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            pointer
        );
        assert_eq!(prepared.bank.get(SampleId(1)).unwrap().samples(), pcm);
        if case == 6 {
            assert!(prepared.sounds[0].gain.is_nan());
        }
        prepared.sounds = original_sounds.clone();
        prepared.bgm_commands = original_bgm.clone();
    }
    for voice in [VoiceId(u64::MAX), VoiceId(u64::MAX - 1)] {
        prepared.bgm_commands[0] = AudioCommand::Play {
            voice,
            sample: SampleId(2),
            at: Timestamp::ZERO,
            gain: 1.0,
        };
        assert!(
            prepare_local_members(
                &prepared,
                &chosen,
                maps(&chosen),
                profile(),
                BmsInputMode::ButtonOnly
            )
            .is_err()
        );
        assert_eq!(prepared.sounds, original_sounds);
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            pointer
        );
    }
    prepared.bgm_commands = original_bgm;
    assert!(
        prepare_local_members(
            &prepared,
            &chosen,
            maps(&chosen),
            profile(),
            BmsInputMode::ButtonOnly
        )
        .is_ok()
    );
}

#[test]
fn voice_namespaces_preserve_bgm_order_and_same_member_replacements_without_changing_sample_fields()
{
    let mut prepared = prepared();
    let chosen = plan(3);
    prepared.sounds[1].voice = prepared.sounds[0].voice;
    prepared.sounds[0].gain = -0.5;
    prepared.sounds[1].gain = 0.25;
    prepared.bgm_commands = [99, 3, 99]
        .into_iter()
        .map(|voice| AudioCommand::Play {
            voice: VoiceId(voice),
            sample: SampleId(2),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .collect();
    let original = prepared.sounds.clone();
    let members = prepare_local_members(
        &prepared,
        &chosen,
        maps(&chosen),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert_eq!(members.reserved, [VoiceId(99), VoiceId(3), VoiceId(99)]);
    for (index, member) in members.configs.iter().enumerate() {
        assert_eq!(member.player, chosen.members()[index].0);
        for (sound, before) in member.sounds.iter().zip(&original) {
            assert_eq!(sound.voice, VoiceId(100 + index as u64));
            let mut preserved = *sound;
            preserved.voice = before.voice;
            assert_eq!(preserved, *before);
        }
    }
    assert_eq!(prepared.sounds, original);
    prepared.bgm_commands = vec![AudioCommand::Play {
        voice: VoiceId(u64::MAX - 1),
        sample: SampleId(2),
        at: Timestamp::ZERO,
        gain: 1.0,
    }];
    let solo = plan(1);
    let final_voice = prepare_local_members(
        &prepared,
        &solo,
        maps(&solo),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert!(
        final_voice.configs[0]
            .sounds
            .iter()
            .all(|sound| sound.voice == VoiceId(u64::MAX))
    );
}

#[test]
fn native_cohort_delegates_the_same_member_results_and_keeps_capture_paths_and_native_admission_rules()
 {
    let prepared = prepared();
    let assignments = [
        (PlayerId(7), DeviceId(91)),
        (PlayerId(u32::MAX), DeviceId(19)),
        (PlayerId(2), DeviceId(u64::MAX)),
    ];
    let chosen = ResolvedInputPlan::new(
        assignments
            .iter()
            .map(|&(player, device)| (player, Some(device)))
            .collect(),
    )
    .unwrap();
    let bindings = BTreeMap::from([(0x11, 4), (0x12, 5)]);
    let mut config = CohortPreparation {
        host: HOST,
        output: OUTPUT,
        early: 11,
        late: 23,
        offset: -7,
        preroll: 100_000_000,
        start: Timestamp::ZERO,
        end: None,
        chart_seed: u64::MAX,
        bindings: &bindings,
        record_replay: Some(Path::new("records/local.bkr")),
        replay_max_bytes: 65_536,
        replay_max_records: 128,
    };
    let judging = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(11),
            late: Duration::from_nanos(23),
        }],
        Duration::from_nanos(-7),
    )
    .unwrap();
    let common = prepare_local_members(
        &prepared,
        &chosen,
        assignments
            .iter()
            .map(|&(_, device)| map(DeviceSelector::Exact(device), controls(0)))
            .collect(),
        judging.clone(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    let native = prepare_cohort(
        &prepared,
        &assignments,
        &CompetitionOptions::default(),
        &config,
    )
    .unwrap();
    assert_eq!(native.reserved, common.reserved);
    let limits = capture_limits(true, 65_536, 128).unwrap().unwrap();
    for (index, (actual, expected)) in native.configs.iter().zip(&common.configs).enumerate() {
        assert_eq!(
            (actual.player, actual.device),
            (expected.player, expected.device)
        );
        assert_eq!(actual.bindings.bindings(), expected.bindings.bindings());
        assert_eq!(actual.sounds, expected.sounds);
        assert_eq!(
            actual.judge.stable_hash().unwrap(),
            expected.judge.stable_hash().unwrap()
        );
        let capture = native.states[index].capture.as_ref().unwrap();
        let pristine = LiveReplayCapture::new_at_with_chart_seed(
            &expected.judge,
            HOST,
            limits,
            Timestamp::ZERO,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(capture.header(), pristine.header());
        assert!(capture.records().is_empty());
        let setup = decode_section_setup(&capture.header().options).unwrap();
        assert_eq!(setup.profile, judging);
        assert_eq!(setup.chart_seed, u64::MAX);
        assert_eq!(setup.start, Timestamp::ZERO);
        assert_eq!(setup.input_mode, BmsInputMode::ButtonOnly);
        assert_eq!(
            native.states[index].last_song,
            Timestamp::from_nanos(-100_000_000)
        );
        assert!(native.states[index].completion.is_some());
        assert!(native.states[index].competition.is_none());
    }
    assert_eq!(
        native.save_paths[0].1.as_deref(),
        Some(Path::new("records/local.p7.bkr"))
    );
    assert_eq!(
        native.save_paths[1].1.as_deref(),
        Some(Path::new("records/local.p4294967295.bkr"))
    );
    for invalid in [
        vec![assignments[0]],
        vec![(PlayerId(7), DeviceId(0)), assignments[1]],
        vec![assignments[0], (PlayerId(7), DeviceId(19))],
        vec![assignments[0], (PlayerId(2), DeviceId(91))],
    ] {
        assert!(
            prepare_cohort(&prepared, &invalid, &CompetitionOptions::default(), &config).is_err()
        );
    }
    config.output = HOST;
    assert!(
        prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config
        )
        .is_err()
    );
    config.output = OUTPUT;
    let missing = BTreeMap::from([(0x11, 4)]);
    config.bindings = &missing;
    assert!(
        prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config
        )
        .is_err()
    );
    let duplicate = BTreeMap::from([(0x11, 4), (0x12, 4)]);
    config.bindings = &duplicate;
    assert!(
        prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &config
        )
        .is_err()
    );
    config.bindings = &bindings;
    let network = CompetitionOptions {
        network: Some(NetworkRole::Join(([127, 0, 0, 1], 1234).into())),
        ..CompetitionOptions::default()
    };
    assert!(prepare_cohort(&prepared, &assignments, &network, &config).is_err());
}
