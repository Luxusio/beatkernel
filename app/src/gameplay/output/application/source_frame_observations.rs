//! Bounded retention of original Source-frame evidence for replacement polling.
use crate::{
    gameplay::output::ports::{
        NativePlaybackObservation, SourcePlaybackFacts, SourcePlaybackIdentity,
        SourcePlaybackPacketError,
    },
    native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate},
};
use beatkernel::{
    audio::OutputFrameBasis,
    time::{ClockDomainId, ClockPoint},
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationError, AsioPresentationObservation},
    presentation::{
        discipline::DisciplineError,
        validation::{NativePresentationValidator, OriginalNativePresentationEvidence},
    },
};

enum EvidenceFamily {
    SuppliedPair,
    Asio(NativePresentationValidator),
}

#[derive(Clone, Copy)]
struct Publication {
    coordinate: CoordinateNativeAudioSnapshot,
    facts: SourcePlaybackFacts,
    frame: u64,
    host: ClockPoint,
}

/// Keeps two selected anchors while validating every newer native publication.
/// Native evidence is moved, without requiring it to implement Clone or Copy.
pub(crate) struct PendingSourceFrameObservations<E> {
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    host_domain: ClockDomainId,
    family: EvidenceFamily,
    pending: [Option<NativePlaybackObservation<E>>; 2],
    latest: Option<Publication>,
    selected: Option<(u64, ClockPoint)>,
}
impl<E> PendingSourceFrameObservations<E> {
    pub(crate) fn new(
        epoch: u64,
        basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        host_domain: ClockDomainId,
    ) -> Self {
        Self {
            epoch,
            basis,
            identity,
            host_domain,
            family: EvidenceFamily::SuppliedPair,
            pending: [None, None],
            latest: None,
            selected: None,
        }
    }

    /// ASIO retains its complete native interval and validates delivered prefixes.
    pub(crate) fn new_asio(
        epoch: u64,
        basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        host_domain: ClockDomainId,
    ) -> Result<Self, SourcePlaybackPacketError> {
        let mut pending = Self::new(epoch, basis, identity, host_domain);
        pending.validate_expectations(epoch, basis, identity, host_domain)?;
        let origin = basis
            .point_at_stream_frame(0)
            .map_err(|_| SourcePlaybackPacketError::Overflow)?;
        pending.family =
            EvidenceFamily::Asio(NativePresentationValidator::new(epoch, origin, host_domain));
        Ok(pending)
    }

    pub(crate) fn clear(&mut self) {
        self.pending = [None, None];
        self.latest = None;
        self.selected = None;
        if let EvidenceFamily::Asio(validator) = &mut self.family {
            *validator = NativePresentationValidator::new(
                self.epoch,
                validator.output_origin(),
                self.host_domain,
            );
        }
    }

