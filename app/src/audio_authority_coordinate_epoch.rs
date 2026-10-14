//! Atomic replacement of original-coordinate correlation, without scalar admission.
use super::*;
use crate::local_input::InputMergerPreparationState;

/// Cold preparation retains the exact prior authority and drained input owner.
/// Its fields grant no native observation, control acknowledgement or publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCoordinateEpoch {
    state: PreparationState,
    merger: InputMergerPreparationState,
    next: AudioAuthorityEpoch,
    next_basis: AudioAuthorityCoordinateBasis,
}
impl PreparedCoordinateEpoch {
    pub const fn next_epoch(&self) -> AudioAuthorityEpoch {
        self.next
    }
    pub const fn next_basis(&self) -> AudioAuthorityCoordinateBasis {
        self.next_basis
    }
}

/// Two unchanged original native pairs and units, staged together for publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedPrimedCoordinateEpoch {
    staged: PreparedCoordinateEpoch,
    observations: [(ClockPair, NativeOutputCoordinate); 2],
    now: ClockPoint,
}
impl PreparedPrimedCoordinateEpoch {
    pub const fn next_epoch(&self) -> AudioAuthorityEpoch {
        self.staged.next
    }
    pub const fn next_basis(&self) -> AudioAuthorityCoordinateBasis {
        self.staged.next_basis
    }
    pub const fn observations(&self) -> [(ClockPair, NativeOutputCoordinate); 2] {
        self.observations
    }
    pub const fn prepared_at(&self) -> ClockPoint {
        self.now
    }
}

impl AudioAuthority {
    /// Prepare an exact new creation descriptor while preserving every frontier.
    /// The bounded merger snapshot is allocated here, outside native callbacks.
    pub fn prepare_coordinate_epoch(
        &self,
        next: AudioAuthorityEpoch,
        next_basis: AudioAuthorityCoordinateBasis,
        merger: &InputMerger,
    ) -> Result<PreparedCoordinateEpoch, AudioAuthorityError> {
        self.validate_coordinate_epoch_descriptor(next, next_basis)?;
        if merger.pending() != 0 {
            return Err(AudioAuthorityError::PendingInputs);
        }
        let merger_state = merger.preparation_state()?;
        self.validate_coordinate_epoch_merger(&merger_state, merger)?;
        self.next_revision()?;
        Ok(PreparedCoordinateEpoch {
            state: self.state(),
            merger: merger_state,
            next,
            next_basis,
        })
    }

