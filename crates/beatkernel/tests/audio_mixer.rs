use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
    transport::Rate,
};

fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn bounds() -> AudioLimits {
    AudioLimits::new(64, 8, 64, 256, 64).unwrap()
}
fn rig(
    output: AudioFormat,
    source_rate: u32,
    samples: &[f32],
    limits: AudioLimits,
    origin: i64,
) -> (CommandProducer, Mixer) {
    let pcm_limits = PcmLimits::new(4096, 8192, 4).unwrap();
    let mut bank = SampleBank::new(output, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            AudioFormat::new(source_rate, output.channels()).unwrap(),
            samples.to_vec(),
            pcm_limits,
        )
        .unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(limits.queue_capacity()).unwrap();
    let config = MixerConfig::new(output, ClockDomainId(71), ts(origin), limits);
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn mono(samples: &[f32]) -> (CommandProducer, Mixer) {
    rig(
        AudioFormat::new(1000, 1).unwrap(),
        1000,
        samples,
        bounds(),
        0,
    )
}
fn play(voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(1),
        at: ts(at),
        gain,
    }
}
fn set_rate(rate: Rate, at: i64) -> AudioCommand {
    AudioCommand::SetRate { rate, at: ts(at) }
}
fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(at),
    }
}
fn seek(at: i64) -> AudioCommand {
    AudioCommand::Seek {
        song_time: ts(-123),
        at: ts(at),
    }
}

#[test]
fn pause_preserves_fractional_voices_future_commands_and_song_anchor() {
    fn setup() -> (CommandProducer, Mixer) {
        let (mut producer, mixer) = mono(&[0.0, 0.2, 0.4, 0.6, 0.8, 0.9]);
        for command in [
            seek(0),
            set_rate(Rate::new(1, 2).unwrap(), 0),
            play(1, 0, 1.0),
            play(2, 5_000_000, 0.5),
            stop(1, 6_000_000),
        ] {
            producer.try_push(command).unwrap();
        }
        (producer, mixer)
    }
    let (_baseline_producer, mut baseline) = setup();
    let mut expected = [0.0; 7];
    baseline.render(&mut expected).unwrap();
    let (mut producer, mut mixer) = setup();
    let mut actual = [0.0; 7];
    let before = mixer.render(&mut actual[..3]).unwrap();
    assert_eq!(before.pending_commands, 2);
    assert_eq!(before.song_position, ts(-123));
    assert_eq!(mixer.playback_frame_cursor(), 3);
    let admissions = producer.counters();
    producer.request_pause(true);
    assert_eq!(producer.counters(), admissions);
    assert!(!mixer.is_paused());
    let mut silence = [99.0; 11];
    let paused = mixer.render(&mut silence).unwrap();
    assert_eq!(silence, [0.0; 11]);
    assert_eq!((paused.start_frame, paused.frames), (3, 11));
    assert_eq!(
        (
            paused.playback_start_frame,
            paused.playback_frames,
            paused.paused
        ),
        (3, 0, true)
    );
    assert_eq!(paused.active_voices, before.active_voices);
    assert_eq!(paused.pending_commands, before.pending_commands);
    assert_eq!(paused.song_position, before.song_position);
    assert_eq!(
        paused.counters.commands_consumed,
        before.counters.commands_consumed
    );
    assert_eq!(
        paused.counters.commands_applied,
        before.counters.commands_applied
    );
    assert_eq!(paused.counters.late_commands, before.counters.late_commands);
    assert_eq!(mixer.rate(), Rate::new(1, 2).unwrap());
    producer.request_pause(false);
    let resumed = mixer.render(&mut actual[3..]).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        (
            resumed.start_frame,
            resumed.playback_start_frame,
            resumed.playback_frames,
            resumed.paused
        ),
        (14, 3, 4, false)
    );
    assert_eq!(mixer.frame_cursor(), 18);
    assert_eq!(mixer.playback_frame_cursor(), 7);
    assert_eq!(resumed.counters.late_commands, 0);
}

#[test]
fn independent_pause_and_resume_remain_reachable_with_a_full_ring() {
    let limits = AudioLimits::new(1, 2, 2, 16, 1).unwrap();
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 1).unwrap(),
        1000,
        &[0.25; 8],
        limits,
        0,
    );
    producer.try_push(play(1, 0, 1.0)).unwrap();
    producer.request_pause(true);
    assert_eq!(
        producer.try_push(stop(1, 1_000_000)).unwrap_err().reason,
        QueuePushError::Full
    );
    let counters = producer.counters();
    let paused = mixer.render(&mut [99.0; 4]).unwrap();
    assert_eq!(paused.counters.commands_consumed, 0);
    assert_eq!(paused.active_voices, 0);
    producer.request_pause(false);
    assert_eq!(producer.counters(), counters);
    let mut output = [99.0];
    let resumed = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    assert_eq!(resumed.counters.commands_consumed, 1);
    assert_eq!((resumed.start_frame, resumed.playback_start_frame), (4, 0));
    producer.try_push(stop(1, 1_000_000)).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [99.0; 3]).unwrap();
    producer.request_pause(false);
    let resumed = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0]);
    assert_eq!(resumed.counters.commands_applied, 2);
    assert_eq!(resumed.counters.late_commands, 0);
}

