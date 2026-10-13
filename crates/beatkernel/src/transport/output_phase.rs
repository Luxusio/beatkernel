//! Mathematical active-output phase values; native admission remains caller-owned.
use super::{LiveTransportSetupError, TransportError};
use crate::{
    audio::SourcePosition,
    time::{ClockDomainId, ClockPoint},
};
use std::fmt;

/// Immutable caller-owned attempt identity and original active source-frame cut.
/// IDs establish equality only, never a native clock relation or admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputPhaseBasis {
    domain: ClockDomainId,
    owner: u64,
    generation: u64,
    active_origin_frame: u64,
    source_hz: u32,
}

/// Validated mathematical phase with the unchanged normalized chronology point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputPhasePoint {
    basis: OutputPhaseBasis,
    logical: ClockPoint,
    elapsed_nanos: i128,
}

/// Refusal before a phase operation changes gameplay or transport state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPhaseError {
    /// The source sample rate is zero.
    ZeroSourceRate,
    /// The fractional source position is not a proper nonnegative fraction.
    InvalidFraction,
    /// The normalized clock domain differs from the bound domain.
    WrongDomain,
    /// Attempt owner, generation, original cut, rate or domain differs.
    WrongBasis,
    /// The phase chronology point differs from the normalized operation point.
    LogicalPointMismatch,
    /// The source position precedes its original active-frame cut.
    BeforeOrigin,
    /// A supplied active duration is negative.
    NegativeElapsed,
    /// Active elapsed time precedes an already admitted/control phase.
    RegressingElapsed,
    /// A paused phase differs from its acknowledged frozen elapsed time.
    InvalidState,
    /// The transport does not have an opted output phase.
    MissingPhase,
    /// Scalar timing cannot operate an opted phase transport.
    PhaseRequired,
    /// Cold whole-preroll setup was invalid.
    Setup(LiveTransportSetupError),
    /// Existing chronology or song representation failed.
    Transport(TransportError),
    /// A required value is unrepresentable.
    Overflow,
}
impl fmt::Display for OutputPhaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "output phase: {self:?}")
    }
}
impl std::error::Error for OutputPhaseError {}

impl OutputPhaseBasis {
    /// Validates mathematical coordinates only. The caller supplies its actual
    /// owner and generation; this does not allocate a global clock/attempt ID.
    pub fn new(
        domain: ClockDomainId,
        owner: u64,
        generation: u64,
        active_origin_frame: u64,
        source_hz: u32,
    ) -> Result<Self, OutputPhaseError> {
        if source_hz == 0 {
            return Err(OutputPhaseError::ZeroSourceRate);
        }
        Ok(Self {
            domain,
            owner,
            generation,
            active_origin_frame,
            source_hz,
        })
    }
    /// Normalized chronology domain; it does not prove a native relation.
    pub const fn domain(self) -> ClockDomainId {
        self.domain
    }
    /// Actual caller-owned session or output identity.
    pub const fn owner(self) -> u64 {
        self.owner
    }
    /// Caller-owned attempt generation.
    pub const fn generation(self) -> u64 {
        self.generation
    }
    /// Original active source-frame cut, excluding acknowledged silence.
    pub const fn active_origin_frame(self) -> u64 {
        self.active_origin_frame
    }
    /// Immutable physical source sample rate.
    pub const fn source_hz(self) -> u32 {
        self.source_hz
    }

    /// Wraps an already once-floored exact relative active duration.
    ///
    /// This mathematical handoff permits interpolation intermediates wider than
    /// a SourcePosition fraction. The caller must derive elapsed from the real
    /// active source cut, removing actual acknowledged silence before flooring.
    /// Domain equality and nonnegative elapsed do not prove clock mapping,
    /// native presentation, pause acknowledgement or input admission.
    pub fn point_from_elapsed_nanos(
        self,
        logical: ClockPoint,
        elapsed: i128,
    ) -> Result<OutputPhasePoint, OutputPhaseError> {
        if logical.domain != self.domain {
            return Err(OutputPhaseError::WrongDomain);
        }
        if elapsed < 0 {
            return Err(OutputPhaseError::NegativeElapsed);
        }
        Ok(OutputPhasePoint {
            basis: self,
            logical,
            elapsed_nanos: elapsed,
        })
    }

    /// Computes ONE elapsed floor from ACTIVE integer/fractional source frames.
    /// A native bridge must first remove actual acknowledged silent frames from
    /// physical SourcePosition. This pure builder proves neither presentation
    /// nor input admission. Proper unreduced fractions are mathematically valid.
    pub fn point(
        self,
        logical: ClockPoint,
        active: SourcePosition,
    ) -> Result<OutputPhasePoint, OutputPhaseError> {
        if logical.domain != self.domain {
            return Err(OutputPhaseError::WrongDomain);
        }
        if active.denominator == 0 || active.numerator >= active.denominator {
            return Err(OutputPhaseError::InvalidFraction);
        }
        let frames = active
            .frame
            .checked_sub(self.active_origin_frame)
            .ok_or(OutputPhaseError::BeforeOrigin)?;
        let hz = u128::from(self.source_hz);
        let nanos = u128::from(frames) * 1_000_000_000;
        let denominator = u128::from(active.denominator);
        // At most 97 bits: avoid multiplying the full frame count by denominator.
        let residual = (nanos % hz) * denominator + u128::from(active.numerator) * 1_000_000_000;
        let elapsed = nanos / hz + residual / (hz * denominator);
        Ok(OutputPhasePoint {
            basis: self,
            logical,
            elapsed_nanos: i128::try_from(elapsed).map_err(|_| OutputPhaseError::Overflow)?,
        })
    }
}
impl OutputPhasePoint {
    /// Immutable basis used to derive this phase.
    pub const fn basis(self) -> OutputPhaseBasis {
        self.basis
    }
    /// Unchanged normalized chronology point.
    pub const fn logical(self) -> ClockPoint {
        self.logical
    }
    /// One floor of relative active source duration in nanoseconds.
    pub const fn elapsed_nanos(self) -> i128 {
        self.elapsed_nanos
    }
}
