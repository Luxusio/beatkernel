//! Portable AudioWorklet owner fixtures using actual core Mixer output.
use crate::worklet_audio::{WorkletAudio, WorkletAudioBuilder, WorkletAudioConfig, WorkletAudioError};
use beatkernel::{
    audio::{AudioCommand, AudioFormat, AudioLimits, PcmLimits, QueuePushError, SampleId, VoiceId},
    time::Timestamp,
};

fn config(rate: u32, channels: u16, max_frames: usize, queue: usize) -> WorkletAudioConfig {
    WorkletAudioConfig {
        format: AudioFormat::new(rate, channels).unwrap(),
        pcm_limits: PcmLimits::new(1024, 4096, 8).unwrap(),
        audio_limits: AudioLimits::new(queue, 4, queue, max_frames, queue).unwrap(),
    }
}
fn audio(max_frames: usize, queue: usize) -> WorkletAudio {
    let mut builder = WorkletAudioBuilder::new(config(1000, 1, max_frames, queue)).unwrap();
    builder
        .insert_sample(
            SampleId(1),
            AudioFormat::new(1000, 1).unwrap(),
            vec![0.25, 0.5, 0.75, 1.0],
        )
        .unwrap();
    builder.finish().unwrap()
}
fn play(sample: u64, voice: u64, ns: i64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: Timestamp::from_nanos(ns),
        gain: 1.0,
    }
}

