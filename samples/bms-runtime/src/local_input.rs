//! Bounded off-audio-thread merging of already acquired local input.
//!
//! Native event timestamps and payloads remain unchanged. A caller drains all
//! sources, releases events through a lagged host frontier, advances every
//! player's deadlines at that same point, and only then commits the frontier.

use beatkernel::{
    input::{DeviceId, PhysicalInputEvent},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{cmp::Ordering, collections::BinaryHeap, fmt};

/// Maximum retained heap-entry and owned payload-capacity bytes.
pub const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;

/// Rejected setup, admission or frontier operation; merger state is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeError {
    /// Setup requires 1..=64 distinct devices and 1..=65536 pending slots.
    InvalidConfiguration,
    /// A supplied clock point or normalized event uses another domain.
    DomainMismatch {
        expected: ClockDomainId,
        actual: ClockDomainId,
    },
    /// The event source is not registered in this merger.
    UnknownDevice(DeviceId),
    /// A clock point/event precedes the fixed session origin.
    BeforeOrigin {
        timestamp: Timestamp,
        origin: Timestamp,
    },
    /// Input cannot occur after its supplied acquisition observation.
    FutureInput {
        timestamp: Timestamp,
        received: Timestamp,
    },
    /// Input at/before an already committed deadline frontier is too late.
    LateInput {
        timestamp: Timestamp,
        committed: Timestamp,
    },
    /// An accepted source's timestamps must not regress.
    SourceTimeRegression {
        source: DeviceId,
        last: Timestamp,
        received: Timestamp,
    },
    /// Source acquisition sequences must not regress; equality permits fanout.
    SequenceRegression {
        source: DeviceId,
        last: u64,
        received: u64,
    },
    /// The configured pending entry capacity is full.
    Capacity,
    /// Heap storage plus owned pending payload capacities exceed 64 MiB.
    StorageCapacity,
    /// Scheduling lag must be nonnegative and at most one second.
    InvalidLag,
    /// A frontier precedes an already committed frontier.
    FrontierRegression {
        last: Timestamp,
        received: Timestamp,
    },
    /// Committing would skip input that still needs to be processed.
    PendingBeforeFrontier {
        timestamp: Timestamp,
        frontier: Timestamp,
    },
    /// Checked watermark, byte count or admission ordinal arithmetic overflowed.
    Overflow,
    /// Off-thread preallocation failed.
    AllocationFailed,
}
impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                f.write_str("input merge needs 1..64 distinct devices and 1..65536 slots")
            }
            Self::DomainMismatch { expected, actual } => write!(
                f,
                "input merge domain mismatch: expected {}, received {}",
                expected.0, actual.0
            ),
            Self::UnknownDevice(source) => write!(f, "unknown local input device {}", source.0),
            Self::BeforeOrigin { timestamp, origin } => write!(
                f,
                "input merge point {}ns precedes origin {}ns",
                timestamp.as_nanos(),
                origin.as_nanos()
            ),
            Self::FutureInput {
                timestamp,
                received,
            } => write!(
                f,
                "input {}ns is later than acquisition observation {}ns",
                timestamp.as_nanos(),
                received.as_nanos()
            ),
            Self::LateInput {
                timestamp,
                committed,
            } => write!(
                f,
                "input {}ns arrived at/before committed {}ns; scheduling lag is insufficient",
                timestamp.as_nanos(),
                committed.as_nanos()
            ),
            Self::SourceTimeRegression {
                source,
                last,
                received,
            } => write!(
                f,
                "input device {} timestamp regressed from {}ns to {}ns",
                source.0,
                last.as_nanos(),
                received.as_nanos()
            ),
            Self::SequenceRegression {
                source,
                last,
                received,
            } => write!(
                f,
                "input device {} sequence regressed from {last} to {received}",
                source.0
            ),
            Self::Capacity => f.write_str("local input pending capacity exhausted"),
            Self::StorageCapacity => f.write_str("local input retained storage exceeds 64 MiB"),
            Self::InvalidLag => f.write_str("input merge lag must be 0..1000000000ns"),
            Self::FrontierRegression { last, received } => write!(
                f,
                "input merge frontier regressed from {}ns to {}ns",
                last.as_nanos(),
                received.as_nanos()
            ),
            Self::PendingBeforeFrontier {
                timestamp,
                frontier,
            } => write!(
                f,
                "input {}ns remains pending through proposed frontier {}ns",
                timestamp.as_nanos(),
                frontier.as_nanos()
            ),
            Self::Overflow => f.write_str("input merge checked arithmetic overflow"),
            Self::AllocationFailed => f.write_str("input merge preallocation failed"),
        }
    }
}
impl std::error::Error for MergeError {}

