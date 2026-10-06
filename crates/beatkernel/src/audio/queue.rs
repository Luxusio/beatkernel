use super::{AudioCommand, AudioError, AudioLimits, SampleId, VoiceId};
use crate::{time::Timestamp, transport::Rate};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering},
};

/// Producer-local saturating admission counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueCounters {
    /// Commands successfully published.
    pub accepted: u64,
    /// Commands returned because all slots were occupied.
    pub full: u64,
    /// Commands returned because the consumer had disconnected.
    pub disconnected: u64,
}

/// Producer admission failure; the exact original scalar command is returned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandPushError {
    /// Original command, including non-finite gain bit patterns.
    pub command: AudioCommand,
    /// Why the command was not published.
    pub reason: QueuePushError,
}

/// Allocation-free producer admission failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuePushError {
    /// The next ring slot remains occupied; no existing command is overwritten.
    Full,
    /// The consumer endpoint has been dropped.
    Disconnected,
}

/// Allocation-free consumer observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuePopError {
    /// No command is published, but the producer remains connected.
    Empty,
    /// The producer has been dropped and all published commands were drained.
    Disconnected,
}

/// Sole queue producer. It is intentionally not clonable.
pub struct CommandProducer {
    shared: Arc<Shared>,
    cursor: usize,
    counters: QueueCounters,
}

/// Sole queue consumer. It is intentionally not clonable.
pub struct CommandConsumer {
    shared: Arc<Shared>,
    cursor: usize,
}

struct Shared {
    slots: Vec<Slot>,
    producer_alive: AtomicBool,
    consumer_alive: AtomicBool,
    pause_requested: AtomicBool,
    pause_held: AtomicBool,
    start_gated: bool,
    start_frame: AtomicU64,
    start_armed: AtomicBool,
    applied_start_frame: AtomicU64,
    start_applied: AtomicBool,
    physical_frontier: AtomicU64,
}

impl Shared {
    fn applied_start_frame(&self) -> Option<u64> {
        self.start_applied
            .load(Ordering::Acquire)
            .then(|| self.applied_start_frame.load(Ordering::Relaxed))
    }
}

/// Exclusive cold pause lease over the existing command queue allocation.
/// Dropping it leaves pause requested; resume must be explicitly reissued.
pub struct PauseHold {
    shared: Arc<Shared>,
}
/// The queue already owns an outstanding exclusive pause lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseHoldError {
    /// A prior hold has not been released.
    AlreadyHeld,
}
impl std::fmt::Display for PauseHoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("audio pause is already held")
    }
}
impl std::error::Error for PauseHoldError {}
impl Drop for PauseHold {
    fn drop(&mut self) {
        self.shared.pause_requested.store(true, Ordering::Release);
        self.shared.pause_held.store(false, Ordering::Release);
    }
}
struct Slot {
    ready: AtomicBool,
    tag: AtomicU8,
    at: AtomicI64,
    first: AtomicU64,
    second: AtomicU64,
    signed: AtomicI64,
    gain: AtomicU32,
}

impl Slot {
    fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            tag: AtomicU8::new(0),
            at: AtomicI64::new(0),
            first: AtomicU64::new(0),
            second: AtomicU64::new(0),
            signed: AtomicI64::new(0),
            gain: AtomicU32::new(0),
        }
    }

    // Called only by the sole producer after Acquire-observing a free slot.
    fn store(&self, command: AudioCommand) {
        self.at.store(command.at().as_nanos(), Ordering::Relaxed);
        let tag = match command {
            AudioCommand::Play {
                voice,
                sample,
                gain,
                ..
            } => {
                self.first.store(voice.0, Ordering::Relaxed);
                self.second.store(sample.0, Ordering::Relaxed);
                self.gain.store(gain.to_bits(), Ordering::Relaxed);
                0
            }
            AudioCommand::Stop { voice, .. } => {
                self.first.store(voice.0, Ordering::Relaxed);
                1
            }
            AudioCommand::SetRate { rate, .. } => {
                self.signed.store(rate.numerator(), Ordering::Relaxed);
                self.first.store(rate.denominator(), Ordering::Relaxed);
                2
            }
            AudioCommand::Seek { song_time, .. } => {
                self.signed.store(song_time.as_nanos(), Ordering::Relaxed);
                3
            }
        };
        self.tag.store(tag, Ordering::Relaxed);
        self.ready.store(true, Ordering::Release);
    }

    // Called only by the sole consumer after Acquire-observing publication.
    fn load(&self) -> AudioCommand {
        let at = Timestamp::from_nanos(self.at.load(Ordering::Relaxed));
        let command = match self.tag.load(Ordering::Relaxed) {
            0 => AudioCommand::Play {
                voice: VoiceId(self.first.load(Ordering::Relaxed)),
                sample: SampleId(self.second.load(Ordering::Relaxed)),
                at,
                gain: f32::from_bits(self.gain.load(Ordering::Relaxed)),
            },
            1 => AudioCommand::Stop {
                voice: VoiceId(self.first.load(Ordering::Relaxed)),
                at,
            },
            2 => AudioCommand::SetRate {
                rate: Rate::new(
                    self.signed.load(Ordering::Relaxed),
                    self.first.load(Ordering::Relaxed),
                )
                .expect("published Rate retains its nonzero denominator"),
                at,
            },
            3 => AudioCommand::Seek {
                song_time: Timestamp::from_nanos(self.signed.load(Ordering::Relaxed)),
                at,
            },
            _ => unreachable!("only scalar AudioCommand tags are published"),
        };
        self.ready.store(false, Ordering::Release);
        command
    }
}

