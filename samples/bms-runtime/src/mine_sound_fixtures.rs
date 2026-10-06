//! Deferred WAV00 plans and actual portable owners. Typed PCM setup here does
//! not bypass the file admission guard or establish native/audio acceptance.
use crate::{
    PreparedBms,
    bgm::BgmFeedReport,
    completion::SongCompletion,
    input_sounds::{InputSoundIdentity, InputSoundPlan},
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::{
        prepare_local_input_sounds, prepare_local_members, prepare_local_mine_sounds,
    },
    local_runtime::{InputResult, RuntimeGroup, VoiceAllocator},
    mine_plan::prepare_judge,
    mine_sounds::{MineSoundIdentity, MineSoundPlan},
    replay_capture::{setup_input_header, setup_input_sound_header},
    replay_playback::validate_section_setup,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepGameplayError, StepLocalGameplay},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig, PcmLimits,
        PcmSample, QueuePushError, SampleBank, SampleId, VoiceId, command_queue,
    },
    chart::{Beat, ObjectId},
    input::*,
    judge::{HazardId, HazardOutcome, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayCodecLimits, decode_replay},
    runtime::{
        RuntimeProcessingClock, SoundBinding,
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
        input_sound::{InputSoundMarker, InputSoundTimeline},
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsChart, BmsInputMode, MineDamage, parse};

const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 20_000_000_000;
const MIXED: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 normal\n#WAV02 press\n#00011:01\n#00031:02\n#00032:02\n#000D1:1E01ZZ00\n#000D2:001E0000\n#00101:01";
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn source(text: &str) -> BmsChart {
    parse(text, Default::default()).unwrap()
}
fn cap() -> PcmLimits {
    PcmLimits::new(1024, 4096, 4).unwrap()
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
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
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: point(1, HOST),
        output_origin: point(2, OUTPUT),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 1,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
    }
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn binding(hazard: u64, voice: u64, gain: f32) -> HazardSoundBinding {
    HazardSoundBinding {
        hazard: HazardId(hazard),
        sample: SampleId(0),
        voice: VoiceId(voice),
        gain,
    }
}
fn ordinary(voice: u64) -> SoundBinding {
    SoundBinding {
        object: ObjectId(1),
        stage: JudgeStage::Instant,
        sample: SampleId(1),
        voice: VoiceId(voice),
        gain: 0.5,
    }
}
fn prepared(source: BmsChart, zero_frames: Option<usize>) -> PreparedBms {
    let compiled = source.compile().unwrap();
    let gain = source.wav_gain().unwrap();
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: note.sample,
            voice: VoiceId(note.object.0 + 10),
            gain,
        })
        .collect();
    let bgm_commands = compiled
        .bgm
        .iter()
        .map(|cue| play(cue.sample.0, 90, cue.at.as_nanos(), gain))
        .collect();
    let format = AudioFormat::new(10, 1).unwrap();
    let mut bank = SampleBank::new(format, cap()).unwrap();
    for (id, pcm) in [(1, vec![0.25, -0.25]), (2, vec![0.5, -0.5])] {
        bank.insert(SampleId(id), PcmSample::new(format, pcm, cap()).unwrap())
            .unwrap();
    }
    if let Some(frames) = zero_frames {
        bank.insert(
            SampleId(0),
            PcmSample::new(format, vec![0.125; frames], cap()).unwrap(),
        )
        .unwrap();
    }
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands,
    }
}
fn keys(selector: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings([0x11, 0x12].map(|control| Binding {
        device: selector,
        physical: PhysicalControlId::keyboard(91u16),
        game_control: GameControlId(control),
    }))
    .unwrap()
}
fn routes() -> ResolvedInputPlan {
    ResolvedInputPlan::new(vec![
        (PlayerId(7), Some(DeviceId(3))),
        (PlayerId(u32::MAX), Some(DeviceId(u64::MAX))),
    ])
    .unwrap()
}
fn local_keys() -> Vec<BindingMap> {
    vec![
        keys(DeviceSelector::Exact(DeviceId(3))),
        keys(DeviceSelector::Exact(DeviceId(u64::MAX))),
    ]
}
fn input(device: u64, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(device), point(1, HOST + song), sequence),
        control: PhysicalControlId::keyboard(91u16),
        state,
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
fn solo(
    prepared: PreparedBms,
    config: StepGameplayConfig,
) -> Result<(StepGameplay, SampleBank), StepGameplayError> {
    StepGameplay::new_section_with_input_mode(
        prepared,
        config,
        keys(DeviceSelector::Any),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
    )
}

