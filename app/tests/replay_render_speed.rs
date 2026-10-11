#[path = "support/replay_speed.rs"]
mod fixture;

use beatkernel::{
    audio::{AudioCommand, QueuePushError, SampleId},
    replay::codec::encode_replay,
    time::{Duration, Timestamp},
    transport::Rate,
};
use beatkernel_bms_runtime::{
    PreparedBms,
    gauge::GaugeProfile,
    offline::{OfflineError, OfflineOptions},
    replay_audio::{plan_audio, plan_audio_with_rate},
    replay_render::{render_replay, render_replay_with_rate},
};
use fixture::*;
use std::io::{self, Write};

const TEXT: &str = "#BPM 60\n#WAV01 tap.wav\n#WAV02 bgm.wav\n#00011:00010000\n#00001:02\n";
fn song() -> PreparedBms {
    prepared(
        TEXT,
        4,
        &[
            (
                1,
                &[
                    -0.0625, -0.125, -0.1875, -0.25, -0.3125, -0.375, -0.4375, -0.5,
                ],
            ),
            (
                2,
                &[0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5],
            ),
        ],
    )
}
fn options(frames: u64, block_frames: usize) -> OfflineOptions {
    OfflineOptions {
        frames,
        block_frames,
        command_capacity: 8,
        max_voices: 8,
    }
}
fn floats(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
struct Sink {
    bytes: Vec<u8>,
    writes: usize,
    flushes: usize,
}
impl Sink {
    fn new() -> Self {
        Self {
            bytes: vec![],
            writes: 0,
            flushes: 0,
        }
    }
}
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn literal_real_pcm_covers_rational_readheads_onsets_pitch_and_long_bgm_tails() {
    // Eight original frames at 4 Hz: source heads advance by exactly the
    // requested ratio. These literal sequences are independent of the planner.
    let cases = [
        (
            1,
            2,
            vec![
                0.0625, 0.09375, 0.125, 0.15625, 0.1875, 0.21875, 0.25, 0.28125, 0.25, 0.25, 0.25,
                0.25, 0.25, 0.25, 0.25, 0.21875, -0.3125, -0.34375, -0.375, -0.40625, -0.4375,
                -0.46875, -0.5, -0.5,
            ],
        ),
        (
            1,
            1,
            vec![
                0.0625, 0.125, 0.1875, 0.25, 0.25, 0.25, 0.25, 0.25, -0.3125, -0.375, -0.4375, -0.5,
            ],
        ),
        (2, 1, vec![0.0625, 0.1875, 0.25, 0.25, -0.3125, -0.4375]),
        (
            3,
            2,
            vec![
                0.0625, 0.15625, 0.25, 0.28125, 0.28125, 0.25, -0.34375, -0.4375, -0.5,
            ],
        ),
    ];
    let p = song();
    let file = captured(&p, 0, Some(4_000_000_001));
    let encoded = encode_replay(&file, limits()).unwrap();
    let original = plan_audio(&p, file.clone(), limits(), point(0), Duration::ZERO).unwrap();
    assert_eq!(original.judge_events.len(), 1);
    for (num, den, mut expected) in cases {
        expected.resize(28, 0.0);
        let rate = Rate::new(num, den).unwrap();
        for block in [1, 2, 3, 7, 32] {
            let mut sink = Sink::new();
            let report = render_replay_with_rate(
                song(),
                file.clone(),
                limits(),
                options(28, block),
                Duration::ZERO,
                rate,
                &mut sink,
            )
            .unwrap();
            assert_eq!(
                floats(&sink.bytes),
                expected,
                "speed {num}/{den}, block {block}"
            );
            assert_eq!(
                (
                    report.frames,
                    report.format.sample_rate(),
                    report.format.channels()
                ),
                (28, 4, 1)
            );
            assert_eq!((report.hits, report.judge_results), (1, 1));
            assert_eq!(report.final_judge_hash, original.final_judge_hash);
            assert_eq!(report.recorded_until, original.recorded_until);
            assert_eq!(
                report.commands_admitted,
                if rate == Rate::NORMAL { 2 } else { 3 }
            );
            let last = report.last_render.unwrap();
            assert_eq!(last.start_frame + last.frames as u64, 28);
            assert_eq!((last.active_voices, last.pending_commands), (0, 0));
            assert_eq!(
                last.counters.commands_applied,
                report.commands_admitted as u64
            );
            assert_eq!(sink.flushes, 0);
        }
        let logical =
            plan_audio_with_rate(&p, file.clone(), limits(), point(0), Duration::ZERO, rate)
                .unwrap();
        assert_eq!(logical.judge_events, original.judge_events);
        assert_eq!(logical.final_judge_hash, original.final_judge_hash);
        assert_eq!(encode_replay(&file, limits()).unwrap(), encoded);
    }
}

#[test]
fn normal_wrapper_and_reduced_one_are_byte_and_report_compatible() {
    let file = captured(&song(), 0, Some(4_000_000_001));
    let mut original = Sink::new();
    let a = render_replay(
        song(),
        file.clone(),
        limits(),
        options(16, 3),
        Duration::ZERO,
        &mut original,
    )
    .unwrap();
    for rate in [Rate::NORMAL, Rate::new(8, 8).unwrap()] {
        let mut sink = Sink::new();
        let b = render_replay_with_rate(
            song(),
            file.clone(),
            limits(),
            options(16, 3),
            Duration::ZERO,
            rate,
            &mut sink,
        )
        .unwrap();
        assert_eq!(sink.bytes, original.bytes);
        assert_eq!(
            (
                b.frames,
                b.commands_admitted,
                b.hits,
                b.judge_results,
                b.recorded_until,
                b.final_judge_hash
            ),
            (
                a.frames,
                a.commands_admitted,
                a.hits,
                a.judge_results,
                a.recorded_until,
                a.final_judge_hash
            )
        );
        assert_eq!(b.last_render.unwrap().counters.commands_applied, 2);
        assert_eq!((sink.writes, sink.flushes), (original.writes, 0));
    }
}

#[test]
fn output_preroll_is_unscaled_and_cutoff_keeps_complete_logical_evidence() {
    let file = captured(&song(), 0, Some(4_000_000_001));
    let expected = [
        0.0, 0.0, 0.0625, 0.1875, 0.25, 0.25, -0.3125, -0.4375, 0.0, 0.0,
    ];
    let mut full = Sink::new();
    let complete = render_replay_with_rate(
        song(),
        file.clone(),
        limits(),
        options(10, 3),
        Duration::from_nanos(500_000_000),
        Rate::new(2, 1).unwrap(),
        &mut full,
    )
    .unwrap();
    assert_eq!(floats(&full.bytes), expected);
    for (extent, admitted) in [(0, 0), (2, 1), (3, 2), (4, 2), (5, 3)] {
        let mut sink = Sink::new();
        let report = render_replay_with_rate(
            song(),
            file.clone(),
            limits(),
            options(extent, 2),
            Duration::from_nanos(500_000_000),
            Rate::new(2, 1).unwrap(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(floats(&sink.bytes), expected[..extent as usize]);
        assert_eq!(report.commands_admitted, admitted);
        assert_eq!(
            (
                report.hits,
                report.judge_results,
                report.recorded_until,
                report.final_judge_hash
            ),
            (
                complete.hits,
                complete.judge_results,
                complete.recorded_until,
                complete.final_judge_hash
            )
        );
        assert_eq!(sink.flushes, 0);
        if extent == 0 {
            assert_eq!(sink.writes, 0);
            assert!(report.last_render.is_none());
        }
    }
}

#[test]
fn origin_rate_control_causes_real_queue_refusal_without_capacity_inflation() {
    let make = || {
        prepared(
            "#BPM 60\n#WAV01 tap.wav\n#00011:01\n",
            4,
            &[(1, &[0.25, 0.5])],
        )
    };
    let file = captured(&make(), 0, None);
    let opts = OfflineOptions {
        command_capacity: 1,
        ..options(4, 2)
    };
    let mut legacy = Sink::new();
    assert_eq!(
        render_replay(
            make(),
            file.clone(),
            limits(),
            opts,
            Duration::ZERO,
            &mut legacy
        )
        .unwrap()
        .commands_admitted,
        1
    );
    let mut sink = Sink::new();
    let error = render_replay_with_rate(
        make(),
        file,
        limits(),
        opts,
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
        &mut sink,
    )
    .unwrap_err();
    let failure = error
        .downcast_ref::<OfflineError>()
        .expect("original typed admission error");
    assert!(failure.last_render.is_none());
    assert_eq!(failure.audio_failures.len(), 1);
    assert_eq!(failure.audio_failures[0].reason, QueuePushError::Full);
    assert!(
        matches!(failure.audio_failures[0].command,AudioCommand::Play{sample:SampleId(1),at,..} if at==Timestamp::ZERO)
    );
    assert_eq!((sink.writes, sink.flushes, sink.bytes.len()), (0, 0, 0));
}

struct PrefixFailure {
    bytes: Vec<u8>,
    remaining: usize,
    flushes: usize,
}
impl Write for PrefixFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("literal speed sink refusal"));
        }
        let n = self.remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..n]);
        self.remaining -= n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}
