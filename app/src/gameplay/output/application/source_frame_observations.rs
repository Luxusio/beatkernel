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
use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;

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
            pending: [None, None],
            latest: None,
            selected: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.pending = [None, None];
        self.latest = None;
        self.selected = None;
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
        let coordinate = observation.coordinate();
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
        let (
            OriginalNativePresentationEvidence::SuppliedPair(pair),
            NativeOutputCoordinate::StreamFrame { frame },
        ) = (snapshot.evidence, coordinate.coordinate())
        else {
            return Err(Error::CoordinateMismatch);
        };
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
            if frame < prior.frame
                || pair.target.timestamp < prior.host.timestamp
                || (frame > prior.frame && pair.target.timestamp == prior.host.timestamp)
            {
                return Err(Error::CoordinateMismatch);
            }
        }
        // Validation also covers input that cannot fit. It must not mark that
        // input selected: an identical read remains eligible after space opens.
        self.latest = Some(Publication {
            coordinate,
            facts,
            frame,
            host: pair.target,
        });
        if self.selected.is_some_and(|(old_frame, old_host)| {
            frame <= old_frame || pair.target.timestamp <= old_host.timestamp
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
        self.selected = Some((frame, pair.target));
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
        let OriginalNativePresentationEvidence::SuppliedPair(pair) =
            first.coordinate().snapshot().evidence
        else {
            unreachable!("only validated Source frame pairs are queued");
        };
        if pair.target.timestamp > now.timestamp {
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
