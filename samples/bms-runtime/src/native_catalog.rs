//! One native startup operation. Only counters are observed before its join.
use crate::player_chart::{ChartLibrary, ScanProgress, scan_library_with};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

#[derive(Clone)]
pub struct CatalogControl {
    cancelled: Arc<AtomicBool>,
    progress: Arc<Mutex<ScanProgress>>,
}
impl CatalogControl {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn checkpoint(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("catalog preparation cancelled".into())
        } else {
            Ok(())
        }
    }
}

/// The preparation callback runs on the same thread as scanning. It may build
/// CPU-only search/font data; native/UI/GPU handles remain with their owners.
pub struct NativeCatalog<T: Send + 'static> {
    control: CatalogControl,
    worker: Option<JoinHandle<Result<T, String>>>,
    progress: ScanProgress,
}
impl<T: Send + 'static> NativeCatalog<T> {
    pub fn spawn(
        root: PathBuf,
        prepare: impl FnOnce(ChartLibrary, &CatalogControl) -> Result<T, String> + Send + 'static,
    ) -> Result<Self, String> {
        let control = CatalogControl {
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(ScanProgress::default())),
        };
        let worker_control = control.clone();
        let worker = thread::Builder::new()
            .name("bms-catalog".into())
            .spawn(move || {
                worker_control.checkpoint()?;
                let library = scan_library_with(&root, |progress| {
                    if let Ok(mut latest) = worker_control.progress.lock() {
                        *latest = progress;
                    }
                    !worker_control.is_cancelled()
                })
                .map_err(|error| error.to_string())?;
                worker_control.checkpoint()?;
                let prepared = prepare(library, &worker_control)?;
                worker_control.checkpoint()?;
                Ok(prepared)
            })
            .map_err(|error| format!("catalog worker could not start: {error}"))?;
        Ok(Self {
            control,
            worker: Some(worker),
            progress: ScanProgress::default(),
        })
    }
    pub fn cancel(&self) {
        self.control.cancelled.store(true, Ordering::Release);
    }
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
    /// The UI never waits for the writer of the coalesced latest counter slot.
    pub fn progress(&mut self) -> ScanProgress {
        if let Ok(latest) = self.control.progress.try_lock() {
            self.progress = *latest;
        }
        self.progress
    }
    pub fn poll(&mut self) -> Option<Result<T, String>> {
        if self.is_finished() {
            self.join()
        } else {
            None
        }
    }
    /// Explicit blocking join for final ownership release, never ordinary UI polling.
    pub fn join(&mut self) -> Option<Result<T, String>> {
        let worker = self.worker.take()?;
        let result = worker
            .join()
            .unwrap_or_else(|_| Err("catalog worker panicked".into()));
        Some(if self.control.is_cancelled() {
            Err("catalog preparation cancelled".into())
        } else {
            result
        })
    }
}
impl<T: Send + 'static> Drop for NativeCatalog<T> {
    fn drop(&mut self) {
        self.cancel();
        let _ = self.join();
    }
}

#[cfg(test)]
#[path = "native_catalog_fixtures.rs"]
mod fixtures;
