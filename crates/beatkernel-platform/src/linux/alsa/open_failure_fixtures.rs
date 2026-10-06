//! Deferred real launch/join policy with memory workers, never NativePcm.
use super::*;
use beatkernel::audio::{
    AudioCommand, AudioFormat, AudioLimits, CommandProducer, MixerConfig, PcmLimits, PcmSample,
    SampleBank, SampleId, VoiceId, command_queue,
};
fn rig() -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
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
    let mut mixer = Mixer::new(
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
    assert_eq!(
        {
            let mut pcm = [0.; 2];
            mixer.render(&mut pcm).unwrap();
            pcm
        },
        [0.25, 0.5]
    );
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_500_000_000),
            gain: 1.,
        })
        .unwrap();
    (producer, mixer)
}
fn request() -> AlsaRequest {
    AlsaRequest {
        device: "memory-only-never-open".into(),
        format: DeviceFormat::new(4, 1, SampleEncoding::Float32, None).unwrap(),
        buffer_frames: 8,
        period_frames: 1,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(1),
    }
}
fn recovered_pcm(mut mixer: Mixer, producer: &mut CommandProducer, physical: u64) {
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.is_paused()
        ),
        (physical, 2, true)
    );
    producer.request_pause(false);
    let mut output = [0.; 6];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.75, 1., 0., 0., 0.25, 0.5]);
}
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn actual_open_preflight_returns_advanced_original_mixer_before_native_acquisition_and_legacy_error_stays_compatible()
 {
    for case in 0..6 {
        let (mut producer, mixer) = rig();
        let before = mixer.output_frame_basis();
        let counters = mixer.counters();
        let mut invalid = request();
        match case {
            0 => invalid.device.clear(),
            1 => invalid.device = "bad\0name".into(),
            2 => invalid.period_frames = 0,
            3 => invalid.buffer_frames = 1,
            4 => invalid.format = DeviceFormat::new(5, 1, SampleEncoding::Float32, None).unwrap(),
            _ => {
                invalid.format = DeviceFormat::new(4, 1, SampleEncoding::Float32, Some(1)).unwrap()
            }
        }
        let failure = match AlsaStream::open_recoverable(invalid.clone(), mixer) {
            Err(failure) => failure,
            Ok(_) => panic!("invalid metadata must refuse before device IO"),
        };
        assert!(matches!(
            failure.error(),
            LinuxError::InvalidConfiguration(_)
        ));
        assert_eq!(failure.mixer().unwrap().output_frame_basis(), before);
        assert_eq!(failure.mixer().unwrap().counters(), counters);
        let (_, legacy_mixer) = rig();
        let legacy = match AlsaStream::open(invalid, legacy_mixer) {
            Err(error) => error,
            Ok(_) => panic!("legacy invalid metadata must refuse"),
        };
        assert_eq!(legacy.to_string(), failure.error().to_string());
        let (_, mixer) = failure.into_parts();
        recovered_pcm(mixer.unwrap(), &mut producer, 5);
    }
}
#[derive(Debug)]
struct LaunchRefusal(u64);
impl std::fmt::Display for LaunchRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original injected launch failure")
    }
}
impl std::error::Error for LaunchRefusal {}
struct RefusingSpawner {
    error: std::io::Error,
}
impl WorkerSpawner<WorkerExit> for RefusingSpawner {
    fn spawn<F>(self, _work: F) -> std::io::Result<JoinHandle<WorkerExit>>
    where
        F: FnOnce() -> WorkerExit + Send + 'static,
    {
        Err(self.error)
    }
}
#[test]
fn actual_launch_helper_keeps_control_owned_mixer_and_original_spawn_error_without_running_work() {
    let (mut producer, mixer) = rig();
    let basis = mixer.output_frame_basis();
    let typed = Box::new(LaunchRefusal(99));
    let pointer = typed.as_ref() as *const LaunchRefusal;
    let spawner = RefusingSpawner {
        error: std::io::Error::new(std::io::ErrorKind::WouldBlock, typed),
    };
    let entered = Arc::new(AtomicBool::new(false));
    let entered_work = entered.clone();
    let failure = match launch_worker(spawner, mixer, move |mixer| {
        entered_work.store(true, Ordering::Release);
        (Ok(()), mixer)
    }) {
        Err(failure) => failure,
        Ok(_) => panic!("injected spawner must refuse"),
    };
    assert!(!entered.load(Ordering::Acquire));
    match failure.error() {
        LinuxError::Io(error) => {
            assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
            let typed = error
                .get_ref()
                .unwrap()
                .downcast_ref::<LaunchRefusal>()
                .unwrap();
            assert_eq!(typed.0, 99);
            assert_eq!(typed as *const LaunchRefusal, pointer);
        }
        _ => panic!("original launch IO error required"),
    }
    assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
    let (_, mixer) = failure.into_parts();
    recovered_pcm(mixer.unwrap(), &mut producer, 5);
}
#[test]
fn actual_normal_worker_join_returns_unique_mixer_and_original_setup_error_even_if_worker_also_reports_failure()
 {
    let (mut producer, mixer) = rig();
    let worker = launch_worker(NativeWorkerSpawner, mixer, |mut mixer| {
        mixer.render(&mut [0.; 1]).unwrap();
        (
            Err(LinuxError::InvalidConfiguration("secondary worker result")),
            mixer,
        )
    });
    let worker = match worker {
        Ok(worker) => worker,
        Err(_) => panic!("memory worker launch required"),
    };
    let failure = join_open_failure(
        worker,
        LinuxError::InvalidConfiguration("original setup refusal"),
    );
    assert!(matches!(
        failure.error(),
        LinuxError::InvalidConfiguration("original setup refusal")
    ));
    assert_eq!(failure.mixer().unwrap().frame_cursor(), 6);
    let (_, mixer) = failure.into_parts();
    recovered_pcm(mixer.unwrap(), &mut producer, 6);
}
#[test]
fn dropped_startup_sender_then_normal_join_recovers_original_mixer_without_claiming_native_ready() {
    let (mut producer, mixer) = rig();
    let (sender, receiver) = mpsc::sync_channel::<Result<AlsaAppliedConfig, LinuxError>>(1);
    let worker = match launch_worker(NativeWorkerSpawner, mixer, move |mixer| {
        drop(sender);
        (Ok(()), mixer)
    }) {
        Ok(worker) => worker,
        Err(_) => panic!("memory worker launch required"),
    };
    assert!(receiver.recv().is_err());
    let failure = join_open_failure(worker, LinuxError::WorkerPanicked);
    assert!(matches!(failure.error(), LinuxError::WorkerPanicked));
    let (_, mixer) = failure.into_parts();
    recovered_pcm(mixer.unwrap(), &mut producer, 5);
}
#[test]
fn panicked_worker_join_preserves_original_startup_error_and_explicit_unavailable_recovery() {
    let (_, mixer) = rig();
    let worker = match launch_worker(NativeWorkerSpawner, mixer, |mixer| {
        let _original = mixer;
        panic!("controlled memory startup panic")
    }) {
        Ok(worker) => worker,
        Err(_) => panic!("memory worker launch required"),
    };
    let failure = join_open_failure(
        worker,
        LinuxError::InvalidConfiguration("original startup receipt failure"),
    );
    assert!(matches!(
        failure.error(),
        LinuxError::InvalidConfiguration("original startup receipt failure")
    ));
    assert!(failure.mixer().is_none());
    let (_, mixer) = failure.into_parts();
    assert!(mixer.is_none());
}