#[test]
fn rate_zero_keeps_playback_scheduling_active_and_pause_requests_can_coalesce() {
    let (mut producer, mut mixer) = mono(&[0.25; 8]);
    for command in [set_rate(Rate::ZERO, 0), play(1, 0, 1.0), stop(1, 2_000_000)] {
        producer.try_push(command).unwrap();
    }
    producer.request_pause(true);
    producer.request_pause(false);
    let mut output = [99.0; 3];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 3]);
    assert!(!report.paused);
    assert_eq!(report.playback_frames, 3);
    assert_eq!(mixer.playback_frame_cursor(), 3);
    assert_eq!(report.active_voices, 0);
    assert_eq!(report.counters.commands_applied, 3);
    assert_eq!(mixer.rate(), Rate::ZERO);
}

#[test]
fn invalid_and_empty_blocks_preserve_output_queue_state_and_pending_requests() {
    let limits = AudioLimits::new(4, 2, 4, 2, 4).unwrap();
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 2).unwrap(),
        1000,
        &[0.25; 8],
        limits,
        0,
    );
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let original = mixer.render(&mut []).unwrap();
    producer.request_pause(true);
    let mut misaligned = [99.0; 3];
    assert_eq!(
        mixer.render(&mut misaligned),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(misaligned, [99.0; 3]);
    let mut large = [99.0; 6];
    assert_eq!(mixer.render(&mut large), Err(AudioError::RenderCapacity));
    assert_eq!(large, [99.0; 6]);
    assert_eq!(mixer.render(&mut []).unwrap(), original);
    assert!(!mixer.is_paused());
    let mut output = [99.0; 2];
    let paused = mixer.render(&mut output).unwrap();
    assert!(mixer.is_paused());
    assert_eq!(paused.counters.commands_consumed, 0);
    producer.request_pause(false);
    let empty = mixer.render(&mut []).unwrap();
    assert!(empty.paused);
    assert_eq!(empty.counters, paused.counters);
    assert_eq!(empty.pending_commands, paused.pending_commands);
    assert_eq!(
        mixer.render(&mut misaligned),
        Err(AudioError::InvalidBuffer)
    );
    assert!(mixer.is_paused());
    let resumed = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25; 2]);
    assert!(!resumed.paused);
    assert_eq!(resumed.counters.commands_consumed, 1);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (2, 1)
    );
}

#[test]
fn pause_silence_and_active_pcm_are_partition_invariant_on_the_playback_grid() {
    fn render(parts: &[usize], pauses: &[usize]) -> (Vec<f32>, AudioCounters, u64, u64) {
        let (mut producer, mut mixer) = mono(&[0.0, 0.2, 0.4, 0.6, 0.8, 0.9]);
        for command in [
            set_rate(Rate::new(1, 2).unwrap(), 0),
            play(1, 0, 1.0),
            play(2, 5_000_000, 0.5),
            stop(1, 6_000_000),
        ] {
            producer.try_push(command).unwrap();
        }
        let mut active = vec![0.0; 3];
        mixer.render(&mut active).unwrap();
        producer.request_pause(true);
        for &frames in pauses {
            let mut silence = vec![99.0; frames];
            let report = mixer.render(&mut silence).unwrap();
            assert_eq!(silence, vec![0.0; frames]);
            assert_eq!(report.playback_start_frame, 3);
            assert_eq!(report.playback_frames, 0);
        }
        producer.request_pause(false);
        for &frames in parts {
            let mut output = vec![99.0; frames];
            mixer.render(&mut output).unwrap();
            active.extend(output);
        }
        (
            active,
            mixer.counters(),
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
        )
    }
    let whole = render(&[4], &[11]);
    let split = render(&[1, 2, 1], &[2, 3, 6]);
    assert_eq!(whole, split);
    assert_eq!((whole.2, whole.3), (18, 7));
    assert_eq!(whole.1.late_commands, 0);
}

