use super::{AudioCommand, AudioError, AudioLimits, SampleId, VoiceId};
use crate::{time::Timestamp, transport::Rate};
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicU8, Ordering},
    Arc,
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
