use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
    transport::Rate,
};
fn at(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn play() -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(1),
        sample: SampleId(1),
        at: Timestamp::ZERO,
        gain: 1.0,
    }
}
fn rig(gated: bool, channels: u16, capacity: usize, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(1000, channels).unwrap();
    let pcm = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    let samples = [0.25, 0.5, 0.75, 1.0, 0.5, 0.25, 0.125, 0.0625]
        .iter()
        .flat_map(|value| std::iter::repeat_n(*value, usize::from(channels)))
        .collect();
    bank.insert(SampleId(1), PcmSample::new(format, samples, pcm).unwrap())
        .unwrap();
    let limits = AudioLimits::new(capacity, 2, capacity, 64, capacity).unwrap();
    let config = MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits);
    let config = end.map_or(config, |end| config.with_playback_end_frame(end));
    let (producer, consumer) = if gated {
        command_queue_with_start_gate(capacity)
    } else {
        command_queue(capacity)
    }
    .unwrap();
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
#[test]
fn exact_inside_block_start_and_finite_suffix_have_literal_pcm_and_markers() {
    let (mut producer, mut mixer) = rig(true, 1, 8, Some(3));
    producer.try_push(play()).unwrap();
    producer.schedule_start_at(2).unwrap();
    let mut output = [99.0; 8];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0, 0.25, 0.5, 0.75, 0.0, 0.0, 0.0]);
    assert_eq!(
        (
            report.start_frame,
            report.frames,
            report.playback_start_frame,
            report.playback_frames
        ),
        (0, 8, 0, 3)
    );
    assert!(report.paused);
    assert_eq!(report.playback_end_physical_frame, Some(5));
    assert_eq!(producer.applied_start_frame(), Some(2));
    producer.request_pause(false);
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 8]);
    assert_eq!(producer.applied_start_frame(), Some(2));
}
#[test]
fn unarmed_and_before_target_hold_seek_rate_and_queued_commands() {
    let (mut producer, mut mixer) = rig(true, 1, 8, None);
    for command in [
        AudioCommand::Seek {
            at: Timestamp::ZERO,
            song_time: at(42),
        },
        AudioCommand::SetRate {
            at: Timestamp::ZERO,
            rate: Rate::new(1, 2).unwrap(),
        },
        play(),
    ] {
        producer.try_push(command).unwrap();
    }
    let mut held = [99.0; 3];
    let first = mixer.render(&mut held).unwrap();
    assert_eq!(held, [0.0; 3]);
    assert_eq!(first.counters.commands_consumed, 0);
    assert_eq!(first.song_position, Timestamp::ZERO);
    assert_eq!(mixer.rate(), Rate::NORMAL);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    assert_eq!(producer.applied_start_frame(), None);
    producer.schedule_start_at(5).unwrap();
    let before = mixer.render(&mut [99.0; 2]).unwrap();
    assert_eq!(before.counters.commands_consumed, 0);
    assert_eq!(producer.applied_start_frame(), None);
    let mut active = [99.0; 3];
    let report = mixer.render(&mut active).unwrap();
    assert_eq!(active, [0.25, 0.375, 0.5]);
    assert_eq!(report.song_position, at(42));
    assert_eq!(report.counters.commands_consumed, 3);
    assert_eq!(producer.applied_start_frame(), Some(5));
}
#[test]
fn split_partition_has_identical_first_frame_pcm_and_finite_end() {
    for parts in [vec![10], vec![2, 1, 3, 4], vec![1; 10]] {
        let (mut producer, mut mixer) = rig(true, 1, 8, Some(4));
        producer.try_push(play()).unwrap();
        producer.schedule_start_at(3).unwrap();
        let mut pcm = Vec::new();
        let mut marker = None;
        for size in parts {
            let mut block = vec![99.0; size];
            let report = mixer.render(&mut block).unwrap();
            marker = report.playback_end_physical_frame;
            pcm.extend(block);
        }
        assert_eq!(
            pcm,
            vec![0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(producer.applied_start_frame(), Some(3));
        assert_eq!(marker, Some(7));
        assert_eq!(
            (mixer.frame_cursor(), mixer.playback_frame_cursor()),
            (10, 4)
        );
    }
}
#[test]
fn pause_spanning_target_fails_atomically_and_resume_cannot_move_it() {
    let (mut producer, mut mixer) = rig(true, 1, 8, None);
    producer.try_push(play()).unwrap();
    producer.schedule_start_at(2).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [99.0; 1]).unwrap();
    let mut failed = [99.0; 2];
    assert_eq!(mixer.render(&mut failed), Err(AudioError::StartGatePaused));
    assert_eq!(failed, [99.0; 2]);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (1, 0)
    );
    assert_eq!(producer.applied_start_frame(), None);
    producer.request_pause(false);
    let report = mixer.render(&mut failed).unwrap();
    assert_eq!(failed, [0.0, 0.25]);
    assert_eq!(report.counters.commands_consumed, 1);
    assert_eq!(producer.applied_start_frame(), Some(2));
    producer.request_pause(true);
    mixer.render(&mut failed).unwrap();
    assert_eq!(failed, [0.0; 2]);
    assert_eq!(mixer.playback_frame_cursor(), 1);
    producer.request_pause(false);
    mixer.render(&mut failed).unwrap();
    assert_eq!(failed, [0.5, 0.75]);
    assert_eq!(producer.applied_start_frame(), Some(2));
}
#[test]
fn invalid_empty_and_full_ring_do_not_adopt_or_consume_gate() {
    let (mut producer, mut mixer) = rig(true, 2, 1, None);
    producer.try_push(play()).unwrap();
    assert_eq!(producer.counters().accepted, 1);
    producer.schedule_start_at(0).unwrap();
    assert_eq!(producer.counters().accepted, 1);
    let empty = mixer.render(&mut []).unwrap();
    assert_eq!(empty.counters.commands_consumed, 0);
    assert_eq!(producer.applied_start_frame(), None);
    let mut misaligned = [99.0; 1];
    assert_eq!(
        mixer.render(&mut misaligned),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(misaligned, [99.0]);
    let mut oversized = [99.0; 130];
    assert_eq!(
        mixer.render(&mut oversized),
        Err(AudioError::RenderCapacity)
    );
    assert_eq!(oversized, [99.0; 130]);
    assert_eq!(mixer.frame_cursor(), 0);
    assert_eq!(producer.applied_start_frame(), None);
    let mut output = [99.0; 2];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25, 0.25]);
    assert_eq!(producer.applied_start_frame(), Some(0));
    assert_eq!(
        producer.schedule_start_at(1),
        Err(AudioError::StartGateAlreadyArmed)
    );
}
#[test]
fn late_admission_disconnect_and_default_queue_are_explicit() {
    let (mut producer, mut mixer) = rig(true, 1, 1, None);
    mixer.render(&mut [99.0; 3]).unwrap();
    assert_eq!(
        producer.schedule_start_at(2),
        Err(AudioError::StartGateMissed)
    );
    assert_eq!(producer.applied_start_frame(), None);
    producer.schedule_start_at(3).unwrap();
    producer.try_push(play()).unwrap();
    mixer.render(&mut [99.0; 1]).unwrap();
    assert_eq!(producer.applied_start_frame(), Some(3));
    let (mut producer, consumer) = command_queue_with_start_gate(1).unwrap();
    drop(consumer);
    assert_eq!(
        producer.schedule_start_at(0),
        Err(AudioError::StartGateDisconnected)
    );
    let (producer, mut mixer) = rig(true, 1, 1, None);
    drop(producer);
    let mut output = [99.0];
    assert_eq!(
        mixer.render(&mut output),
        Err(AudioError::StartGateDisconnected)
    );
    assert_eq!(output, [99.0]);
    let (mut producer, mut mixer) = rig(false, 1, 1, None);
    assert_eq!(
        producer.schedule_start_at(0),
        Err(AudioError::StartGateUnavailable)
    );
    producer.try_push(play()).unwrap();
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25]);
    assert_eq!(producer.applied_start_frame(), None);
}
#[test]
fn zero_endpoint_is_at_armed_physical_frame_without_positive_playback_ack() {
    let (mut producer, mut mixer) = rig(true, 1, 1, Some(0));
    producer.try_push(play()).unwrap();
    let unarmed = mixer.render(&mut [99.0; 2]).unwrap();
    assert_eq!(unarmed.playback_end_physical_frame, None);
    producer.schedule_start_at(4).unwrap();
    let mut output = [99.0; 5];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 5]);
    assert_eq!(report.playback_end_physical_frame, Some(4));
    assert_eq!(report.counters.commands_consumed, 0);
    assert_eq!(producer.applied_start_frame(), None);
}
#[test]
fn armed_producer_disconnect_retains_exact_start_and_future_stop_playback_grid() {
    let (mut producer, mut mixer) = rig(true, 1, 8, None);
    producer.try_push(play()).unwrap();
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(1),
            at: at(2_000_000),
        })
        .unwrap();
    producer.schedule_start_at(2).unwrap();
    drop(producer);
    let mut output = [99.0; 6];
    let report = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0, 0.0, 0.25, 0.5, 0.0, 0.0]);
    assert_eq!(report.counters.late_commands, 0);
    assert_eq!(report.counters.commands_applied, 2);
}