#[test]
fn constructor_requires_exact_bank_format_and_queue_capacity() {
    let limits = bounds();
    let output = AudioFormat::new(48_000, 2).unwrap();
    let config = MixerConfig::new(output, ClockDomainId(7), ts(9), limits);
    for (bank_format, error) in [
        (
            AudioFormat::new(44_100, 2).unwrap(),
            AudioError::InvalidFormat,
        ),
        (
            AudioFormat::new(48_000, 1).unwrap(),
            AudioError::ChannelMismatch,
        ),
    ] {
        let bank = SampleBank::new(bank_format, PcmLimits::new(4, 4, 1).unwrap()).unwrap();
        let (_producer, consumer) = command_queue(limits.queue_capacity()).unwrap();
        assert!(matches!(Mixer::new(config, bank, consumer), Err(actual) if actual == error));
    }
    let bank = SampleBank::new(output, PcmLimits::new(4, 4, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(1).unwrap();
    assert!(matches!(
        Mixer::new(config, bank, consumer),
        Err(AudioError::InvalidCapacity)
    ));
}

#[test]
fn empty_output_preserves_queue_cursor_rate_and_initial_report() {
    let (mut producer, mut mixer) = mono(&[0.25]);
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let config = mixer.config();
    let report = mixer.render(&mut []).unwrap();
    assert_eq!(
        report,
        RenderReport {
            start_frame: 0,
            frames: 0,
            playback_start_frame: 0,
            playback_frames: 0,
            paused: false,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        }
    );
    assert_eq!(mixer.frame_cursor(), 0);
    assert_eq!(mixer.rate(), Rate::NORMAL);
    assert_eq!(mixer.config(), config);
    let mut output = [99.0];
    assert_eq!(
        mixer
            .render(&mut output)
            .unwrap()
            .counters
            .commands_consumed,
        1
    );
    assert_eq!(output, [0.25]);
}

#[test]
fn ceil_grid_places_every_block_edge_and_fractional_neighbor_exactly() {
    for (at, frame) in [
        (0, 0),
        (1, 1),
        (999_999, 1),
        (1_000_000, 1),
        (1_000_001, 2),
        (3_000_000, 3),
        (4_000_000, 4),
    ] {
        let (mut producer, mut mixer) = mono(&[0.5]);
        producer.try_push(play(1, at, 1.0)).unwrap();
        let mut output = [99.0; 4];
        let report = mixer.render(&mut output).unwrap();
        let mut expected = [0.0; 4];
        if frame < 4 {
            expected[frame] = 0.5;
        }
        assert_eq!(output, expected, "at {at}");
        assert_eq!(report.pending_commands, usize::from(frame == 4));
        let mut next = [99.0];
        assert_eq!(mixer.render(&mut next).unwrap().start_frame, 4);
        assert_eq!(next, [if frame == 4 { 0.5 } else { 0.0 }]);
    }
    for (rate, at, frame) in [
        (44_100, 68_027, 3),
        (44_100, 68_028, 4),
        (48_000, 62_499, 3),
        (48_000, 62_500, 3),
        (48_000, 62_501, 4),
    ] {
        let origin = 10_000_000_123;
        let (mut producer, mut mixer) = rig(
            AudioFormat::new(rate, 1).unwrap(),
            rate,
            &[0.5],
            bounds(),
            origin,
        );
        producer.try_push(play(1, origin + at, 1.0)).unwrap();
        let mut output = [99.0; 6];
        mixer.render(&mut output).unwrap();
        let mut expected = [0.0; 6];
        expected[frame] = 0.5;
        assert_eq!(output, expected, "rate {rate}, relative timestamp {at}");
    }
}

#[test]
fn future_submission_cannot_block_earlier_play_and_late_commands_count() {
    let (mut producer, mut mixer) = mono(&[0.25]);
    producer.try_push(play(1, 3_000_000, 1.0)).unwrap();
    producer.try_push(play(2, -1_000_000, 1.0)).unwrap();
    let mut output = [99.0; 4];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25, 0.0, 0.0, 0.25]);
    assert_eq!(report.counters.commands_applied, 2);
    assert_eq!(report.counters.late_commands, 1);
    producer.try_push(play(3, 0, 1.0)).unwrap();
    let mut next = [99.0];
    let report = mixer.render(&mut next).unwrap();
    assert_eq!(next, [0.25]);
    assert_eq!(report.counters.late_commands, 2);
    assert_eq!(report.start_frame, 4);
}

#[test]
fn equal_frame_play_stop_order_is_submission_order() {
    for play_first in [true, false] {
        let (mut producer, mut mixer) = mono(&[0.5, 0.5]);
        let commands = if play_first {
            [play(1, 0, 1.0), stop(1, 0)]
        } else {
            [stop(1, 0), play(1, 0, 1.0)]
        };
        for command in commands {
            producer.try_push(command).unwrap();
        }
        let mut output = [99.0];
        let report = mixer.render(&mut output).unwrap();
        assert_eq!(output, [if play_first { 0.0 } else { 0.5 }]);
        assert_eq!(report.counters.unknown_stops, u64::from(!play_first));
    }
}

