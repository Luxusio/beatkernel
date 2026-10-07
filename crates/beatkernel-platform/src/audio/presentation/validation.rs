//! Native evidence validation with one accepted record, without an estimator or clock.
use super::{observation, observation_with_basis};
use super::discipline::DisciplineError;
use crate::audio::{
    AudioStreamSnapshot,
    asio::{AsioPresentationError, AsioPresentationObservation},
};
use beatkernel::{
    audio::OutputFrameBasis,
    time::{ClockDomainId, ClockPair, ClockPoint, Timestamp},
};

/// Original evidence remains available independently of its correlation pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalNativePresentationEvidence {
    /// Coherent WASAPI telemetry with its original native counter and QPC association.
    Wasapi {
        /// Unmodified caller snapshot; validation does not authenticate native acquisition.
        snapshot: AudioStreamSnapshot,
        /// Captured physical mixer grid, if this stream uses an explicit frame basis.
        basis: Option<OutputFrameBasis>,
    },
    /// Caller-supplied relation without invented WASAPI or ASIO counter metadata.
    SuppliedPair(ClockPair),
    /// Original rendered ASIO block and complete caller-provided HOST interval.
    Asio {
        /// Retains both bracket endpoints; midpoint conversion does not replace them.
        observation: AsioPresentationObservation,
        /// Captured physical mixer grid, if this stream uses an explicit frame basis.
        basis: Option<OutputFrameBasis>,
    },
}

/// Native progress classification independent of estimator retention and transport updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeObservationAdmission {
    /// Valid progress; WASAPI counters may advance at the same integer output nanosecond.
    Progress,
    /// Duplicate or unchanged native position; committing does not refresh the record.
    Unchanged,
    /// A newer ASIO block has the same coarse HOST midpoint; retain previous evidence.
    AwaitingHostProgress,
}

/// One admitted native record; ASIO keeps both HOST bracket endpoints and the render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePresentationRecord {
    evidence: OriginalNativePresentationEvidence,
    pair: ClockPair,
}
impl NativePresentationRecord {
    /// Borrows the unchanged evidence accepted for this record.
    pub const fn evidence(&self) -> &OriginalNativePresentationEvidence {
        &self.evidence
    }
    /// Converted output/HOST relation; an ASIO target is a correlation midpoint only.
    pub const fn pair(&self) -> ClockPair {
        self.pair
    }
}

/// Commit rejection before any metadata or epoch mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePreparationError {
    /// Prior semantic state or reconstructed evidence differs from the prepared token.
    StalePreparation,
}
impl std::fmt::Display for NativePreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native presentation preparation no longer matches its owner")
    }
}
impl std::error::Error for NativePreparationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeState {
    epoch: u64,
    output_origin: ClockPoint,
    host_domain: ClockDomainId,
    latest: Option<NativePresentationRecord>,
}

/// Immutable validated evidence bound to the exact prior epoch, origins and native record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedNativePresentation {
    state: NativeState,
    candidate: NativePresentationRecord,
    admission: NativeObservationAdmission,
}
impl PreparedNativePresentation {
    /// Reports whether this evidence can add a correlation pair or must remain deferred.
    pub const fn admission(&self) -> NativeObservationAdmission {
        self.admission
    }
    /// Returns a pair only for progress; duplicates and coarse HOST deferrals supply none.
    pub const fn correlation_pair(&self) -> Option<ClockPair> {
        match self.admission {
            NativeObservationAdmission::Progress => Some(self.candidate.pair),
            _ => None,
        }
    }
    /// The full original ASIO interval is required by pause/end; its midpoint is not a cutoff.
    pub const fn evidence(&self) -> &OriginalNativePresentationEvidence {
        &self.candidate.evidence
    }
}
/// Strictly newer epoch/origin transition staged against the previous native state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedNativeRebind {
    state: NativeState,
    epoch: u64,
    output_origin: ClockPoint,
}

/// One-record native chronology and source identity owner, with no allocation or rate loop.
#[derive(Clone, Debug)]
pub struct NativePresentationValidator {
    state: NativeState,
}
impl NativePresentationValidator {
    /// Declares stream identity with no accepted evidence or allocation.
    /// Native observations are checked during preparation, without sampling a clock.
    pub const fn new(epoch: u64, output_origin: ClockPoint, host_domain: ClockDomainId) -> Self {
        Self {
            state: NativeState {
                epoch,
                output_origin,
                host_domain,
                latest: None,
            },
        }
    }
    /// Current caller-assigned stream creation epoch.
    pub const fn epoch(&self) -> u64 {
        self.state.epoch
    }
    /// Declared output origin used for native counter/grid conversion.
    pub const fn output_origin(&self) -> ClockPoint {
        self.state.output_origin
    }
    /// Declared domain of original native HOST associations.
    pub const fn host_domain(&self) -> ClockDomainId {
        self.state.host_domain
    }
    /// Last committed progressing evidence; duplicates and deferrals leave it unchanged.
    pub const fn latest_record(&self) -> Option<NativePresentationRecord> {
        self.state.latest
    }
    /// Relation from the last committed progressing record; absent before first admission.
    pub fn latest_pair(&self) -> Option<ClockPair> {
        self.state.latest.map(|record| record.pair)
    }

