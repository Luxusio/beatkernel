//! Deferred typed composition only: prepare_from_source still refuses invisible charts.
//! These fixtures exercise actual software owners, capture/replay and PCM mixing;
//! they do not assert native presentation, asset admission or hardware playback.
use crate::{
    PreparedBms,
    bgm::BgmFeedReport,
    completion::SongCompletion,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::{prepare_local_input_sounds, prepare_local_members},
    local_runtime::{InputResult, RuntimeGroup, SoloRuntime},
    replay_audio::{completed_render_cursor, plan_audio, plan_section_audio},
    section_start::prepare_at,
    step_gameplay::{StepGameplay, StepGameplayConfig, StepLocalGameplay},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        SampleBank, SampleId, VoiceId, command_queue,
    },
    input::{
        AxisEvent, AxisMode, Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId,
        DeviceId, DeviceSelector, EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent,
        Position2, TouchEvent, TouchPhase,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayCodecLimits, decode_replay},
    runtime::{RuntimeProcessingClock, SoundBinding, input_sound::InputSoundTimeline},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsInputMode, parse};

const SOLO: &str = "#BPM 60\n#VOLWAV 50\n#WAV01 one\n#WAV02 two\n#00031:01020000\n#00051:00000101";
const LOCAL: &str =
    "#BPM 60\n#WAV01 one\n#WAV02 two\n#00031:0102\n#00032:0201\n#00011:0001\n#00101:01";
