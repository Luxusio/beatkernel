//! Optional one-shot hazards evaluated against actual input ownership.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use crate::{
    input::{EventMeta, GameControlId},
    time::Timestamp,
};

use super::{snapshot::Encoder, SnapshotError};

/// Caller-owned hazard identity, independent of judged object identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HazardId(pub u64);

/// A one-shot occupancy observation at an exact signed song time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HazardMarker {
    /// Unique identity within this timeline.
    pub id: HazardId,
    /// Inclusive observation boundary on the effective judge timeline.
    pub at: Timestamp,
    /// Logical control whose actual owners are observed.
    pub control: GameControlId,
    /// Opaque application value; the judge does not interpret damage or sound.
    pub value: u64,
}

/// Whether a marker observed at least one actual owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardOutcome {
    /// The marker's control was occupied.
    Triggered,
    /// The marker's control was unoccupied.
    Avoided,
}

/// One consumed marker and, only at an exact input boundary, its provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HazardEvent {
    /// Original marker identity.
    pub id: HazardId,
    /// Original marker time, without substitution by the processing time.
    pub at: Timestamp,
    /// Original logical control.
    pub control: GameControlId,
    /// Unchanged opaque application value.
    pub value: u64,
    /// Actual occupancy at this marker's boundary.
    pub outcome: HazardOutcome,
    /// Original input metadata for an equal-time input, otherwise absent.
    pub input: Option<EventMeta>,
}

/// Invalid timeline setup or an unavailable judge configuration boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardError {
    /// Marker count exceeds the caller's budget.
    Capacity,
    /// A marker identity occurs more than once.
    DuplicateId {
        /// Repeated identity.
        id: HazardId,
    },
    /// This judge already owns a configured hazard timeline.
    AlreadyConfigured,
    /// This judge has accepted an input or advance.
    AlreadyStarted,
    /// The result buffer could not be allocated during setup.
    Allocation,
    /// Complete canonical configuration is unavailable.
    Snapshot(SnapshotError),
}

impl fmt::Display for HazardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "judge hazard: {self:?}")
    }
}
impl std::error::Error for HazardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Snapshot(error) => Some(error),
            _ => None,
        }
    }
}

/// Immutable markers sorted by time, preserving declaration order for ties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HazardTimeline {
    markers: Vec<HazardMarker>,
}

impl HazardTimeline {
    /// Validates the caller's budget and unique IDs. Empty timelines, including
    /// an empty timeline with a zero budget, and all signed times are valid.
    pub fn new(mut markers: Vec<HazardMarker>, max_markers: usize) -> Result<Self, HazardError> {
        if markers.len() > max_markers {
            return Err(HazardError::Capacity);
        }
        let mut identities = BTreeSet::new();
        for marker in &markers {
            if !identities.insert(marker.id) {
                return Err(HazardError::DuplicateId { id: marker.id });
            }
        }
        markers.sort_by_key(|marker| marker.at);
        Ok(Self { markers })
    }

    /// Borrows the validated stable time ordering.
    pub fn markers(&self) -> &[HazardMarker] {
        &self.markers
    }
}

pub(super) struct HazardState {
    timeline: HazardTimeline,
    cursor: usize,
    occupancy: BTreeMap<u32, usize>,
    events: Vec<HazardEvent>,
}

impl HazardState {
    pub(super) fn new(timeline: HazardTimeline) -> Result<Self, HazardError> {
        let mut events = Vec::new();
        events
            .try_reserve_exact(timeline.markers.len())
            .map_err(|_| HazardError::Allocation)?;
        let occupancy = timeline
            .markers
            .iter()
            .map(|marker| (marker.control.0, 0))
            .collect();
        Ok(Self {
            timeline,
            cursor: 0,
            occupancy,
            events,
        })
    }

    pub(super) fn events(&self) -> &[HazardEvent] {
        &self.events
    }

    pub(super) fn count(&self) -> usize {
        self.timeline.markers.len()
    }

    pub(super) fn remaining(&self) -> usize {
        self.timeline.markers.len() - self.cursor
    }

    pub(super) fn clear_events(&mut self) {
        self.events.clear();
    }

    pub(super) fn acquired(&mut self, control: GameControlId) {
        if let Some(count) = self.occupancy.get_mut(&control.0) {
            *count += 1;
        }
    }

    pub(super) fn released(&mut self, control: GameControlId) {
        if let Some(count) = self.occupancy.get_mut(&control.0) {
            *count -= 1;
        }
    }

    pub(super) fn consume(&mut self, time: Timestamp, inclusive: bool, input: Option<EventMeta>) {
        while let Some(marker) = self.timeline.markers.get(self.cursor) {
            if marker.at > time || (!inclusive && marker.at == time) {
                break;
            }
            self.events.push(HazardEvent {
                id: marker.id,
                at: marker.at,
                control: marker.control,
                value: marker.value,
                outcome: if self.occupancy[&marker.control.0] == 0 {
                    HazardOutcome::Avoided
                } else {
                    HazardOutcome::Triggered
                },
                input: if marker.at == time { input } else { None },
            });
            self.cursor += 1;
        }
    }

    pub(super) fn encode(&self, bytes: &mut Encoder) {
        let mut extension = Encoder::new(b"judge-hazards/v1");
        extension.u64(self.timeline.markers.len() as u64);
        for marker in &self.timeline.markers {
            extension.u64(marker.id.0);
            extension.i64(marker.at.as_nanos());
            extension.u32(marker.control.0);
            extension.u64(marker.value);
        }
        extension.u64(self.cursor as u64);
        extension.u64(self.occupancy.len() as u64);
        for (control, count) in &self.occupancy {
            extension.u32(*control);
            extension.u64(*count as u64);
        }
        extension.u64(self.events.len() as u64);
        for event in &self.events {
            extension.u64(event.id.0);
            extension.i64(event.at.as_nanos());
            extension.u32(event.control.0);
            extension.u64(event.value);
            extension.u8(match event.outcome {
                HazardOutcome::Triggered => 0,
                HazardOutcome::Avoided => 1,
            });
            extension.option(event.input, Encoder::meta);
        }
        bytes.bytes(&extension.finish());
    }
}

impl Clone for HazardState {
    fn clone(&self) -> Self {
        // A checkpoint may have a short last report but consume every remaining
        // marker in one later operation. Retain the original setup capacity.
        let mut events = Vec::with_capacity(self.timeline.markers.len());
        events.extend_from_slice(&self.events);
        Self {
            timeline: self.timeline.clone(),
            cursor: self.cursor,
            occupancy: self.occupancy.clone(),
            events,
        }
    }
}