#[test]
fn pending_full_rejects_new_commands_preserves_future_and_continues_budget() {
    let limits = AudioLimits::new(4, 2, 1, 16, 4).unwrap();
    let (mut producer, mut mixer) =
        rig(AudioFormat::new(1000, 1).unwrap(), 1000, &[0.5], limits, 0);
    producer.try_push(play(1, 4_000_000, 1.0)).unwrap();
    producer.try_push(play(2, 0, 1.0)).unwrap();
    producer.try_push(stop(1, 0)).unwrap();
    let mut output = [99.0; 4];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 4]);
    assert_eq!(report.pending_commands, 1);
    assert_eq!(report.counters.commands_consumed, 3);
    assert_eq!(report.counters.pending_full, 2);
    let mut next = [99.0];
    let report = mixer.render(&mut next).unwrap();
    assert_eq!(next, [0.5]);
    assert_eq!(report.pending_commands, 0);
}

#[test]
fn drain_budget_is_bounded_and_leaves_later_queue_commands_for_next_render() {
    let limits = AudioLimits::new(4, 4, 4, 16, 2).unwrap();
    let (mut producer, mut mixer) =
        rig(AudioFormat::new(1000, 1).unwrap(), 1000, &[0.25], limits, 0);
    for voice in 1..=3 {
        producer.try_push(play(voice, 0, 1.0)).unwrap();
    }
    let mut output = [99.0];
    let first = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.5]);
    assert_eq!(first.counters.commands_consumed, 2);
    let second = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    assert_eq!(second.counters.commands_consumed, 3);
    assert_eq!(second.counters.late_commands, 1);
}

#[test]
fn voice_capacity_duplicate_replacement_and_invalid_commands_are_atomic() {
    let limits = AudioLimits::new(16, 1, 16, 16, 16).unwrap();
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 1).unwrap(),
        1000,
        &[0.25; 8],
        limits,
        0,
    );
    producer.try_push(play(1, 0, 1.0)).unwrap();
    producer.try_push(play(2, 0, 1.0)).unwrap();
    let mut output = [99.0];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    assert_eq!(report.counters.voice_full, 1);
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(99),
            at: ts(1_000_000),
            gain: 1.0,
        })
        .unwrap();
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        producer.try_push(play(1, 1_000_000, gain)).unwrap();
    }
    producer.try_push(stop(99, 1_000_000)).unwrap();
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    assert_eq!(report.counters.unknown_samples, 1);
    assert_eq!(report.counters.invalid_gains, 3);
    assert_eq!(report.counters.unknown_stops, 1);
    producer.try_push(play(1, 2_000_000, -2.0)).unwrap();
    assert_eq!(mixer.render(&mut output).unwrap().active_voices, 1);
    assert_eq!(output, [-0.5]);
}

#[test]
fn gains_sum_in_wide_fixed_order_and_clamp_once() {
    let (mut producer, mut mixer) = mono(&[0.75]);
    for (voice, gain) in [(1, 1.0), (2, 1.0), (3, -1.0)] {
        producer.try_push(play(voice, 0, gain)).unwrap();
    }
    let mut output = [99.0];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.75]);
    let (mut producer, mut mixer) = mono(&[1.0]);
    for (voice, gain) in [
        (1, f32::MAX),
        (2, f32::MAX),
        (3, -f32::MAX),
        (4, -f32::MAX),
        (5, 0.25),
    ] {
        producer.try_push(play(voice, 0, gain)).unwrap();
    }
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    for gain in [2.0, -2.0] {
        let (mut producer, mut mixer) = mono(&[0.75]);
        producer.try_push(play(1, 0, gain)).unwrap();
        mixer.render(&mut output).unwrap();
        assert_eq!(output, [gain.signum()]);
    }
}

