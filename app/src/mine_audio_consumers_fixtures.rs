//! Deferred consumers of typed mine setup; no file admission or device claim.
//! Captures, reconstruction, runtime queues, offline output and Mixer are real.
use crate::{
    PreparedBms,
    input_sounds::InputSoundIdentity,
    mine_plan::prepare_judge,
    native_audio::{prepare_input_sounds, prepare_mine_sounds},
    offline::{OfflineError, OfflineOptions, render_offline},
    replay_audio::{ReplayAudioError, completed_render_cursor, plan_audio, plan_section_audio},
    replay_capture::LiveReplayCapture,
    section_start::{prepare_at, prepare_section_replay},
};
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, PcmSample,
        QueuePushError, SampleBank, SampleId, VoiceId, command_queue,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, ContactId, DeviceId,
        DeviceSelector, EventMeta, GameControlId, PhysicalControlId, PhysicalInputEvent, Position2,
        TouchEvent, TouchPhase,
    },
    judge::{HazardOutcome, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow},
    replay::{
        ReplayOperation,
        codec::{ReplayCodecLimits, ReplayFile, decode_replay},
    },
    runtime::{Runtime, RuntimeProcessingClock, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsInputMode, parse};

const HOST: i64 = 10_000_000_000;
const OUTPUT: i64 = 1_000_000_000;
const ORDERED: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#WAV02 press\n#00011:01010000\n#00031:02\n#00032:02\n#000D1:001E0100\n#00001:00010000";
const HELD: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#00051:01000100\n#000D1:001E0000\n#000D2:001E0000";
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(ns),
    }
}
fn pcm_limits() -> PcmLimits {
    PcmLimits::new(256, 2048, 8).unwrap()
}
pub(super) fn replay_limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
pub(super) fn data(text: &str, zero: bool) -> PreparedBms {
    data_with_music(text, zero, &[0.25, 0.75])
}
fn data_with_music(text: &str, zero: bool, music: &[f32]) -> PreparedBms {
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
                .find(|object| object.id == note.object)
                .unwrap();
            SoundBinding {
                object: note.object,
                stage: if object.time.end.is_some() {
                    JudgeStage::HoldHead
                } else {
                    JudgeStage::Instant
                },
                sample: note.sample,
                voice: VoiceId(10 + note.object.0),
                gain,
            }
        })
        .collect();
    let bgm_commands = compiled
        .bgm
        .iter()
        .enumerate()
        .map(|(index, cue)| play(cue.sample.0, 90 + index as u64, cue.at.as_nanos(), gain))
        .collect();
    let format = AudioFormat::new(10, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits()).unwrap();
    for (id, pcm) in [(1, &[1.0, -1.0][..]), (2, music)] {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, pcm.to_vec(), pcm_limits()).unwrap(),
        )
        .unwrap();
    }
    if zero {
        bank.insert(
            SampleId(0),
            PcmSample::new(format, vec![0.5, -0.5], pcm_limits()).unwrap(),
        )
        .unwrap();
    }
    PreparedBms {
        source,
        compiled,
        sounds,
        bgm_commands,
        bank,
    }
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    Press(i64, u16),
    Release(i64, u16),
    Advance(i64),
    Bgm(usize),
}
pub(super) struct Recorded {
    pub(super) file: ReplayFile,
    pub(super) commands: Vec<AudioCommand>,
    pub(super) reports: Vec<RuntimeReport>,
    pub(super) hash: u64,
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
pub(super) fn recorded(
    data: &PreparedBms,
    actions: &[Action],
    mode: BmsInputMode,
    start: i64,
    end: Option<i64>,
    offset: i64,
    preroll: i64,
) -> Recorded {
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(offset),
    )
    .unwrap();
    let judge =
        prepare_judge(&data.source, data.compiled.chart.clone(), profile, mode, 16).unwrap();
    let mut capture = LiveReplayCapture::new_with_input_sounds(
        &judge,
        ClockDomainId(1),
        replay_limits(),
        ts(start),
        0,
        end.map(ts),
        mode,
        InputSoundIdentity::from_source(&data.source).unwrap(),
    )
    .unwrap();
    let presses = prepare_input_sounds(data).unwrap();
    let mines = prepare_mine_sounds(data, presses.as_ref()).unwrap();
    let bindings = BindingMap::from_bindings([(91u16, 0x11), (92u16, 0x12)].map(
        |(key, control)| Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(key),
            game_control: GameControlId(control),
        },
    ))
    .unwrap();
    let (producer, mut consumer) = command_queue(32).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(HOST), ts(start - preroll), Rate::NORMAL),
        bindings,
        judge,
        producer,
        data.sounds.clone(),
        0,
    )
    .unwrap();
    if let Some(timeline) = presses {
        runtime.configure_input_sounds(timeline).unwrap();
    }
    if let Some(timeline) = mines {
        runtime.configure_hazard_sounds(timeline).unwrap();
    }
    if let Some(end) = end {
        runtime.set_song_end(ts(end)).unwrap();
    }
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let mut commands = Vec::new();
    let mut reports = Vec::new();
    for (sequence, action) in actions.iter().copied().enumerate() {
        if let Action::Bgm(index) = action {
            let AudioCommand::Play {
                sample,
                voice,
                at,
                gain,
            } = data.bgm_commands[index]
            else {
                panic!("prepared BGM Play")
            };
            let command = play(
                sample.0,
                voice.0,
                OUTPUT + at.as_nanos() - start + preroll,
                gain,
            );
            runtime.enqueue_audio(command).unwrap();
            assert_eq!(consumer.try_pop().unwrap(), command);
            commands.push(command);
            continue;
        }
        let song = match action {
            Action::Press(song, _) | Action::Release(song, _) | Action::Advance(song) => song,
            Action::Bgm(_) => unreachable!(),
        };
        let host = point(1, HOST + song - start + preroll);
        let output = point(2, OUTPUT + song - start + preroll);
        let report = if matches!(action, Action::Advance(_)) {
            runtime.advance_to(host, &NoMapping, output).unwrap()
        } else {
            let (key, down) = match action {
                Action::Press(_, key) => (key, true),
                Action::Release(_, key) => (key, false),
                _ => unreachable!(),
            };
            let mut meta = EventMeta::new(DeviceId(u64::MAX), host, sequence as u64 + 1);
            meta.original_clock_point = Some(point(77, 9_007_199_254_740_993 + sequence as i64));
            let control = PhysicalControlId::keyboard(key);
            let physical = if mode == BmsInputMode::ButtonOrContact {
                PhysicalInputEvent::Touch(TouchEvent {
                    meta,
                    control,
                    contact: ContactId(u64::MAX - u64::from(key)),
                    phase: if down {
                        TouchPhase::Down
                    } else {
                        TouchPhase::Up
                    },
                    position: Position2 { x: -3.0, y: 12.5 },
                    pressure: Some(0.5),
                })
            } else {
                PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control,
                    state: if down {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                })
            };
            runtime.process_input(physical, &NoMapping, output).unwrap()
        };
        assert!(report.judge_error.is_none() && report.audio_failures.is_empty());
        for command in &report.audio_commands {
            assert_eq!(consumer.try_pop().unwrap(), *command);
        }
        assert!(consumer.try_pop().is_err());
        capture.record_report(&report).unwrap();
        commands.extend_from_slice(&report.audio_commands);
        reports.push(report);
    }
    Recorded {
        file: decode_replay(&capture.into_bytes().unwrap(), replay_limits()).unwrap(),
        commands,
        reports,
        hash: runtime.judge().stable_hash().unwrap(),
    }
}
fn render(bank: SampleBank, commands: &[AudioCommand], end: Option<u64>) -> Vec<f32> {
    let (mut producer, consumer) = command_queue(32).unwrap();
    for &command in commands {
        producer.try_push(command).unwrap();
    }
    let mut config = MixerConfig::new(
        bank.format(),
        ClockDomainId(2),
        ts(OUTPUT),
        AudioLimits::new(32, 16, 32, 32, 32).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    let mut mixer = Mixer::new(config, bank, consumer).unwrap();
    let mut pcm = vec![0.0; 30];
    assert_eq!(
        completed_render_cursor(&mixer.render(&mut pcm).unwrap()).unwrap(),
        30
    );
    pcm
}
fn offline_options(frames: u64, block: usize) -> OfflineOptions {
    OfflineOptions {
        frames,
        block_frames: block,
        command_capacity: 16,
        max_voices: 16,
    }
}
fn floats(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}
fn ordered_actions() -> [Action; 8] {
    [
        Action::Press(0, 91),
        Action::Bgm(0),
        Action::Press(1_000_000_000, 92),
        Action::Press(1_000_000_000, 92),
        Action::Release(1_000_000_000, 91),
        Action::Press(1_000_000_000, 91),
        Action::Advance(2_500_000_000),
        Action::Advance(2_500_000_000),
    ]
}

#[test]
fn actual_capture_replay_preserves_equal_operation_order_delayed_advance_provenance_and_pcm() {
    for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
        let live_data = data(ORDERED, true);
        let actual = recorded(&live_data, &ordered_actions(), mode, 0, None, 0, 0);
        let expected = vec![
            play(1, 11, OUTPUT, 0.5),
            play(1, 90, 2_000_000_000, 0.5),
            play(2, 92, 2_000_000_000, 0.5),
            play(0, 93, 2_000_000_000, 0.5),
            play(1, 12, 2_000_000_000, 0.5),
            play(0, 93, 3_500_000_000, 0.5),
        ];
        assert_eq!(actual.commands, expected);
        assert_eq!(
            actual.reports[1].hazard_events[0].input,
            Some(*actual.reports[1].input.as_ref().unwrap().meta())
        );
        assert_eq!(
            actual.reports[1].hazard_events[0].outcome,
            HazardOutcome::Triggered
        );
        assert!(
            actual.reports[2].audio_commands.is_empty(),
            "duplicate ownership cannot replay an equal boundary"
        );
        assert_eq!(actual.reports[5].hazard_events[0].at, ts(2_000_000_000));
        assert_eq!(actual.reports[5].hazard_events[0].input, None);
        assert!(actual.reports[6].hazard_events.is_empty());
        assert!(matches!(
            actual.file.records.last().unwrap().operation,
            ReplayOperation::Advance
        ));
        let replay_data = data(ORDERED, true);
        let plan = plan_section_audio(
            &replay_data,
            actual.file,
            replay_limits(),
            point(2, OUTPUT),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(plan.commands, expected);
        assert_eq!(plan.final_judge_hash, actual.hash);
        assert_eq!(plan.recorded_until, Some(ts(2_500_000_000)));
        assert_eq!(
            plan.judge_events,
            actual
                .reports
                .iter()
                .flat_map(|report| report.judge_events.clone())
                .collect::<Vec<_>>()
        );
        let mut pcm = vec![0.0; 30];
        pcm[0] = 0.5;
        pcm[1] = -0.5;
        pcm[10] = 1.0;
        pcm[11] = -0.875;
        pcm[25] = 0.25;
        pcm[26] = -0.25;
        assert_eq!(render(live_data.bank, &actual.commands, None), pcm);
        assert_eq!(render(replay_data.bank, &plan.commands, None), pcm);
    }
}

#[test]
fn replay_prefix_and_silent_variants_keep_legacy_order_while_audible_missing_pcm_is_an_error() {
    let text = "#BPM 60\n#WAV00 blast\n#000D1:001E0000";
    let audible = data(text, true);
    let prefix = recorded(
        &audible,
        &[Action::Press(0, 91)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &audible,
        prefix.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap();
    assert!(plan.commands.is_empty());
    assert_eq!(plan.recorded_until, Some(ts(0)));
    assert_eq!(plan.final_judge_hash, prefix.hash);
    let full = recorded(
        &audible,
        &[Action::Press(0, 91), Action::Advance(1_000_000_000)],
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let missing = data(text, false);
    let error = plan_audio(
        &missing,
        full.file.clone(),
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<ReplayAudioError>(),
        Some(ReplayAudioError::MissingSample(SampleId(0)))
    ));
    let complete = plan_audio(
        &audible,
        full.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(complete.commands, [play(0, 1, 2_000_000_000, 1.0)]);
    for text in [
        "#BPM 60\n#000D1:001E0000",
        "#BPM 60\n#WAV00 unused\n#000D1:00ZZ0000",
        "#BPM 60\n#WAV00 unused",
    ] {
        let data = data(text, false);
        let actual = recorded(
            &data,
            &[Action::Press(0, 91), Action::Advance(1_000_000_000)],
            BmsInputMode::ButtonOnly,
            0,
            None,
            0,
            0,
        );
        let plan = plan_audio(
            &data,
            actual.file,
            replay_limits(),
            point(2, OUTPUT),
            Duration::ZERO,
        )
        .unwrap();
        assert!(plan.commands.is_empty());
        assert_eq!(plan.final_judge_hash, actual.hash);
    }
    let legacy_text = ORDERED.replace("#WAV00 blast\n", "");
    let legacy = data(&legacy_text, false);
    let actual = recorded(
        &legacy,
        &ordered_actions(),
        BmsInputMode::ButtonOnly,
        0,
        None,
        0,
        0,
    );
    let plan = plan_audio(
        &legacy,
        actual.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(
        plan.commands,
        [
            play(1, 11, OUTPUT, 0.5),
            play(1, 90, 2_000_000_000, 0.5),
            // Mine sources preserve operation order even without WAV00.
            play(2, 92, 2_000_000_000, 0.5),
            play(1, 12, 2_000_000_000, 0.5)
        ]
    );
    assert_eq!(plan.final_judge_hash, actual.hash);
}

#[test]
fn actual_section_keeps_wav00_uncropped_and_maps_offset_preroll_and_rounded_finite_end_once() {
    let text = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV02 music\n#00001:02\n#000D1:001E1E00";
    let original = data_with_music(text, true, &[0.25; 30]);
    let original_mines = original.source.compile_mines().unwrap();
    let zero_pointer = original.bank.get(SampleId(0)).unwrap().samples().as_ptr();
    let (selected, report) = prepare_at(original, ts(500_000_000), pcm_limits()).unwrap();
    assert_eq!(selected.source.compile_mines().unwrap(), original_mines);
    assert_eq!(
        selected.bank.get(SampleId(0)).unwrap().samples(),
        [0.5, -0.5]
    );
    assert_eq!(
        selected.bank.get(SampleId(0)).unwrap().samples().as_ptr(),
        zero_pointer
    );
    assert_eq!(report.tails.len(), 1);
    assert_eq!(
        (
            report.tails[0].source,
            report.tails[0].suffix,
            report.tails[0].frame
        ),
        (SampleId(2), SampleId(3), 5)
    );
    assert_eq!(
        selected.bank.get(SampleId(3)).unwrap().samples(),
        [0.25; 25]
    );
    let actual = recorded(
        &selected,
        &[
            Action::Bgm(0),
            Action::Press(500_000_000, 91),
            Action::Advance(900_000_000),
            Action::Advance(1_905_000_000),
            Action::Advance(1_910_000_000),
        ],
        BmsInputMode::ButtonOnly,
        500_000_000,
        Some(1_910_000_000),
        100_000_000,
        100_000_000,
    );
    assert_eq!(
        actual.commands,
        [
            play(3, 90, 1_100_000_000, 0.5),
            play(0, 91, 1_500_000_000, 0.5),
            play(0, 91, 2_505_000_000, 0.5)
        ]
    );
    let selected_again = prepare_section_replay(
        data_with_music(text, true, &[0.25; 30]),
        &actual.file,
        replay_limits(),
        pcm_limits(),
    )
    .unwrap();
    assert_eq!(
        selected_again.bank.get(SampleId(0)).unwrap().samples(),
        [0.5, -0.5]
    );
    assert_eq!(
        selected_again.bank.get(SampleId(3)).unwrap().samples(),
        [0.25; 25]
    );
    assert!(
        plan_audio(
            &selected_again,
            actual.file.clone(),
            replay_limits(),
            point(2, OUTPUT),
            Duration::from_nanos(100_000_000)
        )
        .is_err()
    );
    let plan = plan_section_audio(
        &selected_again,
        actual.file,
        replay_limits(),
        point(2, OUTPUT),
        Duration::from_nanos(100_000_000),
    )
    .unwrap();
    assert_eq!(
        plan.commands,
        [
            play(3, 90, 1_100_000_000, 0.5),
            play(0, 91, 1_500_000_000, 0.5)
        ]
    );
    assert_eq!(plan.final_judge_hash, actual.hash);
    assert_eq!(plan.recorded_until, Some(ts(1_910_000_000)));
    let mut pcm = vec![0.0; 30];
    pcm[1..16].fill(0.125);
    pcm[5] = 0.375;
    pcm[6] = -0.125;
    assert_eq!(render(selected_again.bank, &plan.commands, Some(16)), pcm);
}

#[test]
fn offline_real_holds_and_equal_time_inputs_trigger_once_without_synthetic_mine_presses_across_blocks()
 {
    let mut held_expected = vec![0.0; 25];
    held_expected[0] = 0.5;
    held_expected[1] = -0.5;
    held_expected[10] = 0.25;
    held_expected[11] = -0.25;
    let instant = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#WAV02 press\n#00001:01\n#00011:01\n#00031:02\n#000D1:1E";
    for block in [1usize, 3, 7, 32] {
        let mut bytes = Vec::new();
        let report =
            render_offline(data(HELD, true), offline_options(25, block), &mut bytes).unwrap();
        assert_eq!(floats(&bytes), held_expected);
        assert_eq!(
            (report.frames, report.hits, report.judge_results),
            (25, 2, 2)
        );
        let render = report.last_render.unwrap();
        assert_eq!(render.counters.commands_applied, 2);
        assert_eq!(render.counters.late_commands, 0);
        let mut bytes = Vec::new();
        let report =
            render_offline(data(instant, true), offline_options(4, block), &mut bytes).unwrap();
        assert_eq!(floats(&bytes), [1.0, -1.0, 0.0, 0.0]);
        assert_eq!((report.hits, report.judge_results), (1, 1));
        assert_eq!(
            report.last_render.unwrap().counters.commands_applied,
            3,
            "BGM, normal head and one mine; head suppresses invisible fallback"
        );
        for (text, zero) in [
            (
                "#BPM 60\n#WAV00 blast\n#WAV02 press\n#00031:02\n#000D1:001E0000",
                true,
            ),
            ("#BPM 60\n#WAV00 unused\n#000D1:00ZZ0000", false),
            ("#BPM 60\n#000D1:001E0000", false),
        ] {
            let mut bytes = Vec::new();
            let report =
                render_offline(data(text, zero), offline_options(15, block), &mut bytes).unwrap();
            assert_eq!(floats(&bytes), [0.0; 15]);
            assert_eq!((report.hits, report.judge_results), (0, 0));
            assert_eq!(report.last_render.unwrap().counters.commands_applied, 0);
        }
    }
    let legacy = HELD
        .replace("#WAV00 blast\n", "")
        .replace("#000D1:001E0000\n", "")
        .replace("#000D2:001E0000", "");
    let mut bytes = Vec::new();
    let report = render_offline(data(&legacy, false), offline_options(25, 7), &mut bytes).unwrap();
    held_expected[10] = 0.0;
    held_expected[11] = 0.0;
    assert_eq!(floats(&bytes), held_expected);
    assert_eq!(report.hits, 2);
}

#[test]
fn offline_missing_pcm_zero_extent_and_later_queue_failure_preserve_exact_written_prefix() {
    let mut untouched = vec![0x7a];
    assert!(render_offline(data(HELD, false), offline_options(0, 4), &mut untouched).is_err());
    assert_eq!(untouched, [0x7a]);
    let mut empty = Vec::new();
    let report = render_offline(data(HELD, true), offline_options(0, 4), &mut empty).unwrap();
    assert!(empty.is_empty());
    assert_eq!(
        (report.frames, report.hits, report.judge_results),
        (0, 0, 0)
    );
    assert!(report.last_render.is_none());
    let two = "#BPM 60\n#VOLWAV 50\n#WAV00 blast\n#WAV01 note\n#00051:01000100\n#00052:0001000001000000\n#000D1:001E0000\n#000D2:001E0000";
    let mut prefix = Vec::new();
    let mut options = offline_options(25, 5);
    options.command_capacity = 1;
    let error = render_offline(data(two, true), options, &mut prefix).unwrap_err();
    let failure = error.downcast_ref::<OfflineError>().unwrap();
    assert_eq!(
        floats(&prefix),
        [0.5, -0.5, 0.0, 0.0, 0.0, 0.5, -0.5, 0.0, 0.0, 0.0]
    );
    let last = failure.last_render.unwrap();
    assert_eq!(last.start_frame + last.frames as u64, 10);
    assert_eq!(failure.audio_failures.len(), 1);
    assert_eq!(
        failure.audio_failures[0].command,
        play(0, 14, 1_000_000_000, 0.5)
    );
    assert_eq!(failure.audio_failures[0].reason, QueuePushError::Full);
    for (frames, applied, last_sample) in [(10, 2, 0.0), (11, 4, 0.5)] {
        let mut bytes = Vec::new();
        let report =
            render_offline(data(two, true), offline_options(frames, 3), &mut bytes).unwrap();
        assert_eq!(bytes.len(), frames as usize * 4);
        assert_eq!(report.hits, 2);
        assert_eq!(
            report.last_render.unwrap().counters.commands_applied,
            applied
        );
        assert_eq!(*floats(&bytes).last().unwrap(), last_sample);
    }
}
