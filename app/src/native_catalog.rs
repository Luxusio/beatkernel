//! Owned native startup with bounded incremental metadata and explicit exit joining.
use crate::player_chart::{
    scan_library_streamed_with, scan_library_with, ChartLibrary, LibraryEntry, ScanProgress,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// At most two queued publications; an entry batch contains at most 64 items.
#[derive(Debug)]
pub enum CatalogPublication {
    Entries {
        generation: u64,
        entries: Vec<LibraryEntry>,
    },
    MetadataComplete {
        generation: u64,
        diagnostics: Vec<String>,
    },
}
fn send_publication(
    sender: &SyncSender<CatalogPublication>,
    control: &CatalogControl,
    mut publication: CatalogPublication,
) -> Result<(), String> {
    loop {
        control.checkpoint()?;
        match sender.try_send(publication) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(pending)) => {
                publication = pending;
                thread::park_timeout(Duration::from_millis(1));
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err("catalog metadata receiver retired".into())
            }
        }
    }
}

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

/// One owned preparation thread for explicitly transferred `Send` data and
/// handles, optionally including a library scan. Native window acquisition
/// stays on the UI owner; renderer preparation returns its handles at join.
pub struct NativeCatalog<T: Send + 'static> {
    control: CatalogControl,
    worker: Option<JoinHandle<Result<T, String>>>,
    progress: ScanProgress,
    generation: Option<u64>,
    publications: Option<Receiver<CatalogPublication>>,
}
impl<T: Send + 'static> NativeCatalog<T> {
    pub fn spawn(
        root: PathBuf,
        prepare: impl FnOnce(ChartLibrary, &CatalogControl) -> Result<T, String> + Send + 'static,
    ) -> Result<Self, String> {
        Self::spawn_prepared(move |control| {
            let library = scan_library_with(&root, |progress| {
                if let Ok(mut latest) = control.progress.lock() {
                    *latest = progress;
                }
                !control.is_cancelled()
            })
            .map_err(|error| error.to_string())?;
            control.checkpoint()?;
            prepare(library, control)
        })
    }
    /// Admitted paths precede optional metadata/font completion. Sender pressure
    /// remains cancellation-aware and never blocks the UI consumer.
    pub fn spawn_streamed(
        root: PathBuf,
        generation: u64,
        prepare: impl FnOnce(ChartLibrary, &CatalogControl) -> Result<T, String> + Send + 'static,
    ) -> Result<Self, String> {
        if generation == 0 {
            return Err("catalog generation must be nonzero".into());
        }
        let (sender, receiver) = mpsc::sync_channel(2);
        let mut job = Self::spawn_prepared(move |control| {
            let mut batch = Vec::new();
            let mut first = true;
            let mut failure = None;
            let scanned = scan_library_streamed_with(
                &root,
                |progress| {
                    if let Ok(mut latest) = control.progress.lock() {
                        *latest = progress;
                    }
                    !control.is_cancelled()
                },
                |entry| {
                    batch.push(entry.clone());
                    if first || batch.len() == 64 {
                        first = false;
                        if let Err(error) = send_publication(
                            &sender,
                            control,
                            CatalogPublication::Entries {
                                generation,
                                entries: std::mem::take(&mut batch),
                            },
                        ) {
                            failure = Some(error);
                            return false;
                        }
                    }
                    true
                },
            );
            if let Some(error) = failure {
                return Err(error);
            }
            let library = scanned.map_err(|error| error.to_string())?;
            if !batch.is_empty() {
                send_publication(
                    &sender,
                    control,
                    CatalogPublication::Entries {
                        generation,
                        entries: batch,
                    },
                )?;
            }
            send_publication(
                &sender,
                control,
                CatalogPublication::MetadataComplete {
                    generation,
                    diagnostics: library.diagnostics.clone(),
                },
            )?;
            control.checkpoint()?;
            prepare(library, control)
        })?;
        job.generation = Some(generation);
        job.publications = Some(receiver);
        Ok(job)
    }
    pub fn generation(&self) -> Option<u64> {
        self.generation
    }
    /// One event poll acquires at most two bounded batches without waiting.
    pub fn drain_batches(&mut self) -> Vec<CatalogPublication> {
        let mut publications = Vec::new();
        if let Some(receiver) = &self.publications {
            for _ in 0..2 {
                match receiver.try_recv() {
                    Ok(publication) => publications.push(publication),
                    Err(_) => break,
                }
            }
        }
        publications
    }

    /// Prepare explicitly transferred owned data/handles without a directory
    /// scan, using the same cancellation checkpoints and joined-result
    /// ownership as library startup.
    pub fn spawn_prepared(
        prepare: impl FnOnce(&CatalogControl) -> Result<T, String> + Send + 'static,
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
                let prepared = prepare(&worker_control)?;
                worker_control.checkpoint()?;
                Ok(prepared)
            })
            .map_err(|error| format!("catalog worker could not start: {error}"))?;
        Ok(Self {
            control,
            worker: Some(worker),
            progress: ScanProgress::default(),
            generation: None,
            publications: None,
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
        // UI retirement retains unfinished owners explicitly. A generic Drop
        // cannot wait for an indivisible IO/decode operation; final exit joins.
        if self.is_finished() {
            let _ = self.join();
        }
    }
}

#[cfg(test)]
#[path = "native_catalog_fixtures.rs"]
mod fixtures;
