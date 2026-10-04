//! Deferred typed native composition; source-file admission remains guarded.
//! Runtime, cohort, queue and software Mixer are real. No native device is opened.
use crate::{
    ChannelPolicy, PreparedBms,
    competition_live::CompetitionOptions,
    local_players::PlayerId,
    local_runtime::InputResult,
    native_audio::{NativeAudioConfig, prepare_audio, prepare_input_sounds},
    native_chart::{NativeChartConfig, prepare_with},
    native_cohort_setup::{
        CohortPreparation, activate_cohort, activate_cohort_with_input_sounds, prepare_cohort,
    },
    native_judge::NativeJudgeConfig,
    section_start::prepare_at,
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId,
        VoiceId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeStage},
    runtime::{Runtime, RuntimeProcessingClock, SoundBinding, input_sound::InputSoundTimeline},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::parse;
use std::{collections::BTreeMap, path::Path};

const SOLO: &str = "#BPM 60\n#VOLWAV 50\n#WAV01 one\n#WAV02 two\n#00031:01020000\n#00011:00000100";
const LOCAL: &str = "#BPM 60\n#VOLWAV 25\n#WAV01 one\n#WAV02 two\n#00031:01020000\n#00032:0201\n#00011:00000100\n#00101:01";
const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 1_000_000_000;
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn limits() -> PcmLimits {
    PcmLimits::new(64, 128, 4).unwrap()
}
fn prepared(text: &str) -> PreparedBms {
    let source = parse(text, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let gain = source.wav_gain().unwrap();
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: JudgeStage::Instant,
            sample: note.sample,
            voice: VoiceId(10),
            gain,
        })
        .collect();
    let bgm_commands = compiled
        .bgm
        .iter()
        .map(|cue| AudioCommand::Play {
            sample: cue.sample,
            voice: VoiceId(40),
            at: cue.at,
            gain,
        })
        .collect();
    let format = AudioFormat::new(10, 1).unwrap();
    let mut bank = SampleBank::new(format, limits()).unwrap();
    for (id, samples) in [(1, vec![1.0, -1.0]), (2, vec![0.25, 0.75])] {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, samples, limits()).unwrap(),
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
fn judge(prepared: &PreparedBms, offset: i64) -> JudgeEngine {
    let config = NativeJudgeConfig {
        early: 0,
        late: 0,
        offset,
        preroll: 0,
        output: ClockDomainId(2),
        end: None,
    };
    JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        config.profile().unwrap(),
    )
    .unwrap()
}
fn bindings(fanout: bool) -> BindingMap {
    BindingMap::from_bindings([0x11, 0x12].into_iter().map(|lane| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(if lane == 0x11 || fanout { 91 } else { 92 }),
        game_control: GameControlId(lane),
    }))
    .unwrap()
}
fn input(
    device: u64,
    key: u16,
    host: i64,
    sequence: u64,
    state: ButtonState,
) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(device), point(1, host), sequence);
    meta.original_clock_point = Some(point(99, 9_007_199_254_740_993));
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(key),
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
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn audio_config(start: i64, preroll: i64) -> NativeAudioConfig {
    NativeAudioConfig {
        output_origin: point(2, OUTPUT),
        start: ts(start),
        preroll: Duration::from_nanos(preroll),
        lookahead: Duration::from_nanos(1_000_000_000),
        voices: 32,
        max_render_frames: 32,
        playback_end_frame: None,
        gated_start: false,
    }
}