#[derive(Debug)]
struct SourceState {
    device: DeviceId,
    last: Option<(Timestamp, u64)>,
}
#[derive(Debug)]
struct Pending {
    key: (Timestamp, DeviceId, u64, u64),
    event: PhysicalInputEvent,
    payload_bytes: usize,
}
// BinaryHeap is a max heap: reverse only the complete stable ordering key.
// Float payloads (including NaN) are never inspected or compared for ordering.
impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Pending {}
impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        other.key.cmp(&self.key)
    }
}

/// One game-owner input queue sharing a fixed native host domain and origin.
///
/// Event entries are preallocated. Retained storage counts the whole heap's
/// allocated entry capacity plus Vec capacities of pending HID/custom payloads,
/// rather than merely their lengths. The bound excludes allocator bookkeeping
/// and the caller's rejected/in-flight event. No payload is cloned or rewritten.
#[derive(Debug)]
pub struct InputMerger {
    domain: ClockDomainId,
    origin: Timestamp,
    sources: Vec<SourceState>,
    capacity: usize,
    pending: BinaryHeap<Pending>,
    fixed_bytes: usize,
    payload_bytes: usize,
    next_ordinal: u64,
    committed: Option<Timestamp>,
}
impl InputMerger {
    /// Validates and preallocates 1..=64 distinct sources and 1..=65536 slots.
    pub fn new(
        domain: ClockDomainId,
        origin: ClockPoint,
        devices: Vec<DeviceId>,
        capacity: usize,
    ) -> Result<Self, MergeError> {
        if origin.domain != domain {
            return Err(MergeError::DomainMismatch {
                expected: domain,
                actual: origin.domain,
            });
        }
        if !(1..=64).contains(&devices.len()) || !(1..=65536).contains(&capacity) {
            return Err(MergeError::InvalidConfiguration);
        }
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(devices.len())
            .map_err(|_| MergeError::AllocationFailed)?;
        for device in devices {
            if sources
                .iter()
                .any(|source: &SourceState| source.device == device)
            {
                return Err(MergeError::InvalidConfiguration);
            }
            sources.push(SourceState { device, last: None });
        }
        let mut pending = BinaryHeap::new();
        pending
            .try_reserve_exact(capacity)
            .map_err(|_| MergeError::AllocationFailed)?;
        let fixed_bytes = pending
            .capacity()
            .checked_mul(std::mem::size_of::<Pending>())
            .ok_or(MergeError::Overflow)?;
        if fixed_bytes > MAX_PENDING_BYTES {
            return Err(MergeError::StorageCapacity);
        }
        Ok(Self {
            domain,
            origin: origin.timestamp,
            sources,
            capacity,
            pending,
            fixed_bytes,
            payload_bytes: 0,
            next_ordinal: 0,
            committed: None,
        })
    }

    /// Admits exact input after all validation; every failure is state-atomic.
    ///
    /// Both event timestamp and source sequence must be nondecreasing. Equal
    /// values retain admission order, including native report fanout. Rejection
    /// consumes this owned argument; the caller must surface/stop on the error.
    pub fn admit(
        &mut self,
        event: PhysicalInputEvent,
        received: ClockPoint,
    ) -> Result<(), MergeError> {
        self.validate_point(received)?;
        let meta = *event.meta();
        if meta.clock_domain != self.domain {
            return Err(MergeError::DomainMismatch {
                expected: self.domain,
                actual: meta.clock_domain,
            });
        }
        let source_index = self
            .sources
            .iter()
            .position(|source| source.device == meta.source)
            .ok_or(MergeError::UnknownDevice(meta.source))?;
        if meta.timestamp < self.origin {
            return Err(MergeError::BeforeOrigin {
                timestamp: meta.timestamp,
                origin: self.origin,
            });
        }
        if meta.timestamp > received.timestamp {
            return Err(MergeError::FutureInput {
                timestamp: meta.timestamp,
                received: received.timestamp,
            });
        }
        if let Some(committed) = self.committed {
            if meta.timestamp <= committed {
                return Err(MergeError::LateInput {
                    timestamp: meta.timestamp,
                    committed,
                });
            }
        }
        if let Some((last_time, last_sequence)) = self.sources[source_index].last {
            if meta.timestamp < last_time {
                return Err(MergeError::SourceTimeRegression {
                    source: meta.source,
                    last: last_time,
                    received: meta.timestamp,
                });
            }
            if meta.sequence < last_sequence {
                return Err(MergeError::SequenceRegression {
                    source: meta.source,
                    last: last_sequence,
                    received: meta.sequence,
                });
            }
        }
        if self.pending.len() == self.capacity {
            return Err(MergeError::Capacity);
        }
        let payload_bytes = owned_payload_bytes(&event);
        let total_payload = self
            .payload_bytes
            .checked_add(payload_bytes)
            .ok_or(MergeError::Overflow)?;
        let total_bytes = self
            .fixed_bytes
            .checked_add(total_payload)
            .ok_or(MergeError::Overflow)?;
        if total_bytes > MAX_PENDING_BYTES {
            return Err(MergeError::StorageCapacity);
        }
        let next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .ok_or(MergeError::Overflow)?;
        // Setup reserved every possible slot: push cannot allocate here. Only
        // after all fallible checks do chronology, byte count and ordinal change.
        self.pending.push(Pending {
            key: (
                meta.timestamp,
                meta.source,
                meta.sequence,
                self.next_ordinal,
            ),
            event,
            payload_bytes,
        });
        self.sources[source_index].last = Some((meta.timestamp, meta.sequence));
        self.payload_bytes = total_payload;
        self.next_ordinal = next_ordinal;
        Ok(())
    }