const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 1_000_000_000;
fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn point(domain: u32, value: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(value),
    }
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(64, 128, 4).unwrap()
}
fn replay_limits() -> ReplayCodecLimits {
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
        preroll: Duration::from_nanos(100_000_000),
        early_ns: 0,
        late_ns: 0,
        offset_ns: 100_000_000,
        command_capacity: 32,
        bgm_pending: 4,
        bgm_lookahead: Duration::from_nanos(1_000_000_000),
        telemetry_capacity: 0,
    }
}
fn bindings(selector: DeviceSelector, both: bool) -> BindingMap {
    let mut rows = vec![Binding {
        device: selector,
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }];
    if both {
        rows.push(Binding {
            device: selector,
            physical: PhysicalControlId::keyboard(92),
            game_control: GameControlId(0x12),
        });
    }
    BindingMap::from_bindings(rows).unwrap()
}
fn prepared(text: &str, include_second_sample: bool) -> PreparedBms {
    let source = parse(text, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let gain = source.wav_gain().unwrap();
    let sounds = source
        .notes
        .iter()
        .map(|note| {
            let object = compiled
                .chart
                .objects()
                .iter()
                .find(|o| o.id == note.object)
                .unwrap();
            SoundBinding {
                object: note.object,
                stage: if object.time.end.is_some() {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                },
                sample: note.sample,
                voice: VoiceId(note.object.0),
                gain,
            }
        })
        .collect();
    let bgm_commands = compiled
        .bgm
        .iter()
        .map(|cue| AudioCommand::Play {
            sample: cue.sample,
            voice: VoiceId(90),
            at: cue.at,
            gain,
        })
        .collect();
    let format = AudioFormat::new(10, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits()).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0, -1.0], pcm_limits()).unwrap(),
    )
    .unwrap();
    if include_second_sample {
        bank.insert(
            SampleId(2),
            PcmSample::new(format, vec![0.25, 0.75], pcm_limits()).unwrap(),
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
fn section() -> PreparedBms {
    prepare_at(prepared(SOLO, true), ts(500_000_000), pcm_limits())
        .unwrap()
        .0
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
fn input(source: u64, code: u16, host: i64, sequence: u64, kind: u8) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(source), point(1, host), sequence);
    meta.original_clock_point = Some(point(99, 9_007_199_254_740_993));
    let control = PhysicalControlId::keyboard(code);
    match kind {
        0..=2 => PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control,
            state: [ButtonState::Down, ButtonState::Up, ButtonState::Repeat][kind as usize],
        }),
        3 => PhysicalInputEvent::Axis(AxisEvent {
            meta,
            control,
            value: 0.75,
            mode: AxisMode::Absolute,
        }),
        4..=7 => PhysicalInputEvent::Touch(TouchEvent {
            meta,
            control,
            contact: ContactId(u64::MAX),
            phase: [
                TouchPhase::Down,
                TouchPhase::Move,
                TouchPhase::Up,
                TouchPhase::Cancel,
            ][kind as usize - 4],
            position: Position2 { x: -4.0, y: 800.5 },
            pressure: Some(0.5),
        }),
        _ => panic!("fixture event kind"),
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
fn mixer(bank: SampleBank, end: Option<u64>) -> (beatkernel::audio::CommandProducer, Mixer) {
    let (producer, consumer) = command_queue(32).unwrap();
    let mut config = MixerConfig::new(
        bank.format(),
        ClockDomainId(2),
        ts(OUTPUT),
        AudioLimits::new(32, 8, 32, 32, 32).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn render(bank: SampleBank, commands: &[AudioCommand], end: Option<u64>) -> Vec<f32> {
    let (mut producer, mut mixer) = mixer(bank, end);
    for command in commands {
        producer.try_push(*command).unwrap();
    }
    let mut output = vec![0.0; 30];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(completed_render_cursor(&report).unwrap(), 30);
    output
}

#[test]
fn real_step_and_recorded_replay_select_original_song_samples_and_mix_identical_pcm() {
    for (mode, end) in [
        (BmsInputMode::ButtonOnly, None),
        (BmsInputMode::ButtonOnly, Some(ts(3_000_000_000))),
        (BmsInputMode::ButtonOrContact, Some(ts(3_000_000_000))),
    ] {
        let (mut owner, bank) = StepGameplay::new_section_with_input_mode(
            section(),
            config(),
            bindings(DeviceSelector::Any, false),
            ts(500_000_000),
            end,
            mode,
        )
        .unwrap();
        owner.configure_capture(replay_limits(), 7).unwrap();
        owner.activate(config().host_origin).unwrap();
        let (down, up, repeated) = if mode == BmsInputMode::ButtonOrContact {
            (4, 6, 5)
        } else {
            (0, 1, 2)
        };
        let mut actual_commands = Vec::new();
        let mut actual_judgements = Vec::new();
        // 0.9s has effective judge time 1.0s: selection must still use 0.9s.
        // The 1.9s press hits the real 2s HoldHead and suppresses sample 02.
        for (sequence, (song, kind)) in [
            (500_000_000, down),
            (600_000_000, down),
            (700_000_000, up),
            (900_000_000, down),
            (1_000_000_000, repeated),
            (1_100_000_000, up),
            (1_200_000_000, 3),
            (1_300_000_000, down),
            (1_400_000_000, up),
            (1_900_000_000, down),
            (2_000_000_000, down),
            (2_100_000_000, up),
            (2_500_000_000, 0),
            (2_600_000_000, 1),
            (3_000_000_000, down),
        ]
        .into_iter()
        .enumerate()
        {
            let report = owner
                .process_input(
                    input(
                        u64::MAX,
                        91,
                        HOST + song - 400_000_000,
                        sequence as u64 + 1,
                        kind,
                    ),
                    &NoMapping,
                    point(2, OUTPUT + song - 400_000_000),
                )
                .unwrap();
            assert!(report.audio_failures.is_empty() && report.judge_error.is_none());
            actual_commands.extend(report.audio_commands);
            actual_judgements.extend(report.judge_events);
        }
        let advanced = owner
            .advance_to(
                point(1, HOST + 2_600_000_000),
                &NoMapping,
                point(2, OUTPUT + 2_600_000_000),
            )
            .unwrap();
        assert!(advanced.audio_commands.is_empty() && advanced.audio_failures.is_empty());
        actual_judgements.extend(advanced.judge_events);
        let mut expected = vec![
            play(1, 2, 1_100_000_000, 0.5),
            play(1, 2, 1_500_000_000, 0.5),
            play(2, 2, 1_900_000_000, 0.5),
            play(1, 1, 2_500_000_000, 0.5),
            play(2, 2, 3_100_000_000, 0.5),
        ];
        if end.is_none() {
            expected.push(play(2, 2, 3_600_000_000, 0.5));
        }
        assert_eq!(actual_commands, expected);
        let batch = owner.take_commands(32).unwrap().unwrap();
        assert_eq!(batch.commands, expected);
        owner
            .acknowledge(batch.sequence, expected.len(), true)
            .unwrap();
        assert!(owner.take_commands(32).unwrap().is_none());
        let hash = owner.judge().stable_hash().unwrap();
        let end_frame = owner.playback_end_frame();
        assert_eq!(end_frame, end.map(|_| 26));
        owner.fail();
        let captured = owner.take_replay().unwrap().unwrap();
        let replay_prepared = section();
        let file = decode_replay(&captured, replay_limits()).unwrap();
        let planned = if end.is_none() {
            plan_audio(
                &replay_prepared,
                file,
                replay_limits(),
                point(2, OUTPUT),
                config().preroll,
            )
        } else {
            plan_section_audio(
                &replay_prepared,
                file,
                replay_limits(),
                point(2, OUTPUT),
                config().preroll,
            )
        }
        .unwrap();
        assert_eq!(planned.commands, expected);
        assert_eq!(planned.judge_events, actual_judgements);
        assert_eq!(planned.final_judge_hash, hash);
        assert_eq!(planned.recorded_until, Some(ts(3_000_000_000)));
        let mut pcm = vec![0.0; 30];
        for (frame, pair) in [
            (1, [0.5, -0.5]),
            (5, [0.5, -0.5]),
            (9, [0.125, 0.375]),
            (15, [0.5, -0.5]),
            (21, [0.125, 0.375]),
        ] {
            pcm[frame..frame + 2].copy_from_slice(&pair);
        }
        if end.is_none() {
            pcm[26..28].copy_from_slice(&[0.125, 0.375]);
        }
        assert_eq!(render(bank, &actual_commands, end_frame), pcm);
        assert_eq!(
            render(replay_prepared.bank, &planned.commands, end_frame),
            pcm
        );
        let missing = prepare_at(prepared(SOLO, false), ts(500_000_000), pcm_limits())
            .unwrap()
            .0;
        assert!(
            plan_section_audio(
                &missing,
                decode_replay(&captured, replay_limits()).unwrap(),
                replay_limits(),
                point(2, OUTPUT),
                config().preroll
            )
            .is_err()
        );
    }
}

fn local_plan(count: usize) -> ResolvedInputPlan {
    ResolvedInputPlan::new(
        [
            (PlayerId(7), Some(DeviceId(3))),
            (PlayerId(u32::MAX), Some(DeviceId(u64::MAX))),
            (PlayerId(91), Some(DeviceId(4))),
        ][..count]
            .to_vec(),
    )
    .unwrap()
}
fn local_bindings(count: usize, both: bool) -> Vec<BindingMap> {
    [3, u64::MAX, 4][..count]
        .iter()
        .map(|&device| bindings(DeviceSelector::Exact(DeviceId(device)), both))
        .collect()
}

#[test]
fn local_installation_requires_invisible_bindings_and_keeps_all_member_voices_disjoint_on_one_bank()
{
    let prepared = prepared(LOCAL, true);
    let pcm_address = prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr();
    assert!(
        prepare_local_members(
            &prepared,
            &local_plan(3),
            local_bindings(3, false),
            profile(),
            BmsInputMode::ButtonOnly
        )
        .is_err()
    );
    let members = prepare_local_members(
        &prepared,
        &local_plan(3),
        local_bindings(3, true),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert_eq!(members.reserved, [VoiceId(90)]);
    assert_eq!(
        members
            .configs
            .iter()
            .map(|m| m.sounds[0].voice)
            .collect::<Vec<_>>(),
        [VoiceId(91), VoiceId(92), VoiceId(93)]
    );
    let plans = prepare_local_input_sounds(&prepared, &members.configs, &members.reserved).unwrap();
    for ((player, timeline), (expected_player, voices)) in plans.iter().zip([
        (PlayerId(7), [94, 95]),
        (PlayerId(u32::MAX), [96, 97]),
        (PlayerId(91), [98, 99]),
    ]) {
        assert_eq!(*player, expected_player);
        assert_eq!(
            timeline
                .markers()
                .iter()
                .map(|m| (m.control.0, m.sample.0, m.voice.0))
                .collect::<Vec<_>>(),
            [
                (0x11, 1, voices[0]),
                (0x11, 2, voices[0]),
                (0x12, 2, voices[1]),
                (0x12, 1, voices[1])
            ]
        );
    }
    let mut config = config();
    config.preroll = Duration::ZERO;
    config.offset_ns = 0;
    let (mut local, bank) = StepLocalGameplay::new_section(
        prepared,
        config,
        local_plan(3),
        local_bindings(3, true),
        ts(0),
        None,
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    assert_eq!(bank.len(), 2);
    assert_eq!(
        bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        pcm_address
    );
    local.activate(config.host_origin).unwrap();
    assert!(matches!(
        local
            .process_input(input(88, 91, HOST, 1, 0), &NoMapping, point(2, OUTPUT))
            .unwrap(),
        InputResult::Ignored {
            device: DeviceId(88)
        }
    ));
    for ((source, player), voices) in [
        (3, PlayerId(7)),
        (u64::MAX, PlayerId(u32::MAX)),
        (4, PlayerId(91)),
    ]
    .into_iter()
    .zip([[94, 95], [96, 97], [98, 99]])
    {
        for (code, sample, voice) in [(91, 1, voices[0]), (92, 2, voices[1])] {
            let InputResult::Processed(reports) = local
                .process_input(
                    input(source, code, HOST, code as u64, 0),
                    &NoMapping,
                    point(2, OUTPUT),
                )
                .unwrap()
            else {
                panic!("assigned source")
            };
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].player, player);
            assert!(reports[0].report.judge_events.is_empty());
            assert_eq!(
                reports[0].report.audio_commands,
                [play(sample, voice, OUTPUT, 1.0)]
            );
        }
    }
    let missing = prepared_without_second();
    assert!(prepare_local_input_sounds(&missing, &members.configs, &members.reserved).is_err());
}
fn prepared_without_second() -> PreparedBms {
    prepared(LOCAL, false)
}

fn group() -> (
    RuntimeGroup,
    beatkernel::audio::CommandConsumer,
    Vec<(PlayerId, InputSoundTimeline)>,
) {
    let prepared = prepared(LOCAL, true);
    let members = prepare_local_members(
        &prepared,
        &local_plan(2),
        local_bindings(2, true),
        profile(),
        BmsInputMode::ButtonOnly,
    )
    .unwrap();
    let plans = prepare_local_input_sounds(&prepared, &members.configs, &members.reserved).unwrap();
    let (producer, consumer) = command_queue(32).unwrap();
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
    (group, consumer, plans)
}
fn change_voice(plan: &InputSoundTimeline, old: u64, new: u64) -> InputSoundTimeline {
    let mut markers = plan.markers().to_vec();
    for marker in &mut markers {
        if marker.voice == VoiceId(old) {
            marker.voice = VoiceId(new);
        }
    }
    InputSoundTimeline::new(markers, 4).unwrap()
}

#[test]
fn group_configuration_refusals_are_atomic_retryable_and_solo_uses_the_same_real_installation() {
    let (mut group, mut consumer, plans) = group();
    let hashes = [PlayerId(7), PlayerId(u32::MAX)]
        .map(|id| group.member_judge(id).unwrap().stable_hash().unwrap());
    let mut reversed = plans.clone();
    reversed.reverse();
    let mut unknown = plans.clone();
    unknown[1].0 = PlayerId(90);
    let mut duplicate = plans.clone();
    duplicate[1].0 = PlayerId(7);
    let mut ordinary = plans.clone();
    ordinary[1].1 = change_voice(&plans[1].1, 95, 91);
    let mut reserved = plans.clone();
    reserved[1].1 = change_voice(&plans[1].1, 95, 90);
    let mut crossed = plans.clone();
    crossed[1].1 = change_voice(&plans[1].1, 95, 93);
    for invalid in [
        vec![],
        vec![plans[0].clone()],
        reversed,
        unknown,
        duplicate,
        ordinary,
        reserved,
        crossed,
    ] {
        assert!(group.configure_input_sounds(invalid).is_err());
        assert!(!group.poisoned());
        assert_eq!(
            [PlayerId(7), PlayerId(u32::MAX)].map(|id| group
                .member_judge(id)
                .unwrap()
                .stable_hash()
                .unwrap()),
            hashes
        );
    }
    group.configure_input_sounds(plans.clone()).unwrap();
    assert!(group.configure_input_sounds(plans.clone()).is_err());
    for ((source, player), voice) in [(3, PlayerId(7)), (u64::MAX, PlayerId(u32::MAX))]
        .into_iter()
        .zip([93, 95])
    {
        let InputResult::Processed(reports) = group
            .process_input(input(source, 91, HOST, 1, 0), &NoMapping, point(2, OUTPUT))
            .unwrap()
        else {
            panic!("assigned source")
        };
        assert_eq!(reports[0].player, player);
        assert_eq!(
            reports[0].report.audio_commands,
            [play(1, voice, OUTPUT, 1.0)]
        );
        assert_eq!(consumer.try_pop().unwrap(), play(1, voice, OUTPUT, 1.0));
    }
    assert!(group.configure_input_sounds(plans).is_err());
    let (mut started, _consumer, plans) = self::group();
    started
        .advance_to(point(1, HOST), &NoMapping, point(2, OUTPUT))
        .unwrap();
    assert!(started.configure_input_sounds(plans).is_err());
    let (mut failed, consumer, plans) = self::group();
    failed.configure_input_sounds(plans.clone()).unwrap();
    drop(consumer);
    assert!(
        failed
            .process_input(input(3, 91, HOST, 1, 0), &NoMapping, point(2, OUTPUT))
            .is_err()
    );
    assert!(failed.poisoned());
    assert!(failed.configure_input_sounds(plans).is_err());

    let prepared = prepared(SOLO, true);
    let timeline =
        crate::input_sounds::InputSoundPlan::prepare(&prepared.source, &prepared.sounds, &[], 4)
            .unwrap()
            .timeline();
    let judge = JudgeEngine::new(
        prepared.compiled.chart,
        prepared
            .source
            .rules_with_input_mode(BmsInputMode::ButtonOnly),
        profile(),
    )
    .unwrap();
    let (producer, mut consumer) = command_queue(8).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        bindings(DeviceSelector::Any, false),
        judge,
        producer,
        prepared.sounds,
        0,
    )
    .unwrap();
    solo.set_processing_clock(RuntimeProcessingClock::Disabled);
    assert!(
        solo.configure_input_sounds(change_voice(&timeline, 2, 1))
            .is_err()
    );
    solo.configure_input_sounds(timeline.clone()).unwrap();
    assert_eq!(
        solo.process_input(input(9, 91, HOST, 1, 0), &NoMapping, point(2, OUTPUT))
            .unwrap()
            .audio_commands,
        [play(1, 2, OUTPUT, 0.5)]
    );
    assert_eq!(consumer.try_pop().unwrap(), play(1, 2, OUTPUT, 0.5));
    assert!(solo.configure_input_sounds(timeline).is_err());
}

#[test]
fn invalid_installations_fail_and_future_invisible_selection_delays_natural_completion_without_auto_play()
 {
    for value in [prepared(SOLO, false), {
        let mut invalid = prepared(SOLO, true);
        invalid.source.invisible_ticks_per_beat = 0;
        invalid
    }] {
        assert!(
            StepGameplay::new_section_with_input_mode(
                value,
                config(),
                bindings(DeviceSelector::Any, false),
                ts(0),
                None,
                BmsInputMode::ButtonOnly
            )
            .is_err()
        );
    }
    let future = prepared("#BPM 60\n#WAV01 one\n#00231:01", false);
    assert!(future.compiled.chart.objects().is_empty());
    assert!(future.bgm_commands.is_empty() && future.sounds.is_empty());
    assert_eq!(
        future.source.compile_invisible().unwrap()[0].at,
        ts(8_000_000_000)
    );
    let mut completion =
        SongCompletion::prepare(&future, 0, 100_000_000, 0, ClockDomainId(2)).unwrap();
    assert_eq!(
        completion.calibration_seconds(),
        9,
        "8s selection plus 0.2s real PCM"
    );
    let mut config = config();
    config.preroll = Duration::ZERO;
    let (mut owner, bank) = StepGameplay::new_section_with_input_mode(
        future,
        config,
        bindings(DeviceSelector::Any, false),
        ts(0),
        None,
        BmsInputMode::ButtonOrContact,
    )
    .unwrap();
    owner.activate(config.host_origin).unwrap();
    let (producer, mut mixer) = mixer(bank, None);
    let mut output = [0.0; 1];
    let advance = owner
        .advance_to(
            point(1, HOST + 8_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 8_000_000_000),
        )
        .unwrap();
    assert!(advance.judge_events.is_empty() && advance.audio_commands.is_empty());
    let first = mixer.render(&mut output).unwrap();
    assert!(
        !completion
            .observe(
                owner.judge(),
                ts(8_000_000_000),
                BgmFeedReport::default(),
                Some(first),
                Some(point(2, OUTPUT + 100_000_000))
            )
            .unwrap()
    );
    let advance = owner
        .advance_to(
            point(1, HOST + 8_000_000_001),
            &NoMapping,
            point(2, OUTPUT + 8_000_000_001),
        )
        .unwrap();
    assert!(advance.judge_events.is_empty() && advance.audio_commands.is_empty());
    assert!(owner.take_commands(32).unwrap().is_none());
    let barrier = mixer.render(&mut output).unwrap();
    assert!(
        !completion
            .observe(
                owner.judge(),
                ts(8_000_000_001),
                BgmFeedReport::default(),
                Some(barrier),
                Some(point(2, OUTPUT + 200_000_000))
            )
            .unwrap(),
        "pre-selection render must not establish the post-judging drain barrier"
    );
    let idle = mixer.render(&mut output).unwrap();
    assert!(
        !completion
            .observe(
                owner.judge(),
                ts(8_000_000_001),
                BgmFeedReport::default(),
                Some(idle),
                None
            )
            .unwrap(),
        "software idle still needs actual presentation evidence"
    );
    assert_eq!(output, [0.0]);
    // No input occurred and no producer command was admitted; future selection
    // alone must never synthesize a note or Play. The producer remains healthy.
    assert_eq!(producer.counters().accepted, 0);
    let mut missing = prepared("#BPM 60\n#WAV02 two\n#00231:02", false);
    assert!(SongCompletion::prepare(&missing, 0, 0, 0, ClockDomainId(2)).is_err());
    missing.source.invisible.clear();
    assert!(
        prepare_local_input_sounds(&missing, &[], &[])
            .unwrap()
            .is_empty()
    );
}
