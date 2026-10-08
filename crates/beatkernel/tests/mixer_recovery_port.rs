//! Deferred software ownership tests; a fixture retirement flag is no physical fence.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
    transport::Rate,
};
enum Refusal {
    NotRetired,
}
struct Owner {
    mixer: Option<Mixer>,
    retired: bool,
}
impl StoppedMixerSource for Owner {
    type Error = Refusal;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Refusal> {
        if !self.retired {
            return Err(Refusal::NotRetired);
        }
        Ok(self.mixer.take())
    }
}
fn take<P: StoppedMixerSource>(source: &mut P) -> Result<Option<Mixer>, P::Error> {
    source.take_stopped_mixer()
}
fn recovered(owner: &mut Owner) -> Mixer {
    match take(owner) {
        Ok(Some(mixer)) => mixer,
        _ => panic!("retired original mixer required"),
    }
}
fn ts(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn rig(rate: u32, source_rate: u32, end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(rate, 1).unwrap();
    let limits = PcmLimits::new(256, 512, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(
            AudioFormat::new(source_rate, 1).unwrap(),
            vec![0., 1., 2., 3., 4., 5., 6., 7.],
            limits,
        )
        .unwrap(),
    )
    .unwrap();
    bank.insert(
        SampleId(2),
        PcmSample::new(format, vec![1., 0.5, 0., -0.5, -1.], limits).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(16).unwrap();
    let config = MixerConfig::new(
        format,
        ClockDomainId(71),
        ts(0),
        AudioLimits::new(16, 4, 16, 32, 16).unwrap(),
    );
    (
        producer,
        Mixer::new(
            match end {
                Some(end) => config.with_playback_end_frame(end),
                None => config,
            },
            bank,
            consumer,
        )
        .unwrap(),
    )
}
fn play(voice: u64, sample: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(sample),
        at: ts(at),
        gain,
    }
}
#[test]
fn recovered_original_matches_uninterrupted_rational_heads_rates_gains_and_future_queue_commands() {
    let (mut producer, mut mixer) = rig(4, 3, None);
    let (mut reference_producer, mut reference) = rig(4, 3, None);
    for command in [
        AudioCommand::SetRate {
            rate: Rate::new(1, 2).unwrap(),
            at: ts(0),
        },
        play(7, 1, 0, 0.5),
        play(u64::MAX, 2, 2_000_000_000, 0.25),
    ] {
        producer.try_push(command).unwrap();
        reference_producer.try_push(command).unwrap();
    }
    let mut first = [0.; 3];
    let mut expected = [0.; 3];
    assert_eq!(
        mixer.render(&mut first).unwrap(),
        reference.render(&mut expected).unwrap()
    );
    assert_eq!(first, expected);
    assert_eq!(first, [0., 0.1875, 0.375]);
    let counters = mixer.counters();
    let mut owner = Owner {
        mixer: Some(mixer),
        retired: true,
    };
    let mut recovered = recovered(&mut owner);
    assert_eq!(recovered.rate(), Rate::new(1, 2).unwrap());
    assert_eq!(recovered.counters(), counters);
    for command in [
        play(91, 2, 1_250_000_000, -0.5),
        AudioCommand::Stop {
            voice: VoiceId(7),
            at: ts(3_000_000_000),
        },
    ] {
        producer.try_push(command).unwrap();
        reference_producer.try_push(command).unwrap();
    }
    for frames in [2, 3, 4] {
        let mut actual = vec![0.; frames];
        let mut expected = vec![0.; frames];
        assert_eq!(
            recovered.render(&mut actual).unwrap(),
            reference.render(&mut expected).unwrap()
        );
        assert_eq!(actual, expected);
        assert_eq!(
            (
                recovered.frame_cursor(),
                recovered.playback_frame_cursor(),
                recovered.rate(),
                recovered.counters()
            ),
            (
                reference.frame_cursor(),
                reference.playback_frame_cursor(),
                reference.rate(),
                reference.counters()
            )
        );
    }
    assert!(matches!(take(&mut owner), Ok(None)));
}
#[test]
fn paused_physical_and_playback_cursors_finite_fence_and_pending_commands_survive_transfer() {
    let (mut producer, mut mixer) = rig(1000, 1000, Some(5));
    let (mut reference_producer, mut reference) = rig(1000, 1000, Some(5));
    for command in [
        play(7, 1, 0, 0.1),
        play(u64::MAX, 2, 4_000_000, 0.5),
        AudioCommand::Stop {
            voice: VoiceId(7),
            at: ts(6_000_000),
        },
    ] {
        producer.try_push(command).unwrap();
        reference_producer.try_push(command).unwrap();
    }
    mixer.render(&mut [0.; 2]).unwrap();
    reference.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    reference_producer.request_pause(true);
    assert_eq!(
        mixer.render(&mut [0.; 3]).unwrap(),
        reference.render(&mut [0.; 3]).unwrap()
    );
    let mut owner = Owner {
        mixer: Some(mixer),
        retired: true,
    };
    let mut recovered = recovered(&mut owner);
    assert_eq!(
        (
            recovered.frame_cursor(),
            recovered.playback_frame_cursor(),
            recovered.is_paused()
        ),
        (5, 2, true)
    );
    assert_eq!(recovered.config().playback_end_frame(), Some(5));
    producer.request_pause(false);
    reference_producer.request_pause(false);
    let mut actual = [0.; 3];
    let mut expected = [0.; 3];
    let report = recovered.render(&mut actual).unwrap();
    assert_eq!(report, reference.render(&mut expected).unwrap());
    assert_eq!(actual, expected);
    assert_eq!(report.playback_end_physical_frame, Some(8));
    assert_eq!(report.pending_commands, 1);
    producer.try_push(play(91, 2, 6_000_000, 0.5)).unwrap();
    reference_producer
        .try_push(play(91, 2, 6_000_000, 0.5))
        .unwrap();
    assert_eq!(
        recovered.render(&mut actual).unwrap(),
        reference.render(&mut expected).unwrap()
    );
    assert_eq!(actual, [0.; 3]);
    assert_eq!(actual, expected);
}
#[test]
fn generic_take_refuses_before_retirement_without_changing_mixer_and_moves_only_once() {
    let (mut producer, mixer) = rig(4, 4, None);
    producer.try_push(play(7, 2, 0, 0.5)).unwrap();
    let mut owner = Owner {
        mixer: Some(mixer),
        retired: false,
    };
    let before = owner.mixer.as_ref().unwrap().counters();
    assert!(matches!(take(&mut owner), Err(Refusal::NotRetired)));
    assert_eq!(owner.mixer.as_ref().unwrap().frame_cursor(), 0);
    assert_eq!(owner.mixer.as_ref().unwrap().counters(), before);
    owner.retired = true;
    let mut recovered = recovered(&mut owner);
    let mut pcm = [0.; 2];
    recovered.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.5, 0.25]);
    assert!(matches!(take(&mut owner), Ok(None)));
    producer.try_push(play(91, 2, 500_000_000, 1.)).unwrap();
    recovered.render(&mut pcm).unwrap();
    assert_eq!(recovered.counters().commands_consumed, 2);
}