#[test]
fn forward_reverse_and_zero_start_and_retire_at_sample_boundaries() {
    for (rate, expected) in [
        (Rate::NORMAL, [0.0, 0.25, 0.5, 0.75, 0.0]),
        (Rate::REVERSE, [0.75, 0.5, 0.25, 0.0, 0.0]),
    ] {
        let (mut producer, mut mixer) = mono(&[0.0, 0.25, 0.5, 0.75]);
        producer.try_push(set_rate(rate, 0)).unwrap();
        producer.try_push(play(1, 0, 1.0)).unwrap();
        let mut output = [99.0; 5];
        let report = mixer.render(&mut output).unwrap();
        assert_eq!(output, expected);
        assert_eq!(report.active_voices, 0);
    }
    let (mut producer, mut mixer) = mono(&[0.25, 0.5]);
    producer.try_push(set_rate(Rate::ZERO, 0)).unwrap();
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let mut silence = [99.0; 3];
    assert_eq!(mixer.render(&mut silence).unwrap().active_voices, 1);
    assert_eq!(silence, [0.0; 3]);
    producer
        .try_push(set_rate(Rate::NORMAL, 3_000_000))
        .unwrap();
    let mut resumed = [99.0; 3];
    mixer.render(&mut resumed).unwrap();
    assert_eq!(resumed, [0.25, 0.5, 0.0]);
}

#[test]
fn source_rate_interpolates_and_direction_changes_preserve_fractional_head() {
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 1).unwrap(),
        500,
        &[0.0, 0.25, 0.5, 0.75],
        bounds(),
        0,
    );
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let mut output = [99.0; 9];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(
        output,
        [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.75, 0.0]
    );
    assert_eq!(report.active_voices, 0);
    let (mut producer, mut mixer) = mono(&[0.0, 0.25, 0.5, 0.75]);
    producer
        .try_push(set_rate(Rate::new(1, 2).unwrap(), 0))
        .unwrap();
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let mut first = [99.0; 3];
    mixer.render(&mut first).unwrap();
    assert_eq!(first, [0.0, 0.125, 0.25]);
    producer.try_push(set_rate(Rate::ZERO, 3_000_000)).unwrap();
    let mut paused = [99.0; 2];
    mixer.render(&mut paused).unwrap();
    assert_eq!(paused, [0.0, 0.0]);
    producer
        .try_push(set_rate(Rate::new(-1, 2).unwrap(), 5_000_000))
        .unwrap();
    let mut reversed = [99.0; 5];
    mixer.render(&mut reversed).unwrap();
    assert_eq!(reversed, [0.375, 0.25, 0.125, 0.0, 0.0]);
}

#[test]
fn seek_clears_voices_keeps_rate_and_future_work_and_respects_equal_order() {
    for seek_first in [true, false] {
        let (mut producer, mut mixer) = mono(&[0.25; 8]);
        producer
            .try_push(set_rate(Rate::new(1, 2).unwrap(), 0))
            .unwrap();
        producer.try_push(play(99, 0, 1.0)).unwrap();
        producer.try_push(play(2, 4_000_000, 1.0)).unwrap();
        let commands = if seek_first {
            [seek(2_000_000), play(1, 2_000_000, 1.0)]
        } else {
            [play(1, 2_000_000, 1.0), seek(2_000_000)]
        };
        for command in commands {
            producer.try_push(command).unwrap();
        }
        let mut output = [99.0; 5];
        let report = mixer.render(&mut output).unwrap();
        assert_eq!(
            output,
            if seek_first {
                [0.25, 0.25, 0.25, 0.25, 0.5]
            } else {
                [0.25, 0.25, 0.0, 0.0, 0.25]
            }
        );
        assert_eq!(report.song_position, ts(-123));
        assert_eq!(mixer.rate(), Rate::new(1, 2).unwrap());
        assert_eq!(mixer.frame_cursor(), 5);
    }
}

