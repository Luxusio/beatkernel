//! Original native coordinates retained beside their unmodified evidence.
//! Constructors check descriptor consistency, not native acquisition or epoch admission.
use crate::native_audio_presentation::{NativeAudioSnapshot, TargetNativeAudioSnapshot};
use beatkernel::audio::OutputFrameBasisError;
use beatkernel_platform::audio::{
    asio::{AsioPresentationError, AsioPresentationObservation},
    presentation::validation::OriginalNativePresentationEvidence,
};

/// Stream-relative native units. A frame relation is not a hardware counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeOutputCoordinate {
    Counter { position: u64, frequency: u64 },
    StreamFrame { frame: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeOutputCoordinateError {
    InvalidFrequency,
    CoordinateMismatch,
    BasisMismatch,
    UnsupportedEvidence,
    FrameBeforeBasis,
    Overflow,
    MalformedEvidence,
}
impl std::fmt::Display for NativeOutputCoordinateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native output coordinate: {self:?}")
    }
}
impl std::error::Error for NativeOutputCoordinateError {}
impl From<OutputFrameBasisError> for NativeOutputCoordinateError {
    fn from(error: OutputFrameBasisError) -> Self {
        match error {
            OutputFrameBasisError::InvalidFrequency => Self::InvalidFrequency,
            OutputFrameBasisError::Overflow => Self::Overflow,
            OutputFrameBasisError::InvalidRate => Self::MalformedEvidence,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoordinateNativeAudioSnapshot {
    snapshot: NativeAudioSnapshot,
    coordinate: NativeOutputCoordinate,
}
impl CoordinateNativeAudioSnapshot {
    pub fn new(
        snapshot: NativeAudioSnapshot,
        coordinate: NativeOutputCoordinate,
    ) -> Result<Self, NativeOutputCoordinateError> {
        use NativeOutputCoordinateError as Error;
        match (snapshot.evidence, coordinate) {
            (
                OriginalNativePresentationEvidence::Wasapi {
                    snapshot: native,
                    basis,
                },
                NativeOutputCoordinate::Counter {
                    position,
                    frequency,
                },
            ) => {
                if frequency == 0 {
                    return Err(Error::InvalidFrequency);
                }
                if basis != Some(snapshot.basis) {
                    return Err(Error::BasisMismatch);
                }
                let clock = native.clock.ok_or(Error::MalformedEvidence)?;
                if clock.position != position || clock.frequency != frequency {
                    return Err(Error::CoordinateMismatch);
                }
                beatkernel_platform::audio::presentation::observation_with_basis(
                    native,
                    snapshot.basis,
                )
                .map_err(|error| match error {
                    beatkernel_platform::audio::presentation::PresentationError::Overflow => {
                        Error::Overflow
                    }
                    _ => Error::MalformedEvidence,
                })?;
            }
            (
                OriginalNativePresentationEvidence::Asio { observation, basis },
                NativeOutputCoordinate::StreamFrame { frame },
            ) => {
                if basis != Some(snapshot.basis)
                    || observation.sample_rate != snapshot.basis.sample_rate()
                    || observation.output_origin != snapshot.basis.origin()
                {
                    return Err(Error::BasisMismatch);
                }
                let expected = observation
                    .render
                    .start_frame
                    .checked_sub(snapshot.basis.start_physical_frame())
                    .ok_or(Error::FrameBeforeBasis)?;
                if expected != frame {
                    return Err(Error::CoordinateMismatch);
                }
                let validated = AsioPresentationObservation::from_render(
                    observation.render,
                    observation.sample_rate,
                    observation.host,
                    0,
                    0,
                    observation.output_origin,
                )
                .map_err(|error| match error {
                    AsioPresentationError::Overflow => Error::Overflow,
                    _ => Error::MalformedEvidence,
                })?;
                if validated.output != observation.output {
                    return Err(Error::MalformedEvidence);
                }
                if snapshot.basis.point_at_stream_frame(frame)? != observation.output {
                    return Err(Error::CoordinateMismatch);
                }
            }
            (
                OriginalNativePresentationEvidence::SuppliedPair(pair),
                NativeOutputCoordinate::StreamFrame { frame },
            ) => {
                if snapshot.basis.point_at_stream_frame(frame)? != pair.source {
                    return Err(Error::CoordinateMismatch);
                }
            }
            _ => return Err(Error::UnsupportedEvidence),
        }
        Ok(Self {
            snapshot,
            coordinate,
        })
    }
    /// Exact absolute Source duration from the original creation grid and units.
    /// This arithmetic neither admits presentation evidence nor acknowledges output.
    pub fn source_query(
        &self,
    ) -> Result<beatkernel::audio::InterpolatedTargetTime, NativeOutputCoordinateError> {
        let basis = self.snapshot.basis;
        let base = beatkernel::audio::TargetTime::from_frames(
            basis.start_physical_frame(),
            basis.sample_rate(),
        )
        .map_err(|_| NativeOutputCoordinateError::Overflow)?;
        let (units, frequency) = match self.coordinate {
            NativeOutputCoordinate::Counter {
                position,
                frequency,
            } => (position, frequency),
            NativeOutputCoordinate::StreamFrame { frame } => {
                (frame, u64::from(basis.sample_rate()))
            }
        };
        beatkernel::audio::InterpolatedTargetTime::new(base, units, units, frequency, 0, 1)
            .map_err(|_| NativeOutputCoordinateError::Overflow)
    }
    pub const fn snapshot(&self) -> NativeAudioSnapshot {
        self.snapshot
    }
    pub const fn coordinate(&self) -> NativeOutputCoordinate {
        self.coordinate
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoordinateTargetNativeAudioSnapshot {
    snapshot: TargetNativeAudioSnapshot,
    coordinate: NativeOutputCoordinate,
}
impl CoordinateTargetNativeAudioSnapshot {
    pub fn new(
        snapshot: TargetNativeAudioSnapshot,
        coordinate: NativeOutputCoordinate,
    ) -> Result<Self, NativeOutputCoordinateError> {
        use NativeOutputCoordinateError as Error;
        let (
            OriginalNativePresentationEvidence::SuppliedPair(pair),
            NativeOutputCoordinate::StreamFrame { frame },
        ) = (snapshot.evidence, coordinate)
        else {
            return Err(Error::UnsupportedEvidence);
        };
        let point = snapshot
            .basis
            .point_at_stream_frame(frame)
            .map_err(|_| Error::Overflow)?;
        if point != pair.source {
            return Err(Error::CoordinateMismatch);
        }
        Ok(Self {
            snapshot,
            coordinate,
        })
    }
    pub const fn snapshot(&self) -> TargetNativeAudioSnapshot {
        self.snapshot
    }
    pub const fn coordinate(&self) -> NativeOutputCoordinate {
        self.coordinate
    }
}

#[cfg(test)]
#[path = "native_source_coordinate_query_fixtures.rs"]
mod source_query_fixtures;