#[test]
fn finite_worklet_cutoff_is_relative_to_mixer_zero_and_independent_of_callback_partitions() {
    let base = (1u64 << 53) + 100;
    for parts in [vec![12], vec![3, 3, 6], vec![2, 3, 1, 6], vec![1; 12]] {
        let mut builder = WorkletAudioBuilder::new(config(1000, 1, 12, 2)).unwrap();
        builder
            .insert_sample(
                SampleId(1),
                AudioFormat::new(1000, 1).unwrap(),
                vec![0.25, 0.5, 0.75, 1.0],
            )
            .unwrap();
        let mut audio = builder.finish_at(3).unwrap();
        assert_eq!(audio.playback_end_frame(), Some(3));
        let pointer = audio.output_ptr();
        audio.enqueue(play(1, 1, 0)).unwrap();
        audio
            .enqueue(AudioCommand::Stop {
                voice: VoiceId(1),
                at: Timestamp::from_nanos(3_000_000),
            })
            .unwrap();
        let rejected = play(1, 2, 3_000_000);
        let full = audio.enqueue(rejected).unwrap_err();
        assert_eq!(
            (full.reason, full.command),
            (QueuePushError::Full, rejected)
        );
        audio.arm(base + 3, base).unwrap();
        let mut current = base;
        let mut pcm = Vec::new();
        for frames in parts {
            audio.render(current, frames).unwrap();
            pcm.extend_from_slice(&audio.output()[..frames]);
            current += frames as u64;
            assert_eq!(audio.output_ptr(), pointer);
            assert_eq!(audio.context_frame(), Some(current));
            if current <= base + 3 {
                assert!(audio.report().is_none());
            }
        }
        assert_eq!(
            pcm,
            [0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        let ended = audio.report().unwrap();
        assert!(ended.paused);
        assert_eq!(ended.playback_end_physical_frame, Some(3));
        assert_eq!(
            audio.start_frame().unwrap() + ended.playback_end_physical_frame.unwrap(),
            base + 6
        );
        assert_eq!(ended.counters.rendered_frames, 9);
        assert_eq!(
            (
                ended.counters.commands_consumed,
                ended.counters.commands_applied
            ),
            (2, 1)
        );
        assert_eq!(
            (ended.active_voices, ended.pending_commands),
            (1, 1),
            "the endpoint command is retained, not applied at an exclusive fence"
        );
        audio.enqueue(rejected).unwrap();
        audio.enqueue(play(1, 3, 4_000_000)).unwrap();
        audio.render(current, 3).unwrap();
        assert_eq!(&audio.output()[..3], &[0.0; 3]);
        let frozen = audio.report().unwrap();
        assert_eq!(
            (
                frozen.start_frame,
                frozen.playback_start_frame,
                frozen.playback_frames
            ),
            (9, 3, 0)
        );
        assert_eq!(frozen.playback_end_physical_frame, Some(3));
        assert_eq!(
            frozen.counters.commands_consumed,
            ended.counters.commands_consumed
        );
        assert_eq!(
            frozen.counters.commands_applied,
            ended.counters.commands_applied
        );
        assert_eq!(
            audio.enqueue(play(1, 4, 5_000_000)).unwrap_err().reason,
            QueuePushError::Full
        );
        assert_eq!(audio.context_frame(), Some(base + 15));
        assert!(!audio.failed());
    }
}

#[test]
fn zero_endpoint_and_checked_absolute_end_keep_queue_and_arm_ownership_without_changing_unlimited_finish()
 {
    let mut builder = WorkletAudioBuilder::new(config(1000, 1, 8, 1)).unwrap();
    builder
        .insert_sample(SampleId(1), AudioFormat::new(1000, 1).unwrap(), vec![0.5])
        .unwrap();
    let mut zero = builder.finish_at(0).unwrap();
    zero.enqueue(play(1, 1, 0)).unwrap();
    zero.arm(5, 3).unwrap();
    zero.render(3, 2).unwrap();
    assert!(zero.report().is_none());
    zero.render(5, 1).unwrap();
    let ended = zero.report().unwrap();
    assert_eq!(zero.output()[0], 0.0);
    assert!(ended.paused);
    assert_eq!(
        (
            ended.playback_end_physical_frame,
            ended.playback_frames,
            ended.counters.commands_consumed
        ),
        (Some(0), 0, 0)
    );
    assert_eq!(
        zero.enqueue(play(1, 2, 0)).unwrap_err().reason,
        QueuePushError::Full
    );
    assert_eq!(zero.playback_end_frame(), Some(0));

    let mut builder = WorkletAudioBuilder::new(config(1000, 1, 8, 1)).unwrap();
    builder
        .insert_sample(
            SampleId(1),
            AudioFormat::new(1000, 1).unwrap(),
            vec![0.25, 0.5, 0.75, 1.0],
        )
        .unwrap();
    let mut finite = builder.finish_at(5).unwrap();
    finite.enqueue(play(1, 1, 0)).unwrap();
    assert_eq!(
        finite.arm(u64::MAX - 4, u64::MAX - 7),
        Err(WorkletAudioError::Overflow)
    );
    assert_eq!(
        (
            finite.start_frame(),
            finite.context_frame(),
            finite.report()
        ),
        (None, None, None)
    );
    assert!(!finite.failed());
    finite.arm(u64::MAX - 5, u64::MAX - 7).unwrap();
    finite.render(u64::MAX - 7, 4).unwrap();
    assert_eq!(&finite.output()[..4], &[0.0, 0.0, 0.25, 0.5]);
    finite.render(u64::MAX - 3, 3).unwrap();
    assert_eq!(&finite.output()[..3], &[0.75, 1.0, 0.0]);
    let ended = finite.report().unwrap();
    assert_eq!(ended.playback_end_physical_frame, Some(5));
    assert_eq!(finite.context_frame(), Some(u64::MAX));
    finite.render(u64::MAX, 0).unwrap();
    assert_eq!(finite.report(), Some(ended));
    assert!(!finite.failed());

    let mut unlimited = audio(8, 1);
    assert_eq!(unlimited.playback_end_frame(), None);
    unlimited.enqueue(play(1, 1, 0)).unwrap();
    unlimited.arm(0, 0).unwrap();
    unlimited.render(0, 6).unwrap();
    assert_eq!(&unlimited.output()[..6], &[0.25, 0.5, 0.75, 1.0, 0.0, 0.0]);
    let report = unlimited.report().unwrap();
    assert_eq!(
        (report.playback_frames, report.playback_end_physical_frame),
        (6, None)
    );
    assert!(!report.paused);
}

#[test]
fn prestart_silence_retains_commands_and_partial_start_uses_relative_mixer_frame_zero() {
    let mut audio = audio(8, 1);
    let pointer = audio.output_ptr();
    assert_eq!(
        (audio.output_len(), audio.channels(), audio.max_frames()),
        (8, 1, 8)
    );
    assert!(audio.output().iter().all(|sample| *sample == 0.0));
    audio.enqueue(play(1, 1, 0)).unwrap();
    audio.arm(1003, 1000).unwrap();
    audio.render(1000, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.0]);
    assert!(audio.report().is_none());
    assert_eq!(audio.context_frame(), Some(1002));
    assert_eq!(audio.start_frame(), Some(1003));
    let rejected = play(1, 2, 0);
    let error = audio.enqueue(rejected).unwrap_err();
    assert_eq!(
        (error.reason, error.command),
        (QueuePushError::Full, rejected)
    );
    assert!(!audio.failed());
    audio.render(1002, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.0, 0.25, 0.5, 0.75]);
    let report = audio.report().unwrap();
    assert_eq!(
        (
            report.start_frame,
            report.frames,
            report.playback_start_frame,
            report.playback_frames
        ),
        (0, 3, 0, 3)
    );
    assert_eq!(
        (
            report.counters.commands_consumed,
            report.counters.commands_applied
        ),
        (1, 1)
    );
    assert_eq!(audio.context_frame(), Some(1006));
    audio.render(1006, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[1.0, 0.0]);
    assert_eq!(audio.report().unwrap().start_frame, 3);
    assert_eq!(audio.report().unwrap().active_voices, 0);
    assert_eq!(audio.output_ptr(), pointer);
    assert_eq!(audio.output_len(), 8);
}