#[test]
fn static_recovery_port_transfers_complete_converter_owner_only_after_retirement() {
    struct CompletePort {
        owner: Option<ConvertedMixer>,
        retired: bool,
    }
    impl StoppedMixerSource<ConvertedMixer> for CompletePort {
        type Error = Refusal;
        fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedMixer>, Refusal> {
            if !self.retired {
                return Err(Refusal::NotRetired);
            }
            Ok(self.owner.take())
        }
    }
    fn take_complete<P: StoppedMixerSource<ConvertedMixer>>(
        port: &mut P,
    ) -> Result<Option<ConvertedMixer>, P::Error> {
        port.take_stopped_mixer()
    }
    let (mut producer, mixer) = rig(4, 4, None);
    producer.try_push(play(1, 2, 0, 0.5)).unwrap();
    let owner = ConvertedMixer::new(
        mixer,
        AudioFormat::new(8, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        4,
    );
    let mut owner = match owner {
        Ok(owner) => owner,
        Err(_) => panic!("valid owner preparation"),
    };
    let mut pcm = [0.; 3];
    owner.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.5, 0.375, 0.25]);
    let position = owner.converter().source_position();
    let counters = owner.mixer().counters();
    let mut port = CompletePort {
        owner: Some(owner),
        retired: false,
    };
    assert!(matches!(take_complete(&mut port), Err(Refusal::NotRetired)));
    assert_eq!(
        port.owner.as_ref().unwrap().converter().source_position(),
        position
    );
    assert_eq!(port.owner.as_ref().unwrap().mixer().counters(), counters);
    port.retired = true;
    let mut recovered = match take_complete(&mut port) {
        Ok(Some(o)) => o,
        _ => panic!("complete retired owner"),
    };
    assert!(matches!(take_complete(&mut port), Ok(None)));
    recovered.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.125, 0., -0.125]);
    assert_eq!(recovered.mixer().counters().commands_consumed, 1);
}
