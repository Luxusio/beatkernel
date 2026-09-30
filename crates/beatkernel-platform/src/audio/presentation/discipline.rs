//! Bounded off-thread presentation observations and continuous unit-song correction.
use super::{observation, PresentationError};
use crate::audio::AudioStreamSnapshot;
use beatkernel::{
    time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport, TransportError},
};

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
    /// Device position unchanged; no state/freshness update.
    Unchanged,
    /// A newer ASIO block has the same coarse host midpoint; no state or
    /// freshness is updated until the timer relation actually progresses.
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
/// Rejected operation; observer and transport state remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisciplineError {
    /// Configuration is nonpositive, out of bounds or cannot cover its minimum span.
    InvalidConfig,
    /// Preallocation failed before any state was constructed.
    AllocationFailed,
    /// Existing native snapshot validation failed.
    Presentation(PresentationError),
    /// Supplied ASIO rendered-block metadata or bounded host interval is invalid.
    AsioPresentation(crate::audio::asio::AsioPresentationError),
    /// Snapshot/query host domain differs from the explicit configured domain.
    DomainMismatch,
    /// An observer cannot mix WASAPI native counters with caller-supplied pairs.
    ObservationSourceChanged,
    /// Native device frequency changed between accepted observations.
    FrequencyChanged,
    /// Device position, QPC, host or successful-update chronology regressed.
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
    /// Unit-song discipline cannot update a paused/reverse transport.
    NonpositiveTransport,
    /// Historical transport query or continuous rate change rejected.
    Transport(TransportError),
}
impl std::fmt::Display for DisciplineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "presentation discipline: {self:?}")
    }
}
impl std::error::Error for DisciplineError {}
impl From<PresentationError> for DisciplineError {
    fn from(value: PresentationError) -> Self {
        Self::Presentation(value)
    }
}
impl From<TransportError> for DisciplineError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservationSource {
    Wasapi {
        frequency: u64,
        position: u64,
        qpc: u64,
    },
    SuppliedPair,
    Asio {
        sample_rate: u32,
        start_frame: u64,
        end_frame: u64,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observed {
    pair: ClockPair,
    source: ObservationSource,
}
/// Bounded observation owner; rate application belongs on a control thread.
#[derive(Clone, Debug)]
pub struct PresentationDiscipline {
    config: DisciplineConfig,
    output_origin: ClockPoint,
    host_domain: ClockDomainId,
    applied_song_origin: Timestamp,
    retained: Vec<Observed>,
    next: usize,
    latest: Option<Observed>,
    last_retained: Option<Observed>,
    last_update: Option<Timestamp>,
}
impl PresentationDiscipline {
    /// Validates explicit configuration and reserves all observation storage.
    pub fn new(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> Result<Self, DisciplineError> {
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
            return Err(DisciplineError::InvalidConfig);
        }
        let covered = i128::from(config.retention_interval.as_nanos())
            .checked_mul((config.capacity - 1) as i128)
            .ok_or(DisciplineError::Overflow)?;
        if covered < i128::from(config.min_span.as_nanos()) {
            return Err(DisciplineError::InvalidConfig);
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(config.capacity)
            .map_err(|_| DisciplineError::AllocationFailed)?;
        Ok(Self {
            config,
            output_origin,
            host_domain,
            applied_song_origin,
            retained,
            next: 0,
            latest: None,
            last_retained: None,
            last_update: None,
        })
    }
    /// The helper has no numeric hardware accuracy bound.
    pub const fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
    /// Validated caller configuration.
    pub const fn config(&self) -> DisciplineConfig {
        self.config
    }
    /// Latest accepted progressing relation, including decimated progress.
    pub fn latest_pair(&self) -> Option<ClockPair> {
        self.latest.map(|sample| sample.pair)
    }
    /// Current number of retained observations.
    pub fn retained_len(&self) -> usize {
        self.retained.len()
    }
    /// Accept coherent native progress without allocating or reading any clocks.
    pub fn observe(
        &mut self,
        snapshot: AudioStreamSnapshot,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if self
            .latest
            .is_some_and(|previous| previous.source == ObservationSource::SuppliedPair)
        {
            return Err(DisciplineError::ObservationSourceChanged);
        }
        let (pair, frequency, position, qpc) = observation(snapshot, self.output_origin)?;
        if pair.target.domain != self.host_domain {
            return Err(DisciplineError::DomainMismatch);
        }
        let sample = Observed {
            pair,
            source: ObservationSource::Wasapi {
                frequency,
                position,
                qpc,
            },
        };
        if let Some(previous) = self.latest {
            let ObservationSource::Wasapi {
                frequency: previous_frequency,
                position: previous_position,
                qpc: previous_qpc,
            } = previous.source
            else {
                return Err(DisciplineError::ObservationSourceChanged);
            };
            if frequency != previous_frequency {
                return Err(DisciplineError::FrequencyChanged);
            }
            if sample == previous {
                return Ok(ObservationAdmission::Unchanged);
            }
            if position < previous_position
                || qpc <= previous_qpc
                || pair.target.timestamp <= previous.pair.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if position == previous_position {
                return Ok(ObservationAdmission::Unchanged);
            }
        }
        Ok(self.admit(sample))
    }
    /// Admit an explicitly supplied output/host relation without fabricating
    /// native counters. This source cannot be mixed with WASAPI observations.
    /// Duplicates/unchanged output do not refresh accepted progress or retention.
    pub fn observe_clock_pair(
        &mut self,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if pair.source.domain != self.output_origin.domain || pair.target.domain != self.host_domain
        {
            return Err(DisciplineError::DomainMismatch);
        }
        if let Some(previous) = self.latest {
            if previous.source != ObservationSource::SuppliedPair {
                return Err(DisciplineError::ObservationSourceChanged);
            }
            if pair.source.timestamp < previous.pair.source.timestamp
                || pair.target.timestamp < previous.pair.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if pair.source.timestamp == previous.pair.source.timestamp {
                return Ok(ObservationAdmission::Unchanged);
            }
            if pair.target.timestamp == previous.pair.target.timestamp {
                return Err(DisciplineError::NonIncreasing);
            }
        }
        Ok(self.admit(Observed {
            pair,
            source: ObservationSource::SuppliedPair,
        }))
    }

    /// Admits an ASIO rendered block using its bounded host interval midpoint.
    ///
    /// Keeps ASIO rate/frame identity distinct from WASAPI counters and generic
    /// supplied pairs. Midpoints support continuous correction only; discipline
    /// quality remains Unknown and does not discard uncertainty to claim exact
    /// acoustic synchronization. Duplicate blocks do not refresh freshness.
    pub fn observe_asio(
        &mut self,
        observation: crate::audio::asio::AsioPresentationObservation,
    ) -> Result<ObservationAdmission, DisciplineError> {
        use crate::audio::asio::{AsioPresentationError, AsioPresentationObservation};
        if observation.output_origin != self.output_origin
            || observation.host.before.domain != self.host_domain
            || observation.host.after.domain != self.host_domain
        {
            return Err(DisciplineError::DomainMismatch);
        }
        let validated = AsioPresentationObservation::from_render(
            observation.render,
            observation.sample_rate,
            observation.host,
            0,
            0,
            observation.output_origin,
        )
        .map_err(DisciplineError::AsioPresentation)?;
        if validated.output != observation.output {
            return Err(DisciplineError::AsioPresentation(
                AsioPresentationError::Malformed,
            ));
        }
        let midpoint = i128::from(observation.host.before.timestamp.as_nanos())
            + (i128::from(observation.host.after.timestamp.as_nanos())
                - i128::from(observation.host.before.timestamp.as_nanos()))
                / 2;
        let target = ClockPoint {
            domain: self.host_domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(midpoint).map_err(|_| DisciplineError::Overflow)?,
            ),
        };
        let pair = ClockPair {
            source: observation.output,
            target,
        };
        let end_frame = observation
            .render
            .start_frame
            .checked_add(
                u64::try_from(observation.render.frames).map_err(|_| DisciplineError::Overflow)?,
            )
            .ok_or(DisciplineError::Overflow)?;
        if let Some(previous) = self.latest {
            let ObservationSource::Asio {
                sample_rate,
                start_frame,
                end_frame,
            } = previous.source
            else {
                return Err(DisciplineError::ObservationSourceChanged);
            };
            if sample_rate != observation.sample_rate {
                return Err(DisciplineError::FrequencyChanged);
            }
            if observation.render.start_frame < start_frame {
                return Err(DisciplineError::NonIncreasing);
            }
            if observation.render.start_frame == start_frame {
                return Ok(ObservationAdmission::Unchanged);
            }
            if observation.render.start_frame < end_frame
                || pair.source.timestamp <= previous.pair.source.timestamp
                || pair.target.timestamp < previous.pair.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if pair.target.timestamp == previous.pair.target.timestamp {
                return Ok(ObservationAdmission::AwaitingHostProgress);
            }
        }
        Ok(self.admit(Observed {
            pair,
            source: ObservationSource::Asio {
                sample_rate: observation.sample_rate,
                start_frame: observation.render.start_frame,
                end_frame,
            },
        }))
    }
    fn admit(&mut self, sample: Observed) -> ObservationAdmission {
        let pair = sample.pair;
        let retain = self.last_retained.is_none_or(|last| {
            delta(pair.target.timestamp, last.pair.target.timestamp)
                >= i128::from(self.config.retention_interval.as_nanos())
        });
        if retain {
            if self.retained.len() < self.config.capacity {
                self.retained.push(sample);
            } else {
                self.retained[self.next] = sample;
            }
            self.next = (self.next + 1) % self.config.capacity;
            self.last_retained = Some(sample);
        }
        self.latest = Some(sample);
        if retain {
            ObservationAdmission::Retained
        } else {
            ObservationAdmission::Progress
        }
    }
    /// Check explicit host domain and freshness; historical queries are permitted.
    pub fn validate_host(&self, point: ClockPoint) -> Result<(), DisciplineError> {
        if point.domain != self.host_domain {
            return Err(DisciplineError::DomainMismatch);
        }
        let latest = self.latest.ok_or(DisciplineError::NoObservation)?;
        if delta(point.timestamp, latest.pair.target.timestamp)
            > i128::from(self.config.max_observation_age.as_nanos())
        {
            return Err(DisciplineError::Stale);
        }
        Ok(())
    }
    /// Apply bounded rate correction continuously, preserving historical mapping.
    pub fn update(
        &mut self,
        now: ClockPoint,
        transport: &mut Transport,
    ) -> Result<DisciplineUpdate, DisciplineError> {
        self.validate_host(now)?;
        if transport.anchor().rate.numerator() <= 0 {
            return Err(DisciplineError::NonpositiveTransport);
        }
        if self.last_update.is_some_and(|last| now.timestamp < last) {
            return Err(DisciplineError::NonIncreasing);
        }
        let latest = self.latest.ok_or(DisciplineError::NoObservation)?;
        let oldest = self.retained[if self.retained.len() == self.config.capacity {
            self.next
        } else {
            0
        }];
        let host_span = delta(latest.pair.target.timestamp, oldest.pair.target.timestamp);
        if host_span < i128::from(self.config.min_span.as_nanos()) {
            return Ok(DisciplineUpdate::Warmup {
                span_ns: u64::try_from(host_span).map_err(|_| DisciplineError::Overflow)?,
            });
        }
        if self.last_update.is_some_and(|last| {
            delta(now.timestamp, last) < i128::from(self.config.update_interval.as_nanos())
        }) {
            return Ok(DisciplineUpdate::IntervalPending);
        }
        let output_span = delta(latest.pair.source.timestamp, oldest.pair.source.timestamp);
        let base = rounded(
            output_span
                .checked_mul(1_000_000)
                .ok_or(DisciplineError::Overflow)?,
            host_span,
        )?
        .checked_sub(1_000_000)
        .ok_or(DisciplineError::Overflow)?;
        let bound = i128::from(self.config.max_rate_error_ppm);
        if base.abs() > bound {
            return Err(DisciplineError::BaseRateOutOfBounds);
        }
        let desired = i128::from(self.applied_song_origin.as_nanos())
            .checked_add(delta(
                latest.pair.source.timestamp,
                self.output_origin.timestamp,
            ))
            .ok_or(DisciplineError::Overflow)?;
        i64::try_from(desired).map_err(|_| DisciplineError::Overflow)?;
        let actual = transport.position_at(latest.pair.target.timestamp)?;
        let phase = desired
            .checked_sub(i128::from(actual.as_nanos()))
            .ok_or(DisciplineError::Overflow)?;
        if phase.abs() > i128::from(self.config.max_phase_error.as_nanos()) {
            return Err(DisciplineError::PhaseErrorTooLarge);
        }
        let correction = rounded(
            phase
                .checked_mul(1_000_000)
                .ok_or(DisciplineError::Overflow)?,
            i128::from(self.config.correction_horizon.as_nanos()),
        )?;
        let proposed = base
            .checked_add(correction)
            .ok_or(DisciplineError::Overflow)?;
        let applied = proposed.clamp(-bound, bound);
        let rate = Rate::new(
            i64::try_from(1_000_000 + applied).map_err(|_| DisciplineError::Overflow)?,
            1_000_000,
        )
        .map_err(|_| DisciplineError::Overflow)?;
        let report = DisciplineUpdate::Applied {
            base_rate_ppm: i64::try_from(base).map_err(|_| DisciplineError::Overflow)?,
            correction_ppm: i64::try_from(correction).map_err(|_| DisciplineError::Overflow)?,
            applied_rate_ppm: i64::try_from(applied).map_err(|_| DisciplineError::Overflow)?,
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
fn rounded(numerator: i128, denominator: i128) -> Result<i128, DisciplineError> {
    if denominator <= 0 {
        return Err(DisciplineError::Overflow);
    }
    let magnitude = numerator.checked_abs().ok_or(DisciplineError::Overflow)?;
    let result = magnitude
        .checked_add(denominator / 2)
        .ok_or(DisciplineError::Overflow)?
        / denominator;
    Ok(if numerator < 0 { -result } else { result })
}
