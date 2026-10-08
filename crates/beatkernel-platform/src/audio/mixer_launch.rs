//! Cold unique mixer launch ownership shared by native worker backends.
use beatkernel::audio::MixerOpenFailure;
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
pub(crate) fn launch_worker<T, S, F, O>(
    spawner: S,
    mixer: O,
    work: F,
) -> Result<JoinHandle<T>, MixerOpenFailure<io::Error, O>>
where
    T: Send + 'static,
    O: Send + 'static,
    S: WorkerSpawner<T>,
    F: FnOnce(O) -> T + Send + 'static,
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
            Err(MixerOpenFailure::new_state(error, mixer))
        }
    }
}
pub(crate) fn join_open_failure<T, E, F, O>(
    worker: JoinHandle<T>,
    original: E,
    extract: F,
) -> MixerOpenFailure<E, O>
where
    F: FnOnce(T) -> Option<O>,
{
    let mixer = worker.join().ok().and_then(extract);
    MixerOpenFailure::new_state(original, mixer)
}
#[cfg(test)]
#[path = "mixer_launch_fixtures.rs"]
mod mixer_launch_fixtures;