#[test]
fn parsed_plan_uses_nonfatal_original_ids_exact_gain_and_checked_lane_voices_even_when_silent() {
    let source = source(
        "#BASE 62\n#BPM 120\n#BPM01 240\n#STOP01 48\n#VOLWAV 25\n#WAV00 blast\n#00002:0.75\n#00008:000100\n#00009:000100\n#000D2:1E0001\n#000D1:00ZZ00\n#001D1:01",
    );
    let before = source.clone();
    let press = InputSoundTimeline::new(
        vec![InputSoundMarker {
            control: GameControlId(0x11),
            at: ts(0),
            sample: SampleId(1),
            voice: VoiceId(9_007_199_254_741_000),
            gain: -0.5,
        }],
        1,
    )
    .unwrap();
    let plan = MineSoundPlan::prepare(
        &source,
        &[ordinary(8)],
        &[play(1, 10, 0, 1.0)],
        Some(&press),
        4,
    )
    .unwrap();
    assert_eq!(
        source
            .compile_mines()
            .unwrap()
            .iter()
            .map(|mine| (mine.ordinal, mine.lane.channel(), mine.at.as_nanos()))
            .collect::<Vec<_>>(),
        [
            (0, 0x12, 0),
            (2, 0x11, 500_000_000),
            (1, 0x12, 1_000_000_000),
            (3, 0x11, 1_250_000_000),
        ]
    );
    assert_eq!(
        plan.bindings(),
        [
            binding(0, 9_007_199_254_741_002, 0.25),
            binding(1, 9_007_199_254_741_002, 0.25),
            binding(3, 9_007_199_254_741_001, 0.25)
        ]
    );
    assert_eq!(plan.samples(), [SampleId(0)]);
    assert_eq!(plan.timeline().unwrap().bindings(), plan.bindings());
    assert_eq!(source, before);
    assert!(
        MineSoundPlan::prepare(&source, &[], &[], None, 3).is_err(),
        "fatal markers still consume source capacity"
    );
    assert!(MineSoundPlan::prepare(&source, &[ordinary(u64::MAX)], &[], None, 4).is_err());
    let one_lane = self::source("#BPM 60\n#WAV00 blast\n#000D1:1E01");
    assert_eq!(
        MineSoundPlan::prepare(&one_lane, &[ordinary(u64::MAX - 1)], &[], None, 2)
            .unwrap()
            .bindings(),
        [binding(0, u64::MAX, 1.0), binding(1, u64::MAX, 1.0)]
    );
    let mut absent = source.clone();
    absent.samples.remove(&0);
    let mut fatal = self::source("#BPM 60\n#WAV00 unused\n#000D1:ZZ");
    fatal
        .metadata
        .insert("VOLWAV".into(), "invalid-unused-gain".into());
    for silent in [&absent, &fatal, &self::source("#BPM 60\n#WAV00 unused")] {
        let plan = MineSoundPlan::prepare(silent, &[ordinary(u64::MAX)], &[], None, 4).unwrap();
        assert!(
            plan.bindings().is_empty() && plan.samples().is_empty() && plan.timeline().is_none()
        );
    }
    let mut invalid = absent;
    invalid.mine_ticks_per_beat = 0;
    assert!(MineSoundPlan::prepare(&invalid, &[], &[], None, 4).is_err());
    let mut gain = source.clone();
    gain.metadata.insert("VOLWAV".into(), "NaN".into());
    assert!(MineSoundPlan::prepare(&gain, &[], &[], None, 4).is_err());
    for bad_gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            MineSoundPlan::prepare(
                &source,
                &[SoundBinding {
                    gain: bad_gain,
                    ..ordinary(1)
                }],
                &[],
                None,
                4
            )
            .is_err()
        );
    }
}

