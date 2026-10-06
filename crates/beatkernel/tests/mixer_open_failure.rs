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
fn failure_borrows_then_moves_original_opaque_error_and_advanced_paused_mixer_without_losing_future_commands()
 {
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