#[test]
fn native_solo_plan_drives_actual_runtime_and_mixer_with_original_practice_selection_and_hit_priority()
 {
    let (prepared, section) = prepare_at(prepared(SOLO), ts(500_000_000), limits()).unwrap();
    assert_eq!(section.excluded_objects, 0);
    let pcm = prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr();
    let timeline = prepare_input_sounds(&prepared).unwrap().unwrap();
    assert_eq!(
        timeline
            .markers()
            .iter()
            .map(|m| (m.control.0, m.at.as_nanos(), m.sample.0, m.voice.0, m.gain))
            .collect::<Vec<_>>(),
        vec![(0x11, 0, 1, 11, 0.5), (0x11, 1_000_000_000, 2, 11, 0.5)]
    );
    assert_eq!(
        prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        pcm
    );
    let judge = judge(&prepared, 100_000_000);
    let mut audio = prepare_audio(
        prepared.bank,
        prepared.bgm_commands,
        audio_config(500_000_000, 100_000_000),
    )
    .unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(400_000_000), Rate::NORMAL),
        bindings(false),
        judge,
        audio.producer,
        prepared.sounds,
        0,
    )
    .unwrap();
    runtime.configure_input_sounds(timeline).unwrap();
    runtime.set_song_end(ts(2_200_000_000)).unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    assert!(
        runtime
            .advance_to(point(1, HOST), &NoMapping, point(2, OUTPUT))
            .unwrap()
            .audio_commands
            .is_empty()
    );
    let mut commands = Vec::new();
    let mut hits = 0;
    for (sequence, (song, state)) in [
        (500_000_000, ButtonState::Down),
        (600_000_000, ButtonState::Down),
        (700_000_000, ButtonState::Up),
        (900_000_000, ButtonState::Down),
        (950_000_000, ButtonState::Up),
        (1_000_000_000, ButtonState::Down),
        (1_050_000_000, ButtonState::Repeat),
        (1_100_000_000, ButtonState::Up),
        (1_900_000_000, ButtonState::Down),
        (2_000_000_000, ButtonState::Up),
        (2_100_000_000, ButtonState::Down),
        (2_200_000_000, ButtonState::Down),
    ]
    .into_iter()
    .enumerate()
    {
        let original = input(
            u64::MAX,
            91,
            HOST + song - 400_000_000,
            sequence as u64,
            state,
        );
        let report = runtime
            .process_input(
                original.clone(),
                &NoMapping,
                point(2, OUTPUT + song - 400_000_000),
            )
            .unwrap();
        assert!(report.judge_error.is_none() && report.audio_failures.is_empty());
        if song == 2_200_000_000 {
            assert!(report.bound_inputs.is_empty() && report.audio_commands.is_empty());
        } else {
            assert_eq!(report.bound_inputs[0].physical, original);
            assert_eq!(report.song_time, ts(song));
        }
        hits += report.judge_events.len();
        commands.extend(report.audio_commands);
    }
    assert_eq!(hits, 1);
    assert_eq!(
        commands,
        vec![
            play(1, 11, 1_100_000_000, 0.5),
            play(1, 11, 1_500_000_000, 0.5),
            play(2, 11, 1_600_000_000, 0.5),
            play(1, 10, 2_500_000_000, 0.5),
            play(2, 11, 2_700_000_000, 0.5)
        ]
    );
    let mut output = [9.0; 30];
    let report = audio.mixer.render(&mut output).unwrap();
    let mut expected = [0.0; 30];
    expected[1] = 0.5;
    expected[2] = -0.5;
    expected[5] = 0.5;
    expected[6] = 0.125;
    expected[7] = 0.375;
    expected[15] = 0.5;
    expected[16] = -0.5;
    expected[17] = 0.125;
    expected[18] = 0.375;
    assert_eq!(output, expected);
    assert_eq!(report.counters.commands_consumed, 5);
    assert!(
        runtime
            .advance_to(
                point(1, HOST + 3_000_000_000),
                &NoMapping,
                point(2, OUTPUT + 3_000_000_000)
            )
            .unwrap()
            .audio_commands
            .is_empty()
    );
}

