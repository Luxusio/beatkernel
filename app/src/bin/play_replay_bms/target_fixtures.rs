//! Binary target ports and staged replay effects over the actual converter.
use super::*;
use beatkernel::audio::{
    AudioCommand, ChannelMatrix, CommandProducer, ConvertedRenderReport, PcmSample,
    ResampleQuality, SampleBank, SampleId, TargetFrameBasis, VoiceId,
};
use beatkernel_bms_runtime::gameplay::output::ports::TargetOutputTelemetry;
use beatkernel_platform::audio::ConvertedNativeOutputState;
use std::{cell::RefCell, rc::Rc};

fn raw(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: OUTPUT,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: HOST,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn mixer(at: Timestamp) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(44_100, 1).unwrap();
    let limits = PcmLimits::new(4096, 4096, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 512], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(16).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at,
            gain: 1.0,
        })
        .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            OUTPUT,
            Timestamp::ZERO,
            AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (producer, mixer)
}
struct TargetPort {
    owner: ConvertedNativeOutputState,
    basis: TargetFrameBasis,
    converted: Option<ConvertedRenderReport>,
    native_frame: u64,
    held: bool,
    refuse_held: bool,
    trace: Rc<RefCell<Vec<&'static str>>>,
}
impl TargetPort {
    fn render(&mut self, frames: usize, presented: u64) {
        let converted = if self.held {
            self.owner.render_held_pending(frames).unwrap()
        } else {
            self.owner.render_pending(frames).unwrap()
        };
        self.owner.admit(frames).unwrap();
        self.converted = Some(converted);
        self.native_frame = presented;
    }
    fn pair(&self) -> ClockPair {
        let source = self.basis.point_at_stream_frame(self.native_frame).unwrap();
        ClockPair {
            source,
            target: host(1_000_000_000 + source.timestamp.as_nanos()),
        }
    }
}
impl NativeOutput for TargetPort {
    fn target_identity(&self) -> Option<(u64, TargetFrameBasis)> {
        Some((7, self.basis))
    }
    fn target_observation(&mut self) -> Result<Option<TargetReplayObservation>> {
        Ok(Some(TargetReplayObservation {
            epoch: 7,
            basis: self.basis,
            telemetry: Some(TargetOutputTelemetry {
                source: self.owner.last_real_source_report(),
                converted: self.converted,
                facts: self.owner.boundaries(),
            }),
            pair: Some(self.pair()),
        }))
    }
    fn set_target_held(&mut self, held: bool) -> Result<()> {
        self.trace
            .borrow_mut()
            .push(if held { "held true" } else { "held false" });
        if self.refuse_held {
            return Err("actual held effect refused".into());
        }
        self.held = held;
        Ok(())
    }
    fn start(&mut self) -> Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<RenderReport>> {
        Ok(self.owner.last_real_source_report())
    }
    fn presented(&mut self) -> Result<Option<ClockPoint>> {
        panic!("target controls must use original typed observation")
    }
    fn presentation_pair(&mut self) -> Result<Option<ClockPair>> {
        panic!("target controls cannot enter legacy source-grid port")
    }
    fn last_render(&mut self) -> Option<RenderReport> {
        self.owner.last_real_source_report()
    }
    fn final_check(&mut self) -> Result<()> {
        Ok(())
    }
    fn print_native(&mut self) {}
}
fn portable() -> (
    TargetPort,
    CommandProducer,
    ReplayPause,
    Rc<RefCell<Vec<&'static str>>>,
) {
    let (producer, mixer) = mixer(Timestamp::ZERO);
    let owner = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        128,
    )
    .unwrap_or_else(|_| panic!("actual converted replay port"));
    let basis = owner.target_frame_basis();
    let pause = ReplayPause::new(
        raw(0),
        HOST,
        44_100,
        Timestamp::from_nanos(50_000_000),
        Duration::from_nanos(3_000_000),
    )
    .unwrap()
    .with_target_basis(7, basis)
    .unwrap();
    let trace = Rc::new(RefCell::new(Vec::new()));
    (
        TargetPort {
            owner,
            basis,
            converted: None,
            native_frame: 0,
            held: false,
            refuse_held: false,
            trace: trace.clone(),
        },
        producer,
        pause,
        trace,
    )
}
fn update(
    port: &mut TargetPort,
    producer: &mut CommandProducer,
    pause: &mut ReplayPause,
    available: &mut bool,
    desired: bool,
) -> Result<ReplayPauseUpdate> {
    let observation = NativeOutput::target_observation(port)?.unwrap();
    let trace = port.trace.clone();
    update_target_replay_pause(pause, available, observation, desired, port, |paused| {
        trace.borrow_mut().push(if paused {
            "producer true"
        } else {
            "producer false"
        });
        producer.request_pause(paused);
    })
}