#[test]
fn stereo_source_rate_and_block_partitioning_preserve_actual_pcm_without_fixed_quantum() {
    let run = |parts: &[usize]| {
        let mut builder = WorkletAudioBuilder::new(config(1000, 2, 16, 4)).unwrap();
        builder
            .insert_sample(
                SampleId(1),
                AudioFormat::new(500, 2).unwrap(),
                vec![0.0, 0.0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75],
            )
            .unwrap();
        let mut audio = builder.finish().unwrap();
        let pointer = audio.output_ptr();
        audio.enqueue(play(1, 1, 0)).unwrap();
        audio.arm(103, 100).unwrap();
        let mut current = 100;
        let mut pcm = Vec::new();
        for &frames in parts {
            audio.render(current, frames).unwrap();
            pcm.extend_from_slice(&audio.output()[..frames * 2]);
            current += frames as u64;
            assert_eq!(audio.output_ptr(), pointer);
            assert_eq!(audio.output_len(), 32);
        }
        let report = audio.report().unwrap();
        assert_eq!(audio.context_frame(), Some(112));
        assert_eq!(report.counters.rendered_frames, 9);
        assert_eq!(report.active_voices, 0);
        assert_eq!(report.counters.late_commands, 0);
        pcm
    };
    let mut expected = vec![0.0; 6];
    for sample in [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.75, 0.0] {
        expected.extend_from_slice(&[sample, -sample]);
    }
    assert_eq!(run(&[12]), expected);
    assert_eq!(run(&[1; 12]), expected);
    assert_eq!(run(&[2, 3, 1, 6]), expected);
}

