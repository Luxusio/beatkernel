//! Native presentation evidence admission over the pure bounded core estimator.
use super::{PresentationError, observation, observation_with_basis};
use crate::audio::AudioStreamSnapshot;
use beatkernel::{
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp,
        presentation::{EstimatorError, PresentationEstimator},
    },
    transport::{Transport, TransportError},
};

pub use beatkernel::time::presentation::{DisciplineConfig, DisciplineUpdate, ObservationAdmission};

/// Rejected operation; observer and transport state remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisciplineError {
    /// Output epochs must increase strictly without wrapping.
    InvalidEpoch,
    /// The observation belongs to a different output stream epoch.
    EpochMismatch,
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
impl From<EstimatorError> for DisciplineError {
    fn from(value: EstimatorError) -> Self {
        match value {
            EstimatorError::InvalidEpoch => Self::InvalidEpoch,
            EstimatorError::EpochMismatch => Self::EpochMismatch,
            EstimatorError::InvalidConfig => Self::InvalidConfig,
            EstimatorError::AllocationFailed => Self::AllocationFailed,
            EstimatorError::DomainMismatch => Self::DomainMismatch,
            EstimatorError::NonIncreasing => Self::NonIncreasing,
            EstimatorError::NoObservation => Self::NoObservation,
            EstimatorError::Stale => Self::Stale,
            EstimatorError::Overflow => Self::Overflow,
            EstimatorError::BaseRateOutOfBounds => Self::BaseRateOutOfBounds,
            EstimatorError::PhaseErrorTooLarge => Self::PhaseErrorTooLarge,
            EstimatorError::NonpositiveTransport => Self::NonpositiveTransport,
            EstimatorError::Transport(error) => Self::Transport(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservationSource {
    Wasapi {
        basis: Option<beatkernel::audio::OutputFrameBasis>,
        frequency: u64,
        position: u64,
        qpc: u64,
    },
    SuppliedPair,
    Asio {
        basis: Option<beatkernel::audio::OutputFrameBasis>,
        sample_rate: u32,
        start_frame: u64,
        end_frame: u64,
    },
}

/// Native metadata owner; the core estimator alone retains the observation ring.
#[derive(Clone, Debug)]
pub struct PresentationDiscipline {
    estimator: PresentationEstimator,
    output_origin: ClockPoint,
    host_domain: ClockDomainId,
    latest_source: Option<ObservationSource>,
}
impl PresentationDiscipline {
    /// Validates explicit configuration and reserves all observation storage.
    pub fn new(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> Result<Self, DisciplineError> {
        Self::new_with_playback_origin(
            config,
            output_origin,
            output_origin,
            host_domain,
            applied_song_origin,
        )
    }

    /// Separates native stream-clock zero from the physical first playback point.
    /// Native observation identity/conversion always retains output_origin.
    pub fn new_with_playback_origin(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> Result<Self, DisciplineError> {
        let estimator = PresentationEstimator::new_with_playback_origin(
            config,
            output_origin,
            playback_origin,
            host_domain,
            applied_song_origin,
        )?;
        Ok(Self {
            estimator,
            output_origin,
            host_domain,
            latest_source: None,
        })
    }

    /// The helper has no numeric hardware accuracy bound.
    pub const fn quality(&self) -> ClockMappingQuality {
        self.estimator.quality()
    }
    /// Current output observation epoch, initially zero.
    pub const fn epoch(&self) -> u64 {
        self.estimator.epoch()
    }
    /// Rebinds a strictly newer stream epoch and clears source identity only
    /// after core origin validation succeeds; transport remains caller-owned.
    pub fn rebind_output(
        &mut self,
        epoch: u64,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        song_origin: Timestamp,
    ) -> Result<(), DisciplineError> {
        self.estimator
            .rebind_output(epoch, output_origin, playback_origin, song_origin)?;
        self.output_origin = output_origin;
        self.latest_source = None;
        Ok(())
    }
    /// The token must come from the creating stream, never from a delayed callback's admission.
    pub fn observe_in_epoch(
        &mut self,
        epoch: u64,
        snapshot: AudioStreamSnapshot,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if epoch != self.epoch() {
            return Err(DisciplineError::EpochMismatch);
        }
        self.observe(snapshot)
    }
    /// Checks the pair's stream-creation token before source validation or admission.
    pub fn observe_clock_pair_in_epoch(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if epoch != self.epoch() {
            return Err(DisciplineError::EpochMismatch);
        }
        self.observe_clock_pair(pair)
    }
    /// Checks the ASIO observation's original epoch before metadata conversion.
    pub fn observe_asio_in_epoch(
        &mut self,
        epoch: u64,
        observation: crate::audio::asio::AsioPresentationObservation,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if epoch != self.epoch() {
            return Err(DisciplineError::EpochMismatch);
        }
        self.observe_asio(observation)
    }
    /// Validated caller configuration.
    pub const fn config(&self) -> DisciplineConfig {
        self.estimator.config()
    }
    /// Latest accepted progressing relation, including decimated progress.
    pub fn latest_pair(&self) -> Option<ClockPair> {
        self.estimator.latest_pair()
    }
    /// Current number of retained observations.
    pub fn retained_len(&self) -> usize {
        self.estimator.retained_len()
    }

    /// Accept coherent native progress without allocating or reading any clocks.
    pub fn observe(
        &mut self,
        snapshot: AudioStreamSnapshot,
    ) -> Result<ObservationAdmission, DisciplineError> {
        self.observe_wasapi(snapshot, None)
    }
    /// Admits native evidence on a fixed original mixer frame basis within this epoch.
    pub fn observe_with_basis(
        &mut self,
        snapshot: AudioStreamSnapshot,
        basis: beatkernel::audio::OutputFrameBasis,
    ) -> Result<ObservationAdmission, DisciplineError> {
        self.observe_wasapi(snapshot, Some(basis))
    }
    /// Rejects a stale stream token before interpreting basis or native metadata.
    pub fn observe_with_basis_in_epoch(
        &mut self,
        epoch: u64,
        snapshot: AudioStreamSnapshot,
        basis: beatkernel::audio::OutputFrameBasis,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if epoch != self.epoch() {
            return Err(DisciplineError::EpochMismatch);
        }
        self.observe_with_basis(snapshot, basis)
    }
    fn observe_wasapi(
        &mut self,
        snapshot: AudioStreamSnapshot,
        basis: Option<beatkernel::audio::OutputFrameBasis>,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if let Some(basis) = basis {
            if basis
                .point_at_native_counter(0, 1)
                .map_err(|_| DisciplineError::Overflow)?
                != self.output_origin
            {
                return Err(DisciplineError::DomainMismatch);
            }
        }
        if self.latest_source == Some(ObservationSource::SuppliedPair) {
            return Err(DisciplineError::ObservationSourceChanged);
        }
        let (pair, frequency, position, qpc) = if let Some(basis) = basis {
            observation_with_basis(snapshot, basis)?
        } else {
            observation(snapshot, self.output_origin)?
        };
        if pair.target.domain != self.host_domain {
            return Err(DisciplineError::DomainMismatch);
        }
        let source = ObservationSource::Wasapi {
            basis,
            frequency,
            position,
            qpc,
        };
        if let Some(previous_source) = self.latest_source {
            let ObservationSource::Wasapi {
                basis: previous_basis,
                frequency: previous_frequency,
                position: previous_position,
                qpc: previous_qpc,
            } = previous_source
            else {
                return Err(DisciplineError::ObservationSourceChanged);
            };
            if basis != previous_basis {
                return Err(DisciplineError::ObservationSourceChanged);
            }
            let previous = self
                .estimator
                .latest_pair()
                .expect("source metadata follows admitted pair");
            if frequency != previous_frequency {
                return Err(DisciplineError::FrequencyChanged);
            }
            if source == previous_source && pair == previous {
                return Ok(ObservationAdmission::Unchanged);
            }
            if position < previous_position
                || qpc <= previous_qpc
                || pair.target.timestamp <= previous.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if position == previous_position {
                return Ok(ObservationAdmission::Unchanged);
            }
        }
        let admission = self.estimator.observe_progress_pair(pair)?;
        self.latest_source = Some(source);
        Ok(admission)
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
        if self
            .latest_source
            .is_some_and(|source| source != ObservationSource::SuppliedPair)
        {
            return Err(DisciplineError::ObservationSourceChanged);
        }
        let admission = self.estimator.observe_clock_pair(pair)?;
        self.latest_source = Some(ObservationSource::SuppliedPair);
        Ok(admission)
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
        self.observe_asio_impl(observation, None)
    }
    /// Checks the original creation token before admitting an absolute ASIO block
    /// against its original grid and this stream's captured physical frame basis.
    pub fn observe_asio_with_basis_in_epoch(
        &mut self,
        epoch: u64,
        observation: crate::audio::asio::AsioPresentationObservation,
        basis: beatkernel::audio::OutputFrameBasis,
    ) -> Result<ObservationAdmission, DisciplineError> {
        if epoch != self.epoch() {
            return Err(DisciplineError::EpochMismatch);
        }
        self.observe_asio_impl(observation, Some(basis))
    }
    fn observe_asio_impl(
        &mut self,
        observation: crate::audio::asio::AsioPresentationObservation,
        basis: Option<beatkernel::audio::OutputFrameBasis>,
    ) -> Result<ObservationAdmission, DisciplineError> {
        use crate::audio::asio::{AsioPresentationError, AsioPresentationObservation};
        let expected_origin = if let Some(basis) = basis {
            if basis.sample_rate() != observation.sample_rate
                || observation.render.start_frame < basis.start_physical_frame()
            {
                return Err(DisciplineError::AsioPresentation(
                    AsioPresentationError::Malformed,
                ));
            }
            if basis
                .point_at_stream_frame(0)
                .map_err(|_| DisciplineError::Overflow)?
                != self.output_origin
            {
                return Err(DisciplineError::DomainMismatch);
            }
            basis.origin()
        } else {
            self.output_origin
        };
        if observation.output_origin != expected_origin
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
        if let Some(previous_source) = self.latest_source {
            let ObservationSource::Asio {
                basis: previous_basis,
                sample_rate,
                start_frame,
                end_frame,
            } = previous_source
            else {
                return Err(DisciplineError::ObservationSourceChanged);
            };
            if basis != previous_basis {
                return Err(DisciplineError::ObservationSourceChanged);
            }
            let previous = self
                .estimator
                .latest_pair()
                .expect("source metadata follows admitted pair");
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
                || pair.source.timestamp <= previous.source.timestamp
                || pair.target.timestamp < previous.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if pair.target.timestamp == previous.target.timestamp {
                return Ok(ObservationAdmission::AwaitingHostProgress);
            }
        }
        let admission = self.estimator.observe_clock_pair(pair)?;
        self.latest_source = Some(ObservationSource::Asio {
            basis,
            sample_rate: observation.sample_rate,
            start_frame: observation.render.start_frame,
            end_frame,
        });
        Ok(admission)
    }

    /// Check explicit host domain and freshness; historical queries are permitted.
    pub fn validate_host(&self, point: ClockPoint) -> Result<(), DisciplineError> {
        self.estimator
            .validate_host(point)
            .map_err(DisciplineError::from)
    }
    /// Apply bounded rate correction continuously, preserving historical mapping.
    pub fn update(
        &mut self,
        now: ClockPoint,
        transport: &mut Transport,
    ) -> Result<DisciplineUpdate, DisciplineError> {
        self.estimator
            .update(now, transport)
            .map_err(DisciplineError::from)
    }
}

#[cfg(test)]
#[path = "rebind_fixtures.rs"]
mod rebind_fixtures;
