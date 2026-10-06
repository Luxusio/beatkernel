//! Deferred shared launch/retirement policy with memory-only worker bodies.
use super::*;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
struct MemorySpawner;
impl<T: Send + 'static> WorkerSpawner<T> for MemorySpawner {
    fn spawn<F>(self, work: F) -> std::io::Result<std::thread::JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
    {
        std::thread::Builder::new().spawn(work)
    }
}
struct RefusingSpawner(std::io::Error);
impl<T: Send + 'static> WorkerSpawner<T> for RefusingSpawner {
    fn spawn<F>(self, _work: F) -> std::io::Result<std::thread::JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
    {
        Err(self.0)
    }
}
struct BackendError(Box<u64>); // No Clone/Debug/Display/Error constraint.
#[derive(Debug)]
struct LaunchError(u64);
impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original launch refusal")
    }
}
impl std::error::Error for LaunchError {}
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
    mixer.render(&mut [0.; 2]).unwrap();
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
fn assert_pcm(mut mixer: Mixer, producer: &mut CommandProducer, physical: u64) {
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.is_paused()
        ),
        (physical, 2, true)
    );
    producer.request_pause(false);
    let mut pcm = [0.; 6];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.75, 1., 0., 0., 0.25, 0.5]);
}
#[test]
fn shared_spawn_refusal_retains_original_io_payload_and_control_owned_mixer_before_any_work() {
    let (mut producer, mixer) = rig();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let error = Box::new(LaunchError(71));
    let pointer = error.as_ref() as *const LaunchError;
    let error: Box<dyn std::error::Error + Send + Sync> = error;
    let entered = Arc::new(AtomicBool::new(false));
    let worker_entered = entered.clone();
    let result = launch_worker(
        RefusingSpawner(std::io::Error::new(std::io::ErrorKind::WouldBlock, error)),
        mixer,
        move |mixer| {
            worker_entered.store(true, Ordering::Release);
            Some(mixer)
        },
    );
    let failure = match result {
        Err(failure) => failure,
        Ok(_) => panic!("injected refusal required"),
    };
    assert!(!entered.load(Ordering::Acquire));
    assert_eq!(failure.error().kind(), std::io::ErrorKind::WouldBlock);
    let payload = failure
        .error()
        .get_ref()
        .unwrap()
        .downcast_ref::<LaunchError>()
        .unwrap();
    assert_eq!(payload.0, 71);
    assert_eq!(payload as *const LaunchError, pointer);
    assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
    assert_eq!(failure.mixer().unwrap().counters(), counters);
    let (_, mixer) = failure.into_parts();
    assert_pcm(mixer.unwrap(), &mut producer, 5);
}
#[test]
fn shared_tuple_worker_join_retains_original_opaque_backend_error_even_after_secondary_worker_failure()
 {
    let (mut producer, mixer) = rig();
    let worker = match launch_worker(MemorySpawner, mixer, |mut mixer| {
        mixer.render(&mut [0.; 1]).unwrap();
        (Err::<(), _>(BackendError(Box::new(9))), mixer)
    }) {
        Ok(worker) => worker,
        Err(_) => panic!("memory launch required"),
    };
    let original = BackendError(Box::new(17));
    let pointer = original.0.as_ref() as *const u64;
    let failure = join_open_failure(worker, original, |(_, mixer)| Some(mixer));
    assert_eq!(failure.error().0.as_ref() as *const u64, pointer);
    let (error, mixer) = failure.into_parts();
    assert_eq!(error.0.as_ref() as *const u64, pointer);
    assert_pcm(mixer.unwrap(), &mut producer, 6);
}
#[test]
fn shared_option_worker_join_moves_the_native_style_available_mixer_after_retirement() {
    let (mut producer, mixer) = rig();
    let worker = match launch_worker(MemorySpawner, mixer, |mut mixer| {
        mixer.render(&mut [0.; 1]).unwrap();
        Some(mixer)
    }) {
        Ok(worker) => worker,
        Err(_) => panic!("memory launch required"),
    };
    let original = BackendError(Box::new(u64::MAX));
    let pointer = original.0.as_ref() as *const u64;
    let failure = join_open_failure(worker, original, |mixer| mixer);
    assert_eq!(failure.error().0.as_ref() as *const u64, pointer);
    let (_, mixer) = failure.into_parts();
    assert_pcm(mixer.unwrap(), &mut producer, 6);
}
#[test]
fn shared_option_none_never_substitutes_an_empty_mixer_and_preserves_original_error() {
    let (_, mixer) = rig();
    let worker = match launch_worker(MemorySpawner, mixer, |mixer| {
        drop(mixer);
        None::<Mixer>
    }) {
        Ok(worker) => worker,
        Err(_) => panic!("memory launch required"),
    };
    let original = BackendError(Box::new(23));
    let pointer = original.0.as_ref() as *const u64;
    let failure = join_open_failure(worker, original, |mixer| mixer);
    assert!(failure.mixer().is_none());
    let (error, mixer) = failure.into_parts();
    assert!(mixer.is_none());
    assert_eq!(error.0.as_ref() as *const u64, pointer);
}
#[test]
fn shared_panicked_worker_join_returns_original_error_and_explicitly_unavailable_ownership() {
    let (_, mixer) = rig();
    let worker = match launch_worker(MemorySpawner, mixer, |mixer| -> Option<Mixer> {
        let _owned = mixer;
        panic!("controlled shared worker panic")
    }) {
        Ok(worker) => worker,
        Err(_) => panic!("memory launch required"),
    };
    let original = BackendError(Box::new(31));
    let pointer = original.0.as_ref() as *const u64;
    let failure = join_open_failure(worker, original, |mixer| mixer);
    assert!(failure.mixer().is_none());
    assert_eq!(failure.error().0.as_ref() as *const u64, pointer);
    let (_, mixer) = failure.into_parts();
    assert!(mixer.is_none());
}