#[test]
fn writer_failure_retains_actual_render_report_and_partial_written_prefix() {
    let file = captured(&song(), 0, Some(4_000_000_001));
    let mut sink = PrefixFailure {
        bytes: vec![],
        remaining: 10,
        flushes: 0,
    };
    let error = render_replay_with_rate(
        song(),
        file,
        limits(),
        options(10, 2),
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
        &mut sink,
    )
    .unwrap_err();
    let failure = error
        .downcast_ref::<OfflineError>()
        .expect("original typed sink failure");
    assert!(failure.message.contains("literal speed sink refusal"));
    assert!(failure.audio_failures.is_empty());
    let last = failure.last_render.unwrap();
    assert_eq!(
        (last.start_frame, last.frames, last.active_voices),
        (2, 2, 1)
    );
    assert_eq!(last.counters.commands_applied, 3);
    let mut expected = Vec::new();
    for value in [0.0625_f32, 0.1875] {
        expected.extend_from_slice(&value.to_le_bytes());
    }
    expected.extend_from_slice(&0.25_f32.to_le_bytes()[..2]);
    assert_eq!(sink.bytes, expected);
    assert_eq!(sink.flushes, 0);
}

#[test]
fn invalid_rates_and_mapped_overflow_refuse_before_any_sink_effect_even_at_zero_cutoff() {
    let file = captured(&song(), 0, Some(4_000_000_001));
    for frames in [0, 1, 16] {
        for (rate, preroll) in [
            (Rate::ZERO, Duration::ZERO),
            (Rate::REVERSE, Duration::ZERO),
            (Rate::new(1, u64::MAX).unwrap(), Duration::ZERO),
            (Rate::new(2, 1).unwrap(), Duration::from_nanos(i64::MAX)),
        ] {
            let mut sink = Sink::new();
            assert!(
                render_replay_with_rate(
                    song(),
                    file.clone(),
                    limits(),
                    options(frames, 2),
                    preroll,
                    rate,
                    &mut sink
                )
                .is_err()
            );
            assert_eq!((sink.writes, sink.flushes, sink.bytes.len()), (0, 0, 0));
        }
    }
}

