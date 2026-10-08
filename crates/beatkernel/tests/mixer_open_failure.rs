//! Deferred cold unique ownership transfer; error deliberately has no trait bounds.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
struct Opaque(Box<u64>);
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let pcm = PcmLimits::new(256, 512, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            AudioFormat::new(3, 1).unwrap(),
            vec![0., 1., 2., 3., 4., 5., 6., 7.],
            pcm,
        )
        .unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(16).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            ts(-1_000_000_000),
            AudioLimits::new(16, 4, 16, 16, 16).unwrap(),
        )
        .with_playback_end_frame(8),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
fn play(voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(1),
        at: ts(at),
        gain,
    }
}
#[test]
fn failure_borrows_then_moves_original_opaque_error_and_advanced_paused_mixer_without_losing_future_commands(
) {
    let (mut producer, mut mixer) = rig();
    let (mut reference_producer, mut reference) = rig();
    for p in [&mut producer, &mut reference_producer] {
        p.try_push(play(u64::MAX, -1_000_000_000, 0.5)).unwrap();
    }
    let mut pcm = [0.; 2];
    let mut expected = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    reference.render(&mut expected).unwrap();
    assert_eq!(pcm, expected);
    producer.request_pause(true);
    reference_producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    reference.render(&mut [0.; 3]).unwrap();
    producer.try_push(play(7, 500_000_000, 0.25)).unwrap();
    reference_producer
        .try_push(play(7, 500_000_000, 0.25))
        .unwrap();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let error = Opaque(Box::new(0xfeed));
    let error_pointer = error.0.as_ref() as *const u64;
    let failure = MixerOpenFailure::new(error, Some(mixer));
    assert_eq!(failure.error().0.as_ref() as *const u64, error_pointer);
    let borrowed = failure.mixer().unwrap();
    assert_eq!(
        (
            borrowed.frame_cursor(),
            borrowed.playback_frame_cursor(),
            borrowed.is_paused()
        ),
        (5, 2, true)
    );
    assert_eq!(borrowed.output_frame_basis(), basis);
    assert_eq!(borrowed.counters(), counters);
    let (error, recovered) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, error_pointer);
    let mut recovered = recovered.unwrap();
    // The original producer still feeds the one recovered consumer.
    producer.try_push(play(9, 750_000_000, 0.125)).unwrap();
    reference_producer
        .try_push(play(9, 750_000_000, 0.125))
        .unwrap();
    producer.request_pause(false);
    reference_producer.request_pause(false);
    let mut actual = [0.; 8];
    let mut expected = [0.; 8];
    let report = recovered.render(&mut actual).unwrap();
    let reference_report = reference.render(&mut expected).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(report, reference_report);
    assert_eq!(recovered.counters(), reference.counters());
    assert_eq!(recovered.playback_frame_cursor(), 8);
    assert!(report.playback_end_physical_frame.is_some());
}
#[test]
fn unavailable_recovery_retains_only_original_error_without_substituting_empty_mixer() {
    let error = Opaque(Box::new(u64::MAX));
    let pointer = error.0.as_ref() as *const u64;
    let failure = MixerOpenFailure::new(error, None);
    assert!(failure.mixer().is_none());
    assert_eq!(failure.error().0.as_ref() as *const u64, pointer);
    let (error, mixer) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, pointer);
    assert!(mixer.is_none());
}

#[test]
fn generic_open_failure_moves_whole_converter_owner_without_losing_cached_source_or_queue() {
    let (mut producer, mixer) = rig();
    producer.try_push(play(1, -1_000_000_000, 0.5)).unwrap();
    let target = AudioFormat::new(8, 2).unwrap();
    let owner = ConvertedMixer::new(
        mixer,
        target,
        ChannelMatrix::new(1, 2, &[1., -0.5]).unwrap(),
        ResampleQuality::Linear,
        4,
    );
    let mut owner = match owner {
        Ok(o) => o,
        Err(_) => panic!("continuous owner setup"),
    };
    let mut initial = [0.; 6];
    owner.render(&mut initial).unwrap();
    assert_eq!(initial, [0., 0., 0.1875, -0.09375, 0.375, -0.1875]);
    let position = owner.converter().source_position();
    let pulled = owner.mixer().frame_cursor();
    let error = Opaque(Box::new(29));
    let pointer = error.0.as_ref() as *const u64;
    let failure: MixerOpenFailure<Opaque, ConvertedMixer> =
        MixerOpenFailure::new_state(error, Some(owner));
    assert_eq!(failure.error().0.as_ref() as *const u64, pointer);
    assert_eq!(
        failure.mixer().unwrap().converter().source_position(),
        position
    );
    assert_eq!(failure.mixer().unwrap().mixer().frame_cursor(), pulled);
    let (error, owner) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, pointer);
    let mut owner = owner.unwrap();
    producer.try_push(play(2, 2_000_000_000, 0.25)).unwrap();
    let mut next = [0.; 6];
    let report = owner.render(&mut next).unwrap();
    // Mixer source frames are [0, 3/8, 3/4, 1]: the fourth frame's
    // sample/gain sum 9/8 saturates before the output converter interpolates.
    // Next output source positions 3/2, 2, 5/2 therefore yield 9/16, 3/4, 7/8.
    assert_eq!(next, [0.5625, -0.28125, 0.75, -0.375, 0.875, -0.4375]);
    assert_eq!(owner.mixer().counters().commands_consumed, 2);
    assert_eq!(report.source.unwrap().pending_commands, 1);
}
