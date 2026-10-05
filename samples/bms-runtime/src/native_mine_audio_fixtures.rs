//! Deferred native WAV00 composition using real queues, judges and software PCM.
//! Typed setup does not remove the mine-file guard or open a native device.
use crate::{
    PreparedBms,
    competition::OpponentKind,
    competition_live::CompetitionOptions,
    local_players::PlayerId,
    local_runtime::{InputResult, SoloRuntime},
    native_audio::{NativeAudioConfig, prepare_audio, prepare_input_sounds, prepare_mine_sounds},
    native_cohort_setup::{
        CohortPreparation, PreparedCohort, activate_cohort, activate_cohort_with_input_sounds,
        activate_cohort_with_sounds, prepare_cohort,
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
    judge::{HazardId, HazardOutcome, JudgeStage},
    runtime::{
        RuntimeProcessingClock, SoundBinding,
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::parse;
use std::{collections::BTreeMap, path::PathBuf};

const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 1_000_000_000;
const MIXED: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#WAV02 press\n#00011:01\n#00031:02\n#00032:02\n#000D1:1E01ZZ01\n#000D2:001E0000\n#00101:01";
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
    PcmLimits::new(64, 256, 4).unwrap()
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn hazard(id: u64, voice: u64, gain: f32) -> HazardSoundBinding {
    HazardSoundBinding {
        hazard: HazardId(id),
        sample: SampleId(0),
        voice: VoiceId(voice),
        gain,
    }
}
fn prepared(text: &str, with_zero: bool) -> PreparedBms {
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
        .map(|cue| play(cue.sample.0, 40, cue.at.as_nanos(), gain))
        .collect();
    let format = AudioFormat::new(10, 1).unwrap();
    let mut bank = SampleBank::new(format, limits()).unwrap();
    for (id, pcm) in [(1, vec![1.0, -1.0]), (2, vec![0.25, 0.75])] {
        bank.insert(SampleId(id), PcmSample::new(format, pcm, limits()).unwrap())
            .unwrap();
    }
    if with_zero {
        bank.insert(
            SampleId(0),
            PcmSample::new(format, vec![0.5, -0.5], limits()).unwrap(),
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
fn judge_config(offset: i64) -> NativeJudgeConfig {
    NativeJudgeConfig {
        early: 0,
        late: 0,
        offset,
        preroll: 0,
        output: ClockDomainId(2),
        end: None,
    }
}
fn audio_config(start: i64, end_frame: Option<u64>) -> NativeAudioConfig {
    NativeAudioConfig {
        output_origin: point(2, OUTPUT),
        start: ts(start),
        preroll: Duration::ZERO,
        lookahead: Duration::from_nanos(1_000_000_000),
        voices: 32,
        max_render_frames: 32,
        playback_end_frame: end_frame,
        gated_start: false,
    }
}
fn keys(fanout: bool) -> BindingMap {
    BindingMap::from_bindings([0x11, 0x12].map(|lane| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(if fanout || lane == 0x11 { 91u16 } else { 92u16 }),
        game_control: GameControlId(lane),
    }))
    .unwrap()
}
fn input(
    device: u64,
    key: u16,
    song_from_origin: i64,
    sequence: u64,
    state: ButtonState,
) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(
        DeviceId(device),
        point(1, HOST + song_from_origin),
        sequence,
    );
    meta.original_clock_point = Some(point(77, 9_007_199_254_740_993));
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
fn assignments(count: usize) -> Vec<(PlayerId, DeviceId)> {
    [
        (PlayerId(u32::MAX), DeviceId(u64::MAX)),
        (PlayerId(7), DeviceId(3)),
        (PlayerId(1001), DeviceId(9_007_199_254_740_993)),
        (PlayerId(42), DeviceId(17)),
    ][..count]
        .to_vec()
}
fn cohort_config(bindings: &BTreeMap<u8, u16>) -> CohortPreparation<'_> {
    CohortPreparation {
        host: ClockDomainId(1),
        output: ClockDomainId(2),
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        start: ts(0),
        end: Some(ts(1_500_000_000)),
        chart_seed: 0,
        bindings,
        record_replay: None,
        replay_max_bytes: 0,
        replay_max_records: 0,
    }
}
fn cohort(data: &PreparedBms, count: usize) -> PreparedCohort {
    let keys = BTreeMap::from([(0x11, 91u16), (0x12, 92u16)]);
    prepare_cohort(
        data,
        &assignments(count),
        &CompetitionOptions::default(),
        &cohort_config(&keys),
    )
    .unwrap()
}

#[test]
fn native_plan_uses_actual_press_reservations_and_refuses_missing_pcm_or_invalid_audible_setup() {
    let data = prepared(MIXED, true);
    let source = data.source.clone();
    let pointer = data.bank.get(SampleId(0)).unwrap().samples().as_ptr();
    let presses = prepare_input_sounds(&data).unwrap().unwrap();
    let mines = prepare_mine_sounds(&data, Some(&presses)).unwrap().unwrap();
    assert_eq!(
        mines.bindings(),
        [
            hazard(0, 43, 0.5),
            hazard(1, 43, 0.5),
            hazard(3, 43, 0.5),
            hazard(4, 44, 0.5)
        ]
    );
    assert_eq!(data.source, source);
    assert_eq!(
        data.bank.get(SampleId(0)).unwrap().samples().as_ptr(),
        pointer
    );
    assert!(prepare_mine_sounds(&prepared(MIXED, false), Some(&presses)).is_err());
    for mutate in [
        (|p: &mut PreparedBms| p.source.mine_ticks_per_beat = 0) as fn(&mut PreparedBms),
        |p| {
            p.source.metadata.insert("VOLWAV".into(), "bad".into());
        },
        |p| p.sounds[0].gain = f32::NAN,
        |p| p.sounds[0].voice = VoiceId(u64::MAX),
        |p| p.bgm_commands.push(play(1, u64::MAX, 0, 1.0)),
        |p| {
            p.bgm_commands.push(AudioCommand::Stop {
                voice: VoiceId(1),
                at: ts(0),
            })
        },
    ] {
        let mut bad = prepared(MIXED, true);
        mutate(&mut bad);
        let before = bad.source.clone();
        let bytes = bad.bank.total_bytes();
        assert!(prepare_mine_sounds(&bad, Some(&presses)).is_err());
        assert_eq!(bad.source, before);
        assert_eq!(bad.bank.total_bytes(), bytes);
    }
    let mut max_press = presses.markers().to_vec();
    max_press[0].voice = VoiceId(u64::MAX);
    let max_press =
        beatkernel::runtime::input_sound::InputSoundTimeline::new(max_press, 2).unwrap();
    assert!(prepare_mine_sounds(&data, Some(&max_press)).is_err());
    for text in ["#BPM 60\n#000D1:1E", "#BPM 60\n#WAV00 unused\n#000D1:ZZ"] {
        let mut silent = prepared(text, false);
        silent
            .source
            .metadata
            .insert("VOLWAV".into(), "unused-invalid-gain".into());
        assert!(
            prepare_mine_sounds(&silent, Some(&max_press))
                .unwrap()
                .is_none()
        );
    }
    let mut legacy = prepared(MIXED, false);
    legacy.source.mines.clear();
    legacy.source.mine_ticks_per_beat = 0;
    legacy.sounds[0].gain = f32::NAN;
    assert!(
        prepare_mine_sounds(&legacy, Some(&max_press))
            .unwrap()
            .is_none()
    );
}

#[test]
fn actual_native_solo_orders_note_press_and_mine_pcm_and_preserves_practice_offset_and_end_fences()
{
    let data = prepared(MIXED, true);
    let presses = prepare_input_sounds(&data).unwrap().unwrap();
    let mines = prepare_mine_sounds(&data, Some(&presses)).unwrap().unwrap();
    let judge = judge_config(0)
        .judge(&data.source, data.compiled.chart.clone())
        .unwrap();
    let mut audio = prepare_audio(data.bank, data.bgm_commands, audio_config(0, Some(25))).unwrap();
    let mut runtime = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        keys(true),
        judge,
        audio.producer,
        data.sounds,
        0,
    )
    .unwrap();
    runtime.configure_input_sounds(presses).unwrap();
    runtime.configure_hazard_sounds(mines).unwrap();
    runtime.set_song_end(ts(2_500_000_000)).unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let original = input(u64::MAX, 91, 0, u64::MAX, ButtonState::Down);
    let first = runtime
        .process_input(original.clone(), &NoMapping, point(2, OUTPUT))
        .unwrap();
    assert_eq!(first.bound_inputs.len(), 2);
    assert!(
        first
            .bound_inputs
            .iter()
            .all(|bound| bound.physical == original)
    );
    assert_eq!(
        first.audio_commands,
        [
            play(1, 10, OUTPUT, 0.5),
            play(2, 42, OUTPUT, 0.5),
            play(0, 43, OUTPUT, 0.5)
        ]
    );
    assert_eq!(first.hazard_events[0].input, Some(*original.meta()));
    let held = runtime
        .advance_to(
            point(1, HOST + 1_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 1_000_000_000),
        )
        .unwrap();
    assert_eq!(
        held.hazard_events
            .iter()
            .map(|event| (event.id.0, event.outcome, event.input))
            .collect::<Vec<_>>(),
        [
            (1, HazardOutcome::Triggered, None),
            (4, HazardOutcome::Triggered, None)
        ]
    );
    assert_eq!(
        held.audio_commands,
        [
            play(0, 43, OUTPUT + 1_000_000_000, 0.5),
            play(0, 44, OUTPUT + 1_000_000_000, 0.5)
        ]
    );
    let fatal = runtime
        .advance_to(
            point(1, HOST + 2_000_000_000),
            &NoMapping,
            point(2, OUTPUT + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(fatal.hazard_events[0].value, 1295);
    assert_eq!(fatal.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert!(fatal.audio_commands.is_empty());
    for at in [2_500_000_000, 3_000_000_000] {
        let end = runtime
            .process_input(
                input(u64::MAX, 91, at, u64::MAX, ButtonState::Down),
                &NoMapping,
                point(2, OUTPUT + at),
            )
            .unwrap();
        assert!(end.song_end_reached && end.bound_inputs.is_empty());
        assert!(end.hazard_events.is_empty() && end.audio_commands.is_empty());
    }
    let mut pcm = [9.0; 30];
    let rendered = audio.mixer.render(&mut pcm).unwrap();
    let mut expected = [0.0; 30];
    expected[0] = 0.875;
    expected[1] = -0.375;
    expected[10] = 0.5;
    expected[11] = -0.5;
    assert_eq!(pcm, expected);
    assert_eq!(rendered.counters.commands_consumed, 5);
    assert_eq!(rendered.playback_frames, 25);

    for offset in [-100_000_000, 100_000_000] {
        for held in [false, true] {
            let data = prepared("#BPM 60\n#WAV00 blast\n#000D1:001E", true);
            let original_mines = data.source.compile_mines().unwrap();
            let (data, section) = prepare_at(data, ts(1_000_000_000), limits()).unwrap();
            assert_eq!(section.excluded_objects, 0);
            assert_eq!(data.source.compile_mines().unwrap(), original_mines);
            let mines = prepare_mine_sounds(&data, None).unwrap().unwrap();
            let judge = judge_config(offset)
                .judge(&data.source, data.compiled.chart.clone())
                .unwrap();
            let mut audio = prepare_audio(
                data.bank,
                data.bgm_commands,
                audio_config(1_000_000_000, None),
            )
            .unwrap();
            let mut runtime = SoloRuntime::new(
                ClockDomainId(1),
                ClockDomainId(2),
                Transport::new(ts(HOST), ts(1_000_000_000), Rate::NORMAL),
                keys(false),
                judge,
                audio.producer,
                data.sounds,
                0,
            )
            .unwrap();
            runtime.configure_hazard_sounds(mines).unwrap();
            runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
            if held {
                let report = runtime
                    .process_input(
                        input(3, 91, 0, 1, ButtonState::Down),
                        &NoMapping,
                        point(2, OUTPUT),
                    )
                    .unwrap();
                assert!(report.audio_commands.is_empty());
            }
            let boundary = 2_000_000_000 - offset;
            let report = runtime
                .advance_to(
                    point(1, HOST + boundary - 1_000_000_000),
                    &NoMapping,
                    point(2, OUTPUT + 1_000_000_000),
                )
                .unwrap();
            assert_eq!(report.song_time, ts(boundary));
            assert_eq!(report.hazard_events[0].at, ts(2_000_000_000));
            assert_eq!(
                report.hazard_events[0].outcome,
                if held {
                    HazardOutcome::Triggered
                } else {
                    HazardOutcome::Avoided
                }
            );
            assert_eq!(
                report.audio_commands,
                if held {
                    vec![play(0, 1, OUTPUT + 1_000_000_000, 1.0)]
                } else {
                    vec![]
                }
            );
            let mut pcm = [0.0; 12];
            audio.mixer.render(&mut pcm).unwrap();
            let mut expected = [0.0; 12];
            if held {
                expected[10] = 0.5;
                expected[11] = -0.5;
            }
            assert_eq!(pcm, expected);
        }
    }
}

#[test]
fn real_two_three_four_member_preparation_and_merger_share_pcm_but_isolate_all_sound_voices() {
    for count in [2usize, 3, 4] {
        let data = prepared(MIXED, true);
        let pointer = data.bank.get(SampleId(0)).unwrap().samples().as_ptr();
        let cohort = cohort(&data, count);
        assert_eq!(
            data.bank.get(SampleId(0)).unwrap().samples().as_ptr(),
            pointer
        );
        assert_eq!(cohort.reserved, [VoiceId(40)]);
        assert_eq!(cohort.hazard_sounds.len(), count);
        for (index, ((player, timeline), member)) in
            cohort.hazard_sounds.iter().zip(&cohort.configs).enumerate()
        {
            assert_eq!(*player, assignments(count)[index].0);
            assert_eq!(member.sounds[0].voice, VoiceId(41 + index as u64));
            let first = 41 + 3 * count as u64 + 2 * index as u64;
            assert_eq!(
                timeline.bindings(),
                [
                    hazard(0, first, 0.5),
                    hazard(1, first, 0.5),
                    hazard(3, first, 0.5),
                    hazard(4, first + 1, 0.5)
                ]
            );
        }
        let mut audio =
            prepare_audio(data.bank, data.bgm_commands, audio_config(0, Some(15))).unwrap();
        let (mut group, mut merger) = activate_cohort_with_sounds(
            cohort.configs,
            &cohort.reserved,
            point(1, HOST),
            ClockDomainId(2),
            Transport::new(ts(HOST), ts(0), Rate::NORMAL),
            audio.producer,
            Some(ts(1_500_000_000)),
            cohort.input_sounds,
            cohort.hazard_sounds,
        )
        .unwrap();
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        assert!(matches!(
            group
                .process_input(
                    input(5, 91, 0, 1, ButtonState::Down),
                    &NoMapping,
                    point(2, OUTPUT)
                )
                .unwrap(),
            InputResult::Ignored {
                device: DeviceId(5)
            }
        ));
        for (_, device) in assignments(count) {
            merger
                .admit(
                    input(device.0, 91, 0, u64::MAX - 1, ButtonState::Down),
                    point(1, HOST),
                )
                .unwrap();
            merger
                .admit(
                    input(device.0, 92, 0, u64::MAX, ButtonState::Down),
                    point(1, HOST),
                )
                .unwrap();
        }
        let mut delivered = Vec::new();
        while let Some(event) = merger.pop_ready(point(1, HOST)).unwrap() {
            let original = event.clone();
            let index = assignments(count)
                .iter()
                .position(|(_, device)| *device == event.meta().source)
                .unwrap();
            let InputResult::Processed(rows) = group
                .process_input(event, &NoMapping, point(2, OUTPUT))
                .unwrap()
            else {
                panic!("assigned native input")
            };
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].player, assignments(count)[index].0);
            assert_eq!(rows[0].report.bound_inputs[0].physical, original);
            let ordinary = 41 + index as u64;
            let press = 42 + count as u64 + 2 * index as u64;
            let mine = 41 + 3 * count as u64 + 2 * index as u64;
            let expected = if original.meta().sequence == u64::MAX - 1 {
                vec![play(1, ordinary, OUTPUT, 0.5), play(0, mine, OUTPUT, 0.5)]
            } else {
                vec![play(2, press, OUTPUT, 0.5)]
            };
            assert_eq!(rows[0].report.audio_commands, expected);
            delivered.push((original.meta().source, original.meta().sequence));
        }
        let mut expected_order = assignments(count)
            .into_iter()
            .map(|(_, device)| device)
            .collect::<Vec<_>>();
        expected_order.sort_by_key(|device| device.0);
        assert_eq!(
            delivered,
            expected_order
                .into_iter()
                .flat_map(|device| [(device, u64::MAX - 1), (device, u64::MAX)])
                .collect::<Vec<_>>()
        );
        merger.commit(point(1, HOST)).unwrap();
        let reports = group
            .advance_to(
                point(1, HOST + 1_000_000_000),
                &NoMapping,
                point(2, OUTPUT + 1_000_000_000),
            )
            .unwrap();
        for (index, row) in reports.iter().enumerate() {
            assert_eq!(row.player, assignments(count)[index].0);
            let first = 41 + 3 * count as u64 + 2 * index as u64;
            assert_eq!(
                row.report.audio_commands,
                [
                    play(0, first, OUTPUT + 1_000_000_000, 0.5),
                    play(0, first + 1, OUTPUT + 1_000_000_000, 0.5)
                ]
            );
        }
        let end = group
            .advance_to(
                point(1, HOST + 3_000_000_000),
                &NoMapping,
                point(2, OUTPUT + 3_000_000_000),
            )
            .unwrap();
        assert!(end.iter().all(|row| row.report.song_end_reached
            && row.report.hazard_events.is_empty()
            && row.report.audio_commands.is_empty()));
        for (player, _) in assignments(count) {
            assert_eq!(group.member_judge(player).unwrap().remaining_hazards(), 2);
        }
        let mut pcm = [9.0; 20];
        let rendered = audio.mixer.render(&mut pcm).unwrap();
        let mut expected = [0.0; 20];
        // The actual software mixer clamps the aggregate once per channel.
        expected[0] = 1.0;
        expected[1] = if count == 2 { -0.75 } else { -1.0 };
        expected[10] = 1.0;
        expected[11] = -1.0;
        assert_eq!(pcm, expected);
        assert_eq!(rendered.playback_frames, 15);
        assert_eq!(rendered.counters.commands_consumed, 5 * count as u64);
    }
}

#[test]
fn native_activation_rejects_bad_timeline_rosters_without_commands_and_actual_queue_failure_keeps_prefix()
 {
    for case in 0..5 {
        let data = prepared(MIXED, true);
        let mut prepared = cohort(&data, 2);
        match case {
            0 => {
                prepared.hazard_sounds.pop();
            }
            1 => prepared.hazard_sounds.swap(0, 1),
            2 => prepared.hazard_sounds[1].0 = PlayerId(99),
            3 | 4 => {
                let mut bindings = prepared.hazard_sounds[1].1.bindings().to_vec();
                let old = bindings[0].voice;
                let collision = if case == 3 {
                    prepared.input_sounds[0].1.markers()[0].voice
                } else {
                    prepared.hazard_sounds[0].1.bindings()[0].voice
                };
                for binding in &mut bindings {
                    if binding.voice == old {
                        binding.voice = collision;
                    }
                }
                prepared.hazard_sounds[1].1 = HazardSoundTimeline::new(bindings, 5).unwrap();
            }
            _ => unreachable!(),
        }
        let (producer, mut consumer) = command_queue(8).unwrap();
        assert!(
            activate_cohort_with_sounds(
                prepared.configs,
                &prepared.reserved,
                point(1, HOST),
                ClockDomainId(2),
                Transport::new(ts(HOST), ts(0), Rate::NORMAL),
                producer,
                None,
                prepared.input_sounds,
                prepared.hazard_sounds
            )
            .is_err()
        );
        assert!(
            consumer.try_pop().is_err(),
            "activation must not publish sound commands"
        );
    }
    let data = prepared(MIXED, true);
    let prepared = cohort(&data, 2);
    let valid = prepared.hazard_sounds;
    let (producer, mut consumer) = command_queue(8).unwrap();
    let (mut group, _) = activate_cohort_with_input_sounds(
        prepared.configs,
        &prepared.reserved,
        point(1, HOST),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        producer,
        None,
        prepared.input_sounds,
    )
    .unwrap();
    assert!(consumer.try_pop().is_err());
    let before = group
        .member_judge(PlayerId(u32::MAX))
        .unwrap()
        .stable_hash()
        .unwrap();
    let mut wrong = valid.clone();
    wrong.swap(0, 1);
    assert!(group.configure_hazard_sounds(wrong).is_err());
    assert_eq!(
        group
            .member_judge(PlayerId(u32::MAX))
            .unwrap()
            .stable_hash()
            .unwrap(),
        before
    );
    group.configure_hazard_sounds(valid).unwrap();
    assert!(consumer.try_pop().is_err());

    let prepared = cohort(&data, 2);
    let (producer, mut consumer) = command_queue(1).unwrap();
    let (mut group, _) = activate_cohort_with_sounds(
        prepared.configs,
        &prepared.reserved,
        point(1, HOST),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(0), Rate::NORMAL),
        producer,
        None,
        prepared.input_sounds,
        prepared.hazard_sounds,
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    assert!(consumer.try_pop().is_err());
    let failed = group
        .process_input(
            input(u64::MAX, 91, 0, 1, ButtonState::Down),
            &NoMapping,
            point(2, OUTPUT),
        )
        .unwrap_err();
    assert_eq!(failed.failed_player, Some(PlayerId(u32::MAX)));
    assert_eq!(failed.completed_reports.len(), 1);
    let report = &failed.completed_reports[0].report;
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(report.audio_commands, [play(1, 41, OUTPUT, 0.5)]);
    assert_eq!(report.audio_failures.len(), 1);
    assert_eq!(report.audio_failures[0].command, play(0, 47, OUTPUT, 0.5));
    assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(consumer.try_pop().unwrap(), play(1, 41, OUTPUT, 0.5));
    assert!(group.poisoned());
    assert!(
        group
            .advance_to(point(1, HOST + 1), &NoMapping, point(2, OUTPUT + 1))
            .is_err()
    );
    assert!(consumer.try_pop().is_err());
}

#[test]
fn native_cohort_preflights_mine_lanes_and_pcm_before_opponents_and_preserves_both_legacy_wrappers()
{
    let keys = BTreeMap::from([(0x11, 91u16), (0x12, 92u16)]);
    let options = CompetitionOptions {
        ghosts: vec![(
            OpponentKind::Own,
            PathBuf::from("must-not-open-before-mine-preparation.bkr"),
        )],
        ..CompetitionOptions::default()
    };
    let missing_lane = prepared("#BPM 60\n#000D9:1E", false);
    let error = prepare_cohort(
        &missing_lane,
        &assignments(2),
        &options,
        &cohort_config(&keys),
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("missing --bind for BMS channel19")
    );
    let missing_pcm = prepared(MIXED, false);
    let error = prepare_cohort(
        &missing_pcm,
        &assignments(2),
        &options,
        &cohort_config(&keys),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("PCM bank"));
    let mut bad = prepared(MIXED, true);
    bad.source.mine_ticks_per_beat = 0;
    let error = prepare_cohort(&bad, &assignments(2), &options, &cohort_config(&keys))
        .err()
        .unwrap();
    assert!(
        !error
            .to_string()
            .contains("must-not-open-before-mine-preparation.bkr")
    );
    for presses in [false, true] {
        let mut data = prepared(MIXED, false);
        data.source.mines.clear();
        data.source.mine_ticks_per_beat = 0;
        if !presses {
            data.source.invisible.clear();
        }
        let prepared = cohort(&data, 2);
        assert!(prepared.hazard_sounds.is_empty());
        let (producer, mut consumer) = command_queue(8).unwrap();
        let (mut group, _) = if presses {
            activate_cohort_with_input_sounds(
                prepared.configs,
                &prepared.reserved,
                point(1, HOST),
                ClockDomainId(2),
                Transport::new(ts(HOST), ts(0), Rate::NORMAL),
                producer,
                None,
                prepared.input_sounds,
            )
            .unwrap()
        } else {
            assert!(prepared.input_sounds.is_empty());
            activate_cohort(
                prepared.configs,
                &prepared.reserved,
                point(1, HOST),
                ClockDomainId(2),
                Transport::new(ts(HOST), ts(0), Rate::NORMAL),
                producer,
                None,
            )
            .unwrap()
        };
        group.set_processing_clock(RuntimeProcessingClock::Disabled);
        assert!(consumer.try_pop().is_err());
        let InputResult::Processed(rows) = group
            .process_input(
                input(u64::MAX, 92, 0, 1, ButtonState::Down),
                &NoMapping,
                point(2, OUTPUT),
            )
            .unwrap()
        else {
            panic!("assigned legacy input")
        };
        assert!(rows[0].report.hazard_events.is_empty());
        let expected = if presses {
            vec![play(2, 44, OUTPUT, 0.5)]
        } else {
            vec![]
        };
        assert_eq!(rows[0].report.audio_commands, expected);
        if presses {
            assert_eq!(consumer.try_pop().unwrap(), play(2, 44, OUTPUT, 0.5));
        }
        assert!(consumer.try_pop().is_err());
    }
}