#[test]
fn rate_extension_preserves_legacy_finite_and_nondefault_gauge_refusal() {
    let p = song();
    let finite = captured_at(
        &p,
        0,
        Timestamp::ZERO,
        Some(Timestamp::from_nanos(4_000_000_000)),
        Some(4_000_000_000),
    );
    let gauge = GaugeProfile::new(1, 0, 0, -1, true, vec![]).unwrap();
    let policy = capture_events(
        &p,
        0,
        Timestamp::ZERO,
        None,
        &gauge,
        &[],
        Some(4_000_000_001),
    );
    for file in [finite, policy] {
        for rate in [Rate::NORMAL, Rate::new(2, 1).unwrap()] {
            let mut sink = Sink::new();
            assert!(
                render_replay_with_rate(
                    song(),
                    file.clone(),
                    limits(),
                    options(16, 2),
                    Duration::ZERO,
                    rate,
                    &mut sink
                )
                .is_err()
            );
            assert_eq!((sink.writes, sink.flushes, sink.bytes.len()), (0, 0, 0));
        }
    }
}

const FATAL_LEGACY: &str = "#BPM 60\n#VOLWAV 50\n#WAV00 blast.wav\n#WAV01 note.wav\n#WAV02 music.wav\n#00011:01000101\n#00031:02\n#000D1:00ZZ0000\n#000D2:00000001\n#00001:02000002\n";
fn fatal_song() -> PreparedBms {
    let music = [0.25; 40];
    let mut p = prepared(
        FATAL_LEGACY,
        10,
        &[(0, &[0.5, -0.5]), (1, &[1.0, -1.0]), (2, &music)],
    );
    for sound in &mut p.sounds {
        sound.gain = 0.5;
    }
    for command in &mut p.bgm_commands {
        if let AudioCommand::Play { gain, .. } = command {
            *gain = 0.5;
        }
    }
    p
}
fn fatal_recording() -> beatkernel::replay::codec::ReplayFile {
    use beatkernel::{
        audio::command_queue,
        input::{
            Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
            GameControlId, PhysicalControlId, PhysicalInputEvent,
        },
        judge::{JudgeGrade, JudgeProfile, JudgeWindow},
        runtime::Runtime,
        time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint},
        transport::Transport,
    };
    use beatkernel_bms::BmsInputMode;
    use beatkernel_bms_runtime::{
        input_sounds::InputSoundIdentity, mine_plan::prepare_judge,
        replay_capture::LiveReplayCapture,
    };
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
            (from.domain == to).then_some(from.timestamp)
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Exact
        }
    }
    let p = fatal_song();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(2),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )
    .unwrap();
    let judge = prepare_judge(
        &p.source,
        p.compiled.chart.clone(),
        profile,
        BmsInputMode::ButtonOnly,
        16,
    )
    .unwrap();
    let mut capture = LiveReplayCapture::new_with_input_sounds(
        &judge,
        DOMAIN,
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        InputSoundIdentity::from_source(&p.source).unwrap(),
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(1)),
        physical: PhysicalControlId::keyboard(91),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(1).unwrap();
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    // The actual advance crosses the fatal mine while lane 11 is held.
    // Later actual input still enters the legacy log after the gauge failure.
    for (sequence, (ns, state)) in [
        (0, Some(ButtonState::Down)),
        (1_000_000_000, None),
        (1_000_000_000, Some(ButtonState::Up)),
        (2_000_000_000, Some(ButtonState::Down)),
        (2_000_000_000, Some(ButtonState::Up)),
        (3_000_000_000, Some(ButtonState::Down)),
    ]
    .into_iter()
    .enumerate()
    {
        let report = if let Some(state) = state {
            runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(DeviceId(1), point(ns), sequence as u64),
                        control: PhysicalControlId::keyboard(91),
                        state,
                    }),
                    &Identity,
                    point(ns),
                )
                .unwrap()
        } else {
            runtime.advance_to(point(ns), &Identity, point(ns)).unwrap()
        };
        assert!(report.judge_error.is_none());
        if state.is_none() {
            assert_eq!(report.hazard_events[0].value, 1295);
        }
        capture.record_report(&report).unwrap();
    }
    capture.into_file()
}

