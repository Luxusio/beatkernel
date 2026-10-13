//! Same-publication source control facts, without a native ACK or gameplay commit.
use crate::{
    audio_authority::AudioAuthorityCoordinateBasis,
    gameplay::output::ports::{
        NativePlaybackObservation, NativeTargetPlaybackFacts, NativeTargetPlaybackObservation,
        SourcePlaybackFacts, SourcePlaybackIdentity,
    },
    native_audio_presentation::NativeAudioPresentation,
    native_output_coordinate::{
        CoordinateNativeAudioSnapshot, CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
    },
};
use beatkernel::{
    audio::{InterpolatedTargetTime, RenderReport},
    time::ClockPair,
};
use beatkernel_platform::audio::ConvertedBoundaryFacts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateControlError {
    Unsupported,
    Identity,
    Descriptor,
    Observation,
    Arithmetic,
}
impl std::fmt::Display for CoordinateControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "coordinate control facts: {self:?}")
    }
}
impl std::error::Error for CoordinateControlError {}

/// Preserves the admitted original observation and its coherent source publication.
/// Construction checks the existing owners; it does not authenticate hardware,
/// drain input, prepare a control cutoff or acknowledge a source adoption.
#[derive(Clone, Copy, Debug)]
pub struct CoordinateControlFacts {
    basis: AudioAuthorityCoordinateBasis,
    identity: SourcePlaybackIdentity,
    coordinate: CoordinateNativeAudioSnapshot,
    pair: ClockPair,
    source: SourcePlaybackFacts,
    query: InterpolatedTargetTime,
}
impl CoordinateControlFacts {
    pub fn from_source<E>(
        observation: &NativePlaybackObservation<E>,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
    ) -> Result<Self, CoordinateControlError> {
        let basis = presentation
            .authority()
            .coordinate_basis()
            .ok_or(CoordinateControlError::Unsupported)?;
        if matches!(basis, AudioAuthorityCoordinateBasis::TargetFrame(_)) {
            return Err(CoordinateControlError::Unsupported);
        }
        let pair = presentation
            .authority()
            .latest_coordinate_observation()
            .ok_or(CoordinateControlError::Observation)?
            .pair();
        let packet = observation.source();
        let facts = Self {
            basis,
            identity: packet.identity(),
            coordinate: observation.coordinate(),
            pair,
            source: packet.facts(),
            query: packet.query(),
        };
        facts.validate(presentation, expected_identity)?;
        Ok(facts)
    }
    pub const fn epoch(&self) -> u64 {
        self.coordinate.snapshot().epoch
    }
    pub const fn basis(&self) -> AudioAuthorityCoordinateBasis {
        self.basis
    }
    pub const fn identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub const fn coordinate(&self) -> CoordinateNativeAudioSnapshot {
        self.coordinate
    }
    pub const fn pair(&self) -> ClockPair {
        self.pair
    }
    /// Feed this same report into existing native pause/end staging, without rereading IO.
    pub const fn source(&self) -> SourcePlaybackFacts {
        self.source
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.query
    }

