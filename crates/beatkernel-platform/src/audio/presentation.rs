//! Pure conversion of observed WASAPI stream-relative positions to output/host maps.
pub mod discipline;

use super::{AudioClockReadingQuality, AudioStreamSnapshot, AudioStreamStatus};
use beatkernel::{
    time::{
        AffineClockMapper, CalibrationError, CalibrationUncertainty, ClockInterval,
        ClockMappingQuality, ClockPair, ClockPoint, ExtrapolationPolicy, Timestamp,
    },
    transport::{Rate, Transport},
};

/// Failed observation/calibration; no assumed relation is substituted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationError {
    /// Snapshot is unavailable, nonrunning or missing its clock/host association.
    Unavailable,
    /// Native reading is degraded or has unknown accuracy status.
    Inaccurate,
    /// Initial propagation has not produced a nonzero position yet.
    BeforePresentation,
    /// Native frequency is zero or changed between observations.
    FrequencyChanged,
    /// Position, QPC or normalized host association did not strictly increase.
    NonIncreasing,
    /// Checked conversion or inverse transport rate is not representable.
    Overflow,
    /// Explicit affine calibration rejected these supplied relations or limits.
    Calibration(CalibrationError),
}
impl std::fmt::Display for PresentationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WASAPI presentation: {self:?}")
    }
}
impl std::error::Error for PresentationError {}
impl From<CalibrationError> for PresentationError {
    fn from(value: CalibrationError) -> Self {
        Self::Calibration(value)
    }
}
fn observation(
    snapshot: AudioStreamSnapshot,
    output_origin: ClockPoint,
) -> Result<(ClockPair, u64, u64, u64), PresentationError> {
    observation_impl(snapshot, output_origin, None)
}
/// Converts native counter evidence with one rational floor on the original grid.
pub fn observation_with_basis(
    snapshot: AudioStreamSnapshot,
    basis: beatkernel::audio::OutputFrameBasis,
) -> Result<(ClockPair, u64, u64, u64), PresentationError> {
    observation_impl(snapshot, basis.origin(), Some(basis))
}
fn observation_impl(
    snapshot: AudioStreamSnapshot,
    output_origin: ClockPoint,
    basis: Option<beatkernel::audio::OutputFrameBasis>,
) -> Result<(ClockPair, u64, u64, u64), PresentationError> {
    if !snapshot.telemetry_available || snapshot.status != AudioStreamStatus::Running {
        return Err(PresentationError::Unavailable);
    }
    let clock = snapshot.clock.ok_or(PresentationError::Unavailable)?;
    if clock.reading_quality != AudioClockReadingQuality::Accurate {
        return Err(PresentationError::Inaccurate);
    }
    if clock.position == 0 {
        return Err(PresentationError::BeforePresentation);
    }
    if clock.frequency == 0 {
        return Err(PresentationError::FrequencyChanged);
    }
    let host = clock.host_point.ok_or(PresentationError::Unavailable)?;
    let source = if let Some(basis) = basis {
        basis
            .point_at_native_counter(clock.position, clock.frequency)
            .map_err(|_| PresentationError::Overflow)?
    } else {
        let offset = i128::from(clock.position) * 1_000_000_000 / i128::from(clock.frequency);
        let source = i128::from(output_origin.timestamp.as_nanos()) + offset;
        ClockPoint {
            domain: output_origin.domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(source).map_err(|_| PresentationError::Overflow)?,
            ),
        }
    };
    Ok((
        ClockPair {
            source,
            target: host,
        },
        clock.frequency,
        clock.position,
        clock.qpc_100ns,
    ))
}

/// Finite observed stream/output-to-host relation; no hardware readings are taken here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasapiPresentationClock {
    mapper: AffineClockMapper,
    output_origin: ClockPoint,
    host_domain: beatkernel::time::ClockDomainId,
}
impl WasapiPresentationClock {
    /// Uses two coherent running, accurate and strictly increasing observations.
    ///
    /// Validity and extrapolation use output-domain nanoseconds. Position/frequency
    /// supplies the stream-relative offset; it is never treated as sample frames.
    /// Observation uncertainty must be supplied honestly; absence of drift bounds
    /// retains Unknown even when GetPosition's status was S_OK.
    pub fn from_snapshots(
        first: AudioStreamSnapshot,
        second: AudioStreamSnapshot,
        output_origin: ClockPoint,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
        uncertainty: CalibrationUncertainty,
    ) -> Result<Self, PresentationError> {
        let (first, first_frequency, first_position, first_qpc) =
            observation(first, output_origin)?;
        let (second, second_frequency, second_position, second_qpc) =
            observation(second, output_origin)?;
        Self::from_observations(
            (first, first_frequency, first_position, first_qpc),
            (second, second_frequency, second_position, second_qpc),
            output_origin,
            validity,
            extrapolation,
            uncertainty,
        )
    }
    /// Calibrates fresh native counters against an existing physical mixer frame basis.
    pub fn from_snapshots_with_basis(
        first: AudioStreamSnapshot,
        second: AudioStreamSnapshot,
        basis: beatkernel::audio::OutputFrameBasis,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
        uncertainty: CalibrationUncertainty,
    ) -> Result<Self, PresentationError> {
        let first = observation_with_basis(first, basis)?;
        let second = observation_with_basis(second, basis)?;
        let origin = basis
            .point_at_native_counter(0, 1)
            .map_err(|_| PresentationError::Overflow)?;
        Self::from_observations(first, second, origin, validity, extrapolation, uncertainty)
    }
    fn from_observations(
        first: (ClockPair, u64, u64, u64),
        second: (ClockPair, u64, u64, u64),
        output_origin: ClockPoint,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
        uncertainty: CalibrationUncertainty,
    ) -> Result<Self, PresentationError> {
        let (first, first_frequency, first_position, first_qpc) = first;
        let (second, second_frequency, second_position, second_qpc) = second;
        if first_frequency != second_frequency {
            return Err(PresentationError::FrequencyChanged);
        }
        if second_position <= first_position
            || second_qpc <= first_qpc
            || second.target.timestamp <= first.target.timestamp
        {
            return Err(PresentationError::NonIncreasing);
        }
        let mapper =
            AffineClockMapper::from_pairs(first, second, validity, extrapolation, uncertainty)?;
        Ok(Self {
            mapper,
            output_origin,
            host_domain: first.target.domain,
        })
    }
    /// Borrows the finite mapper for explicit input/output clock normalization.
    pub const fn mapper(&self) -> &AffineClockMapper {
        &self.mapper
    }
    /// Caller-selected output frame-zero point used for this fresh client.
    pub const fn output_origin(&self) -> ClockPoint {
        self.output_origin
    }
    /// Explicit mapping quality including caller-estimated/unknown measurement error.
    pub fn quality(&self) -> ClockMappingQuality {
        use beatkernel::time::ClockMapper;
        self.mapper.quality()
    }
    /// Maps output frame zero to host time and preserves the observed inverse slope.
    ///
    /// Callers must also enforce mapper validity for each later host query. The
    /// ordinary Transport does not itself impose the calibration's expiration.
    pub fn transport(&self, applied_song_time: Timestamp) -> Result<Transport, PresentationError> {
        let origin = self
            .mapper
            .map_checked(self.output_origin, self.host_domain)?;
        let (host_numerator, host_denominator) = self.mapper.rate();
        let rate = Rate::new(
            i64::try_from(host_denominator).map_err(|_| PresentationError::Overflow)?,
            host_numerator,
        )
        .map_err(|_| PresentationError::Overflow)?;
        Ok(Transport::new(origin, applied_song_time, rate))
    }
}
