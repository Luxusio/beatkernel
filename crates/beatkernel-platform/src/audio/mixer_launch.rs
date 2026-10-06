//! Cold unique mixer launch ownership shared by native worker backends.
use beatkernel::audio::{Mixer, MixerOpenFailure};
use std::{
    io,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

pub(crate) trait WorkerSpawner<T: Send + 'static> {
    fn spawn<F>(self, work: F) -> io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static;
}
pub(crate) fn launch_worker<T, S, F>(
    spawner: S,
    mixer: Mixer,
    work: F,
) -> Result<JoinHandle<T>, MixerOpenFailure<io::Error>>
where
    T: Send + 'static,
    S: WorkerSpawner<T>,
    F: FnOnce(Mixer) -> T + Send + 'static,
{
    let slot = Arc::new(Mutex::new(Some(mixer)));
    let worker_slot = Arc::clone(&slot);
    let launched = spawner.spawn(move || {
        let mixer = worker_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("worker claims one launch mixer");
        // No launch lock/reference enters native setup or render processing.
        drop(worker_slot);
        work(mixer)
    });
    match launched {
        Ok(worker) => Ok(worker),
        Err(error) => {
            let mixer = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            Err(MixerOpenFailure::new(error, mixer))
        }
    }
}
pub(crate) fn join_open_failure<T, E, F>(
    worker: JoinHandle<T>,
    original: E,
    extract: F,
) -> MixerOpenFailure<E>
where
    F: FnOnce(T) -> Option<Mixer>,
{
    let mixer = worker.join().ok().and_then(extract);
    MixerOpenFailure::new(original, mixer)
}
#[cfg(test)]
#[path = "mixer_launch_fixtures.rs"]
mod mixer_launch_fixtures;