    /// A later native observation invalidates these facts for a new preparation.
    /// This is provenance validation only, not freshness or acquired-input permission.
    pub fn validate(
        &self,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
    ) -> Result<(), CoordinateControlError> {
        use CoordinateControlError as Error;
        presentation
            .validate_coordinate_seed()
            .map_err(|_| Error::Observation)?;
        let authority = presentation.authority();
        let epoch = authority.epoch();
        let snapshot = self.coordinate.snapshot();
        if snapshot.epoch != epoch.id || self.identity != expected_identity {
            return Err(Error::Identity);
        }
        let basis = match (self.basis, self.coordinate.coordinate()) {
            (
                AudioAuthorityCoordinateBasis::SourceFrame(basis),
                NativeOutputCoordinate::StreamFrame { .. },
            ) => basis,
            (
                AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
                NativeOutputCoordinate::Counter {
                    frequency: actual, ..
                },
            ) if frequency == actual && frequency != 0 => basis,
            (AudioAuthorityCoordinateBasis::TargetFrame(_), _) => return Err(Error::Unsupported),
            _ => return Err(Error::Descriptor),
        };
        if authority.coordinate_basis() != Some(self.basis)
            || snapshot.basis != basis
            || presentation.basis() != Some(basis)
            || presentation.target_basis().is_some()
            || basis
                .point_at_stream_frame(0)
                .map_err(|_| Error::Arithmetic)?
                != epoch.stream_origin
            || self.identity.hz() != basis.sample_rate()
            || self.identity.initial_physical() > basis.start_physical_frame()
            || self.pair.target.domain != epoch.host_domain
        {
            return Err(Error::Descriptor);
        }
        let latest = authority
            .latest_coordinate_observation()
            .ok_or(Error::Observation)?;
        let record = presentation.latest_record().ok_or(Error::Observation)?;
        if latest.pair() != self.pair
            || latest.coordinate() != Some(self.coordinate.coordinate())
            || record.pair() != self.pair
            || *record.evidence() != snapshot.evidence
        {
            return Err(Error::Observation);
        }
        let raw = self
            .query
            .point(basis.origin())
            .map_err(|_| Error::Arithmetic)?;
        if raw != self.pair.source {
            return Err(Error::Observation);
        }
        epoch.logical_output(raw).map_err(|_| Error::Descriptor)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_coordinate_control_fixtures.rs"]
mod fixtures;

/// The admitted original Target observation and its compound converted publication.
/// These facts establish provenance, not native freshness, input drain or an ACK.
#[derive(Clone, Copy, Debug)]
pub struct TargetCoordinateControlFacts {
    basis: AudioAuthorityCoordinateBasis,
    identity: SourcePlaybackIdentity,
    coordinate: CoordinateTargetNativeAudioSnapshot,
    pair: ClockPair,
    target: NativeTargetPlaybackFacts,
    query: InterpolatedTargetTime,
}
impl TargetCoordinateControlFacts {
    pub fn from_target<E>(
        observation: &NativeTargetPlaybackObservation<E>,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
    ) -> Result<Self, CoordinateControlError> {
        let basis = presentation
            .authority()
            .coordinate_basis()
            .ok_or(CoordinateControlError::Unsupported)?;
        if !matches!(basis, AudioAuthorityCoordinateBasis::TargetFrame(_)) {
            return Err(CoordinateControlError::Unsupported);
        }
        let pair = presentation
            .authority()
            .latest_coordinate_observation()
            .ok_or(CoordinateControlError::Observation)?
            .pair();
        let packet = observation.playback();
        let facts = Self {
            basis,
            identity: packet.identity(),
            coordinate: observation.coordinate(),
            pair,
            target: packet.facts(),
            query: packet.query(),
        };
        facts.validate(presentation, expected_identity)?;
        Ok(facts)
    }
    pub const fn epoch(&self) -> u64 {
        self.coordinate.snapshot().epoch
    }
    pub const fn basis(&self) -> AudioAuthorityCoordinateBasis {
        self.basis
    }
    pub const fn identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub const fn coordinate(&self) -> CoordinateTargetNativeAudioSnapshot {
        self.coordinate
    }
    pub const fn pair(&self) -> ClockPair {
        self.pair
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        self.query
    }
    /// Original reports and projected boundaries, without a fabricated Source prefix.
    pub const fn target(&self) -> NativeTargetPlaybackFacts {
        self.target
    }
    /// A different latest admission invalidates this publication for new preparation.
    pub fn validate(
        &self,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
    ) -> Result<(), CoordinateControlError> {
        use CoordinateControlError as Error;
        presentation
            .validate_coordinate_seed()
            .map_err(|_| Error::Observation)?;
        let authority = presentation.authority();
        let epoch = authority.epoch();
        let snapshot = self.coordinate.snapshot();
        if snapshot.epoch != epoch.id || self.identity != expected_identity {
            return Err(Error::Identity);
        }
        let (
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
            NativeOutputCoordinate::StreamFrame { frame },
        ) = (self.basis, self.coordinate.coordinate())
        else {
            return Err(Error::Unsupported);
        };
        if authority.coordinate_basis() != Some(self.basis)
            || snapshot.basis != basis
            || presentation.target_basis() != Some(basis)
            || presentation.basis().is_some()
            || basis
                .point_at_stream_frame(0)
                .map_err(|_| Error::Arithmetic)?
                != epoch.stream_origin
            || self.pair.target.domain != epoch.host_domain
        {
            return Err(Error::Descriptor);
        }
        let latest = authority
            .latest_coordinate_observation()
            .ok_or(Error::Observation)?;
        let record = presentation.latest_record().ok_or(Error::Observation)?;
        if latest.pair() != self.pair
            || latest.coordinate() != Some(self.coordinate.coordinate())
            || record.pair() != self.pair
            || *record.evidence() != snapshot.evidence
        {
            return Err(Error::Observation);
        }
        if !self
            .query
            .compare_native_time_with_offset(
                basis.start_time(),
                frame,
                u64::from(basis.sample_rate()),
                0,
            )
            .map_err(|_| Error::Arithmetic)?
            .is_eq()
        {
            return Err(Error::Observation);
        }
        let raw = self
            .query
            .point(basis.origin())
            .map_err(|_| Error::Arithmetic)?;
        if raw != self.pair.source {
            return Err(Error::Observation);
        }
        epoch.logical_output(raw).map_err(|_| Error::Descriptor)?;
        Ok(())
    }
}

/// Common control input preserves each publication's original source semantics.
#[derive(Clone, Copy, Debug)]
pub enum CoordinatePlaybackControlFacts {
    Source(CoordinateControlFacts),
    Target(TargetCoordinateControlFacts),
}
impl CoordinatePlaybackControlFacts {
    pub const fn epoch(&self) -> u64 {
        match self {
            Self::Source(facts) => facts.epoch(),
            Self::Target(facts) => facts.epoch(),
        }
    }
    pub const fn basis(&self) -> AudioAuthorityCoordinateBasis {
        match self {
            Self::Source(facts) => facts.basis(),
            Self::Target(facts) => facts.basis(),
        }
    }
    pub const fn identity(&self) -> SourcePlaybackIdentity {
        match self {
            Self::Source(facts) => facts.identity(),
            Self::Target(facts) => facts.identity(),
        }
    }
    pub const fn pair(&self) -> ClockPair {
        match self {
            Self::Source(facts) => facts.pair(),
            Self::Target(facts) => facts.pair(),
        }
    }
    pub const fn query(&self) -> InterpolatedTargetTime {
        match self {
            Self::Source(facts) => facts.query(),
            Self::Target(facts) => facts.query(),
        }
    }
    pub const fn report(&self) -> Option<RenderReport> {
        match self {
            Self::Source(facts) => facts.source().render,
            Self::Target(facts) => facts.target().source,
        }
    }
    pub const fn target_boundaries(&self) -> Option<ConvertedBoundaryFacts> {
        match self {
            Self::Source(_) => None,
            Self::Target(facts) => Some(facts.target().boundaries),
        }
    }
    pub fn validate(
        &self,
        presentation: &NativeAudioPresentation,
        expected_identity: SourcePlaybackIdentity,
    ) -> Result<(), CoordinateControlError> {
        match self {
            Self::Source(facts) => facts.validate(presentation, expected_identity),
            Self::Target(facts) => facts.validate(presentation, expected_identity),
        }
    }
}
impl From<CoordinateControlFacts> for CoordinatePlaybackControlFacts {
    fn from(facts: CoordinateControlFacts) -> Self {
        Self::Source(facts)
    }
}
impl From<TargetCoordinateControlFacts> for CoordinatePlaybackControlFacts {
    fn from(facts: TargetCoordinateControlFacts) -> Self {
        Self::Target(facts)
    }
}

#[cfg(test)]
#[path = "native_target_coordinate_control_fixtures.rs"]
mod native_target_coordinate_control_fixtures;
