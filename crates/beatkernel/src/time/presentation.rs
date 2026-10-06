//! Bounded presentation relations and continuous correction from supplied evidence.
//!
//! This estimator never reads a clock or interprets native device metadata.

use super::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp};
use crate::transport::{Rate, Transport, TransportError};

/// Explicit positive spans and rate bounds for the observer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisciplineConfig {
    /// Retained pairs, in 2..=1024.
    pub capacity: usize,
    /// Minimum host spacing of retained pairs.
    pub retention_interval: Duration,
    /// Minimum host span used to estimate drift.
    pub min_span: Duration,
    /// Minimum spacing of successful transport updates.
    pub update_interval: Duration,
    /// Time horizon over which phase correction is spread.
    pub correction_horizon: Duration,
    /// Maximum host age of the latest progressing observation.
    pub max_observation_age: Duration,
    /// Maximum absolute song/output phase error.
    pub max_phase_error: Duration,
    /// Maximum signed deviation from normal speed, positive and below 1,000,000.
    pub max_rate_error_ppm: u32,
}
impl Default for DisciplineConfig {
    fn default() -> Self {
        Self {
            capacity: 64,
            retention_interval: Duration::from_nanos(100_000_000),
            min_span: Duration::from_nanos(1_000_000_000),
            update_interval: Duration::from_nanos(1_000_000_000),
            correction_horizon: Duration::from_nanos(10_000_000_000),
            max_observation_age: Duration::from_nanos(2_000_000_000),
            max_phase_error: Duration::from_nanos(250_000_000),
            max_rate_error_ppm: 1000,
        }
    }
}

/// Admission distinguishes retained observations, decimated progress and no progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationAdmission {
    /// Progressing observation added to the fixed-capacity ring.
    Retained,
    /// Progress accepted for freshness, below retained spacing.
    Progress,
    /// Output position unchanged; no state/freshness update.
    Unchanged,
    /// An adapter has newer evidence at the same coarse host midpoint and waits
    /// for host progress without updating state or freshness.
    AwaitingHostProgress,
}

/// Explicit skip or successfully applied continuous correction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisciplineUpdate {
    /// Not enough observed host span to estimate drift.
    Warmup {
        /// Current available host span in nanoseconds.
        span_ns: u64,
    },
    /// The successful-update interval has not elapsed.
    IntervalPending,
    /// Rate changed/validated continuously; ppm fields are deviations from normal.
    Applied {
        /// Rounded observed output/host deviation from normal.
        base_rate_ppm: i64,
        /// Rounded phase/horizon correction before limiting.
        correction_ppm: i64,
        /// Final deviation from normal after limiting.
        applied_rate_ppm: i64,
        /// Desired song minus historical transport position at the observation.
        phase_error_ns: i128,
        /// Proposed base plus correction exceeded the configured rate range.
        limited: bool,
    },
}

/// Rejected operation; estimator and transport state remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimatorError {
    /// Output epochs must increase strictly without wrapping.
    InvalidEpoch,
    /// The observation belongs to a different output stream epoch.
    EpochMismatch,
    /// Configuration is nonpositive, out of bounds or cannot cover its minimum span.
    InvalidConfig,
    /// Preallocation failed before any state was constructed.
    AllocationFailed,
    /// Supplied relation/query domains differ from the configured domains.
    DomainMismatch,
    /// Output, host or successful-update chronology regressed or failed to progress.
    NonIncreasing,
    /// No accepted presentation progress exists.
    NoObservation,
    /// Latest accepted progress is older than the configured maximum.
    Stale,
    /// Checked arithmetic or timestamp conversion is not representable.
    Overflow,
    /// Rounded observed base rate exceeds the configured drift bound.
    BaseRateOutOfBounds,
    /// Absolute phase exceeds the configured correction bound.
    PhaseErrorTooLarge,
    /// Unit-song correction cannot update a paused/reverse transport.
    NonpositiveTransport,
    /// Historical transport query or continuous rate change rejected.
    Transport(TransportError),
}
impl std::fmt::Display for EstimatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "presentation estimator: {self:?}")
    }
}
impl std::error::Error for EstimatorError {}
impl From<TransportError> for EstimatorError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