#[test]
fn rational_phase_and_scheduler_are_partition_invariant_at_44100_and_48000() {
    for rate in [44_100, 48_000] {
        let origin = 10_000_000_123;
        let run = |parts: &[usize]| {
            let (mut producer, mut mixer) = rig(
                AudioFormat::new(rate, 1).unwrap(),
                rate,
                &[0.0, 0.25, 0.5, 0.75],
                bounds(),
                origin,
            );
            producer
                .try_push(set_rate(Rate::new(1, 3).unwrap(), origin))
                .unwrap();
            producer.try_push(play(1, origin, 1.0)).unwrap();
            let mut result = Vec::new();
            for &frames in parts {
                let mut block = vec![99.0; frames];
                mixer.render(&mut block).unwrap();
                result.extend(block);
            }
            (result, mixer.counters(), mixer.frame_cursor())
        };
        let expected = vec![
            0.0,
            1.0 / 12.0,
            1.0 / 6.0,
            0.25,
            1.0 / 3.0,
            5.0 / 12.0,
            0.5,
            7.0 / 12.0,
            2.0 / 3.0,
            0.75,
            0.75,
            0.75,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let whole = run(&[16]);
        assert_eq!(whole.0, expected);
        assert_eq!(run(&[1; 16]), whole);
        assert_eq!(run(&[3, 1, 5, 2, 5]), whole);
    }
}

#[test]
fn extreme_rates_are_supported_and_actual_rational_overflow_rejects_atomically() {
    for (rate, first) in [
        (Rate::new(i64::MAX, 1).unwrap(), 0.25),
        (Rate::new(i64::MIN, 1).unwrap(), 0.75),
    ] {
        let (mut producer, mut mixer) = mono(&[0.25, 0.5, 0.75]);
        producer.try_push(set_rate(rate, 0)).unwrap();
        producer.try_push(play(1, 0, 1.0)).unwrap();
        let mut output = [99.0; 2];
        let report = mixer.render(&mut output).unwrap();
        assert_eq!(output, [first, 0.0]);
        assert_eq!(mixer.rate(), rate);
        assert_eq!(report.counters.invalid_rates, 0);
    }
    let (mut producer, mut mixer) = mono(&[0.0, 0.25, 0.5, 0.75]);
    let extreme = Rate::new(i64::MIN, u64::MAX).unwrap();
    producer.try_push(set_rate(extreme, 0)).unwrap();
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let mut output = [99.0; 4];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.75, 0.625, 0.5, 0.375]);
    assert_eq!(mixer.rate(), extreme);
    let (mut producer, mut mixer) = mono(&[0.25; 8]);
    let retained = Rate::new(1, u64::MAX).unwrap();
    producer.try_push(set_rate(retained, 0)).unwrap();
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let mut one = [99.0];
    mixer.render(&mut one).unwrap();
    producer
        .try_push(set_rate(Rate::new(1, u64::MAX - 2).unwrap(), 1_000_000))
        .unwrap();
    producer.try_push(play(2, 1_000_000, 1.0)).unwrap();
    let report = mixer.render(&mut one).unwrap();
    assert_eq!(one, [0.5]);
    assert_eq!(report.counters.invalid_rates, 1);
    assert_eq!(mixer.rate(), retained);
    assert_eq!(report.active_voices, 2);
}

#[test]
fn invalid_render_preserves_buffer_queue_and_all_mixer_state() {
    let limits = AudioLimits::new(4, 2, 4, 2, 4).unwrap();
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 2).unwrap(),
        1000,
        &[0.25, -0.5],
        limits,
        0,
    );
    producer.try_push(play(1, 0, 1.0)).unwrap();
    let mut unaligned = [99.0; 3];
    assert_eq!(mixer.render(&mut unaligned), Err(AudioError::InvalidBuffer));
    assert_eq!(unaligned, [99.0; 3]);
    let mut oversized = [99.0; 6];
    assert_eq!(
        mixer.render(&mut oversized),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(oversized, [99.0; 6]);
    assert_eq!(mixer.frame_cursor(), 0);
    assert_eq!(mixer.counters(), AudioCounters::default());
    assert_eq!(mixer.rate(), Rate::NORMAL);
    let mut valid = [99.0; 2];
    let report = mixer.render(&mut valid).unwrap();
    assert_eq!(valid, [0.25, -0.5]);
    assert_eq!(report.counters.commands_consumed, 1);
}

#[test]
fn preroll_subframe_is_late_and_late_commands_keep_original_target_order() {
    let (mut producer, mut mixer) = mono(&[0.25; 8]);
    producer.try_push(play(1, -1, 1.0)).unwrap();
    let mut one = [99.0];
    assert_eq!(mixer.render(&mut one).unwrap().counters.late_commands, 1);
    assert_eq!(one, [0.25]);
    let (mut producer, mut mixer) = mono(&[0.25; 8]);
    mixer.render(&mut [0.0; 3]).unwrap();
    producer.try_push(play(1, 1_000_000, 1.0)).unwrap();
    producer.try_push(seek(0)).unwrap();
    let report = mixer.render(&mut one).unwrap();
    assert_eq!(one, [0.25]);
    assert_eq!(report.song_position, ts(-123));
    assert_eq!(report.counters.late_commands, 2);
}

