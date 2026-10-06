//! Deferred actual stop/join/take with worker-only memory state; never NativePcm.
use super::*;
use beatkernel::audio::{
    StoppedMixerSource, AudioFormat, AudioLimits, MixerConfig, PcmLimits, PcmSample, SampleBank,
    SampleId, VoiceId, AudioCommand, CommandProducer, command_queue,
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
    let (producer, consumer) = command_queue(8).unwrap();
    (
        producer,
        Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(7),
                Timestamp::ZERO,
                AudioLimits::new(8, 4, 8, 4, 8).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap(),
    )
}
fn config() -> AlsaAppliedConfig {
    let format = DeviceFormat::new(4, 1, SampleEncoding::Float32, None).unwrap();
    AlsaAppliedConfig {
        requested: AlsaRequest {
            device: "memory-worker-only".into(),
            format,
            buffer_frames: 4,
            period_frames: 1,
            allow_size_rounding: false,
            monotonic_domain: ClockDomainId(8),
        },
        format,
        buffer_frames: 4,
        period_frames: 1,
        sizing_adjusted: false,
        output_domain: ClockDomainId(7),
        output_origin: Timestamp::ZERO,
    }
}
fn worker(error: bool) -> (AlsaStream, CommandProducer, mpsc::Sender<()>) {
    let (mut producer, mut mixer) = rig();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let shared = Arc::new(Shared::new());
    let worker_shared = shared.clone();
    let (release, ready) = mpsc::channel();
    let handle = thread::spawn(move || {
        ready.recv().unwrap();
        let report = mixer.render(&mut [0.; 1]).unwrap();
        worker_shared
            .rendered
            .store(report.counters.rendered_frames, Ordering::Relaxed);
        worker_shared.renders.store(1, Ordering::Relaxed);
        worker_shared.cadence.record(
            Timestamp::from_nanos(123),
            report.start_frame,
            report.frames as u64,
        );
        let mut version = 0;
        worker_shared.render_telemetry.publish(
            AudioStreamSnapshot {
                telemetry_available: true,
                status: AudioStreamStatus::Running,
                counters: StreamCounters::default(),
                render: Some(report),
                clock: None,
            },
            &mut version,
        );
        let result = if error {
            worker_shared.status.store(3, Ordering::Release);
            worker_shared.errno.store(-32, Ordering::Relaxed);
            worker_shared.failures.store(1, Ordering::Relaxed);
            Err(LinuxError::InvalidConfiguration(
                "original memory worker failure",
            ))
        } else {
            Ok(())
        };
        (result, mixer)
    });
    (
        AlsaStream {
            configuration: config(),
            shared,
            worker: Some(handle),
            recovered_mixer: None,
            retired: false,
        },
        producer,
        release,
    )
}
#[test]
fn before_stop_refuses_without_request_or_join_then_actual_join_moves_mixer_once_with_live_consumer()
 {
    let (mut stream, mut producer, release) = worker(false);
    let configuration = stream.configuration().clone();
    assert!(matches!(
        stream.take_stopped_mixer(),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(stream.worker.is_some());
    assert!(!stream.shared.stop.load(Ordering::Acquire));
    assert_eq!(stream.snapshot().rendered_frames, 0);
    release.send(()).unwrap();
    stream.stop().unwrap();
    let before = stream.snapshot();
    let cadence = stream.render_cadence().unwrap().unwrap();
    let report = stream.last_render_report().unwrap();
    let mut mixer = stream.take_stopped_mixer().unwrap().unwrap();
    assert_eq!(mixer.frame_cursor(), 1);
    assert_eq!(mixer.counters().rendered_frames, 1);
    assert!(stream.take_stopped_mixer().unwrap().is_none());
    assert_eq!(stream.configuration(), &configuration);
    assert_eq!(stream.snapshot().rendered_frames, before.rendered_frames);
    assert_eq!(stream.render_cadence().unwrap(), Some(cadence));
    assert_eq!(stream.last_render_report(), Some(report));
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::from_nanos(750_000_000),
            gain: 0.5,
        })
        .unwrap();
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.5, 0.75]);
    stream.stop().unwrap();
    assert!(stream.take_stopped_mixer().unwrap().is_none());
    assert!(stream.timing_snapshot().is_none());
}
#[test]
fn ordinary_joined_worker_error_returns_original_diagnostic_and_retains_rendered_model_for_recovery()
 {
    let (mut stream, _, release) = worker(true);
    release.send(()).unwrap();
    assert!(matches!(
        stream.stop(),
        Err(LinuxError::InvalidConfiguration(
            "original memory worker failure"
        ))
    ));
    assert_eq!(stream.snapshot().status, AlsaStatus::Failed { code: -32 });
    assert_eq!(stream.snapshot().failures, 1);
    let report = stream.last_render_report().unwrap();
    let mut mixer = stream.take_stopped_mixer().unwrap().unwrap();
    assert_eq!(mixer.frame_cursor(), report.counters.rendered_frames);
    let mut pcm = [0.; 1];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.5]);
    assert_eq!(stream.snapshot().failures, 1);
    assert!(stream.take_stopped_mixer().unwrap().is_none());
}
#[test]
fn worker_panic_never_claims_retirement_or_recovers_destroyed_worker_state() {
    let (_, mixer) = rig();
    let handle: JoinHandle<(Result<(), LinuxError>, Mixer)> = thread::spawn(move || {
        let _owned = mixer;
        panic!("controlled memory worker panic")
    });
    let mut stream = AlsaStream {
        configuration: config(),
        shared: Arc::new(Shared::new()),
        worker: Some(handle),
        recovered_mixer: None,
        retired: false,
    };
    assert!(matches!(stream.stop(), Err(LinuxError::WorkerPanicked)));
    assert_eq!(stream.snapshot().status, AlsaStatus::WorkerPanicked);
    assert!(matches!(
        stream.take_stopped_mixer(),
        Err(LinuxError::InvalidLifecycle)
    ));
    stream.stop().unwrap();
    assert!(matches!(
        stream.take_stopped_mixer(),
        Err(LinuxError::InvalidLifecycle)
    ));
    assert!(stream.recovered_mixer.is_none());
}