/// Pure bounded observation owner and continuous transport correction policy.
#[derive(Debug)]
pub struct PresentationEstimator {
    epoch: u64,
    config: DisciplineConfig,
    output_origin: ClockPoint,
    playback_origin: ClockPoint,
    host_domain: ClockDomainId,
    applied_song_origin: Timestamp,
    retained: Vec<ClockPair>,
    next: usize,
    latest: Option<ClockPair>,
    last_retained: Option<ClockPair>,
    last_update: Option<Timestamp>,
}
impl Clone for PresentationEstimator {
    fn clone(&self) -> Self {
        let mut retained = Vec::with_capacity(self.config.capacity);
        retained.extend_from_slice(&self.retained);
        Self {
            epoch: self.epoch,
            config: self.config,
            output_origin: self.output_origin,
            playback_origin: self.playback_origin,
            host_domain: self.host_domain,
            applied_song_origin: self.applied_song_origin,
            retained,
            next: self.next,
            latest: self.latest,
            last_retained: self.last_retained,
            last_update: self.last_update,
        }
    }
}
impl PresentationEstimator {
    /// Validates explicit configuration and reserves all observation storage.
    pub fn new(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> Result<Self, EstimatorError> {
        Self::new_with_playback_origin(
            config,
            output_origin,
            output_origin,
            host_domain,
            applied_song_origin,
        )
    }

    /// Separates stream-clock zero from the physical first playback point.
    pub fn new_with_playback_origin(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> Result<Self, EstimatorError> {
        if playback_origin.domain != output_origin.domain {
            return Err(EstimatorError::DomainMismatch);
        }
        if playback_origin.timestamp < output_origin.timestamp {
            return Err(EstimatorError::InvalidConfig);
        }
        let spans = [
            config.retention_interval,
            config.min_span,
            config.update_interval,
            config.correction_horizon,
            config.max_observation_age,
            config.max_phase_error,
        ];
        if !(2..=1024).contains(&config.capacity)
            || spans.iter().any(|span| span.as_nanos() <= 0)
            || !(1..1_000_000).contains(&config.max_rate_error_ppm)
        {
            return Err(EstimatorError::InvalidConfig);
        }
        let covered = i128::from(config.retention_interval.as_nanos())
            .checked_mul((config.capacity - 1) as i128)
            .ok_or(EstimatorError::Overflow)?;
        if covered < i128::from(config.min_span.as_nanos()) {
            return Err(EstimatorError::InvalidConfig);
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(config.capacity)
            .map_err(|_| EstimatorError::AllocationFailed)?;
        Ok(Self {
            epoch: 0,
            config,
            output_origin,
            playback_origin,
            host_domain,
            applied_song_origin,
            retained,
            next: 0,
            latest: None,
            last_retained: None,
            last_update: None,
        })
    }

    /// Current output observation epoch, initially zero and preserved by cloning.
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Replaces only observation origins/history; startup and transport remain caller-owned.
    pub fn rebind_output(
        &mut self,
        epoch: u64,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        song_origin: Timestamp,
    ) -> Result<(), EstimatorError> {
        if epoch <= self.epoch {
            return Err(EstimatorError::InvalidEpoch);
        }
        if playback_origin.domain != output_origin.domain {
            return Err(EstimatorError::DomainMismatch);
        }
        if playback_origin.timestamp < output_origin.timestamp {
            return Err(EstimatorError::InvalidConfig);
        }
        self.epoch = epoch;
        self.output_origin = output_origin;
        self.playback_origin = playback_origin;
        self.applied_song_origin = song_origin;
        self.retained.clear();
        self.next = 0;
        self.latest = None;
        self.last_retained = None;
        self.last_update = None;
        Ok(())
    }
    /// Admits a pair tagged when its stream observation was created.
    /// A mismatched token refuses before domain, history or freshness mutation.
    pub fn observe_clock_pair_in_epoch(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, EstimatorError> {
        if epoch != self.epoch {
            return Err(EstimatorError::EpochMismatch);
        }
        self.observe_clock_pair(pair)
    }
    /// Admits externally proven progress with its original stream epoch token.
    /// Token validation precedes admission; the caller still proves progress in
    /// original evidence and must not relabel a delayed observation.
    pub fn observe_progress_pair_in_epoch(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, EstimatorError> {
        if epoch != self.epoch {
            return Err(EstimatorError::EpochMismatch);
        }
        self.observe_progress_pair(pair)
    }

    /// Supplied relations provide no numeric hardware accuracy bound.
    pub const fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
    /// Validated caller configuration.
    pub const fn config(&self) -> DisciplineConfig {
        self.config
    }
    /// Latest accepted progressing relation, including decimated progress.
    pub fn latest_pair(&self) -> Option<ClockPair> {
        self.latest
    }
    /// Current number of retained observations.
    pub fn retained_len(&self) -> usize {
        self.retained.len()
    }

    /// Admits a supplied relation. Unchanged output cannot refresh freshness.
    pub fn observe_clock_pair(
        &mut self,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, EstimatorError> {
        self.observe_pair(pair, false)
    }

    /// Admits externally proven progress even when output quantization is unchanged.
    /// The caller proves progress in its original evidence; this method checks
    /// domains and nondecreasing output with strictly advancing host time.
    /// Exact duplicates still leave all state and freshness unchanged.
    pub fn observe_progress_pair(
        &mut self,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, EstimatorError> {
        self.observe_pair(pair, true)
    }

    fn observe_pair(
        &mut self,
        pair: ClockPair,
        progress: bool,
    ) -> Result<ObservationAdmission, EstimatorError> {
        if pair.source.domain != self.output_origin.domain || pair.target.domain != self.host_domain
        {
            return Err(EstimatorError::DomainMismatch);
        }
        if let Some(previous) = self.latest {
            if pair.source.timestamp < previous.source.timestamp
                || pair.target.timestamp < previous.target.timestamp
            {
                return Err(EstimatorError::NonIncreasing);
            }
            if pair.source.timestamp == previous.source.timestamp
                && (!progress || pair.target.timestamp == previous.target.timestamp)
            {
                return Ok(ObservationAdmission::Unchanged);
            }
            if pair.target.timestamp == previous.target.timestamp {
                return Err(EstimatorError::NonIncreasing);
            }
        }
        Ok(self.admit(pair))
    }

    fn admit(&mut self, pair: ClockPair) -> ObservationAdmission {
        let retain = self.last_retained.is_none_or(|last| {
            delta(pair.target.timestamp, last.target.timestamp)
                >= i128::from(self.config.retention_interval.as_nanos())
        });
        if retain {
            if self.retained.len() < self.config.capacity {
                self.retained.push(pair);
            } else {
                self.retained[self.next] = pair;
            }
            self.next = (self.next + 1) % self.config.capacity;
            self.last_retained = Some(pair);
        }
        self.latest = Some(pair);
        if retain {
            ObservationAdmission::Retained
        } else {
            ObservationAdmission::Progress
        }
    }

    /// Checks explicit host domain and freshness; historical queries are permitted.
    pub fn validate_host(&self, point: ClockPoint) -> Result<(), EstimatorError> {
        if point.domain != self.host_domain {
            return Err(EstimatorError::DomainMismatch);
        }
        let latest = self.latest.ok_or(EstimatorError::NoObservation)?;
        if delta(point.timestamp, latest.target.timestamp)
            > i128::from(self.config.max_observation_age.as_nanos())
        {
            return Err(EstimatorError::Stale);
        }
        Ok(())
    }

    /// Applies bounded rate correction continuously, preserving historical mapping.
    pub fn update(
        &mut self,
        now: ClockPoint,
        transport: &mut Transport,
    ) -> Result<DisciplineUpdate, EstimatorError> {
        self.validate_host(now)?;
        if transport.anchor().rate.numerator() <= 0 {
            return Err(EstimatorError::NonpositiveTransport);
        }
        if self.last_update.is_some_and(|last| now.timestamp < last) {
            return Err(EstimatorError::NonIncreasing);
        }
        let latest = self.latest.ok_or(EstimatorError::NoObservation)?;
        let oldest = self.retained[if self.retained.len() == self.config.capacity {
            self.next
        } else {
            0
        }];
        let host_span = delta(latest.target.timestamp, oldest.target.timestamp);
        if host_span < i128::from(self.config.min_span.as_nanos()) {
            return Ok(DisciplineUpdate::Warmup {
                span_ns: u64::try_from(host_span).map_err(|_| EstimatorError::Overflow)?,
            });
        }
        if self.last_update.is_some_and(|last| {
            delta(now.timestamp, last) < i128::from(self.config.update_interval.as_nanos())
        }) {
            return Ok(DisciplineUpdate::IntervalPending);
        }
        let output_span = delta(latest.source.timestamp, oldest.source.timestamp);
        let base = rounded(
            output_span
                .checked_mul(1_000_000)
                .ok_or(EstimatorError::Overflow)?,
            host_span,
        )?
        .checked_sub(1_000_000)
        .ok_or(EstimatorError::Overflow)?;
        let bound = i128::from(self.config.max_rate_error_ppm);
        if base.abs() > bound {
            return Err(EstimatorError::BaseRateOutOfBounds);
        }
        let desired = i128::from(self.applied_song_origin.as_nanos())
            .checked_add(delta(
                latest.source.timestamp,
                self.playback_origin.timestamp,
            ))
            .ok_or(EstimatorError::Overflow)?;
        i64::try_from(desired).map_err(|_| EstimatorError::Overflow)?;
        let actual = transport.position_at(latest.target.timestamp)?;
        let phase = desired
            .checked_sub(i128::from(actual.as_nanos()))
            .ok_or(EstimatorError::Overflow)?;
        if phase.abs() > i128::from(self.config.max_phase_error.as_nanos()) {
            return Err(EstimatorError::PhaseErrorTooLarge);
        }
        let correction = rounded(
            phase
                .checked_mul(1_000_000)
                .ok_or(EstimatorError::Overflow)?,
            i128::from(self.config.correction_horizon.as_nanos()),
        )?;
        let proposed = base
            .checked_add(correction)
            .ok_or(EstimatorError::Overflow)?;
        let applied = proposed.clamp(-bound, bound);
        let rate = Rate::new(
            i64::try_from(1_000_000 + applied).map_err(|_| EstimatorError::Overflow)?,
            1_000_000,
        )
        .map_err(|_| EstimatorError::Overflow)?;
        let report = DisciplineUpdate::Applied {
            base_rate_ppm: i64::try_from(base).map_err(|_| EstimatorError::Overflow)?,
            correction_ppm: i64::try_from(correction).map_err(|_| EstimatorError::Overflow)?,
            applied_rate_ppm: i64::try_from(applied).map_err(|_| EstimatorError::Overflow)?,
            phase_error_ns: phase,
            limited: proposed != applied,
        };
        transport.set_rate(now.timestamp, rate)?;
        self.last_update = Some(now.timestamp);
        Ok(report)
    }
}

fn delta(later: Timestamp, earlier: Timestamp) -> i128 {
    i128::from(later.as_nanos()) - i128::from(earlier.as_nanos())
}
fn rounded(numerator: i128, denominator: i128) -> Result<i128, EstimatorError> {
    if denominator <= 0 {
        return Err(EstimatorError::Overflow);
    }
    let magnitude = numerator.checked_abs().ok_or(EstimatorError::Overflow)?;
    let result = magnitude
        .checked_add(denominator / 2)
        .ok_or(EstimatorError::Overflow)?
        / denominator;
    Ok(if numerator < 0 { -result } else { result })
}

#[cfg(test)]
#[path = "presentation_rebind_fixtures.rs"]
mod presentation_rebind_fixtures;