    /// Computes now-lag; a backlog or initial preroll yields no release frontier.
    ///
    /// Lag is 0..=1s. Domain/origin/lag/arithmetic errors remain errors during backlog.
    /// Increasing lag after a commit may regress the frontier and is rejected.
    /// The caller must refresh now after gathering every source's native batch.
    pub fn watermark(
        &self,
        now: ClockPoint,
        lag_ns: i64,
        backlog: bool,
    ) -> Result<Option<ClockPoint>, MergeError> {
        self.validate_point(now)?;
        if !(0..=1_000_000_000).contains(&lag_ns) {
            return Err(MergeError::InvalidLag);
        }
        let nanos = i128::from(now.timestamp.as_nanos())
            .checked_sub(i128::from(lag_ns))
            .ok_or(MergeError::Overflow)?;
        let timestamp =
            Timestamp::from_nanos(i64::try_from(nanos).map_err(|_| MergeError::Overflow)?);
        if backlog {
            return Ok(None);
        }
        if let Some(last) = self.committed {
            if timestamp < last {
                return Err(MergeError::FrontierRegression {
                    last,
                    received: timestamp,
                });
            }
        }
        if timestamp < self.origin {
            return Ok(None);
        }
        Ok(Some(ClockPoint {
            domain: self.domain,
            timestamp,
        }))
    }

    /// Removes the earliest exact event at/before a validated frontier.
    ///
    /// This does not commit deadlines. The caller must process each returned
    /// event, advance all members, and then call commit with that same frontier.
    pub fn pop_ready(
        &mut self,
        frontier: ClockPoint,
    ) -> Result<Option<PhysicalInputEvent>, MergeError> {
        self.validate_frontier(frontier)?;
        if self
            .pending
            .peek()
            .is_none_or(|item| item.key.0 > frontier.timestamp)
        {
            return Ok(None);
        }
        let item = self
            .pending
            .pop()
            .expect("peek established a pending event");
        self.payload_bytes -= item.payload_bytes;
        Ok(Some(item.event))
    }

    /// Commits deadlines only after all input through this frontier is removed.
    /// Equal commits are idempotent; any rejection preserves the prior frontier.
    pub fn commit(&mut self, frontier: ClockPoint) -> Result<(), MergeError> {
        self.validate_frontier(frontier)?;
        if let Some(item) = self.pending.peek() {
            if item.key.0 <= frontier.timestamp {
                return Err(MergeError::PendingBeforeFrontier {
                    timestamp: item.key.0,
                    frontier: frontier.timestamp,
                });
            }
        }
        self.committed = Some(frontier.timestamp);
        Ok(())
    }

    /// Number of exact events awaiting release, excluding already returned input.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    fn validate_point(&self, point: ClockPoint) -> Result<(), MergeError> {
        if point.domain != self.domain {
            return Err(MergeError::DomainMismatch {
                expected: self.domain,
                actual: point.domain,
            });
        }
        if point.timestamp < self.origin {
            return Err(MergeError::BeforeOrigin {
                timestamp: point.timestamp,
                origin: self.origin,
            });
        }
        Ok(())
    }
    fn validate_frontier(&self, frontier: ClockPoint) -> Result<(), MergeError> {
        self.validate_point(frontier)?;
        if let Some(last) = self.committed {
            if frontier.timestamp < last {
                return Err(MergeError::FrontierRegression {
                    last,
                    received: frontier.timestamp,
                });
            }
        }
        Ok(())
    }
}