/// Allocates a bounded scalar SPSC queue before the real-time boundary.
///
/// Capacity must be 1 through [`AudioLimits::MAX_COMMANDS`]. Slot storage uses
/// fallible reservation; the one Arc control allocation follows Rust's usual
/// allocation-failure behavior. Operations never allocate or free storage.
///
/// Publication proof: the producer observes `ready=false` with Acquire, writes
/// scalar payload atomics, then Release-publishes `true`. The consumer observes
/// `true` with Acquire, reads the payload, then Release-reclaims `false`. These
/// edges prevent reuse while reads are in progress. Each endpoint exclusively
/// owns its bounded cursor. On wrap, atomic write-read coherence prevents the
/// producer observing an old false preceding its own true publication, or the
/// consumer observing an old true preceding its own false reclamation. A bool
/// therefore does not introduce ABA ownership; no unbounded sequence is used.
///
/// Keep the consumer owned until callback quiescence. Endpoint destruction
/// publishes disconnect; the final endpoint destruction frees backing storage
/// and must happen outside rendering. Concurrent stress supplements this proof.
pub fn command_queue(capacity: usize) -> Result<(CommandProducer, CommandConsumer), AudioError> {
    queue(capacity, false)
}

/// Allocates a queue whose mixer initially advances only silent physical frames.
/// Commands remain queued until its one-shot exact start target is reached.
/// Storage and disconnect rules are identical to [`command_queue`].
pub fn command_queue_with_start_gate(
    capacity: usize,
) -> Result<(CommandProducer, CommandConsumer), AudioError> {
    queue(capacity, true)
}
fn queue(
    capacity: usize,
    start_gated: bool,
) -> Result<(CommandProducer, CommandConsumer), AudioError> {
    if capacity == 0 || capacity > AudioLimits::MAX_COMMANDS {
        return Err(AudioError::InvalidCapacity);
    }
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(capacity)
        .map_err(|_| AudioError::AllocationFailed)?;
    slots.resize_with(capacity, Slot::new);
    let shared = Arc::new(Shared {
        slots,
        producer_alive: AtomicBool::new(true),
        consumer_alive: AtomicBool::new(true),
        pause_requested: AtomicBool::new(false),
        pause_held: AtomicBool::new(false),
        start_gated,
        start_frame: AtomicU64::new(0),
        start_armed: AtomicBool::new(false),
        applied_start_frame: AtomicU64::new(0),
        start_applied: AtomicBool::new(false),
        physical_frontier: AtomicU64::new(0),
    });
    Ok((
        CommandProducer {
            shared: Arc::clone(&shared),
            cursor: 0,
            counters: QueueCounters::default(),
        },
        CommandConsumer { shared, cursor: 0 },
    ))
}

impl CommandProducer {
    /// Arms one exact initial physical frame independently of command capacity.
    /// A callback racing admission rechecks the target; missed targets never clamp.
    /// A concurrent consumer drop may follow the connected observation.
    pub fn schedule_start_at(&mut self, frame: u64) -> Result<(), AudioError> {
        if !self.shared.start_gated {
            return Err(AudioError::StartGateUnavailable);
        }
        if self.is_disconnected() {
            return Err(AudioError::StartGateDisconnected);
        }
        if self.shared.start_armed.load(Ordering::Acquire) {
            return Err(AudioError::StartGateAlreadyArmed);
        }
        if frame < self.shared.physical_frontier.load(Ordering::Acquire) {
            return Err(AudioError::StartGateMissed);
        }
        self.shared.start_frame.store(frame, Ordering::Relaxed);
        self.shared.start_armed.store(true, Ordering::Release);
        Ok(())
    }
    /// Actual immutable physical frame of first positive playback, if observed.
    /// Empty, held and zero-length finite playback never publish this evidence.
    pub fn applied_start_frame(&self) -> Option<u64> {
        self.shared.applied_start_frame()
    }

