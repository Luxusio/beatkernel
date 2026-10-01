use beatkernel::{
    audio::*,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage, JudgeWindow},
    replay::codec::{ReplayCodecLimits, ReplayFile},
    runtime::{Runtime, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms::{ParseOptions, parse};
use beatkernel_bms_runtime::{
    PreparedBms,
    offline::{OfflineError, OfflineOptions},
    replay_audio::plan_audio,
    replay_capture::LiveReplayCapture,
    replay_render::render_replay,
};
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

const DOMAIN: ClockDomainId = ClockDomainId(31);
const MAIN: &str = "#BPM 60\n#LNTYPE 1\n#WAV01 tap.wav\n#WAV02 hold.wav\n#WAV03 layer.wav\n#WAV04 later.wav\n#00011:00010000\n#00052:00000202\n#00013:00000001\n#00001:00030000\n#00201:04\n";
fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp: Timestamp::from_nanos(nanos),
    }
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(
        16 << 20,
        20_000,
        4096,
        CodecLimits::new(4096, 1024).unwrap(),
    )
    .unwrap()
}
fn options(frames: u64, block: usize) -> OfflineOptions {
    OfflineOptions {
        frames,
        block_frames: block,
        command_capacity: 4,
        max_voices: 4,
    }
}
fn prepared(text: &str, rate: u32, assets: &[(u64, &[f32])]) -> PreparedBms {
    let source = parse(text, ParseOptions::default()).unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(rate, 1).unwrap();
    let pcm_limits = PcmLimits::new(4096, 16384, 8).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    for &(id, samples) in assets {
        bank.insert(
            SampleId(id),
            PcmSample::new(format, samples.to_vec(), pcm_limits).unwrap(),
        )
        .unwrap();
    }
    let holds: BTreeMap<_, _> = compiled
        .chart
        .objects()
        .iter()
        .map(|object| (object.id, object.time.end.is_some()))
        .collect();
    let sounds = source
        .notes
        .iter()
        .map(|note| SoundBinding {
            object: note.object,
            stage: if holds[&note.object] {
                JudgeStage::HoldHead
            } else {
                JudgeStage::Instant
            },
            sample: note.sample,
            voice: VoiceId(note.object.0),
            gain: 1.0,
        })
        .collect();
    let first_bgm = compiled
        .chart
        .objects()
        .iter()
        .map(|object| object.id.0)
        .max()
        .unwrap_or(0)
        + 1;
    let bgm_commands = compiled
        .bgm
        .iter()
        .enumerate()
        .map(|(index, event)| AudioCommand::Play {
            voice: VoiceId(first_bgm + index as u64),
            sample: event.sample,
            at: event.at,
            gain: 1.0,
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands,
    }
}
fn main_prepared() -> PreparedBms {
    prepared(
        MAIN,
        2,
        &[
            (1, &[0.5, 0.25]),
            (2, &[0.75, 0.375]),
            (3, &[0.125]),
            (4, &[0.875]),
        ],
    )
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn captured(
    prepared: &PreparedBms,
    offset: i64,
    skip_control: Option<u32>,
    finish: Option<i64>,
) -> ReplayFile {
    captured_at(prepared, offset, skip_control, finish, Timestamp::ZERO)
}
fn captured_at(
    prepared: &PreparedBms,
    offset: i64,
    skip_control: Option<u32>,
    finish: Option<i64>,
    start: Timestamp,
) -> ReplayFile {
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(2),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(offset),
    )
    .unwrap();
    let judge = JudgeEngine::new(
        prepared.compiled.chart.clone(),
        prepared.source.rules(),
        profile,
    )
    .unwrap();
    let mut capture = LiveReplayCapture::new_at(&judge, DOMAIN, limits(), start).unwrap();
    let controls: BTreeMap<_, _> = prepared
        .source
        .notes
        .iter()
        .map(|note| (note.object, note.lane.control()))
        .collect();
    let unique: BTreeMap<_, _> = prepared
        .source
        .notes
        .iter()
        .map(|note| (note.lane.control().0, note.lane.control()))
        .collect();
    let bindings = BindingMap::from_bindings(unique.values().map(|&control| Binding {
        device: DeviceSelector::Exact(DeviceId(1)),
        physical: PhysicalControlId::keyboard(control.0 as u16),
        game_control: control,
    }))
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let origin = Timestamp::from_nanos(-offset.abs());
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        Transport::new(origin, origin, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    let mut events = Vec::new();
    for object in prepared.compiled.chart.objects() {
        let control = controls[&object.id];
        if Some(control.0) == skip_control {
            continue;
        }
        let at = object.time.start.as_nanos() - offset;
        events.push((at, events.len(), control, ButtonState::Down));
        let end = object.time.end.map_or(at, |end| end.as_nanos() - offset);
        events.push((end, events.len(), control, ButtonState::Up));
    }
    events.sort_by_key(|event| (event.0, event.1));
    for (sequence, (at, _, control, state)) in events.into_iter().enumerate() {
        let report = runtime
            .process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(1), point(at), sequence as u64),
                    control: PhysicalControlId::keyboard(control.0 as u16),
                    state,
                }),
                &Identity,
                point(at),
            )
            .unwrap();
        assert!(report.judge_error.is_none());
        capture.record_report(&report).unwrap();
    }
    if let Some(at) = finish {
        capture
            .record_report(&runtime.advance_to(point(at), &Identity, point(at)).unwrap())
            .unwrap();
    }
    capture.into_file()
}
fn floats(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

#[test]
fn recorded_section_restores_original_pcm_tail_and_subtracts_start_once() {
    use beatkernel::replay::{ReplayOperation, ReplayRecord};
    use beatkernel_bms_runtime::section_start::{prepare_at, prepare_replay};
    let text = "#BPM 120\n#WAV01 music.wav\n#00001:01\n#00111:01\n";
    let pcm: Vec<_> = (0..16).map(|frame| frame as f32 / 16.0).collect();
    let original = || prepared(text, 8, &[(1, &pcm)]);
    let start = Timestamp::from_nanos(550_000_000);
    let pcm_limits = PcmLimits::new(4096, 16384, 8).unwrap();
    let (section, _) = prepare_at(original(), start, pcm_limits).unwrap();
    let judge = JudgeEngine::new(
        section.compiled.chart.clone(),
        section.source.rules(),
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
    .unwrap();
    let capture = LiveReplayCapture::new_at(&judge, DOMAIN, limits(), start).unwrap();
    let file = ReplayFile::new(
        capture.header().clone(),
        vec![ReplayRecord {
            ordinal: 0,
            song_time: Timestamp::from_nanos(1_500_000_000),
            operation: ReplayOperation::Advance,
        }],
    );
    let restored = prepare_replay(original(), &file, limits(), pcm_limits).unwrap();
    let plan = plan_audio(
        &restored,
        file.clone(),
        limits(),
        point(100),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(plan.commands[0].at().as_nanos(), 75_000_100);
    assert_eq!(
        plan.recorded_until,
        Some(Timestamp::from_nanos(1_500_000_000))
    );
    for block in [1, 3] {
        let mut bytes = Vec::new();
        let report = render_replay(
            original(),
            file.clone(),
            limits(),
            options(4, block),
            Duration::ZERO,
            &mut bytes,
        )
        .unwrap();
        assert_eq!(floats(&bytes), [0.0, 5.0 / 16.0, 6.0 / 16.0, 7.0 / 16.0]);
        assert_eq!(report.final_judge_hash, plan.final_judge_hash);
    }
    let with_hits = captured_at(&section, 100_000_000, None, None, start);
    let restored = prepare_replay(original(), &with_hits, limits(), pcm_limits).unwrap();
    let plan = plan_audio(
        &restored,
        with_hits,
        limits(),
        point(100),
        Duration::from_nanos(200_000_000),
    )
    .unwrap();
    assert_eq!(
        plan.commands
            .iter()
            .map(|command| command.at().as_nanos())
            .collect::<Vec<_>>(),
        [275_000_100, 1_550_000_100]
    );
    assert_eq!(plan.judge_events[0].at.as_nanos(), 2_000_000_000);
}

#[test]
fn literal_tap_hold_head_silent_tail_miss_and_bgm_pcm_is_partition_invariant() {
    let prepared = main_prepared();
    let file = captured(&prepared, 0, Some(0x13), Some(4_000_000_001));
    let plan = plan_audio(
        &prepared,
        file.clone(),
        limits(),
        point(100),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(plan.output_origin, point(100));
    assert_eq!(
        plan.recorded_until,
        Some(Timestamp::from_nanos(4_000_000_001))
    );
    assert_eq!(plan.commands.len(), 3);
    assert_eq!(plan.judge_events.len(), 4);
    assert_eq!(
        plan.judge_events
            .iter()
            .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
            .count(),
        3
    );
    assert_eq!(
        plan.commands
            .iter()
            .map(|command| command.at().as_nanos())
            .collect::<Vec<_>>(),
        vec![1_000_000_100, 1_000_000_100, 2_000_000_100]
    );
    let expected = vec![0.0, 0.0, 0.625, 0.25, 0.75, 0.375, 0.0, 0.0, 0.0, 0.0];
    for block in [1, 3, 7, 64] {
        let mut output = Vec::new();
        let report = render_replay(
            main_prepared(),
            file.clone(),
            limits(),
            options(10, block),
            Duration::ZERO,
            &mut output,
        )
        .unwrap();
        assert_eq!(floats(&output), expected);
        assert_eq!(
            (report.commands_admitted, report.hits, report.judge_results),
            (3, 3, 4)
        );
        assert_eq!(report.final_judge_hash, plan.final_judge_hash);
        assert_eq!(report.recorded_until, plan.recorded_until);
        let render = report.last_render.unwrap();
        assert_eq!(render.counters.commands_applied, 3);
        assert_eq!(render.counters.late_commands, 0);
    }
}

#[test]
fn signed_offsets_are_removed_once_then_preroll_uses_integer_ceil_frame() {
    let text = "#BPM 60\n#WAV01 tap.wav\n#00011:00010000\n";
    for (offset, expected_frame) in [(250_000_000, 3), (-250_000_000, 4)] {
        let prepared = prepared(text, 3, &[(1, &[0.5])]);
        let file = captured(&prepared, offset, None, None);
        let plan = plan_audio(
            &prepared,
            file.clone(),
            limits(),
            point(-50),
            Duration::from_nanos(1),
        )
        .unwrap();
        assert_eq!(
            plan.commands[0].at().as_nanos(),
            1_000_000_000 - offset - 49
        );
        let mut output = Vec::new();
        let report = render_replay(
            prepared,
            file,
            limits(),
            options(6, 2),
            Duration::from_nanos(1),
            &mut output,
        )
        .unwrap();
        let mut expected = vec![0.0; 6];
        expected[expected_frame] = 0.5;
        assert_eq!(floats(&output), expected);
        assert_eq!((report.hits, report.commands_admitted), (1, 1));
    }
}

#[test]
fn equal_time_bgm_precedes_hit_and_explicit_hold_tail_binding_sounds() {
    let mut prepared = main_prepared();
    let file = captured(&prepared, 0, Some(0x13), Some(4_000_000_001));
    let tap = prepared
        .sounds
        .iter()
        .find(|sound| sound.stage == JudgeStage::Instant)
        .unwrap()
        .voice;
    if let AudioCommand::Play { voice, .. } = &mut prepared.bgm_commands[0] {
        *voice = tap;
    }
    let hold = *prepared
        .sounds
        .iter()
        .find(|sound| sound.stage == JudgeStage::HoldHead)
        .unwrap();
    prepared.sounds.push(SoundBinding {
        stage: JudgeStage::HoldTail,
        sample: SampleId(3),
        ..hold
    });
    let plan = plan_audio(&prepared, file.clone(), limits(), point(0), Duration::ZERO).unwrap();
    assert!(matches!(
        plan.commands[0],
        AudioCommand::Play {
            sample: SampleId(3),
            ..
        }
    ));
    assert!(matches!(
        plan.commands[1],
        AudioCommand::Play {
            sample: SampleId(1),
            ..
        }
    ));
    let mut output = Vec::new();
    let report = render_replay(
        prepared,
        file,
        limits(),
        options(10, 3),
        Duration::ZERO,
        &mut output,
    )
    .unwrap();
    assert_eq!(
        floats(&output),
        vec![0.0, 0.0, 0.5, 0.25, 0.75, 0.375, 0.125, 0.0, 0.0, 0.0]
    );
    assert_eq!(report.commands_admitted, 4);
}

#[test]
fn empty_prefix_and_output_cutoffs_preserve_full_logical_counts() {
    let prepared = main_prepared();
    let file = captured(&prepared, 0, Some(0x13), Some(4_000_000_001));
    for (frames, admitted) in [(0, 0), (2, 0), (3, 2), (4, 2), (5, 3)] {
        let mut output = Vec::new();
        let report = render_replay(
            main_prepared(),
            file.clone(),
            limits(),
            options(frames, 2),
            Duration::ZERO,
            &mut output,
        )
        .unwrap();
        assert_eq!(output.len(), frames as usize * 4);
        assert_eq!(
            (report.hits, report.judge_results, report.commands_admitted),
            (3, 4, admitted)
        );
        assert_eq!(report.last_render.is_none(), frames == 0);
    }
    let mut empty = file.clone();
    empty.records.clear();
    let plan = plan_audio(&prepared, empty.clone(), limits(), point(0), Duration::ZERO).unwrap();
    assert!(plan.commands.is_empty());
    assert!(plan.judge_events.is_empty());
    assert_eq!(plan.recorded_until, None);
    let mut output = Vec::new();
    let report = render_replay(
        prepared,
        empty,
        limits(),
        options(4, 2),
        Duration::ZERO,
        &mut output,
    )
    .unwrap();
    assert_eq!(floats(&output), vec![0.0; 4]);
    assert_eq!((report.hits, report.commands_admitted), (0, 0));
    let mut prefix = file;
    prefix.records.truncate(2);
    let plan = plan_audio(&main_prepared(), prefix, limits(), point(0), Duration::ZERO).unwrap();
    assert_eq!(plan.commands.len(), 2);
    assert_eq!(plan.judge_events.len(), 1);
}

#[test]
fn five_thousand_sparse_replayed_notes_need_only_one_queue_slot_and_voice() {
    let mut text = String::from("#BPM 60\n#WAV01 tap.wav\n");
    for measure in 0..1000 {
        text.push_str(&format!("#{measure:03}11:0101010101\n"));
    }
    let prepared = prepared(&text, 5, &[(1, &[0.25])]);
    let file = captured(&prepared, 0, None, None);
    let mut output = Vec::new();
    let report = render_replay(
        prepared,
        file,
        limits(),
        OfflineOptions {
            frames: 20_000,
            block_frames: 7,
            command_capacity: 1,
            max_voices: 1,
        },
        Duration::ZERO,
        &mut output,
    )
    .unwrap();
    assert_eq!(
        (report.hits, report.judge_results, report.commands_admitted),
        (5000, 5000, 5000)
    );
    for (frame, sample) in floats(&output).into_iter().enumerate() {
        assert_eq!(sample, if frame % 4 == 0 { 0.25 } else { 0.0 });
    }
    assert_eq!(report.last_render.unwrap().counters.voice_full, 0);
}

struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("literal replay writer failure"))
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("renderer must not flush caller sink")
    }
}
#[test]
fn exact_queue_failure_voice_counters_and_writer_errors_are_not_silent_success() {
    let prepared = main_prepared();
    let file = captured(&prepared, 0, Some(0x13), Some(4_000_000_001));
    let error = render_replay(
        main_prepared(),
        file.clone(),
        limits(),
        OfflineOptions {
            command_capacity: 1,
            ..options(10, 2)
        },
        Duration::ZERO,
        &mut Vec::new(),
    )
    .err()
    .expect("same-frame queue must overflow");
    let failure = error
        .downcast_ref::<OfflineError>()
        .expect("typed exact admission failure");
    assert_eq!(failure.audio_failures.len(), 1);
    assert_eq!(failure.audio_failures[0].reason, QueuePushError::Full);
    assert!(matches!(
        failure.audio_failures[0].command,
        AudioCommand::Play {
            sample: SampleId(1),
            ..
        }
    ));
    let error = render_replay(
        main_prepared(),
        file.clone(),
        limits(),
        OfflineOptions {
            max_voices: 1,
            ..options(10, 2)
        },
        Duration::ZERO,
        &mut Vec::new(),
    )
    .err()
    .expect("distinct tied voices cannot fit");
    assert_eq!(
        error
            .downcast_ref::<OfflineError>()
            .unwrap()
            .last_render
            .unwrap()
            .counters
            .voice_full,
        1
    );
    assert!(render_replay(
        prepared,
        file,
        limits(),
        options(10, 2),
        Duration::ZERO,
        &mut BrokenWriter
    )
    .is_err());
}

#[test]
fn invalid_assets_gain_background_variant_identity_and_wide_time_mapping_reject() {
    let main = main_prepared();
    let file = captured(&main, 0, Some(0x13), Some(4_000_000_001));
    assert!(plan_audio(
        &main,
        file.clone(),
        limits(),
        point(0),
        Duration::from_nanos(-1)
    )
    .is_err());
    assert!(plan_audio(
        &main,
        file.clone(),
        limits(),
        point(i64::MAX),
        Duration::ZERO
    )
    .is_err());
    let mut wrong = file.clone();
    wrong.header.chart_identity[0] ^= 1;
    assert!(plan_audio(&main, wrong, limits(), point(0), Duration::ZERO).is_err());
    for invalid in [
        OfflineOptions {
            block_frames: 0,
            ..options(10, 2)
        },
        OfflineOptions {
            command_capacity: 0,
            ..options(10, 2)
        },
        OfflineOptions {
            max_voices: 0,
            ..options(10, 2)
        },
        options(u64::MAX, 2),
    ] {
        let mut output = Vec::new();
        assert!(render_replay(
            main_prepared(),
            file.clone(),
            limits(),
            invalid,
            Duration::ZERO,
            &mut output
        )
        .is_err());
        assert!(output.is_empty());
    }
    for variant in 0..3 {
        let mut invalid = main_prepared();
        match variant {
            0 => invalid.sounds[0].sample = SampleId(999),
            1 => invalid.sounds[0].gain = f32::NAN,
            _ => {
                invalid.bgm_commands[0] = AudioCommand::Stop {
                    voice: VoiceId(0),
                    at: Timestamp::ZERO,
                }
            }
        }
        assert!(plan_audio(&invalid, file.clone(), limits(), point(0), Duration::ZERO).is_err());
        assert!(render_replay(
            invalid,
            file.clone(),
            limits(),
            options(0, 2),
            Duration::ZERO,
            &mut Vec::new()
        )
        .is_err());
    }
    let text = "#BPM 60\n#WAV01 tap.wav\n#00011:01\n";
    let early = prepared(text, 2, &[(1, &[0.5])]);
    let negative = captured(&early, 10, None, None);
    assert!(plan_audio(&early, negative.clone(), limits(), point(0), Duration::ZERO).is_err());
    assert!(plan_audio(
        &early,
        negative,
        limits(),
        point(i64::MIN),
        Duration::from_nanos(i64::MAX)
    )
    .is_ok());
}