#[test]
fn nonnormal_fatal_legacy_pcm_preserves_bgm_and_exact_owned_stop_diagnostics() {
    let file = fatal_recording();
    let original = plan_audio(
        &fatal_song(),
        file.clone(),
        limits(),
        point(0),
        Duration::ZERO,
    )
    .unwrap();
    assert_eq!(original.judge_events.len(), 3);
    // At 2x, the short key reads source frame 0 then ends; its original
    // negative source frame is skipped. Fatal Stops execute at output frame 5.
    // The independent BGM continues, and its second cue starts at frame 15.
    for block in [1, 3, 7, 64] {
        let mut sink = Sink::new();
        let report = render_replay_with_rate(
            fatal_song(),
            file.clone(),
            limits(),
            OfflineOptions {
                command_capacity: 16,
                max_voices: 16,
                ..options(19, block)
            },
            Duration::ZERO,
            Rate::new(2, 1).unwrap(),
            &mut sink,
        )
        .unwrap();
        let mut expected = vec![0.125; 19];
        expected[0] = 0.625;
        expected[15..].fill(0.25);
        assert_eq!(floats(&sink.bytes), expected);
        assert_eq!(
            (
                report.frames,
                report.hits,
                report.judge_results,
                report.commands_admitted
            ),
            (19, 3, 3, 9)
        );
        assert_eq!(
            report.recorded_until,
            Some(Timestamp::from_nanos(3_000_000_000))
        );
        assert_eq!(report.final_judge_hash, original.final_judge_hash);
        let last = report.last_render.unwrap();
        assert_eq!(
            last.counters.commands_applied, 9,
            "one control, two BGM, one gameplay Play, five Stops"
        );
        assert_eq!(
            last.counters.unknown_stops, 5,
            "only the five actual owned Stops purchase diagnostic allowance"
        );
        assert_eq!(
            (
                last.counters.unknown_samples,
                last.counters.voice_full,
                last.counters.late_commands
            ),
            (0, 0, 0)
        );
        assert_eq!(sink.flushes, 0);
    }
}

#[test]
fn nonnormal_partial_stop_admission_refuses_original_stop_and_retains_executed_prefix() {
    let file = fatal_recording();
    // Three origin commands fit exactly. The fourth of five Stops at frame 5
    // fails in that same bounded queue; accepted Stops have not executed yet.
    let mut sink = Sink::new();
    let error = render_replay_with_rate(
        fatal_song(),
        file,
        limits(),
        OfflineOptions {
            command_capacity: 3,
            max_voices: 16,
            ..options(19, 3)
        },
        Duration::ZERO,
        Rate::new(2, 1).unwrap(),
        &mut sink,
    )
    .unwrap_err();
    let failure = error.downcast_ref::<OfflineError>().unwrap();
    assert_eq!(floats(&sink.bytes), [0.625, 0.125, 0.125, 0.125, 0.125]);
    assert_eq!(failure.audio_failures.len(), 1);
    assert_eq!(failure.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(
        failure.audio_failures[0].command,
        AudioCommand::Stop {
            voice: beatkernel::audio::VoiceId(6),
            at: Timestamp::from_nanos(500_000_000)
        }
    );
    let last = failure.last_render.unwrap();
    assert_eq!(last.start_frame + last.frames as u64, 5);
    assert_eq!(last.counters.commands_applied, 3);
    assert_eq!(last.counters.unknown_stops, 0);
    assert_eq!(sink.flushes, 0);
}