    /// Requests silence with playback scheduling frozen on a subsequent valid
    /// nonempty render. This independent desired state uses no queue slot and
    /// does not change admission counters; requests may coalesce before render.
    pub fn request_pause(&mut self, paused: bool) {
        if !paused && self.shared.pause_held.load(Ordering::Acquire) {
            return;
        }
        self.shared.pause_requested.store(paused, Ordering::Release);
    }
    /// Pins effective pause without using a queue slot or allocating new storage.
    /// Resume requests while held are ignored and must be reissued after release.
    pub fn hold_pause(&mut self) -> Result<PauseHold, PauseHoldError> {
        self.shared
            .pause_held
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| PauseHoldError::AlreadyHeld)?;
        Ok(PauseHold {
            shared: Arc::clone(&self.shared),
        })
    }
    /// Publishes one unmodified command or returns it on full/disconnect.
    ///
    /// Queue admission does not validate gains, sample IDs, timing or rates.
    /// A concurrent consumer drop may occur after the connected observation;
    /// successful publication then belongs to that preceding connected state.
    pub fn try_push(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        let reason = if self.is_disconnected() {
            self.counters.disconnected = self.counters.disconnected.saturating_add(1);
            Some(QueuePushError::Disconnected)
        } else if self.shared.slots[self.cursor].ready.load(Ordering::Acquire) {
            self.counters.full = self.counters.full.saturating_add(1);
            Some(QueuePushError::Full)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(CommandPushError { command, reason });
        }
        self.shared.slots[self.cursor].store(command);
        self.cursor = next_cursor(self.cursor, self.capacity());
        self.counters.accepted = self.counters.accepted.saturating_add(1);
        Ok(())
    }

    /// Fixed number of ring slots.
    pub fn capacity(&self) -> usize {
        self.shared.slots.len()
    }

    /// Whether consumer destruction has been observed.
    pub fn is_disconnected(&self) -> bool {
        !self.shared.consumer_alive.load(Ordering::Acquire)
    }

    /// Copies producer-local counters without allocation or synchronization.
    pub const fn counters(&self) -> QueueCounters {
        self.counters
    }
}

impl CommandConsumer {
    pub(crate) fn applied_start_frame(&self) -> Option<u64> {
        self.shared.applied_start_frame()
    }
    pub(crate) fn start_gate(&self) -> Option<Option<u64>> {
        self.shared.start_gated.then(|| {
            self.shared
                .start_armed
                .load(Ordering::Acquire)
                .then(|| self.shared.start_frame.load(Ordering::Relaxed))
        })
    }
    pub(crate) fn publish_physical_frontier(&self, frame: u64) {
        if self.shared.start_gated {
            self.shared
                .physical_frontier
                .store(frame, Ordering::Release);
        }
    }
    pub(crate) fn publish_applied_start(&self, frame: u64) {
        self.shared
            .applied_start_frame
            .store(frame, Ordering::Relaxed);
        self.shared.start_applied.store(true, Ordering::Release);
    }

    pub(crate) fn pause_requested(&self) -> bool {
        self.shared.pause_held.load(Ordering::Acquire)
            || self.shared.pause_requested.load(Ordering::Acquire)
    }
    /// Retrieves one command; published commands drain after producer drop.
    pub fn try_pop(&mut self) -> Result<AudioCommand, QueuePopError> {
        let slot = &self.shared.slots[self.cursor];
        if !slot.ready.load(Ordering::Acquire) {
            if !self.is_disconnected() {
                return Err(QueuePopError::Empty);
            }
            // Producer disconnect Release follows its final publication. After
            // observing disconnect, re-read so an earlier empty observation
            // cannot hide a command published immediately before producer drop.
            if !slot.ready.load(Ordering::Acquire) {
                return Err(QueuePopError::Disconnected);
            }
        }
        let command = slot.load();
        self.cursor = next_cursor(self.cursor, self.capacity());
        Ok(command)
    }

    /// Fixed number of ring slots.
    pub fn capacity(&self) -> usize {
        self.shared.slots.len()
    }

    /// Whether producer destruction has been observed, even with queued data.
    pub fn is_disconnected(&self) -> bool {
        !self.shared.producer_alive.load(Ordering::Acquire)
    }

    /// Counts contiguous publications, scanning at most the fixed capacity.
    ///
    /// Concurrent publication may add commands during the bounded observation.
    /// No command is consumed; use a tighter budget on a real-time callback.
    pub fn available(&self) -> usize {
        self.available_up_to(self.capacity())
    }

    /// Counts ready commands with work bounded by `min(limit, capacity)`.
    ///
    /// A renderer can capture this bounded count once and pop at most that
    /// number, without chasing a producer until the queue becomes empty.
    pub fn available_up_to(&self, limit: usize) -> usize {
        let mut cursor = self.cursor;
        let mut available = 0;
        for _ in 0..limit.min(self.capacity()) {
            if !self.shared.slots[cursor].ready.load(Ordering::Acquire) {
                break;
            }
            available += 1;
            cursor = next_cursor(cursor, self.capacity());
        }
        available
    }
}

impl Drop for CommandProducer {
    fn drop(&mut self) {
        self.shared.producer_alive.store(false, Ordering::Release);
    }
}

impl Drop for CommandConsumer {
    fn drop(&mut self) {
        self.shared.consumer_alive.store(false, Ordering::Release);
    }
}

fn next_cursor(cursor: usize, capacity: usize) -> usize {
    if cursor + 1 == capacity {
        0
    } else {
        cursor + 1
    }
}
