//! Actual memory-worker stop/take; authored threads never touch native PCM.
use super::*;
use beatkernel::audio::{
    AudioFormat, AudioLimits, MixerConfig, PcmLimits, SampleBank, StoppedMixerSource, command_queue,
};
#[test]
fn advanced_paused_mixer_basis_is_captured_before_worker_and_survives_join_and_take_with_relative_counters()
 {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            Timestamp::from_nanos(-1_000_000_000),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    let basis = mixer.output_frame_basis();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (5, 2)
    );
    let device_format = DeviceFormat::new(3, 1, SampleEncoding::Float32, None).unwrap();
    let configuration = AlsaAppliedConfig {
        requested: AlsaRequest {
            device: "memory-only".into(),
            format: device_format,
            buffer_frames: 8,
            period_frames: 1,
            allow_size_rounding: false,
            monotonic_domain: ClockDomainId(1),
        },
        format: device_format,
        buffer_frames: 8,
        period_frames: 1,
        sizing_adjusted: false,
        output_domain: ClockDomainId(u32::MAX),
        output_origin: Timestamp::from_nanos(-1_000_000_000),
    };
    let shared = Arc::new(Shared::new());
    let worker_shared = shared.clone();
    let (release, ready) = mpsc::channel();
    let worker = thread::spawn(move || {
        ready.recv().unwrap();
        let report = mixer.render(&mut [0.; 1]).unwrap();
        worker_shared
            .rendered
            .store(u64::try_from(report.frames).unwrap(), Ordering::Relaxed);
        worker_shared
            .submitted
            .store(u64::try_from(report.frames).unwrap(), Ordering::Relaxed);
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
        (Ok(()), mixer)
    });
    let mut stream = AlsaStream {
        configuration,
        basis,
        shared,
        worker: Some(worker),
        recovered_mixer: None,
        retired: false,
    };
    assert_eq!(stream.frame_basis(), basis);
    assert_eq!(stream.snapshot().rendered_frames, 0);
    assert_eq!(
        stream
            .frame_basis()
            .point_at_stream_frame(1)
            .unwrap()
            .timestamp,
        Timestamp::from_nanos(1_000_000_000)
    );
    release.send(()).unwrap();
    stream.stop().unwrap();
    let report = stream.last_render_report().unwrap();
    assert_eq!((report.start_frame, report.playback_start_frame), (5, 2));
    assert_eq!(stream.snapshot().rendered_frames, 1);
    assert_eq!(stream.snapshot().submitted_frames, 1);
    assert_eq!(stream.frame_basis(), basis);
    let recovered = stream.take_stopped_mixer().unwrap().unwrap();
    assert_eq!(
        (recovered.frame_cursor(), recovered.playback_frame_cursor()),
        (6, 2)
    );
    assert_eq!(stream.frame_basis(), basis);
    assert!(stream.take_stopped_mixer().unwrap().is_none());
    assert_eq!(stream.frame_basis(), basis);
}
