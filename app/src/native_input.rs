//! Bounded acquisition transport. Only the source owns native drain evidence.

use beatkernel::{
    input::PhysicalInputEvent,
    time::{ClockDomainId, ClockPoint},
};
use std::{fmt, time::Duration};

#[derive(Clone, Copy, Debug)]
pub struct CollectorConfig {
    pub domain: ClockDomainId,
    pub entries: usize,
    pub bytes: usize,
    pub max_payload_bytes: usize,
    pub service_quantum: usize,
    pub idle_wait: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectorError {
    InvalidConfiguration,
    Spawn(String),
    Initialization(String),
    Source(String),
    Close(String),
    Panic,
    Disconnected,
    EntryCapacity,
    ByteCapacity,
    PayloadCapacity,
    ServiceQuantum,
    DomainMismatch,
    CutRegression,
    FailedClose {
        primary: Box<CollectorError>,
        close: String,
    },
}
impl fmt::Display for CollectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native input collector: {self:?}")
    }
}
impl std::error::Error for CollectorError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct SourceDrain {
    /// Inclusive, conservative cut justified by every selected native source.
    pub completed_through: Option<ClockPoint>,
    /// No immediately available work; the collector waits with cancellation wakeup.
    pub idle: bool,
    /// Ordinary source/window completion, distinct from native failure.
    pub closed: bool,
}

/// Implementations must bound *all* native work, including ignored records, by
/// `quantum`, and must return without blocking on the gameplay consumer.
pub trait NativeInputSource {
    fn service(
        &mut self,
        sink: &mut InputPublisher<'_>,
        quantum: usize,
    ) -> Result<SourceDrain, String>;
    fn close(&mut self) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CollectorDrain {
    /// Total consumed FIFO records, including completed-drain markers.
    pub items: usize,
    pub backlog: bool,
    pub closed: bool,
    /// Only cuts consumed in this call; queue emptiness cannot manufacture one.
    pub completed_through: Option<ClockPoint>,
}

#[cfg(not(target_arch = "wasm32"))]
mod threaded {
    use super::*;
    use std::{
        collections::VecDeque,
        mem::size_of,
        panic::{catch_unwind, AssertUnwindSafe},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
            Arc,
        },
        thread::{self, JoinHandle},
    };

    enum Entry {
        Event(PhysicalInputEvent),
        Cut(ClockPoint),
    }
    impl Entry {
        fn bytes(&self) -> usize {
            let payload = match self {
                Self::Event(PhysicalInputEvent::RawHidReport(e)) => e.data.capacity(),
                Self::Event(PhysicalInputEvent::Custom(e)) => e.payload.capacity(),
                _ => 0,
            };
            size_of::<Self>().saturating_add(payload)
        }
    }
    struct Shared {
        cancel: AtomicBool,
        entries: AtomicUsize,
        bytes: AtomicUsize,
    }