    /// Validate epoch, basis, running/accurate telemetry and native counter chronology.
    /// Position/frequency are native units; advancing counters may quantize to equal nanoseconds.
    /// Rejection and preparation alone leave the accepted record unchanged.
    pub fn prepare_wasapi(
        &self,
        epoch: u64,
        snapshot: AudioStreamSnapshot,
        basis: Option<OutputFrameBasis>,
    ) -> Result<PreparedNativePresentation, DisciplineError> {
        self.validate_epoch(epoch)?;
        if let Some(basis) = basis {
            if basis
                .point_at_native_counter(0, 1)
                .map_err(|_| DisciplineError::Overflow)?
                != self.output_origin()
            {
                return Err(DisciplineError::DomainMismatch);
            }
        }
        // Preserve the legacy precedence: supplied-source mixing precedes snapshot conversion.
        if self.state.latest.is_some_and(|previous| {
            matches!(
                previous.evidence,
                OriginalNativePresentationEvidence::SuppliedPair(_)
            )
        }) {
            return Err(DisciplineError::ObservationSourceChanged);
        }
        let (pair, frequency, position, qpc) = if let Some(basis) = basis {
            observation_with_basis(snapshot, basis)?
        } else {
            observation(snapshot, self.output_origin())?
        };
        if pair.target.domain != self.host_domain() {
            return Err(DisciplineError::DomainMismatch);
        }
        let candidate = NativePresentationRecord {
            pair,
            evidence: OriginalNativePresentationEvidence::Wasapi { snapshot, basis },
        };
        if let Some(previous) = self.state.latest {
            let OriginalNativePresentationEvidence::Wasapi {
                snapshot: previous_snapshot,
                basis: previous_basis,
            } = previous.evidence
            else {
                return Err(DisciplineError::ObservationSourceChanged);
            };
            if basis != previous_basis {
                return Err(DisciplineError::ObservationSourceChanged);
            }
            let previous_clock = previous_snapshot
                .clock
                .expect("accepted WASAPI evidence has a native clock");
            if frequency != previous_clock.frequency {
                return Err(DisciplineError::FrequencyChanged);
            }
            if position == previous_clock.position
                && qpc == previous_clock.qpc_100ns
                && pair == previous.pair
            {
                return Ok(self.prepared(candidate, NativeObservationAdmission::Unchanged));
            }
            if position < previous_clock.position
                || qpc <= previous_clock.qpc_100ns
                || pair.target.timestamp <= previous.pair.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if position == previous_clock.position {
                return Ok(self.prepared(candidate, NativeObservationAdmission::Unchanged));
            }
            // Native counter progress may quantize to an unchanged output nanosecond.
            if pair.source.timestamp < previous.pair.source.timestamp {
                return Err(DisciplineError::NonIncreasing);
            }
        }
        Ok(self.prepared(candidate, NativeObservationAdmission::Progress))
    }

    /// Validate a supplied relation without manufacturing native source metadata.
    /// Source kinds cannot mix; increasing output requires strictly increasing HOST time.
    /// Unchanged output with nonregressing HOST time does not refresh accepted progress.
    pub fn prepare_pair(
        &self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<PreparedNativePresentation, DisciplineError> {
        self.validate_epoch(epoch)?;
        if pair.source.domain != self.output_origin().domain
            || pair.target.domain != self.host_domain()
        {
            return Err(DisciplineError::DomainMismatch);
        }
        let candidate = NativePresentationRecord {
            pair,
            evidence: OriginalNativePresentationEvidence::SuppliedPair(pair),
        };
        if let Some(previous) = self.state.latest {
            if !matches!(
                previous.evidence,
                OriginalNativePresentationEvidence::SuppliedPair(_)
            ) {
                return Err(DisciplineError::ObservationSourceChanged);
            }
            if pair.source.timestamp < previous.pair.source.timestamp
                || pair.target.timestamp < previous.pair.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if pair.source.timestamp == previous.pair.source.timestamp {
                return Ok(self.prepared(candidate, NativeObservationAdmission::Unchanged));
            }
            if pair.target.timestamp == previous.pair.target.timestamp {
                return Err(DisciplineError::NonIncreasing);
            }
        }
        Ok(self.prepared(candidate, NativeObservationAdmission::Progress))
    }

    /// Validate original ASIO rate/grid/extent, full HOST bracket and block chronology.
    /// Preserve both interval endpoints while deriving a checked midpoint for correlation.
    /// A newer block at the same midpoint defers without changing accepted evidence.
    pub fn prepare_asio(
        &self,
        epoch: u64,
        observation: AsioPresentationObservation,
        basis: Option<OutputFrameBasis>,
    ) -> Result<PreparedNativePresentation, DisciplineError> {
        self.validate_epoch(epoch)?;
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
                != self.output_origin()
            {
                return Err(DisciplineError::DomainMismatch);
            }
            basis.origin()
        } else {
            self.output_origin()
        };
        if observation.output_origin != expected_origin
            || observation.host.before.domain != self.host_domain()
            || observation.host.after.domain != self.host_domain()
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
        let pair = ClockPair {
            source: observation.output,
            target: ClockPoint {
                domain: self.host_domain(),
                timestamp: Timestamp::from_nanos(
                    i64::try_from(midpoint).map_err(|_| DisciplineError::Overflow)?,
                ),
            },
        };
        // Validate the native extent even when this block is a duplicate.
        asio_end(observation)?;
        let candidate = NativePresentationRecord {
            pair,
            evidence: OriginalNativePresentationEvidence::Asio { observation, basis },
        };
        if let Some(previous) = self.state.latest {
            let OriginalNativePresentationEvidence::Asio {
                observation: previous_observation,
                basis: previous_basis,
            } = previous.evidence
            else {
                return Err(DisciplineError::ObservationSourceChanged);
            };
            if basis != previous_basis {
                return Err(DisciplineError::ObservationSourceChanged);
            }
            if previous_observation.sample_rate != observation.sample_rate {
                return Err(DisciplineError::FrequencyChanged);
            }
            if observation.render.start_frame < previous_observation.render.start_frame {
                return Err(DisciplineError::NonIncreasing);
            }
            if observation.render.start_frame == previous_observation.render.start_frame {
                return Ok(self.prepared(candidate, NativeObservationAdmission::Unchanged));
            }
            if observation.render.start_frame < asio_end(previous_observation)?
                || pair.source.timestamp <= previous.pair.source.timestamp
                || pair.target.timestamp < previous.pair.target.timestamp
            {
                return Err(DisciplineError::NonIncreasing);
            }
            if pair.target.timestamp == previous.pair.target.timestamp {
                return Ok(
                    self.prepared(candidate, NativeObservationAdmission::AwaitingHostProgress)
                );
            }
        }
        Ok(self.prepared(candidate, NativeObservationAdmission::Progress))
    }