#[test]
fn native_plan_refuses_bad_resources_and_exhaustion_while_queue_failure_retains_the_actual_prefix()
{
    let mut missing = prepared(SOLO);
    missing.bank = SampleBank::new(AudioFormat::new(10, 1).unwrap(), limits()).unwrap();
    assert!(prepare_input_sounds(&missing).is_err());
    for mutate in [
        (|p: &mut PreparedBms| p.source.invisible_ticks_per_beat = 0) as fn(&mut PreparedBms),
        |p| {
            p.source.metadata.insert("VOLWAV".into(), "bad".into());
        },
        |p| p.sounds[0].gain = f32::NAN,
        |p| p.sounds[0].voice = VoiceId(u64::MAX),
        |p| {
            p.bgm_commands.push(AudioCommand::Stop {
                voice: VoiceId(1),
                at: ts(0),
            })
        },
        |p| p.bgm_commands.push(play(1, u64::MAX, 0, 1.0)),
    ] {
        let mut bad = prepared(SOLO);
        mutate(&mut bad);
        let before = bad.source.clone();
        let bytes = bad.bank.total_bytes();
        assert!(prepare_input_sounds(&bad).is_err());
        assert_eq!(bad.source, before);
        assert_eq!(bad.bank.total_bytes(), bytes);
    }
    let mut legacy = prepared(SOLO);
    legacy.source.invisible.clear();
    legacy.source.invisible_ticks_per_beat = 0;
    legacy.sounds[0].voice = VoiceId(u64::MAX);
    assert!(
        prepare_input_sounds(&legacy).unwrap().is_none(),
        "unused input-sound fields do not change legacy setup"
    );

    let prepared = prepared("#BPM 60\n#WAV01 one\n#WAV02 two\n#00031:01\n#00032:02");
    let timeline = prepare_input_sounds(&prepared).unwrap().unwrap();
    let (producer, mut consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        bindings(true),
        judge(&prepared, 0),
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.configure_input_sounds(timeline).unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let first = runtime
        .process_input(
            input(3, 91, HOST, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap();
    assert_eq!(first.bound_inputs.len(), 2);
    assert_eq!(first.audio_commands, vec![play(1, 1, OUTPUT, 1.0)]);
    assert_eq!(first.audio_failures.len(), 1);
    assert_eq!(first.audio_failures[0].command, play(2, 2, OUTPUT, 1.0));
    assert_eq!(first.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(consumer.try_pop().unwrap(), play(1, 1, OUTPUT, 1.0));
    let duplicate = runtime
        .process_input(
            input(3, 91, HOST + 1, 2, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + 1),
        )
        .unwrap();
    assert!(duplicate.audio_commands.is_empty() && duplicate.audio_failures.is_empty());
    assert!(
        consumer.try_pop().is_err(),
        "a failed sound cannot become a synthetic retry on duplicate Down"
    );
}

fn chart_config<'a>(bindings: &'a BTreeMap<u8, u16>, start: i64) -> NativeChartConfig<'a> {
    NativeChartConfig {
        path: Path::new("not-read/native.bms"),
        format: AudioFormat::new(10, 1).unwrap(),
        limits: limits(),
        channels: ChannelPolicy::Exact,
        chart_seed: u64::MAX,
        start: ts(start),
        bindings,
    }
}
#[test]
fn native_chart_load_seam_preserves_request_and_requires_original_invisible_lanes_in_empty_practice()
 {
    let text = "#BPM 60\n#WAV01 one\n#WAV02 two\n#00011:01\n#00032:0102";
    let original = prepared(text);
    let invisible = original.source.invisible.clone();
    let pcm = original.bank.get(SampleId(1)).unwrap().samples().as_ptr();
    let bindings = BTreeMap::from([(0x12, 92_u16)]);
    let (selected, section) = prepare_with(chart_config(&bindings, 3_000_000_000), |request| {
        assert_eq!(request.path, Path::new("not-read/native.bms"));
        assert_eq!(request.start, ts(3_000_000_000));
        assert_eq!(request.chart_seed, u64::MAX);
        assert_eq!(request.channels, ChannelPolicy::Exact);
        assert_eq!(request.format, AudioFormat::new(10, 1).unwrap());
        assert_eq!(request.limits.max_total_bytes(), 128);
        Ok(original)
    })
    .unwrap();
    assert_eq!(section.excluded_objects, 1);
    assert!(selected.source.notes.is_empty() && selected.compiled.chart.objects().is_empty());
    assert_eq!(selected.source.invisible, invisible);
    assert_eq!(
        selected.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
        pcm
    );
    let timeline = prepare_input_sounds(&selected).unwrap().unwrap();
    assert_eq!(
        timeline.command_for(GameControlId(0x12), ts(3_000_000_000), ts(7)),
        Some(play(2, 1, 7, 1.0))
    );
    let visible_only = BTreeMap::from([(0x11, 91_u16)]);
    for start in [0, 3_000_000_000] {
        let error = prepare_with(chart_config(&visible_only, start), |_| Ok(prepared(text)))
            .err()
            .unwrap();
        assert!(error.to_string().contains("12"));
    }
    assert!(
        prepare_with(chart_config(&bindings, -1), |_| panic!(
            "negative start must precede loading"
        ))
        .is_err()
    );
    let ordinary = text.replace("\n#00032:0102", "");
    let (legacy, _) =
        prepare_with(chart_config(&visible_only, 0), |_| Ok(prepared(&ordinary))).unwrap();
    assert!(prepare_input_sounds(&legacy).unwrap().is_none());
    assert!(
        prepare_with(chart_config(&bindings, 3_000_000_000), |_| Ok(prepared(
            &ordinary
        )))
        .is_err()
    );
}

fn cohort_config<'a>(keys: &'a BTreeMap<u8, u16>) -> CohortPreparation<'a> {
    CohortPreparation {
        host: ClockDomainId(1),
        output: ClockDomainId(2),
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        start: ts(0),
        end: Some(ts(200_000_000)),
        chart_seed: 0,
        bindings: keys,
        record_replay: None,
        replay_max_bytes: 0,
        replay_max_records: 0,
    }
}
#[test]
fn actual_native_cohort_activation_installs_disjoint_shared_bank_sounds_and_preserves_legacy_and_end_fences()
 {
    let keys = BTreeMap::from([(0x11, 91_u16), (0x12, 92_u16)]);
    for count in [2_usize, 3, 4] {
        let prepared = prepared(LOCAL);
        let pcm = prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr();
        let assignments: Vec<_> = (0..count)
            .map(|n| (PlayerId(u32::MAX - n as u32), DeviceId(u64::MAX - n as u64)))
            .collect();
        let cohort = prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &cohort_config(&keys),
        )
        .unwrap();
        assert_eq!(
            prepared.bank.get(SampleId(1)).unwrap().samples().as_ptr(),
            pcm
        );
        assert_eq!(cohort.reserved, vec![VoiceId(40)]);
        assert_eq!(cohort.input_sounds.len(), count);
        for (index, ((player, timeline), member)) in
            cohort.input_sounds.iter().zip(&cohort.configs).enumerate()
        {
            assert_eq!(*player, assignments[index].0);
            assert_eq!(member.sounds[0].voice, VoiceId(41 + index as u64));
            let first = 41 + count as u64 + 2 * index as u64;
            assert_eq!(
                timeline.command_for(GameControlId(0x11), ts(0), ts(OUTPUT)),
                Some(play(1, first, OUTPUT, 0.25))
            );
            assert_eq!(
                timeline.command_for(GameControlId(0x12), ts(0), ts(OUTPUT)),
                Some(play(2, first + 1, OUTPUT, 0.25))
            );
        }
        let mut config = audio_config(0, 0);
        config.playback_end_frame = Some(2);
        let mut audio = prepare_audio(prepared.bank, prepared.bgm_commands, config).unwrap();
        let (mut group, mut merger) = activate_cohort_with_input_sounds(
            cohort.configs,
            &cohort.reserved,
            point(1, HOST),
            ClockDomainId(2),
            Transport::new(ts(HOST), ts(0), Rate::NORMAL),
            audio.producer,
            Some(ts(200_000_000)),
            cohort.input_sounds,
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        assert!(matches!(
            group
                .process_input(
                    input(3, 91, HOST, 1, ButtonState::Down),
                    &NoMapping,
                    point(2, OUTPUT)
                )
                .unwrap(),
            InputResult::Ignored {
                device: DeviceId(3)
            }
        ));
        for &(_, device) in &assignments {
            merger
                .admit(
                    input(device.0, 91, HOST, u64::MAX, ButtonState::Down),
                    point(1, HOST),
                )
                .unwrap();
        }
        let mut reports = Vec::new();
        while let Some(event) = merger.pop_ready(point(1, HOST)).unwrap() {
            let original = event.clone();
            let InputResult::Processed(mut current) = group
                .process_input(event, &NoMapping, point(2, OUTPUT))
                .unwrap()
            else {
                panic!("assigned device")
            };
            assert_eq!(current.len(), 1);
            assert_eq!(current[0].report.bound_inputs[0].physical, original);
            assert!(current[0].report.judge_events.is_empty());
            reports.push(current.remove(0));
        }
        for report in &reports {
            let index = assignments
                .iter()
                .position(|&(player, _)| player == report.player)
                .unwrap();
            assert_eq!(
                report.report.audio_commands,
                vec![play(1, 41 + count as u64 + 2 * index as u64, OUTPUT, 0.25)]
            );
        }
        assert_eq!(reports.len(), count);
        assert!(
            group
                .advance_to(point(1, HOST), &NoMapping, point(2, OUTPUT))
                .unwrap()
                .iter()
                .all(|r| r.report.audio_commands.is_empty())
        );
        merger.commit(point(1, HOST)).unwrap();
        let mut output = [9.0; 4];
        let report = audio.mixer.render(&mut output).unwrap();
        assert_eq!(
            output,
            [count as f32 * 0.25, -(count as f32) * 0.25, 0.0, 0.0]
        );
        assert_eq!(report.playback_frames, 2);
        assert_eq!(report.counters.commands_consumed, count as u64);
        for &(player, device) in &assignments {
            let InputResult::Processed(rows) = group
                .process_input(
                    input(
                        device.0,
                        92,
                        HOST + 200_000_000,
                        u64::MAX,
                        ButtonState::Down,
                    ),
                    &NoMapping,
                    point(2, OUTPUT + 200_000_000),
                )
                .unwrap()
            else {
                panic!("assigned endpoint")
            };
            assert_eq!(rows[0].player, player);
            assert!(
                rows[0].report.bound_inputs.is_empty() && rows[0].report.audio_commands.is_empty()
            );
        }
    }
    let assignments = [(PlayerId(1), DeviceId(3)), (PlayerId(2), DeviceId(4))];
    for wrong_order in [false, true] {
        let prepared = prepared(LOCAL);
        let mut cohort = prepare_cohort(
            &prepared,
            &assignments,
            &CompetitionOptions::default(),
            &cohort_config(&keys),
        )
        .unwrap();
        if wrong_order {
            cohort.input_sounds.swap(0, 1);
        } else {
            let mut markers = cohort.input_sounds[0].1.markers().to_vec();
            markers[0].voice = cohort.configs[0].sounds[0].voice;
            cohort.input_sounds[0].1 = InputSoundTimeline::new(markers, 16).unwrap();
        }
        let (producer, mut consumer) = command_queue(4).unwrap();
        assert!(
            activate_cohort_with_input_sounds(
                cohort.configs,
                &cohort.reserved,
                point(1, HOST),
                ClockDomainId(2),
                Transport::new(ts(HOST), ts(0), Rate::NORMAL),
                producer,
                None,
                cohort.input_sounds
            )
            .is_err()
        );
        assert!(consumer.try_pop().is_err());
    }
    let mut legacy = prepared(LOCAL);
    legacy.source.invisible.clear();
    let cohort = prepare_cohort(
        &legacy,
        &assignments,
        &CompetitionOptions::default(),
        &cohort_config(&keys),
    )
    .unwrap();
    assert!(cohort.input_sounds.is_empty());
    let (producer, mut consumer) = command_queue(4).unwrap();
    let (mut group, _) = activate_cohort(
        cohort.configs,
        &cohort.reserved,
        point(1, HOST),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        producer,
        None,
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    let InputResult::Processed(rows) = group
        .process_input(
            input(3, 91, HOST, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap()
    else {
        panic!("legacy assigned device")
    };
    assert!(rows[0].report.audio_commands.is_empty());
    assert!(consumer.try_pop().is_err());
    group
        .process_input(
            input(3, 91, HOST + 1_000_000_000, 2, ButtonState::Up),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    let InputResult::Processed(rows) = group
        .process_input(
            input(3, 91, HOST + 2_000_000_000, 3, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap()
    else {
        panic!("legacy ordinary hit")
    };
    assert_eq!(rows[0].report.judge_events.len(), 1);
    assert_eq!(
        rows[0].report.audio_commands,
        vec![play(1, 41, OUTPUT + 2_000_000_000, 0.25)]
    );
    assert_eq!(
        consumer.try_pop().unwrap(),
        play(1, 41, OUTPUT + 2_000_000_000, 0.25)
    );
    let mut missing = prepared(LOCAL);
    missing.bank = SampleBank::new(AudioFormat::new(10, 1).unwrap(), limits()).unwrap();
    assert!(
        prepare_cohort(
            &missing,
            &assignments,
            &CompetitionOptions::default(),
            &cohort_config(&keys)
        )
        .is_err()
    );
}