    pub struct InputPublisher<'a> {
        sender: &'a SyncSender<Entry>,
        shared: &'a Shared,
        config: CollectorConfig,
        last_cut: Option<ClockPoint>,
        emitted: usize,
        failure: Option<CollectorError>,
    }
    impl InputPublisher<'_> {
        pub fn publish(&mut self, event: PhysicalInputEvent) -> Result<(), CollectorError> {
            let result = self.publish_event(event);
            if let Err(error) = &result {
                self.failure.get_or_insert_with(|| error.clone());
            }
            result
        }
        fn publish_event(&mut self, event: PhysicalInputEvent) -> Result<(), CollectorError> {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            if self.emitted >= self.config.service_quantum {
                return Err(CollectorError::ServiceQuantum);
            }
            if event.meta().clock_domain != self.config.domain {
                return Err(CollectorError::DomainMismatch);
            }
            let payload = match &event {
                PhysicalInputEvent::RawHidReport(e) => e.data.capacity(),
                PhysicalInputEvent::Custom(e) => e.payload.capacity(),
                _ => 0,
            };
            if payload > self.config.max_payload_bytes {
                return Err(CollectorError::PayloadCapacity);
            }
            self.send(Entry::Event(event))?;
            self.emitted += 1;
            Ok(())
        }
        fn send(&self, entry: Entry) -> Result<(), CollectorError> {
            let bytes = entry.bytes();
            if self
                .shared
                .bytes
                .load(Ordering::Acquire)
                .checked_add(bytes)
                .is_none_or(|total| total > self.config.bytes)
            {
                return Err(CollectorError::ByteCapacity);
            }
            self.shared.bytes.fetch_add(bytes, Ordering::AcqRel);
            self.shared.entries.fetch_add(1, Ordering::AcqRel);
            match self.sender.try_send(entry) {
                Ok(()) => Ok(()),
                Err(error) => {
                    self.shared.bytes.fetch_sub(bytes, Ordering::AcqRel);
                    self.shared.entries.fetch_sub(1, Ordering::AcqRel);
                    Err(match error {
                        TrySendError::Full(_) => CollectorError::EntryCapacity,
                        TrySendError::Disconnected(_) => CollectorError::Disconnected,
                    })
                }
            }
        }
        fn complete(&mut self, cut: ClockPoint) -> Result<(), CollectorError> {
            if cut.domain != self.config.domain {
                return Err(CollectorError::DomainMismatch);
            }
            if self
                .last_cut
                .is_some_and(|last| cut.timestamp < last.timestamp)
            {
                return Err(CollectorError::CutRegression);
            }
            // Repeated evidence does not consume transport capacity or change a cut.
            if self.last_cut == Some(cut) {
                return Ok(());
            }
            self.send(Entry::Cut(cut))?;
            self.last_cut = Some(cut);
            Ok(())
        }
    }

    pub struct NativeInputCollector {
        receiver: Receiver<Entry>,
        ready: Receiver<Result<(), CollectorError>>,
        terminal: Receiver<Result<(), CollectorError>>,
        outcome: Option<Result<(), CollectorError>>,
        shared: Arc<Shared>,
        worker: Option<JoinHandle<()>>,
        drain_limit: usize,
    }
    impl NativeInputCollector {
        pub fn spawn<F, S>(config: CollectorConfig, factory: F) -> Result<Self, CollectorError>
        where
            F: FnOnce() -> Result<S, String> + Send + 'static,
            S: NativeInputSource + 'static,
        {
            if config.entries == 0
                || config.entries > 65536
                || config.bytes < size_of::<Entry>()
                || config.bytes > 64 * 1024 * 1024
                || config.max_payload_bytes > config.bytes
                || config.service_quantum == 0
                || config.service_quantum > 65536
                || config.idle_wait.is_zero()
                || config.idle_wait > Duration::from_secs(1)
            {
                return Err(CollectorError::InvalidConfiguration);
            }
            let (sender, receiver) = mpsc::sync_channel(config.entries);
            let (ready_sender, ready) = mpsc::sync_channel(1);
            let (terminal_sender, terminal) = mpsc::sync_channel(1);
            let shared = Arc::new(Shared {
                cancel: AtomicBool::new(false),
                entries: AtomicUsize::new(0),
                bytes: AtomicUsize::new(0),
            });
            let owner = shared.clone();
            let worker = thread::Builder::new()
                .name("native-input".into())
                .spawn(move || {
                    let result = run(config, factory, &sender, &owner, &ready_sender);
                    let _ = terminal_sender.try_send(result);
                })
                .map_err(|e| CollectorError::Spawn(e.to_string()))?;
            Ok(Self {
                receiver,
                ready,
                terminal,
                outcome: None,
                shared,
                worker: Some(worker),
                drain_limit: config.entries,
            })
        }
        pub fn wait_ready(&mut self) -> Result<(), CollectorError> {
            self.ready
                .recv()
                .map_err(|_| CollectorError::Disconnected)?
        }
        pub fn wake(&self) {
            if let Some(worker) = &self.worker {
                worker.thread().unpark();
            }
        }
        pub fn cancel(&self) {
            self.shared.cancel.store(true, Ordering::Release);
            self.wake();
        }
        pub fn status(&mut self) -> Result<bool, CollectorError> {
            if self.outcome.is_none() {
                match self.terminal.try_recv() {
                    Ok(result) => self.outcome = Some(result),
                    Err(TryRecvError::Empty) => return Ok(false),
                    Err(TryRecvError::Disconnected) => {
                        self.outcome = Some(Err(CollectorError::Disconnected))
                    }
                }
            }
            self.outcome
                .as_ref()
                .expect("terminal outcome")
                .clone()
                .map(|()| true)
        }
        pub fn drain(
            &mut self,
            events: &mut VecDeque<PhysicalInputEvent>,
            max_items: usize,
        ) -> Result<CollectorDrain, CollectorError> {
            let closed = self.status()?;
            let mut result = CollectorDrain::default();
            result.closed = closed;
            for _ in 0..max_items.min(self.drain_limit) {
                let entry = match self.receiver.try_recv() {
                    Ok(entry) => entry,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.status()?;
                        break;
                    }
                };
                self.shared.bytes.fetch_sub(entry.bytes(), Ordering::AcqRel);
                self.shared.entries.fetch_sub(1, Ordering::AcqRel);
                match entry {
                    Entry::Event(event) => events.push_back(event),
                    Entry::Cut(cut) => result.completed_through = Some(cut),
                }
                result.items += 1;
            }
            result.backlog = self.shared.entries.load(Ordering::Acquire) != 0;
            Ok(result)
        }
        pub fn stop_and_join(&mut self) -> Result<(), CollectorError> {
            self.cancel();
            if let Some(worker) = self.worker.take() {
                if worker.join().is_err() {
                    self.outcome = Some(Err(CollectorError::Panic));
                }
            }
            self.status().map(|_| ())
        }
    }
    impl Drop for NativeInputCollector {
        fn drop(&mut self) {
            let _ = self.stop_and_join();
        }
    }

    fn run<F, S>(
        config: CollectorConfig,
        factory: F,
        sender: &SyncSender<Entry>,
        shared: &Shared,
        ready: &SyncSender<Result<(), CollectorError>>,
    ) -> Result<(), CollectorError>
    where
        F: FnOnce() -> Result<S, String>,
        S: NativeInputSource,
    {
        let mut source = match catch_unwind(AssertUnwindSafe(factory)) {
            Ok(Ok(source)) => source,
            Ok(Err(error)) => {
                let result = Err(CollectorError::Initialization(error));
                let _ = ready.try_send(result.clone());
                return result;
            }
            Err(_) => {
                let result = Err(CollectorError::Panic);
                let _ = ready.try_send(result.clone());
                return result;
            }
        };
        let _ = ready.try_send(Ok(()));
        let primary = catch_unwind(AssertUnwindSafe(|| {
            let mut publisher = InputPublisher {
                sender,
                shared,
                config,
                last_cut: None,
                emitted: 0,
                failure: None,
            };
            while !shared.cancel.load(Ordering::Acquire) {
                publisher.emitted = 0;
                let drained = source.service(&mut publisher, config.service_quantum);
                if let Some(error) = publisher.failure.take() {
                    return Err(error);
                }
                let drained = drained.map_err(CollectorError::Source)?;
                if let Some(cut) = drained.completed_through {
                    publisher.complete(cut)?;
                }
                if drained.closed {
                    return Ok(());
                }
                if drained.idle && !shared.cancel.load(Ordering::Acquire) {
                    thread::park_timeout(config.idle_wait);
                }
            }
            Ok(())
        }))
        .unwrap_or(Err(CollectorError::Panic));
        let close = catch_unwind(AssertUnwindSafe(|| source.close()));
        let dropped = catch_unwind(AssertUnwindSafe(|| drop(source)));
        let close = match close {
            Ok(result) => result,
            Err(_) => Err("source close panicked".into()),
        };
        let close = if dropped.is_err() {
            Err("source drop panicked".into())
        } else {
            close
        };
        match (primary, close) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(()), Err(close)) => Err(CollectorError::Close(close)),
            (Err(primary), Err(close)) => Err(CollectorError::FailedClose {
                primary: Box::new(primary),
                close,
            }),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use threaded::{InputPublisher, NativeInputCollector};

// Native acquisition is unavailable on WASM; retain pure contract types without
// importing a thread implementation into browser-only builds.
#[cfg(target_arch = "wasm32")]
pub struct InputPublisher<'a>(std::marker::PhantomData<&'a ()>);