#[test]
fn binary_target_control_dispatch_holds_only_after_native_ack_and_releases_before_source_resume() {
    let (mut port, mut producer, mut pause, trace) = portable();
    let mut available = false;
    port.render(1, 1);
    let requested = update(&mut port, &mut producer, &mut pause, &mut available, true).unwrap();
    assert!(requested.became_available);
    assert_eq!(requested.requested, Some(true));
    assert!(requested.boundary.is_none());
    assert_eq!(*trace.borrow(), vec!["producer true"]);
    port.render(8, 2);
    assert!(
        update(&mut port, &mut producer, &mut pause, &mut available, true)
            .unwrap()
            .boundary
            .is_none()
    );
    assert!(!port.held);
    port.native_frame = 3;
    let acknowledged = update(&mut port, &mut producer, &mut pause, &mut available, true)
        .unwrap()
        .boundary
        .unwrap();
    assert!(acknowledged.paused);
    assert_eq!(
        acknowledged.song,
        Timestamp::from_nanos(47_000_000 + 2 * 1_000_000_000 / 44_100)
    );
    assert_eq!(*trace.borrow(), vec!["producer true", "held true"]);
    let source_cursor = port.owner.mixer().playback_frame_cursor();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: Timestamp::from_nanos(4 * 1_000_000_000 / 44_100),
            gain: 0.5,
        })
        .unwrap();
    port.render(128, 137);
    assert_eq!(port.owner.mixer().playback_frame_cursor(), source_cursor);
    assert_eq!(port.owner.mixer().counters().commands_applied, 1);
    let resumed_request =
        update(&mut port, &mut producer, &mut pause, &mut available, false).unwrap();
    assert_eq!(resumed_request.requested, Some(false));
    assert!(resumed_request.boundary.is_none());
    assert_eq!(
        *trace.borrow(),
        vec!["producer true", "held true", "held false", "producer false"]
    );
    assert!(!port.held);
    port.render(16, 153);
    let resumed = update(&mut port, &mut producer, &mut pause, &mut available, false)
        .unwrap()
        .boundary
        .unwrap();
    assert!(!resumed.paused);
    assert_eq!(resumed.song, acknowledged.song);
    assert_eq!(pause.phase(), PausePhase::Running);
    assert_eq!(port.owner.mixer().counters().commands_applied, 2);
    assert_eq!(port.owner.boundaries().source_rate, 44_100);
    assert_eq!(port.target_identity(), Some((7, port.basis)));
}

#[test]
fn binary_target_held_effect_refusal_keeps_staged_pause_and_source_producer_ownership() {
    let (mut port, mut producer, mut pause, trace) = portable();
    let mut available = false;
    port.render(1, 1);
    update(&mut port, &mut producer, &mut pause, &mut available, true).unwrap();
    port.render(8, 3);
    port.refuse_held = true;
    let source_before = pause.last_render_report();
    assert!(update(&mut port, &mut producer, &mut pause, &mut available, true).is_err());
    assert_eq!(pause.phase(), PausePhase::Pausing);
    assert_eq!(pause.last_render_report(), source_before);
    assert!(!port.held);
    assert!(port.owner.mixer().pause_requested());
    assert_eq!(*trace.borrow(), vec!["producer true", "held true"]);
    port.refuse_held = false;
    update(&mut port, &mut producer, &mut pause, &mut available, true).unwrap();
    assert_eq!(pause.phase(), PausePhase::Paused);
    port.render(128, 137);
    port.refuse_held = true;
    let source_before = pause.last_render_report();
    assert!(update(&mut port, &mut producer, &mut pause, &mut available, false).is_err());
    assert_eq!(pause.phase(), PausePhase::Paused);
    assert_eq!(pause.last_render_report(), source_before);
    assert!(port.held);
    assert!(port.owner.mixer().pause_requested());
    assert!(!trace.borrow().contains(&"producer false"));
    port.refuse_held = false;
    update(&mut port, &mut producer, &mut pause, &mut available, false).unwrap();
    assert_eq!(pause.phase(), PausePhase::Resuming);
    assert_eq!(
        &trace.borrow()[trace.borrow().len() - 2..],
        &["held false", "producer false"]
    );
}