#[test]
fn queue_rejection_keeps_exact_command_and_successful_prefix_reaches_mixer() {
    let mut audio = audio(8, 2);
    audio.enqueue(play(1, 1, 0)).unwrap();
    let stop = AudioCommand::Stop {
        voice: VoiceId(1),
        at: Timestamp::from_nanos(2_000_000),
    };
    audio.enqueue(stop).unwrap();
    let later = play(1, 2, 4_000_000);
    let error = audio.enqueue(later).unwrap_err();
    assert_eq!((error.reason, error.command), (QueuePushError::Full, later));
    assert!(!audio.failed());
    audio.arm(0, 0).unwrap();
    audio.render(0, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.25, 0.5, 0.0, 0.0]);
    let prefix = audio.report().unwrap();
    assert_eq!(
        (
            prefix.counters.commands_consumed,
            prefix.counters.commands_applied
        ),
        (2, 2)
    );
    // The rejected command remains caller-owned and is admitted explicitly once.
    audio.enqueue(error.command).unwrap();
    audio.render(4, 4).unwrap();
    assert_eq!(&audio.output()[..4], &[0.25, 0.5, 0.75, 1.0]);
    let report = audio.report().unwrap();
    assert_eq!(
        (
            report.counters.commands_consumed,
            report.counters.commands_applied
        ),
        (3, 3)
    );
    assert_eq!(report.counters.late_commands, 0);
}

#[test]
fn arm_is_once_only_and_zero_blocks_do_not_adopt_clock_or_consume_audio() {
    let mut audio = audio(8, 4);
    let pointer = audio.output_ptr();
    audio.enqueue(play(1, 1, 0)).unwrap();
    audio.render(u64::MAX, 0).unwrap();
    assert_eq!(audio.context_frame(), None);
    assert_eq!(audio.start_frame(), None);
    assert!(audio.report().is_none());
    audio.render(200, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.0]);
    assert_eq!(audio.context_frame(), Some(202));
    assert_eq!(
        audio.arm(201, 202).unwrap_err(),
        WorkletAudioError::StaleStart
    );
    assert_eq!(audio.start_frame(), None);
    assert!(!audio.failed());
    audio.arm(202, 202).unwrap();
    assert_eq!(
        audio.arm(203, 202).unwrap_err(),
        WorkletAudioError::AlreadyArmed
    );
    assert_eq!(audio.start_frame(), Some(202));
    assert!(!audio.failed());
    audio.render(202, 1).unwrap();
    assert_eq!(audio.output()[0], 0.25);
    let report = audio.report().unwrap();
    let output = audio.output().to_vec();
    audio.render(0, 0).unwrap();
    assert_eq!(audio.context_frame(), Some(203));
    assert_eq!(audio.report(), Some(report));
    assert_eq!(audio.output(), output);
    assert_eq!(audio.output_ptr(), pointer);
    audio.render(203, 1).unwrap();
    assert_eq!(audio.output()[0], 0.5);
}

#[test]
fn bad_extent_gap_or_regression_fences_and_silences_without_inventing_render_evidence() {
    for (current, frames, expected) in [
        (105, 1, WorkletAudioError::Chronology),
        (103, 1, WorkletAudioError::Chronology),
        (104, 9, WorkletAudioError::InvalidExtent),
    ] {
        let mut audio = audio(8, 4);
        audio.enqueue(play(1, 1, 0)).unwrap();
        audio.arm(100, 100).unwrap();
        audio.render(100, 4).unwrap();
        assert_eq!(&audio.output()[..4], &[0.25, 0.5, 0.75, 1.0]);
        let retained = audio.report();
        let pointer = audio.output_ptr();
        assert_eq!(audio.render(current, frames).unwrap_err(), expected);
        assert!(audio.failed());
        assert!(audio.output().iter().all(|sample| *sample == 0.0));
        assert_eq!(audio.report(), retained);
        let command = play(1, 2, 4_000_000);
        let error = audio.enqueue(command).unwrap_err();
        assert_eq!(
            (error.reason, error.command),
            (QueuePushError::Disconnected, command)
        );
        assert_eq!(audio.render(104, 1).unwrap_err(), WorkletAudioError::Failed);
        assert_eq!(audio.render(0, 0).unwrap_err(), WorkletAudioError::Failed);
        assert_eq!(audio.arm(104, 104).unwrap_err(), WorkletAudioError::Failed);
        assert_eq!(audio.report(), retained);
        assert_eq!(audio.output_ptr(), pointer);
    }
}