#[test]
fn fractional_scheduled_commands_match_literal_pcm_across_callback_partitions() {
    for (rate, boundary, frame6, frame8, frame10, frame12, frame14) in [
        (44_100, 68_027, 136_054, 181_405, 226_757, 272_108, 317_460),
        (48_000, 62_500, 125_000, 166_666, 208_333, 250_000, 291_666),
    ] {
        let origin = 10_000_000_123;
        let run = |parts: &[usize]| {
            let (mut producer, mut mixer) = rig(
                AudioFormat::new(rate, 1).unwrap(),
                rate,
                &[0.25; 16],
                bounds(),
                origin,
            );
            // Later targets arrive first. Three neighboring nanosecond values
            // straddle the frame-three boundary; other commands cross blocks.
            for command in [
                play(3, origin + boundary + 1, 1.0),
                stop(1, origin + frame6),
                set_rate(Rate::new(1, 2).unwrap(), origin),
                play(1, origin + boundary - 1, 1.0),
                play(2, origin + boundary, 1.0),
                seek(origin + frame8),
                play(4, origin + frame8, 1.0),
                set_rate(Rate::ZERO, origin + frame10),
                set_rate(Rate::NORMAL, origin + frame12),
                stop(4, origin + frame14),
            ] {
                producer.try_push(command).unwrap();
            }
            let mut output = Vec::new();
            for &frames in parts {
                let mut block = vec![99.0; frames];
                mixer.render(&mut block).unwrap();
                output.extend(block);
            }
            (output, mixer.counters(), mixer.rate(), mixer.frame_cursor())
        };
        let whole = run(&[20]);
        assert_eq!(
            whole.0,
            vec![
                0.0, 0.0, 0.0, 0.5, 0.75, 0.75, 0.5, 0.5, 0.25, 0.25, 0.0, 0.0, 0.25, 0.25, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0
            ]
        );
        assert_eq!(whole.1.commands_consumed, 10);
        assert_eq!(whole.1.commands_applied, 10);
        assert_eq!(whole.1.late_commands, 0);
        assert_eq!(whole.2, Rate::NORMAL);
        assert_eq!(whole.3, 20);
        assert_eq!(run(&[1; 20]), whole);
        assert_eq!(run(&[3, 1, 2, 2, 1, 1, 2, 2, 6]), whole);
    }
}

#[test]
fn maximum_origin_does_not_impose_a_nanosecond_end_limit_on_output_frames() {
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 1).unwrap(),
        1000,
        &[0.25, 0.5],
        bounds(),
        i64::MAX,
    );
    producer.try_push(play(1, i64::MAX, 1.0)).unwrap();
    let mut output = [99.0; 3];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25, 0.5, 0.0]);
    assert_eq!(mixer.frame_cursor(), 3);
}

#[test]
fn unrepresentable_command_time_is_counted_and_following_play_still_runs() {
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(u32::MAX, 1).unwrap(),
        u32::MAX,
        &[0.5],
        bounds(),
        i64::MIN,
    );
    producer.try_push(play(99, i64::MAX, 1.0)).unwrap();
    producer.try_push(play(1, i64::MIN, 1.0)).unwrap();
    let mut output = [99.0];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.5]);
    assert_eq!(report.counters.invalid_times, 1);
    assert_eq!(report.counters.commands_consumed, 2);
}

#[test]
fn bounded_admission_budget_makes_queued_stop_callback_partition_visible() {
    let run = |parts: &[usize]| {
        let limits = AudioLimits::new(2, 1, 2, 8, 1).unwrap();
        let (mut producer, mut mixer) = rig(
            AudioFormat::new(1000, 1).unwrap(),
            1000,
            &[0.25, 0.5, 0.75, 1.0],
            limits,
            0,
        );
        producer.try_push(play(1, 0, 1.0)).unwrap();
        producer.try_push(stop(1, 0)).unwrap();
        let mut output = Vec::new();
        for &frames in parts {
            let mut block = vec![99.0; frames];
            mixer.render(&mut block).unwrap();
            output.extend(block);
        }
        (output, mixer.counters())
    };
    let whole = run(&[4]);
    let split = run(&[2, 2]);
    assert_eq!(whole.0, [0.25, 0.5, 0.75, 1.0]);
    assert_eq!(whole.1.commands_consumed, 1);
    assert_eq!(whole.1.late_commands, 0);
    assert_eq!(split.0, [0.25, 0.5, 0.0, 0.0]);
    assert_eq!(split.1.commands_consumed, 2);
    assert_eq!(split.1.late_commands, 1);
}

#[test]
fn unequal_44100_to_48000_stereo_resampling_matches_rational_oracle_and_partitions() {
    let samples: Vec<f32> = (0..32)
        .flat_map(|frame| [frame as f32 / 64.0, -(frame as f32) / 128.0])
        .collect();
    let run = |parts: &[usize]| {
        let origin = 9_000_000_123;
        let (mut producer, mut mixer) = rig(
            AudioFormat::new(48_000, 2).unwrap(),
            44_100,
            &samples,
            bounds(),
            origin,
        );
        producer.try_push(play(1, origin, 1.0)).unwrap();
        let mut output = Vec::new();
        for &frames in parts {
            let mut block = vec![99.0; frames * 2];
            mixer.render(&mut block).unwrap();
            output.extend(block);
        }
        (output, mixer.counters())
    };
    // The independent linear ramp has slope 1/64 left and -1/128 right.
    // Source position is n*147/160; the final upper neighbor is held.
    let expected: Vec<f32> = (0..40)
        .flat_map(|frame| {
            let numerator = frame * 147;
            if numerator >= 32 * 160 {
                [0.0, 0.0]
            } else {
                let position = f64::from(numerator.min(31 * 160)) / 160.0;
                [(position / 64.0) as f32, (-position / 128.0) as f32]
            }
        })
        .collect();
    let whole = run(&[40]);
    assert_eq!(whole.0, expected);
    assert_eq!(run(&[1; 40]), whole);
    assert_eq!(run(&[7, 2, 11, 1, 19]), whole);
}