    /// Expectations come from the current output and requested Source identity.
    pub(crate) fn validate_expectations(
        &self,
        epoch: u64,
        basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        host_domain: ClockDomainId,
    ) -> Result<(), SourcePlaybackPacketError> {
        use SourcePlaybackPacketError as Error;
        if epoch != self.epoch {
            return Err(Error::EpochMismatch);
        }
        if basis != self.basis
            || basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(Error::BasisMismatch);
        }
        if identity != self.identity {
            return Err(Error::InvalidIdentity);
        }
        if host_domain != self.host_domain {
            return Err(Error::CoordinateMismatch);
        }
        Ok(())
    }

    pub(crate) fn push(
        &mut self,
        observation: NativePlaybackObservation<E>,
    ) -> Result<(), SourcePlaybackPacketError> {
        use SourcePlaybackPacketError as Error;
        let packet = observation.source();
        let mut coordinate = observation.coordinate();
        let snapshot = coordinate.snapshot();
        self.validate_expectations(
            packet.epoch(),
            packet.basis(),
            packet.identity(),
            self.host_domain,
        )?;
        if snapshot.epoch != self.epoch {
            return Err(Error::EpochMismatch);
        }
        if snapshot.basis != self.basis {
            return Err(Error::BasisMismatch);
        }
        let NativeOutputCoordinate::StreamFrame { frame } = coordinate.coordinate() else {
            return Err(Error::CoordinateMismatch);
        };
        let (host, prepared_asio) = match (&self.family, snapshot.evidence) {
            (
                EvidenceFamily::SuppliedPair,
                OriginalNativePresentationEvidence::SuppliedPair(pair),
            ) => {
                if pair.source.domain != self.basis.origin().domain
                    || pair.target.domain != self.host_domain
                    || self
                        .basis
                        .point_at_stream_frame(frame)
                        .map_err(|_| Error::Overflow)?
                        != pair.source
                {
                    return Err(Error::CoordinateMismatch);
                }
                (pair.target, None)
            }
            (
                EvidenceFamily::Asio(validator),
                OriginalNativePresentationEvidence::Asio { observation, basis },
            ) => {
                if basis != Some(self.basis)
                    || self.basis.start_physical_frame().checked_add(frame)
                        != Some(observation.delivered_first_frame)
                {
                    return Err(Error::CoordinateMismatch);
                }
                let original = self.latest.and_then(|prior| {
                    let OriginalNativePresentationEvidence::Asio {
                        observation: previous,
                        basis: previous_basis,
                    } = prior.coordinate.snapshot().evidence
                    else {
                        return None;
                    };
                    let unchanged = AsioPresentationObservation {
                        host: previous.host,
                        ..observation
                    };
                    (basis == previous_basis && unchanged == previous).then_some((prior, previous))
                });
                let invalid = |error| match error {
                    DisciplineError::Overflow
                    | DisciplineError::AsioPresentation(AsioPresentationError::Overflow) => {
                        Error::Overflow
                    }
                    _ => Error::CoordinateMismatch,
                };
                // A renewed clock anchor may remap the same callback earlier or
                // later. Validate that current relation before retaining its first
                // genuine interval; its new bracket is not callback progress.
                let prepared = if let Some((prior, previous)) = original {
                    let current = NativePresentationValidator::new(
                        self.epoch,
                        validator.output_origin(),
                        self.host_domain,
                    );
                    current
                        .prepare_asio(self.epoch, observation, basis)
                        .map_err(invalid)?;
                    coordinate = prior.coordinate;
                    validator
                        .prepare_asio(self.epoch, previous, basis)
                        .map_err(invalid)?
                } else {
                    validator
                        .prepare_asio(self.epoch, observation, basis)
                        .map_err(invalid)?
                };
                let delivered_end =
                    observation
                        .delivered_end_frame()
                        .map_err(|error| match error {
                            AsioPresentationError::Overflow => Error::Overflow,
                            _ => Error::CoordinateMismatch,
                        })?;
                if packet.facts().render != Some(observation.render) {
                    return Err(Error::InvalidReport);
                }
                if delivered_end > packet.facts().submitted_source_end {
                    return Err(Error::InvalidPrefix);
                }
                let host = if let Some((prior, _)) = original {
                    prior.host
                } else {
                    let midpoint = i128::from(observation.host.before.timestamp.as_nanos())
                        + (i128::from(observation.host.after.timestamp.as_nanos())
                            - i128::from(observation.host.before.timestamp.as_nanos()))
                            / 2;
                    ClockPoint {
                        domain: self.host_domain,
                        timestamp: beatkernel::time::Timestamp::from_nanos(
                            i64::try_from(midpoint).map_err(|_| Error::Overflow)?,
                        ),
                    }
                };
                (host, Some(prepared))
            }
            _ => return Err(Error::CoordinateMismatch),
        };
        let facts = packet.facts();
        if facts.revision == 0 || facts.revision & 1 != 0 {
            return Err(Error::InvalidRevision);
        }
        if let Some(prior) = self.latest {
            if facts.revision < prior.facts.revision
                || (facts.revision == prior.facts.revision
                    && (facts != prior.facts || coordinate != prior.coordinate))
            {
                return Err(Error::InvalidRevision);
            }
            if facts.pulled_source_end < prior.facts.pulled_source_end
                || facts.submitted_source_end < prior.facts.submitted_source_end
            {
                return Err(Error::InvalidPrefix);
            }
            if matches!(self.family, EvidenceFamily::SuppliedPair)
                && (frame < prior.frame
                    || host.timestamp < prior.host.timestamp
                    || (frame > prior.frame && host.timestamp == prior.host.timestamp))
            {
                return Err(Error::CoordinateMismatch);
            }
        }
        let observation = if coordinate == observation.coordinate() {
            observation
        } else {
            NativePlaybackObservation::new(coordinate, observation.into_source())?
        };
        if let (EvidenceFamily::Asio(validator), Some(prepared)) = (&mut self.family, prepared_asio)
        {
            validator
                .commit(prepared)
                .map_err(|_| Error::CoordinateMismatch)?;
        }
        // Validation also covers input that cannot fit. It must not mark that
        // input selected: an identical read remains eligible after space opens.
        self.latest = Some(Publication {
            coordinate,
            facts,
            frame,
            host,
        });
        if self.selected.is_some_and(|(old_frame, old_host)| {
            frame <= old_frame || host.timestamp <= old_host.timestamp
        }) {
            return Ok(());
        }
        let slot = if self.pending[0].is_none() {
            0
        } else if self.pending[1].is_none() {
            1
        } else {
            return Ok(());
        };
        self.pending[slot] = Some(observation);
        self.selected = Some((frame, host));
        Ok(())
    }

    pub(crate) fn take_covered(
        &mut self,
        now: ClockPoint,
    ) -> Result<Option<NativePlaybackObservation<E>>, SourcePlaybackPacketError> {
        if now.domain != self.host_domain {
            return Err(SourcePlaybackPacketError::CoordinateMismatch);
        }
        let Some(first) = &self.pending[0] else {
            return Ok(None);
        };
        let covered_after = match first.coordinate().snapshot().evidence {
            OriginalNativePresentationEvidence::SuppliedPair(pair) => pair.target,
            OriginalNativePresentationEvidence::Asio { observation, .. } => observation.host.after,
            _ => unreachable!("only validated Source frame evidence is queued"),
        };
        if covered_after.timestamp > now.timestamp {
            return Ok(None);
        }
        let covered = self.pending[0].take();
        self.pending[0] = self.pending[1].take();
        Ok(covered)
    }
}

#[cfg(test)]
#[path = "source_frame_observations_fixtures.rs"]
mod source_frame_observations_fixtures;

#[cfg(test)]
#[path = "source_frame_asio_observations_fixtures.rs"]
mod source_frame_asio_observations_fixtures;
