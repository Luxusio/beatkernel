//! Deferred exclusive pause gate with actual queue/Mixer PCM, no callback threads.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let pcm = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
#[test]
fn exclusive_hold_ignores_resume_leaves_pause_on_drop_and_preserves_original_queued_pcm_and_counters()
 {
    let (mut producer, mut mixer) = rig();
    let before = producer.counters();
    let hold = match producer.hold_pause() {
        Ok(hold) => hold,
        Err(_) => panic!("first hold required"),
    };
    assert!(producer.hold_pause().is_err());
    assert_eq!(producer.counters(), before);
    assert!(mixer.pause_requested());
    producer.request_pause(false);
    let mut output = [99.; 2];
    let held = mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.; 2]);
    assert!(held.paused);
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (2, 0)
    );
    producer.request_pause(false);
    drop(hold);
    assert!(mixer.pause_requested());
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.; 2]);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    producer.request_pause(false);
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.25, 0.5]);
    assert_eq!(mixer.playback_frame_cursor(), 2);
    assert_eq!(producer.counters(), before);
    let second = match producer.hold_pause() {
        Ok(hold) => hold,
        Err(_) => panic!("released gate must admit new hold"),
    };
    drop(second);
    assert!(mixer.pause_requested());
}
#[test]
fn pending_resume_on_applied_paused_mixer_is_pinned_without_advancing_cursor_and_stays_paused_after_release()
 {
    let (mut producer, mut mixer) = rig();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 1]).unwrap();
    producer.request_pause(false);
    assert!(mixer.is_paused());
    assert!(!mixer.pause_requested());
    let hold = match producer.hold_pause() {
        Ok(hold) => hold,
        Err(_) => panic!("hold required"),
    };
    assert!(mixer.pause_requested());
    mixer.render(&mut [0.; 2]).unwrap();
    assert_eq!(mixer.playback_frame_cursor(), 2);
    drop(hold);
    mixer.render(&mut [0.; 1]).unwrap();
    assert_eq!(mixer.playback_frame_cursor(), 2);
    producer.request_pause(false);
    let mut output = [0.; 2];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.75, 1.]);
}
#[test]
fn producer_drop_cannot_turn_owned_pause_hold_into_resume_or_fabricate_a_new_consumer() {
    let (mut producer, mut mixer) = rig();
    let hold = match producer.hold_pause() {
        Ok(hold) => hold,
        Err(_) => panic!("hold required"),
    };
    drop(producer);
    let report = mixer.render(&mut [0.; 2]).unwrap();
    assert!(report.producer_disconnected);
    assert!(report.paused);
    assert_eq!(mixer.playback_frame_cursor(), 0);
    drop(hold);
    let report = mixer.render(&mut [0.; 2]).unwrap();
    assert!(report.producer_disconnected);
    assert!(report.paused);
    assert!(mixer.pause_requested());
    assert_eq!(mixer.playback_frame_cursor(), 0);
}