#[test]
fn rate_rejection_preflights_all_live_voices_before_changing_any_head() {
    let retained = Rate::new(1, u64::MAX).unwrap();
    let build = |reject: bool| {
        let format = AudioFormat::new(1, 1).unwrap();
        let pcm_limits = PcmLimits::new(128, 256, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        // 65535 divides u64::MAX: the first live head's reduced denominator
        // accepts the new common denominator, while the second head cannot.
        bank.insert(
            SampleId(1),
            PcmSample::new(
                AudioFormat::new(65_535, 1).unwrap(),
                vec![0.25, 0.5, 0.75],
                pcm_limits,
            )
            .unwrap(),
        )
        .unwrap();
        bank.insert(
            SampleId(2),
            PcmSample::new(format, vec![-0.125, -0.25, -0.375], pcm_limits).unwrap(),
        )
        .unwrap();
        let limits = bounds();
        let (mut producer, consumer) = command_queue(limits.queue_capacity()).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(71), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        producer.try_push(set_rate(retained, 0)).unwrap();
        producer.try_push(play(1, 0, 1.0)).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(2),
                sample: SampleId(2),
                at: ts(0),
                gain: 1.0,
            })
            .unwrap();
        let mut first = [99.0];
        mixer.render(&mut first).unwrap();
        assert_eq!(first, [0.125]);
        if reject {
            producer
                .try_push(set_rate(
                    Rate::new(i64::MAX, u64::MAX - 2).unwrap(),
                    1_000_000_000,
                ))
                .unwrap();
        }
        let mut output = [99.0; 2];
        let report = mixer.render(&mut output).unwrap();
        (output, report, mixer.rate())
    };
    let control = build(false);
    let rejected = build(true);
    assert_eq!(rejected.0, control.0);
    assert_eq!(rejected.0, [0.125, 0.125]);
    assert_eq!(rejected.1.active_voices, 2);
    assert_eq!(control.1.active_voices, 2);
    assert_eq!(rejected.2, retained);
    assert_eq!(rejected.1.counters.invalid_rates, 1);
    assert_eq!(
        rejected.1.counters.commands_applied,
        control.1.counters.commands_applied
    );
}

#[test]
fn applied_counter_distinguishes_handled_unknown_stop_from_rejected_commands() {
    let limits = AudioLimits::new(16, 1, 16, 8, 16).unwrap();
    let (mut producer, mut mixer) = rig(
        AudioFormat::new(1000, 1).unwrap(),
        1000,
        &[0.25; 4],
        limits,
        0,
    );
    for command in [
        play(1, 0, 1.0),
        stop(99, 0),
        play(2, 0, 1.0),
        play(1, 0, f32::NAN),
        AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(99),
            at: ts(0),
            gain: 1.0,
        },
        set_rate(Rate::NORMAL, 0),
    ] {
        producer.try_push(command).unwrap();
    }
    let mut output = [99.0];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    assert_eq!(report.counters.commands_consumed, 6);
    assert_eq!(report.counters.commands_applied, 3);
    assert_eq!(report.counters.unknown_stops, 1);
    assert_eq!(report.counters.voice_full, 1);
    assert_eq!(report.counters.invalid_gains, 1);
    assert_eq!(report.counters.unknown_samples, 1);
}

#[test]
fn empty_asset_is_inactive_and_disconnected_producer_does_not_drop_scheduled_work() {
    let (mut producer, mut mixer) = mono(&[]);
    producer.try_push(play(1, 0, 1.0)).unwrap();
    drop(producer);
    let mut output = [99.0; 2];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 2]);
    assert_eq!(report.active_voices, 0);
    assert!(report.producer_disconnected);
    let (mut producer, mut mixer) = mono(&[0.5]);
    producer.try_push(play(1, 2_000_000, 1.0)).unwrap();
    drop(producer);
    let mut output = [99.0; 3];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0, 0.5]);
    assert!(report.producer_disconnected);
    assert_eq!(report.pending_commands, 0);
}