    /// Stage actual original native coordinates; no affine scalar mapper is made.
    pub fn prepare_primed_coordinate_epoch(
        &self,
        staged: PreparedCoordinateEpoch,
        observations: [(ClockPair, NativeOutputCoordinate); 2],
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<PreparedPrimedCoordinateEpoch, AudioAuthorityError> {
        let prepared = PreparedPrimedCoordinateEpoch {
            staged,
            observations,
            now,
        };
        self.validate_primed_coordinate_epoch(&prepared, now, merger)?;
        Ok(prepared)
    }

    /// Recheck unchanged owners, exact chronology and freshness without allocation.
    pub fn validate_primed_coordinate_epoch(
        &self,
        prepared: &PreparedPrimedCoordinateEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if merger.pending() != 0 {
            return Err(AudioAuthorityError::PendingInputs);
        }
        let staged = &prepared.staged;
        if staged.state != self.state() || !merger.matches_preparation(&staged.merger) {
            return Err(AudioAuthorityError::StalePreparation);
        }
        self.validate_coordinate_epoch_descriptor(staged.next, staged.next_basis)?;
        self.validate_coordinate_epoch_merger(&staged.merger, merger)?;
        self.validate_host(prepared.now)?;
        self.validate_host(now)?;
        if now.timestamp < prepared.now.timestamp {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        let [first, second] =
            prepared
                .observations
                .map(|(pair, coordinate)| AudioAuthorityObservation {
                    pair,
                    coordinate: Some(coordinate),
                });
        for (pair, coordinate) in prepared.observations {
            if pair.source.domain != staged.next.stream_origin.domain
                || pair.target.domain != staged.next.host_domain
            {
                return Err(AudioAuthorityError::DomainMismatch);
            }
            if pair.target.timestamp < staged.merger.origin() {
                return Err(MergeError::BeforeOrigin {
                    timestamp: pair.target.timestamp,
                    origin: staged.merger.origin(),
                }
                .into());
            }
            if pair.target.timestamp > now.timestamp {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            if staged.next_basis.point(coordinate)? != pair.source {
                return Err(AudioAuthorityError::CoordinateMismatch);
            }
            let query = staged.next_basis.query(AudioAuthorityObservation {
                pair,
                coordinate: Some(coordinate),
            })?;
            self.validate_coordinate_epoch_progress(query)?;
            let output = Self::logical_in_epoch(staged.next, pair.source)?;
            if self
                .operation
                .is_some_and(|last| output.timestamp < last.timestamp)
            {
                return Err(AudioAuthorityError::OperationRegression);
            }
            if self
                .presentation
                .is_some_and(|last| output.timestamp < last.timestamp)
            {
                return Err(AudioAuthorityError::ObservationRegression);
            }
            if !self.pair_is_fresh(pair, now) {
                return Err(AudioAuthorityError::HistoryExpired);
            }
        }
        if CoordinateInputMapper::units(first)? >= CoordinateInputMapper::units(second)?
            || first.pair.source.timestamp > second.pair.source.timestamp
            || first.pair.target.timestamp >= second.pair.target.timestamp
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        for last in [
            self.acquired,
            self.closed,
            self.input_host,
            self.latest_observation().map(|pair| pair.target),
        ]
        .into_iter()
        .flatten()
        {
            if second.pair.target.timestamp < last.timestamp {
                return Err(AudioAuthorityError::ObservationRegression);
            }
        }
        // Exact units can progress while their rounded nanosecond endpoints agree.
        staged
            .next_basis
            .interpolate(first, second, second.pair.target)?;
        self.next_revision()?;
        Ok(())
    }

    /// Publish only the epoch, basis and original observations in reserved storage.
    /// Acquired/input/operation/presentation frontiers remain verbatim unchanged.
    pub fn commit_primed_coordinate_epoch(
        &mut self,
        prepared: PreparedPrimedCoordinateEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.commit_primed_coordinate_epoch_borrowed(&prepared, now, merger)
    }

    pub(crate) fn commit_primed_coordinate_epoch_borrowed(
        &mut self,
        prepared: &PreparedPrimedCoordinateEpoch,
        now: ClockPoint,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        self.validate_primed_coordinate_epoch(prepared, now, merger)?;
        let revision = self.next_revision()?;
        self.epoch = prepared.staged.next;
        self.coordinate_basis = Some(prepared.staged.next_basis);
        self.history.clear();
        self.history
            .extend(
                prepared
                    .observations
                    .map(|(pair, coordinate)| AudioAuthorityObservation {
                        pair,
                        coordinate: Some(coordinate),
                    }),
            );
        self.revision = revision;
        Ok(())
    }

    fn validate_coordinate_epoch_descriptor(
        &self,
        next: AudioAuthorityEpoch,
        next_basis: AudioAuthorityCoordinateBasis,
    ) -> Result<(), AudioAuthorityError> {
        let previous_basis = self
            .coordinate_basis
            .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
        validate_epoch_domains(next)?;
        if next.id <= self.epoch.id {
            return Err(AudioAuthorityError::WrongEpoch);
        }
        if next.host_domain != self.epoch.host_domain
            || next.logical_origin.domain != self.epoch.logical_origin.domain
            || next.stream_origin.domain != self.epoch.stream_origin.domain
        {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        if !matches!(
            (previous_basis, next_basis),
            (
                AudioAuthorityCoordinateBasis::SourceFrame(_),
                AudioAuthorityCoordinateBasis::SourceFrame(_)
            ) | (
                AudioAuthorityCoordinateBasis::SourceCounter { .. },
                AudioAuthorityCoordinateBasis::SourceCounter { .. }
            ) | (
                AudioAuthorityCoordinateBasis::TargetFrame(_),
                AudioAuthorityCoordinateBasis::TargetFrame(_)
            )
        ) {
            return Err(AudioAuthorityError::CoordinateModeMismatch);
        }
        let (base, frequency, physical_origin) = next_basis.descriptor()?;
        let (previous_base, _, previous_origin) = previous_basis.descriptor()?;
        if frequency == 0 {
            return Err(AudioAuthorityError::InvalidConfig);
        }
        if physical_origin.domain != previous_origin.domain {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        if physical_origin != previous_origin
            || next_basis.origin()? != next.stream_origin
            || Self::logical_in_epoch(self.epoch, next.stream_origin)? != next.logical_origin
        {
            return Err(AudioAuthorityError::CoordinateMismatch);
        }
        let query = InterpolatedTargetTime::new(base, 0, 0, frequency, 0, 1)
            .map_err(|_| AudioAuthorityError::Overflow)?;
        if query.compare_target_time(previous_base).is_lt() {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        self.validate_coordinate_epoch_progress(query)?;
        if self
            .operation
            .is_some_and(|last| next.logical_origin.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        if self
            .presentation
            .is_some_and(|last| next.logical_origin.timestamp < last.timestamp)
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        Ok(())
    }

    fn validate_coordinate_epoch_progress(
        &self,
        query: InterpolatedTargetTime,
    ) -> Result<(), AudioAuthorityError> {
        if self
            .coordinate_operation
            .is_some_and(|last| query.compare_interpolated_time(last).is_lt())
        {
            return Err(AudioAuthorityError::OperationRegression);
        }
        if self
            .coordinate_presentation
            .is_some_and(|last| query.compare_interpolated_time(last).is_lt())
        {
            return Err(AudioAuthorityError::ObservationRegression);
        }
        if let Some(latest) = self.history.back().copied() {
            let basis = self
                .coordinate_basis
                .ok_or(AudioAuthorityError::CoordinateModeMismatch)?;
            if query
                .compare_interpolated_time(basis.query(latest)?)
                .is_lt()
            {
                return Err(AudioAuthorityError::ObservationRegression);
            }
        }
        Ok(())
    }

    fn validate_coordinate_epoch_merger(
        &self,
        state: &InputMergerPreparationState,
        merger: &InputMerger,
    ) -> Result<(), AudioAuthorityError> {
        if state.domain() != self.epoch.host_domain {
            return Err(AudioAuthorityError::DomainMismatch);
        }
        for point in [
            self.acquired,
            self.closed,
            self.input_host,
            self.history.front().map(|entry| entry.pair.target),
        ]
        .into_iter()
        .flatten()
        {
            if point.timestamp < state.origin() {
                return Err(MergeError::BeforeOrigin {
                    timestamp: point.timestamp,
                    origin: state.origin(),
                }
                .into());
            }
        }
        if let Some(prefix) = self.acquired {
            merger.peek_ready(prefix)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "audio_authority_coordinate_epoch_fixtures.rs"]
mod fixtures;