#[test]
fn audible_semantic_identity_excludes_paths_voices_and_nonfatal_damage_but_actual_capture_binds_setup()
 {
    let original = source(MIXED);
    let identity = MineSoundIdentity::from_source(&original).unwrap().unwrap();
    let combined = InputSoundIdentity::from_source(&original).unwrap().unwrap();
    let original_press = InputSoundPlan::prepare(&original, &[], &[], 4)
        .unwrap()
        .timeline();
    let remapped_press = InputSoundPlan::prepare(&original, &[ordinary(100)], &[], 4)
        .unwrap()
        .timeline();
    let original_plan =
        MineSoundPlan::prepare(&original, &[], &[], Some(&original_press), 4).unwrap();
    let remapped_plan =
        MineSoundPlan::prepare(&original, &[ordinary(100)], &[], Some(&remapped_press), 4).unwrap();
    assert_eq!(
        original_plan.bindings(),
        [binding(0, 3, 0.5), binding(1, 3, 0.5), binding(3, 4, 0.5)]
    );
    assert_eq!(
        remapped_plan.bindings(),
        [
            binding(0, 103, 0.5),
            binding(1, 103, 0.5),
            binding(3, 104, 0.5)
        ]
    );
    assert_eq!(original_plan.samples(), remapped_plan.samples());
    assert_eq!(
        InputSoundIdentity::from_source(&original).unwrap(),
        Some(combined)
    );
    let mut renamed = original.clone();
    renamed.samples.insert(0, "elsewhere/爆発.wav".into());
    renamed.mines.reverse();
    for mine in &mut renamed.mines {
        mine.line = usize::MAX;
    }
    assert_eq!(
        MineSoundIdentity::from_source(&renamed).unwrap(),
        Some(identity)
    );
    assert_eq!(
        InputSoundIdentity::from_source(&renamed).unwrap(),
        Some(combined)
    );
    let mut numeric = original.clone();
    numeric.mines[0].damage = MineDamage::from_raw(1).unwrap();
    assert_eq!(
        MineSoundIdentity::from_source(&numeric).unwrap(),
        Some(identity)
    );
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.mines[0].ordinal = u64::MAX;
    changes.push(changed);
    let mut changed = original.clone();
    changed.mines[0].beat = Beat::new(3).unwrap();
    changes.push(changed);
    let mut changed = original.clone();
    changed.mines[0].lane = original
        .mines
        .iter()
        .find(|mine| mine.lane != original.mines[0].lane)
        .unwrap()
        .lane;
    changes.push(changed);
    let mut changed = original.clone();
    changed.mines[0].damage = MineDamage::from_raw(1295).unwrap();
    changes.push(changed);
    let mut changed = original.clone();
    changed.metadata.insert("VOLWAV".into(), "25".into());
    changes.push(changed);
    for changed in &changes {
        assert_ne!(
            MineSoundIdentity::from_source(changed)
                .unwrap()
                .unwrap()
                .fingerprint(),
            identity.fingerprint()
        );
    }
    let mut silent = original.clone();
    silent.samples.remove(&0);
    let mut legacy = silent.clone();
    legacy.mines.clear();
    assert_eq!(MineSoundIdentity::from_source(&silent).unwrap(), None);
    assert_eq!(
        InputSoundIdentity::from_source(&silent).unwrap(),
        InputSoundIdentity::from_source(&legacy).unwrap()
    );
    let mut fatal_only = original.clone();
    for mine in &mut fatal_only.mines {
        mine.damage = MineDamage::from_raw(1295).unwrap();
    }
    assert_eq!(
        InputSoundIdentity::from_source(&fatal_only).unwrap(),
        InputSoundIdentity::from_source(&legacy).unwrap()
    );
    let empty = source("#BPM 60\n#WAV00 unused\n#000D1:ZZ");
    assert_eq!(InputSoundIdentity::from_source(&empty).unwrap(), None);
    let judge = prepare_judge(
        &empty,
        empty.compile().unwrap().chart,
        profile(),
        BmsInputMode::ButtonOnly,
        4,
    )
    .unwrap();
    assert_eq!(
        setup_input_sound_header(
            &judge,
            ClockDomainId(1),
            limits(),
            ts(0),
            0,
            None,
            BmsInputMode::ButtonOnly,
            None
        )
        .unwrap(),
        setup_input_header(
            &judge,
            ClockDomainId(1),
            limits(),
            ts(0),
            0,
            None,
            BmsInputMode::ButtonOnly
        )
        .unwrap()
    );
    let (mut game, _) = solo(prepared(original.clone(), Some(2)), config()).unwrap();
    let header = game.competition_header(limits(), 0).unwrap();
    let mut expected = b"bms-judge-setup/v2:".to_vec();
    expected.extend_from_slice(&game.judge().stable_hash().unwrap().to_le_bytes());
    expected.extend_from_slice(&combined.fingerprint().to_le_bytes());
    assert_eq!(header.chart_identity, expected);
    game.configure_capture(limits(), 0).unwrap();
    game.activate(config().host_origin).unwrap();
    game.process_input(
        input(3, 0, 1, ButtonState::Down),
        &NoMapping,
        point(2, OUTPUT),
    )
    .unwrap();
    game.fail(); // Explicitly stop acquisition before exporting its accepted prefix.
    let file = decode_replay(&game.take_replay().unwrap().unwrap(), limits()).unwrap();
    assert_eq!(file.header, header);
    assert!(validate_section_setup(&original, &file, limits()).is_ok());
    assert!(validate_section_setup(&renamed, &file, limits()).is_ok());
    assert!(
        validate_section_setup(&numeric, &file, limits()).is_err(),
        "ordinary pristine hazard identity still owns the opaque damage value"
    );
    assert!(validate_section_setup(&silent, &file, limits()).is_err());
    for changed in changes {
        assert!(validate_section_setup(&changed, &file, limits()).is_err());
    }
}

