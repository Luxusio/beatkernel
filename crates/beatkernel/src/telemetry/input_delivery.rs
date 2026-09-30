//! Age of supplied event timestamps at an explicit same-domain receipt point.
use super::{RuntimeCounters, RuntimeTelemetry, TimingSummary};
use crate::time::{ClockDomainId, ClockPoint};

/// Explicit setup or observation failure, before any observer state changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDeliveryError {
    /// Retention capacity exceeds the finite limit.
    InvalidCapacity,
    /// Event or receipt point uses a different clock domain.
    ClockDomainMismatch {
        /// Configured observer domain.
        expected: ClockDomainId,
        /// Rejected point's domain.
        received: ClockDomainId,
    },
    /// The event timestamp is later than the supplied receipt timestamp.
    FutureEvent,
    /// The receipt timestamp regressed behind the last accepted receipt.
    ReceivedTimeRegression,
    /// Checked timestamp subtraction cannot be represented as an unsigned age.
    Overflow,
    /// Setup could not reserve its bounded retention storage.
    AllocationFailed,
}
impl std::fmt::Display for InputDeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "input delivery age: {self:?}")
    }
}
impl std::error::Error for InputDeliveryError {}

/// Actual supplied points and their nonnegative difference, without clock inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputDeliveryObservation {
    /// Unchanged canonical event clock point.
    pub event: ClockPoint,
    /// Caller-supplied fresh point after acquisition.
    pub received: ClockPoint,
    /// Receipt minus event, including the complete signed timestamp span.
    pub age_ns: u64,
}

/// Single-owner bounded event-age observations, separate from CPU processing.
///
/// This observer acquires no clock, converts no domain and changes no input or
/// gameplay state. The meaning and accuracy of the event timestamp belong to
/// its acquisition backend. Ages do not prove physical input-to-sound latency.
#[derive(Clone, Debug)]
pub struct InputDeliveryTelemetry {
    domain: ClockDomainId,
    delays: RuntimeTelemetry,
    last_received: Option<ClockPoint>,
    events: u64,
}
impl InputDeliveryTelemetry {
    /// Maximum retained ages; zero capacity disables retention but keeps counts.
    pub const MAX_CAPACITY: usize = 65_536;

    /// Fallibly reserves a bounded duration ring in the explicit host domain.
    pub fn new(capacity: usize, domain: ClockDomainId) -> Result<Self, InputDeliveryError> {
        if capacity > Self::MAX_CAPACITY {
            return Err(InputDeliveryError::InvalidCapacity);
        }
        let mut durations = Vec::new();
        durations
            .try_reserve_exact(capacity)
            .map_err(|_| InputDeliveryError::AllocationFailed)?;
        Ok(Self {
            domain,
            delays: RuntimeTelemetry {
                durations,
                capacity,
                next: 0,
                counters: RuntimeCounters::default(),
            },
            last_received: None,
            events: 0,
        })
    }

    /// Records a same-domain age without allocation or timestamp replacement.
    ///
    /// Receipt points must not regress. Event points may arrive out of order;
    /// gameplay chronology is a separate Runtime concern. Errors preserve the
    /// ring, count and receipt baseline. Equal points are a real zero-age sample.
    pub fn observe(
        &mut self,
        event: ClockPoint,
        received: ClockPoint,
    ) -> Result<InputDeliveryObservation, InputDeliveryError> {
        for point in [event, received] {
            if point.domain != self.domain {
                return Err(InputDeliveryError::ClockDomainMismatch {
                    expected: self.domain,
                    received: point.domain,
                });
            }
        }
        if received.timestamp < event.timestamp {
            return Err(InputDeliveryError::FutureEvent);
        }
        if self
            .last_received
            .is_some_and(|previous| received.timestamp < previous.timestamp)
        {
            return Err(InputDeliveryError::ReceivedTimeRegression);
        }
        let age = i128::from(received.timestamp.as_nanos())
            .checked_sub(i128::from(event.timestamp.as_nanos()))
            .ok_or(InputDeliveryError::Overflow)?;
        let age_ns = u64::try_from(age).map_err(|_| InputDeliveryError::Overflow)?;
        self.delays.record_processing_ns(age_ns);
        self.last_received = Some(received);
        self.events = self.events.saturating_add(1);
        Ok(InputDeliveryObservation {
            event,
            received,
            age_ns,
        })
    }

    /// Retained-tail nearest-rank percentiles, copied/sorted outside callbacks.
    /// None means no retained ages; it is not a manufactured zero measurement.
    pub fn summary(&self) -> Option<TimingSummary> {
        self.delays.processing()
    }

    /// Saturating successful observation count, independent of retention.
    pub const fn observed_events(&self) -> u64 {
        self.events
    }

    /// Configured retention bound, including zero when disabled.
    pub const fn capacity(&self) -> usize {
        self.delays.capacity
    }

    /// Domain required of both supplied points.
    pub const fn domain(&self) -> ClockDomainId {
        self.domain
    }
}