    /// Commit only after the caller's own pair consumer has accepted the prepared evidence.
    pub fn commit(
        &mut self,
        prepared: PreparedNativePresentation,
    ) -> Result<NativeObservationAdmission, NativePreparationError> {
        if self.state != prepared.state {
            return Err(NativePreparationError::StalePreparation);
        }
        let expected = match prepared.candidate.evidence {
            OriginalNativePresentationEvidence::Wasapi { snapshot, basis } => {
                self.prepare_wasapi(prepared.state.epoch, snapshot, basis)
            }
            OriginalNativePresentationEvidence::SuppliedPair(pair) => {
                self.prepare_pair(prepared.state.epoch, pair)
            }
            OriginalNativePresentationEvidence::Asio { observation, basis } => {
                self.prepare_asio(prepared.state.epoch, observation, basis)
            }
        };
        if expected.ok() != Some(prepared) {
            return Err(NativePreparationError::StalePreparation);
        }
        if prepared.admission == NativeObservationAdmission::Progress {
            self.state.latest = Some(prepared.candidate);
        }
        Ok(prepared.admission)
    }
    /// Stage a strictly newer creation epoch and output origin without changing this owner.
    /// The original HOST domain remains fixed; transport/playback-origin validation is external.
    pub fn prepare_rebind(
        &self,
        epoch: u64,
        output_origin: ClockPoint,
    ) -> Result<PreparedNativeRebind, DisciplineError> {
        if epoch <= self.epoch() {
            return Err(DisciplineError::InvalidEpoch);
        }
        Ok(PreparedNativeRebind {
            state: self.state,
            epoch,
            output_origin,
        })
    }
    /// Recheck staged semantic state, replace epoch/origin and clear only native evidence.
    /// Caller-owned estimator, transport and physical stream state are not modified.
    pub fn commit_rebind(
        &mut self,
        prepared: PreparedNativeRebind,
    ) -> Result<(), NativePreparationError> {
        if self.state != prepared.state
            || self
                .prepare_rebind(prepared.epoch, prepared.output_origin)
                .ok()
                != Some(prepared)
        {
            return Err(NativePreparationError::StalePreparation);
        }
        self.state.epoch = prepared.epoch;
        self.state.output_origin = prepared.output_origin;
        self.state.latest = None;
        Ok(())
    }
    fn validate_epoch(&self, epoch: u64) -> Result<(), DisciplineError> {
        if epoch != self.epoch() {
            Err(DisciplineError::EpochMismatch)
        } else {
            Ok(())
        }
    }
    fn prepared(
        &self,
        candidate: NativePresentationRecord,
        admission: NativeObservationAdmission,
    ) -> PreparedNativePresentation {
        PreparedNativePresentation {
            state: self.state,
            candidate,
            admission,
        }
    }
}
fn asio_end(observation: AsioPresentationObservation) -> Result<u64, DisciplineError> {
    observation
        .render
        .start_frame
        .checked_add(
            u64::try_from(observation.render.frames).map_err(|_| DisciplineError::Overflow)?,
        )
        .ok_or(DisciplineError::Overflow)
}