type PressPlans = Vec<(PlayerId, InputSoundTimeline)>;
type MinePlans = Vec<(PlayerId, HazardSoundTimeline)>;
fn group() -> (RuntimeGroup, CommandConsumer, PressPlans, MinePlans) {
    let prepared = prepared(source(MIXED), Some(2));
    let members = prepare_local_members(
        &prepared,
        &routes(),
        local_keys(),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    let presses =
        prepare_local_input_sounds(&prepared, &members.configs, &members.reserved).unwrap();
    let mines = prepare_local_mine_sounds(&prepared, &members.configs, &members.reserved, &presses)
        .unwrap();
    assert_eq!(
        mines
            .iter()
            .map(|(id, timeline)| (
                *id,
                timeline
                    .bindings()
                    .iter()
                    .map(|b| b.voice.0)
                    .collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>(),
        [
            (PlayerId(7), vec![97, 97, 98]),
            (PlayerId(u32::MAX), vec![99, 99, 100]),
        ]
    );
    let (producer, consumer) = command_queue(16).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        producer,
        members.configs,
        0,
        &members.reserved,
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer, presses, mines)
}
fn replace_mine_voice(plan: &HazardSoundTimeline, old: u64, new: u64) -> HazardSoundTimeline {
    let mut bindings = plan.bindings().to_vec();
    for binding in &mut bindings {
        if binding.voice == VoiceId(old) {
            binding.voice = VoiceId(new);
        }
    }
    HazardSoundTimeline::new(bindings, 4).unwrap()
}

#[test]
fn allocator_and_both_group_installation_orders_preserve_aliases_and_reject_collisions_atomically()
{
    let mut allocator = VoiceAllocator::new(u64::MAX - 1);
    let mut aliases = [binding(0, 9, 1.0), binding(1, 9, 1.0), binding(2, 3, 1.0)];
    allocator.remap_hazard_sounds(&mut aliases).unwrap();
    assert_eq!(
        aliases,
        [
            binding(0, u64::MAX - 1, 1.0),
            binding(1, u64::MAX - 1, 1.0),
            binding(2, u64::MAX, 1.0)
        ]
    );
    let mut extra = [binding(3, 8, 1.0)];
    let before = extra;
    assert!(allocator.remap_hazard_sounds(&mut extra).is_err());
    assert_eq!(extra, before);
    let mut retry = VoiceAllocator::new(u64::MAX);
    let mut pair = [binding(0, 9, 1.0), binding(1, 3, 1.0)];
    let before = pair;
    assert!(retry.remap_hazard_sounds(&mut pair).is_err());
    assert_eq!(pair, before);
    pair[1].voice = VoiceId(9);
    retry.remap_hazard_sounds(&mut pair).unwrap();
    assert_eq!(pair, [binding(0, u64::MAX, 1.0), binding(1, u64::MAX, 1.0)]);
    retry.remap_hazard_sounds(&mut []).unwrap();
    for mines_first in [false, true] {
        let (mut owner, mut consumer, presses, mines) = group();
        let hashes = [PlayerId(7), PlayerId(u32::MAX)]
            .map(|id| owner.member_judge(id).unwrap().stable_hash().unwrap());
        let mut reversed = mines.clone();
        reversed.reverse();
        let mut unknown = mines.clone();
        unknown[1].0 = PlayerId(8);
        let mut duplicated = mines.clone();
        duplicated[1].0 = PlayerId(7);
        let mut invalid = vec![
            vec![],
            vec![mines[0].clone()],
            reversed,
            unknown,
            duplicated,
        ];
        for voice in [90, 91, 97] {
            let mut crossed = mines.clone();
            crossed[1].1 = replace_mine_voice(&crossed[1].1, 99, voice);
            invalid.push(crossed);
        }
        for candidate in invalid {
            assert!(owner.configure_hazard_sounds(candidate).is_err());
            assert!(!owner.poisoned());
            assert_eq!(
                [PlayerId(7), PlayerId(u32::MAX)].map(|id| owner
                    .member_judge(id)
                    .unwrap()
                    .stable_hash()
                    .unwrap()),
                hashes
            );
        }
        if mines_first {
            owner.configure_hazard_sounds(mines.clone()).unwrap();
            let mut collision = presses.clone();
            let mut markers = collision[1].1.markers().to_vec();
            markers[0].voice = VoiceId(97);
            collision[1].1 = InputSoundTimeline::new(markers, 4).unwrap();
            assert!(owner.configure_input_sounds(collision).is_err());
            owner.configure_input_sounds(presses.clone()).unwrap();
        } else {
            owner.configure_input_sounds(presses.clone()).unwrap();
            let mut collision = mines.clone();
            collision[1].1 = replace_mine_voice(&collision[1].1, 99, 95);
            assert!(owner.configure_hazard_sounds(collision).is_err());
            owner.configure_hazard_sounds(mines.clone()).unwrap();
        }
        assert!(owner.configure_hazard_sounds(mines.clone()).is_err());
        for (device, player, voices) in [
            (3, PlayerId(7), [91, 94, 97]),
            (u64::MAX, PlayerId(u32::MAX), [92, 96, 99]),
        ] {
            let InputResult::Processed(reports) = owner
                .process_input(
                    input(device, 0, 1, ButtonState::Down),
                    &NoMapping,
                    point(2, OUTPUT),
                )
                .unwrap()
            else {
                panic!("admitted source")
            };
            assert_eq!(reports[0].player, player);
            assert_eq!(
                reports[0].report.audio_commands,
                [
                    play(1, voices[0], OUTPUT, 0.5),
                    play(2, voices[1], OUTPUT, 0.5),
                    play(0, voices[2], OUTPUT, 0.5)
                ]
            );
            for command in &reports[0].report.audio_commands {
                assert_eq!(consumer.try_pop().unwrap(), *command);
            }
        }
        assert!(owner.configure_input_sounds(presses).is_err());
        assert!(!owner.poisoned());
    }
    let (mut late, _consumer, _, mines) = group();
    late.advance_to(point(1, HOST), &NoMapping, point(2, OUTPUT))
        .unwrap();
    assert!(late.configure_hazard_sounds(mines).is_err());
}

#[test]
fn real_step_solo_and_local_publish_disjoint_wav00_batches_and_keep_failure_prefixes() {
    let (mut game, bank) = solo(prepared(source(MIXED), Some(2)), config()).unwrap();
    assert_eq!(bank.len(), 3);
    game.activate(config().host_origin).unwrap();
    let first = game
        .process_input(
            input(3, 0, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert_eq!(
        first.audio_commands,
        [
            play(1, 11, OUTPUT, 0.5),
            play(2, 92, OUTPUT, 0.5),
            play(0, 93, OUTPUT, 0.5)
        ]
    );
    assert_eq!(first.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(
        (game.score().hits, game.mine_damage().half_percent_damage),
        (1, 50)
    );
    let batch = game.take_commands(16).unwrap().unwrap();
    assert_eq!(batch.commands, first.audio_commands);
    game.acknowledge(batch.sequence, batch.commands.len(), true)
        .unwrap();
    let next = game
        .advance_to(
            point(1, HOST + 1_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    assert_eq!(
        next.audio_commands,
        [
            play(0, 93, OUTPUT + 1_000_000_000, 0.5),
            play(0, 94, OUTPUT + 1_000_000_000, 0.5)
        ]
    );
    let fatal = game
        .advance_to(
            point(1, HOST + 2_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(fatal.hazard_events[0].value, 1295);
    assert!(fatal.audio_commands.is_empty() && game.mine_damage().instant_death);
    assert!(
        !game.failed(),
        "sound installation does not invent a fatal-stop policy"
    );
    let prepared = prepared(source(MIXED), Some(2));
    let pointer = prepared.bank.get(SampleId(0)).unwrap().samples().as_ptr();
    let (mut local, bank) = StepLocalGameplay::new_section(
        prepared,
        config(),
        routes(),
        local_keys(),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert_eq!(bank.get(SampleId(0)).unwrap().samples().as_ptr(), pointer);
    local.activate(config().host_origin).unwrap();
    for (device, player, voices) in [
        (3, PlayerId(7), [91, 94, 97]),
        (u64::MAX, PlayerId(u32::MAX), [92, 96, 99]),
    ] {
        let InputResult::Processed(reports) = local
            .process_input(
                input(device, 0, 1, ButtonState::Down),
                &NoMapping,
                point(2, OUTPUT),
            )
            .unwrap()
        else {
            panic!("admitted local source")
        };
        assert_eq!(reports[0].player, player);
        assert_eq!(
            reports[0].report.audio_commands,
            [
                play(1, voices[0], OUTPUT, 0.5),
                play(2, voices[1], OUTPUT, 0.5),
                play(0, voices[2], OUTPUT, 0.5)
            ]
        );
        assert_eq!(local.mine_damage(player).unwrap().half_percent_damage, 50);
    }
    assert!(solo(self::prepared(source(MIXED), None), config()).is_err());
    assert!(
        StepLocalGameplay::new_section(
            self::prepared(source(MIXED), None),
            config(),
            routes(),
            local_keys(),
            ts(0),
            None,
            BmsInputMode::ButtonOnly
        )
        .is_err()
    );
    let mut small = config();
    small.command_capacity = 2;
    let (mut failing, _) = solo(self::prepared(source(MIXED), Some(2)), small).unwrap();
    failing.activate(small.host_origin).unwrap();
    let StepGameplayError::Report {
        report,
        score_error,
        capture_error,
    } = failing
        .process_input(
            input(3, 0, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap_err()
    else {
        panic!("actual queue failure report")
    };
    assert!(score_error.is_none() && capture_error.is_none());
    assert_eq!(
        report.audio_commands,
        [play(1, 11, OUTPUT, 0.5), play(2, 92, OUTPUT, 0.5)]
    );
    assert_eq!(report.audio_failures.len(), 1);
    assert_eq!(report.audio_failures[0].command, play(0, 93, OUTPUT, 0.5));
    assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(
        (
            failing.score().hits,
            failing.mine_damage().half_percent_damage
        ),
        (1, 50)
    );
    assert!(failing.failed());
}

#[test]
fn completion_accounts_for_real_explosion_pcm_tail_offset_and_actual_silent_output_barrier() {
    let text = "#BPM 60\n#WAV00 blast\n#000D1:001E";
    for (offset, seconds) in [(0, 5), (500_000_000, 4), (-500_000_000, 5)] {
        let data = prepared(source(text), Some(25));
        assert_eq!(
            SongCompletion::prepare(&data, 100_000_000_000, offset, 0, ClockDomainId(2))
                .unwrap()
                .calibration_seconds(),
            seconds
        );
    }
    let data = prepared(source(text), Some(25));
    assert!(SongCompletion::prepare(&data, 0, i64::MIN, 0, ClockDomainId(2)).is_err());
    assert!(SongCompletion::prepare(&data, 0, 0, i64::MAX, ClockDomainId(2)).is_err());
    assert!(
        SongCompletion::prepare(&prepared(source(text), None), 0, 0, 0, ClockDomainId(2)).is_err()
    );
    for silent in [
        "#BPM 60\n#000D1:001E",
        "#BPM 60\n#WAV00 unused\n#000D1:00ZZ",
    ] {
        let data = prepared(source(silent), None);
        assert_eq!(
            SongCompletion::prepare(&data, 0, 0, 0, ClockDomainId(2))
                .unwrap()
                .calibration_seconds(),
            3
        );
        assert!(solo(data, config()).is_ok());
    }
    let mut judge = prepare_judge(
        &data.source,
        data.compiled.chart.clone(),
        profile(),
        BmsInputMode::ButtonOnly,
        4,
    )
    .unwrap();
    let bound = GameInputEvent {
        game_control: GameControlId(0x11),
        physical: input(3, 0, 1, ButtonState::Down),
    };
    judge.push_input(&bound, ts(0)).unwrap();
    judge.advance_to(ts(2_000_000_001)).unwrap();
    let timeline = MineSoundPlan::prepare(&data.source, &data.sounds, &[], None, 4)
        .unwrap()
        .timeline()
        .unwrap();
    let command = timeline
        .command_for(&judge.hazard_events()[0], ts(2_000_000_000))
        .unwrap();
    assert_eq!(command, play(0, 1, 2_000_000_000, 1.0));
    let mut completion = SongCompletion::prepare(&data, 0, 0, 0, ClockDomainId(2)).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer.try_push(command).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            data.bank.format(),
            ClockDomainId(2),
            ts(0),
            AudioLimits::new(8, 4, 8, 16, 16).unwrap(),
        ),
        data.bank,
        consumer,
    )
    .unwrap();
    for second in 1..=5 {
        let mut pcm = [0.0; 10];
        let rendered = mixer.render(&mut pcm).unwrap();
        let finished = completion
            .observe(
                &judge,
                ts(2_000_000_001),
                BgmFeedReport::default(),
                Some(rendered),
                Some(point(2, second * 1_000_000_000)),
            )
            .unwrap();
        assert_eq!(finished, second == 5);
        if second <= 2 {
            assert_eq!(pcm, [0.0; 10]);
        }
        if second == 3 || second == 4 {
            assert_eq!(pcm, [0.125; 10]);
        }
        if second == 5 {
            assert_eq!(
                pcm,
                [0.125, 0.125, 0.125, 0.125, 0.125, 0.0, 0.0, 0.0, 0.0, 0.0]
            );
        }
    }
}