#[test]
fn binary_target_missing_or_malformed_associations_do_not_publish_control_or_audio_effects() {
    let (mut port, mut producer, mut pause, trace) = portable();
    port.render(1, 1);
    let mut available = false;
    let observation = port.target_observation().unwrap().unwrap();
    for omit_tuple in [true, false] {
        let mut missing = observation;
        if omit_tuple {
            missing.telemetry = None;
        } else {
            missing.pair = None;
        }
        let update = update_target_replay_pause(
            &mut pause,
            &mut available,
            missing,
            true,
            &mut port,
            |paused| producer.request_pause(paused),
        )
        .unwrap();
        assert!(!update.became_available);
        assert!(update.requested.is_none() && update.boundary.is_none());
        assert!(!available);
        assert_eq!(pause.phase(), PausePhase::Running);
        assert!(!port.owner.mixer().pause_requested());
    }
    let mut invalid = observation;
    invalid.telemetry.as_mut().unwrap().facts.source_rate = 48_000;
    assert!(update_target_replay_pause(
        &mut pause,
        &mut available,
        invalid,
        true,
        &mut port,
        |paused| producer.request_pause(paused)
    )
    .is_err());
    assert!(!available);
    assert_eq!(pause.phase(), PausePhase::Running);
    assert!(!port.owner.mixer().pause_requested());
    assert!(trace.borrow().is_empty());
    invalid = observation;
    invalid.epoch += 1;
    assert!(update_target_replay_pause(
        &mut pause,
        &mut available,
        invalid,
        true,
        &mut port,
        |paused| producer.request_pause(paused)
    )
    .is_err());
    assert!(!available);
    assert!(trace.borrow().is_empty());
    update(&mut port, &mut producer, &mut pause, &mut available, true).unwrap();
    assert_eq!(*trace.borrow(), vec!["producer true"]);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit native ALSA null replay factory/recovery diagnostic; no acoustic proof"]
fn actual_null_replay_target_factory_retains_source_cue_and_recovers_complete_held_converter() {
    use beatkernel_platform::linux::AlsaRequest;
    let (producer, mixer) = mixer(Timestamp::from_nanos(2_000_000));
    let mut stream = native::open_target_output(
        AlsaRequest {
            device: "null".into(),
            format: DeviceFormat::new(48_000, 1, SampleEncoding::Float32, None).unwrap(),
            period_frames: 64,
            buffer_frames: 256,
            allow_size_rounding: false,
            monotonic_domain: HOST,
        },
        mixer,
    )
    .unwrap();
    let (epoch, basis) = NativeOutput::target_identity(&stream).unwrap();
    assert_eq!(epoch, 0);
    assert_eq!(basis.origin(), raw(0));
    assert_eq!(basis.sample_rate(), 48_000);
    NativeOutput::set_target_held(&mut stream, true).unwrap();
    NativeOutput::start(&mut stream).unwrap();
    std::thread::sleep(WallDuration::from_millis(10));
    let observation = NativeOutput::target_observation(&mut stream);
    if let Ok(Some(observation)) = observation {
        assert_eq!(observation.epoch, epoch);
        assert_eq!(observation.basis, basis);
        assert!(
            observation.pair.is_none(),
            "null endpoint cannot invent source-grid presentation"
        );
    }
    NativeOutput::stop(&mut stream).unwrap();
    NativeOutput::final_check(&mut stream).unwrap();
    let mut recovered = stream.take_recovered().unwrap();
    assert_eq!(recovered.mixer().config().format().sample_rate(), 44_100);
    assert_eq!(recovered.mixer().playback_frame_cursor(), 0);
    assert_eq!(recovered.mixer().counters().commands_consumed, 0);
    assert_eq!(recovered.converter_owner().source_position().frame, 0);
    let generated_time = recovered.converter_owner().target_time();
    let creation_time = basis.start_time();
    assert!(
        generated_time.seconds() > creation_time.seconds()
            || (generated_time.seconds() == creation_time.seconds()
                && u128::from(generated_time.numerator())
                    * u128::from(creation_time.denominator())
                    > u128::from(creation_time.numerator())
                        * u128::from(generated_time.denominator()))
    );
    if recovered.pending_frames() != 0 {
        assert!(recovered.pending_is_held());
        assert!(recovered
            .pending_samples()
            .iter()
            .all(|sample| *sample == 0.0));
        recovered.admit(recovered.pending_frames()).unwrap();
    }
    let mut first_positive = None;
    for block in 0..3u64 {
        recovered.render_pending(64).unwrap();
        if first_positive.is_none() {
            first_positive = recovered
                .pending_samples()
                .iter()
                .position(|sample| *sample > 0.0)
                .map(|offset| block * 64 + offset as u64);
        }
        recovered.admit(64).unwrap();
    }
    // Source timestamp 2ms remains on source frame89, independently of held
    // native output time. Linear interpolation first samples the zero/active source pair at target sample96.
    assert_eq!(first_positive, Some(96));
    assert_eq!(recovered.mixer().counters().commands_applied, 1);
    assert!(!producer.is_disconnected());
}

#[test]
fn binary_target_controls_accept_more_than_64_original_observations_without_consuming_source_during_hold(
) {
    let (mut port, mut producer, mut pause, trace) = portable();
    let mut available = false;
    port.render(1, 1);
    update(&mut port, &mut producer, &mut pause, &mut available, true).unwrap();
    port.render(8, 3);
    update(&mut port, &mut producer, &mut pause, &mut available, true).unwrap();
    assert!(port.held);
    let source = port.owner.last_real_source_report();
    let cursor = port.owner.mixer().playback_frame_cursor();
    let mut previous = port.pair();
    for index in 1..=96 {
        let frame = 9 + 64 * index;
        port.render(64, frame);
        let observation = port.target_observation().unwrap().unwrap();
        let pair = observation.pair.unwrap();
        assert!(pair.source.timestamp > previous.source.timestamp);
        assert!(pair.target.timestamp > previous.target.timestamp);
        let update = update_target_replay_pause(
            &mut pause,
            &mut available,
            observation,
            true,
            &mut port,
            |paused| producer.request_pause(paused),
        )
        .unwrap();
        assert!(update.requested.is_none() && update.boundary.is_none());
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert_eq!(port.owner.mixer().playback_frame_cursor(), cursor);
        assert_eq!(port.owner.last_real_source_report(), source);
        previous = pair;
    }
    assert_eq!(*trace.borrow(), vec!["producer true", "held true"]);
    assert!(available);
}

#[test]
fn replay_output_rate_parser_preserves_source_preparation_rate_and_refuses_unsupported_requests() {
    let base: Vec<String> = [
        "--chart",
        "chart.bms",
        "--replay",
        "session.bkr",
        "--device",
        "null",
        "--rate",
        "44100",
        "--channels",
        "1",
        "--buffer-frames",
        "256",
        "--period-frames",
        "64",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let unchanged = parse(&base, Backend::Linux).unwrap();
    assert_eq!(unchanged.format.sample_rate(), 44_100);
    assert_eq!(unchanged.output_rate, None);
    let mut converted = base.clone();
    converted.extend(["--output-rate".into(), "48000".into()]);
    let options = parse(&converted, Backend::Linux).unwrap();
    assert_eq!(options.format.sample_rate(), 44_100);
    assert_eq!(options.format.channels(), 1);
    assert_eq!(options.output_rate, Some(48_000));
    for value in ["0", "invalid", "48000.0", "-1", "4294967296"] {
        let mut invalid = base.clone();
        invalid.extend(["--output-rate".into(), value.into()]);
        assert!(parse(&invalid, Backend::Linux).is_err(), "{value}");
    }
    converted.extend(["--output-rate".into(), "32000".into()]);
    assert!(parse(&converted, Backend::Linux)
        .unwrap_err()
        .to_string()
        .contains("duplicate option"));
    for host in [Backend::Windows, Backend::Macos] {
        let mut unsupported = base.clone();
        unsupported.extend(["--output-rate".into(), "48000".into()]);
        assert!(parse(&unsupported, host)
            .unwrap_err()
            .to_string()
            .contains("--output-rate applies only to Linux ALSA"));
    }
}
