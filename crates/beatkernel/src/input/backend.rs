use std::{
    collections::{btree_map::Entry, vec_deque, BTreeMap, VecDeque},
    error::Error,
    fmt,
};

use crate::time::{ClockDomainId, ClockMapper, ClockPoint};

use super::{DeviceDescriptor, DeviceId, PhysicalInputEvent};

/// A rejected virtual device registration or input event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VirtualInputError {
    /// The runtime identity has already been registered, even if retired.
    DuplicateDevice(DeviceId),
    /// The source identity is unknown or retired.
    UnknownDevice(DeviceId),
    /// The acquisition sequence is below the last accepted source sequence.
    SequenceRegression {
        /// The source device identity.
        device: DeviceId,
        /// The last accepted acquisition sequence.
        last: u64,
        /// The rejected acquisition sequence.
        received: u64,
    },
    /// The mapper could not produce a timestamp in the output clock domain.
    UnmappedClock {
        /// The incoming event clock domain.
        from: ClockDomainId,
        /// The backend output clock domain.
        to: ClockDomainId,
    },
}

impl fmt::Display for VirtualInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateDevice(device) => {
                write!(f, "device ID {} has already been registered", device.0)
            }
            Self::UnknownDevice(device) => {
                write!(f, "device ID {} is not active", device.0)
            }
            Self::SequenceRegression {
                device,
                last,
                received,
            } => write!(
                f,
                "device ID {} sequence regressed from {last} to {received}",
                device.0
            ),
            Self::UnmappedClock { from, to } => {
                write!(f, "cannot map clock domain {} to {}", from.0, to.0)
            }
        }
    }
}

impl Error for VirtualInputError {}

#[derive(Debug)]
struct DeviceRecord {
    descriptor: DeviceDescriptor,
    active: bool,
    last_sequence: Option<u64>,
}

/// A single-owner virtual device registry and acquisition-order FIFO.
///
/// IDs remain reserved for the backend lifetime after retirement. Sequence
/// validation is per device and allows equality for raw report fanout. Timestamp
/// regressions are allowed; queued events are never sorted by time. Registration
/// and enqueue may allocate, so this is not a real-time callback queue.
#[derive(Debug)]
pub struct VirtualInputBackend {
    output_clock: ClockDomainId,
    devices: BTreeMap<DeviceId, DeviceRecord>,
    pending: VecDeque<PhysicalInputEvent>,
}

impl VirtualInputBackend {
    /// Constructs an empty backend whose accepted events use `output_clock`.
    pub fn new(output_clock: ClockDomainId) -> Self {
        Self {
            output_clock,
            devices: BTreeMap::new(),
            pending: VecDeque::new(),
        }
    }

    /// Registers a device using the descriptor's caller-assigned runtime ID.
    ///
    /// Returns [`VirtualInputError::DuplicateDevice`] for any previously used
    /// ID, including a retired one. Rejection leaves the registry unchanged.
    pub fn register_device(&mut self, device: DeviceDescriptor) -> Result<(), VirtualInputError> {
        match self.devices.entry(device.runtime_id) {
            Entry::Occupied(entry) => Err(VirtualInputError::DuplicateDevice(*entry.key())),
            Entry::Vacant(entry) => {
                entry.insert(DeviceRecord {
                    descriptor: device,
                    active: true,
                    last_sequence: None,
                });
                Ok(())
            }
        }
    }

    /// Retires an active device and returns whether its state changed.
    ///
    /// Previously queued events remain deliverable. Unknown or already retired
    /// IDs return `false`; the ID remains reserved after retirement.
    pub fn unregister_device(&mut self, id: DeviceId) -> bool {
        let Some(record) = self.devices.get_mut(&id) else {
            return false;
        };
        let was_active = record.active;
        record.active = false;
        was_active
    }

    /// Returns the descriptor of an active device, or `None` otherwise.
    pub fn device(&self, id: DeviceId) -> Option<&DeviceDescriptor> {
        self.devices
            .get(&id)
            .filter(|record| record.active)
            .map(|record| &record.descriptor)
    }

    /// Iterates active device descriptors in ascending runtime ID order.
    pub fn devices(&self) -> impl Iterator<Item = &DeviceDescriptor> {
        self.devices
            .values()
            .filter(|record| record.active)
            .map(|record| &record.descriptor)
    }

    /// Returns the number of accepted events awaiting delivery.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Validates and appends an event, mapping its clock domain when necessary.
    ///
    /// The source must be active and its sequence must be at least its last
    /// accepted sequence. Different clock domains require a successful explicit
    /// mapping; same-domain events bypass the mapper. Conversion saves the
    /// incoming clock point only when no origin is already present and preserves
    /// native provenance and payloads. Every validation error leaves the queue
    /// and last accepted sequences unchanged.
    pub fn push(
        &mut self,
        mut event: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
    ) -> Result<(), VirtualInputError> {
        let meta = *event.meta();
        let record = self
            .devices
            .get_mut(&meta.source)
            .filter(|record| record.active)
            .ok_or(VirtualInputError::UnknownDevice(meta.source))?;
        if let Some(last) = record.last_sequence {
            if meta.sequence < last {
                return Err(VirtualInputError::SequenceRegression {
                    device: meta.source,
                    last,
                    received: meta.sequence,
                });
            }
        }
        if meta.clock_domain != self.output_clock {
            let point = ClockPoint {
                domain: meta.clock_domain,
                timestamp: meta.timestamp,
            };
            let timestamp =
                mapper
                    .map(point, self.output_clock)
                    .ok_or(VirtualInputError::UnmappedClock {
                        from: meta.clock_domain,
                        to: self.output_clock,
                    })?;
            let normalized = event.meta_mut();
            normalized.original_clock_point.get_or_insert(point);
            normalized.timestamp = timestamp;
            normalized.clock_domain = self.output_clock;
        }
        record.last_sequence = Some(meta.sequence);
        self.pending.push_back(event);
        Ok(())
    }

    /// Removes the oldest accepted event, or returns `None` for an empty queue.
    pub fn pop(&mut self) -> Option<PhysicalInputEvent> {
        self.pending.pop_front()
    }

    /// Drains all accepted events in FIFO order, including retired sources.
    ///
    /// Dropping the drain discards any remaining events in the drained range,
    /// following the standard [`VecDeque::drain`] behavior.
    pub fn drain_events(&mut self) -> vec_deque::Drain<'_, PhysicalInputEvent> {
        self.pending.drain(..)
    }
}