fn owned_payload_bytes(event: &PhysicalInputEvent) -> usize {
    match event {
        PhysicalInputEvent::RawHidReport(event) => event.data.capacity(),
        PhysicalInputEvent::Custom(event) => event.payload.capacity(),
        PhysicalInputEvent::Button(_)
        | PhysicalInputEvent::Axis(_)
        | PhysicalInputEvent::Touch(_)
        | PhysicalInputEvent::Pointer(_)
        | PhysicalInputEvent::Pose(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatkernel::input::{
        BackendId, ButtonEvent, ButtonState, CustomInputEvent, EventMeta, NativeEventMeta,
        PhysicalControlId, RawHidReportEvent, VendorNamespaceId,
    };

    fn point(nanos: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(10),
            timestamp: Timestamp::from_nanos(nanos),
        }
    }
    fn button(source: u64, nanos: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
        let mut meta = EventMeta::new(DeviceId(source), point(nanos), sequence);
        meta.native = Some(NativeEventMeta {
            backend: BackendId(2),
            code: Some(30),
            timestamp: Some(ClockPoint {
                domain: ClockDomainId(99),
                timestamp: Timestamp::from_nanos(nanos - 1),
            }),
        });
        meta.original_clock_point = meta.native.and_then(|native| native.timestamp);
        PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(4),
            state,
        })
    }
    fn merger(capacity: usize) -> InputMerger {
        InputMerger::new(
            ClockDomainId(10),
            point(0),
            vec![DeviceId(4), DeviceId(3), DeviceId(2), DeviceId(1)],
            capacity,
        )
        .unwrap()
    }

    #[test]
    fn four_sources_order_by_timestamp_identity_sequence_then_stable_fanout() {
        let mut merge = merger(8);
        let events = [
            button(4, 20, 1, ButtonState::Down),
            button(3, 10, 2, ButtonState::Down),
            button(2, 10, 9, ButtonState::Down),
            button(1, 10, 5, ButtonState::Down),
            button(1, 10, 5, ButtonState::Up),
            button(1, 10, 6, ButtonState::Repeat),
        ];
        for event in &events {
            merge.admit(event.clone(), point(30)).unwrap();
        }
        assert!(matches!(
            merge.commit(point(10)),
            Err(MergeError::PendingBeforeFrontier { .. })
        ));
        for index in [3, 4, 5, 2, 1] {
            assert_eq!(
                merge.pop_ready(point(10)).unwrap(),
                Some(events[index].clone())
            );
        }
        assert_eq!(merge.pop_ready(point(10)).unwrap(), None);
        merge.commit(point(10)).unwrap();
        assert_eq!(merge.pending(), 1);
        assert_eq!(merge.pop_ready(point(20)).unwrap(), Some(events[0].clone()));
        merge.commit(point(20)).unwrap();
    }

    #[test]
    fn admission_rejections_preserve_source_chronology_ordinals_and_pending_input() {
        let mut merge = merger(8);
        merge
            .admit(button(1, 20, 10, ButtonState::Down), point(30))
            .unwrap();
        let original = merge.pending.peek().unwrap().event.clone();
        for event in [
            button(1, 19, 11, ButtonState::Up),
            button(1, 21, 9, ButtonState::Up),
            button(9, 21, 11, ButtonState::Up),
            button(2, 31, 1, ButtonState::Down),
            button(2, -1, 1, ButtonState::Down),
        ] {
            assert!(merge.admit(event, point(30)).is_err());
            assert_eq!(merge.pending(), 1);
            assert_eq!(merge.next_ordinal, 1);
            assert_eq!(merge.pending.peek().unwrap().event, original);
        }
        let mut wrong_domain = button(2, 21, 1, ButtonState::Down);
        wrong_domain.meta_mut().clock_domain = ClockDomainId(11);
        assert!(matches!(
            merge.admit(wrong_domain, point(30)),
            Err(MergeError::DomainMismatch { .. })
        ));
        merge
            .admit(button(2, 21, 1, ButtonState::Down), point(30))
            .unwrap();
        assert_eq!(merge.next_ordinal, 2);
        assert_eq!(
            merge
                .sources
                .iter()
                .find(|source| source.device == DeviceId(1))
                .unwrap()
                .last,
            Some((Timestamp::from_nanos(20), 10))
        );
    }

    #[test]
    fn backlog_lag_commit_and_late_input_never_invent_timestamps() {
        let mut merge = merger(4);
        assert_eq!(merge.watermark(point(5), 10, false).unwrap(), None);
        assert_eq!(merge.watermark(point(30), 10, true).unwrap(), None);
        assert_eq!(
            merge.watermark(point(30), 10, false).unwrap(),
            Some(point(20))
        );
        assert!(matches!(
            merge.watermark(point(30), -1, true),
            Err(MergeError::InvalidLag)
        ));
        assert!(matches!(
            merge.watermark(point(30), 1_000_000_001, false),
            Err(MergeError::InvalidLag)
        ));
        merge.commit(point(20)).unwrap();
        assert!(matches!(
            merge.admit(button(1, 20, 1, ButtonState::Down), point(30)),
            Err(MergeError::LateInput { .. })
        ));
        assert!(matches!(
            merge.commit(point(19)),
            Err(MergeError::FrontierRegression { .. })
        ));
        assert!(matches!(
            merge.pop_ready(point(19)),
            Err(MergeError::FrontierRegression { .. })
        ));
        assert!(matches!(
            merge.watermark(point(30), 11, false),
            Err(MergeError::FrontierRegression { .. })
        ));
        assert_eq!(merge.committed, Some(Timestamp::from_nanos(20)));
        merge.commit(point(20)).unwrap();
        let extreme =
            InputMerger::new(ClockDomainId(10), point(i64::MIN), vec![DeviceId(1)], 1).unwrap();
        assert_eq!(
            extreme.watermark(point(i64::MIN), 1, false),
            Err(MergeError::Overflow)
        );
    }

    #[test]
    fn capacity_and_owned_payload_capacity_reject_atomically_and_release_on_pop() {
        let mut merge = merger(1);
        merge
            .admit(button(1, 1, 1, ButtonState::Down), point(10))
            .unwrap();
        assert_eq!(
            merge.admit(button(2, 2, 1, ButtonState::Down), point(10)),
            Err(MergeError::Capacity)
        );
        merge.pop_ready(point(1)).unwrap();
        merge
            .admit(button(2, 2, 1, ButtonState::Down), point(10))
            .unwrap();
        assert!(
            InputMerger::new(
                ClockDomainId(10),
                point(0),
                vec![DeviceId(1), DeviceId(1)],
                2
            )
            .is_err()
        );
        assert!(InputMerger::new(ClockDomainId(10), point(0), vec![], 2).is_err());
        assert!(InputMerger::new(ClockDomainId(10), point(0), vec![DeviceId(1)], 0).is_err());
        // A logically empty report still owns its allocation. Exercise the
        // public boundary with real capacity, not an invented byte-count state.
        let mut merge = merger(2);
        let data = Vec::with_capacity(MAX_PENDING_BYTES);
        let rejected = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
            meta: EventMeta::new(DeviceId(1), point(1), 1),
            report_id: Some(7),
            data,
        });
        assert_eq!(
            merge.admit(rejected, point(10)),
            Err(MergeError::StorageCapacity)
        );
        assert_eq!(merge.pending(), 0);
        assert_eq!(merge.payload_bytes, 0);
        assert_eq!(merge.next_ordinal, 0);
        assert_eq!(
            merge
                .sources
                .iter()
                .find(|source| source.device == DeviceId(1))
                .unwrap()
                .last,
            None
        );
        let payload = Vec::with_capacity(8);
        let expected_capacity = payload.capacity();
        let event = PhysicalInputEvent::Custom(CustomInputEvent {
            meta: EventMeta::new(DeviceId(1), point(1), 1),
            namespace: VendorNamespaceId(9),
            type_id: 2,
            payload,
        });
        merge.admit(event.clone(), point(10)).unwrap();
        // Clone can shrink the Vec's spare capacity: admit a directly owned
        // payload to check retained capacity, not only its logical length.
        merge.pop_ready(point(1)).unwrap();
        merge.admit(event, point(10)).unwrap();
        assert_eq!(merge.payload_bytes, expected_capacity);
        let returned = merge.pop_ready(point(1)).unwrap().unwrap();
        match returned {
            PhysicalInputEvent::Custom(event) => {
                assert_eq!(event.payload.capacity(), expected_capacity)
            }
            _ => panic!("wrong exact event variant"),
        }
        assert_eq!(merge.payload_bytes, 0);
    }
}