#[test]
fn long_absolute_context_positions_remain_exact_and_final_frame_overflow_is_terminal() {
    for base in [44_100 * 604_800u64, (1u64 << 53) + 123] {
        let mut audio = audio(8, 4);
        audio.enqueue(play(1, 1, 0)).unwrap();
        audio.arm(base + 3, base).unwrap();
        audio.render(base, 5).unwrap();
        assert_eq!(&audio.output()[..5], &[0.0, 0.0, 0.0, 0.25, 0.5]);
        assert_eq!(audio.context_frame(), Some(base + 5));
        assert_eq!(audio.start_frame(), Some(base + 3));
        let report = audio.report().unwrap();
        assert_eq!(
            (
                report.start_frame,
                report.frames,
                report.counters.rendered_frames
            ),
            (0, 2, 2)
        );
    }
    let mut audio = audio(8, 4);
    audio.enqueue(play(1, 1, 0)).unwrap();
    audio.arm(u64::MAX - 2, u64::MAX - 3).unwrap();
    audio.render(u64::MAX - 3, 2).unwrap();
    assert_eq!(&audio.output()[..2], &[0.0, 0.25]);
    audio.render(u64::MAX - 1, 1).unwrap();
    assert_eq!(audio.output()[0], 0.5);
    assert_eq!(audio.context_frame(), Some(u64::MAX));
    let retained = audio.report();
    assert_eq!(
        audio.render(u64::MAX, 1).unwrap_err(),
        WorkletAudioError::Overflow
    );
    assert!(audio.failed());
    assert!(audio.output().iter().all(|sample| *sample == 0.0));
    assert_eq!(audio.report(), retained);
}

#[test]
fn builder_rejections_preserve_admitted_pcm_and_empty_banks_render_real_silence() {
    let mut cfg = config(1000, 1, 8, 4);
    cfg.pcm_limits = PcmLimits::new(16, 24, 2).unwrap();
    let mut builder = WorkletAudioBuilder::new(cfg).unwrap();
    let mono = AudioFormat::new(1000, 1).unwrap();
    builder
        .insert_sample(SampleId(1), mono, vec![0.25; 4])
        .unwrap();
    assert!(builder.insert_sample(SampleId(1), mono, vec![1.0]).is_err());
    assert!(
        builder
            .insert_sample(SampleId(2), mono, vec![f32::NAN])
            .is_err()
    );
    assert!(
        builder
            .insert_sample(
                SampleId(2),
                AudioFormat::new(1000, 2).unwrap(),
                vec![1.0, 1.0]
            )
            .is_err()
    );
    assert!(
        builder
            .insert_sample(SampleId(2), mono, vec![0.5; 3])
            .is_err()
    );
    builder
        .insert_sample(SampleId(2), mono, vec![0.5; 2])
        .unwrap();
    assert!(builder.insert_sample(SampleId(3), mono, vec![]).is_err());
    let mut audio = builder.finish().unwrap();
    audio.enqueue(play(1, 1, 0)).unwrap();
    audio.enqueue(play(2, 1, 4_000_000)).unwrap();
    audio.arm(0, 0).unwrap();
    audio.render(0, 8).unwrap();
    assert_eq!(
        audio.output(),
        &[0.25, 0.25, 0.25, 0.25, 0.5, 0.5, 0.0, 0.0]
    );
    let mut empty = WorkletAudioBuilder::new(config(48_000, 2, 7, 4))
        .unwrap()
        .finish()
        .unwrap();
    empty.arm(90, 90).unwrap();
    empty.render(90, 7).unwrap();
    assert_eq!(empty.output(), &[0.0; 14]);
    let report = empty.report().unwrap();
    assert_eq!(
        (
            report.frames,
            report.active_voices,
            report.counters.commands_consumed
        ),
        (7, 0, 0)
    );
    assert_eq!(empty.context_frame(), Some(97));
}
