//! Finite estimated ASIO output/host relation from two actual block observations.
use super::presentation::{AsioPresentationError, AsioPresentationObservation};
use beatkernel::{
    time::{
        AffineClockMapper, CalibrationUncertainty, ClockInterval, ClockMapper, ClockMappingQuality,
        ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp,
    },
    transport::{Rate, Transport},
};

/// Immutable finite relation; supplied intervals and drift bounds are estimates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioPresentationClock {
    first: AsioPresentationObservation,
    second: AsioPresentationObservation,
    mapper: AffineClockMapper,
}
fn validate(
    observation: AsioPresentationObservation,
) -> Result<(i64, i128, u64), AsioPresentationError> {
    if observation.sample_rate == 0
        || observation.render.frames == 0
        || observation.host.before.domain != observation.host.after.domain
        || observation.host.before.timestamp > observation.host.after.timestamp
        || observation.output_origin.domain != observation.output.domain
        || observation.output.domain == observation.host.before.domain
    {
        return Err(AsioPresentationError::Malformed);
    }
    let end = observation
        .render
        .start_frame
        .checked_add(
            u64::try_from(observation.render.frames)
                .map_err(|_| AsioPresentationError::Overflow)?,
        )
        .ok_or(AsioPresentationError::Overflow)?;
    let grid = i128::from(observation.output_origin.timestamp.as_nanos())
        + i128::from(observation.render.start_frame) * 1_000_000_000
            / i128::from(observation.sample_rate);
    if grid != i128::from(observation.output.timestamp.as_nanos()) {
        return Err(AsioPresentationError::Malformed);
    }
    let before = i128::from(observation.host.before.timestamp.as_nanos());
    let width = i128::from(observation.host.after.timestamp.as_nanos()) - before;
    let midpoint =
        i64::try_from(before + width / 2).map_err(|_| AsioPresentationError::Overflow)?;
    Ok((midpoint, width / 2 + width % 2, end))
}
fn ceil_ratio(numerator: i128, denominator: i128) -> i128 {
    numerator / denominator + i128::from(numerator % denominator != 0)
}
impl AsioPresentationClock {
    /// Constructs a positive finite affine relation from same-stream block intervals.
    ///
    /// The caller must supply observations from the same actual stream; matching
    /// rate/origin metadata alone does not establish driver identity.
    /// Interval radii and less-than-one-nanosecond output-grid quantization are
    /// conservatively amplified at validity endpoints outside the observed span.
    /// One additional host nanosecond covers affine rounding. Residual drift is
    /// explicitly caller-assessed over the entire validity interval; None retains
    /// Unknown quality. Even zero supplied drift never implies Exact hardware time.
    pub fn from_observations(
        first: AsioPresentationObservation,
        second: AsioPresentationObservation,
        validity: ClockInterval,
        extrapolation: ExtrapolationPolicy,
        residual_drift_error: Option<Duration>,
    ) -> Result<Self, AsioPresentationError> {
        let (first_midpoint, first_radius, first_end) = validate(first)?;
        let (second_midpoint, second_radius, _) = validate(second)?;
        if first.sample_rate != second.sample_rate {
            return Err(AsioPresentationError::RateChanged);
        }
        if first.output_origin != second.output_origin
            || first.output.domain != second.output.domain
            || first.host.before.domain != second.host.before.domain
            || second.render.start_frame <= first.render.start_frame
            || second.render.start_frame < first_end
            || second_midpoint <= first_midpoint
        {
            return Err(AsioPresentationError::Malformed);
        }
        let source_span = i128::from(second.output.timestamp.as_nanos())
            - i128::from(first.output.timestamp.as_nanos());
        let target_span = i128::from(second_midpoint) - i128::from(first_midpoint);
        if source_span <= 0 {
            return Err(AsioPresentationError::Malformed);
        }
        let outside = (i128::from(first.output.timestamp.as_nanos())
            - i128::from(validity.start.as_nanos()))
        .max(i128::from(validity.end.as_nanos()) - i128::from(second.output.timestamp.as_nanos()))
        .max(0);
        let base = first_radius
            .max(second_radius)
            .checked_add(ceil_ratio(target_span, source_span))
            .ok_or(AsioPresentationError::Overflow)?;
        let factor_numerator = outside
            .checked_mul(2)
            .and_then(|extra| source_span.checked_add(extra))
            .ok_or(AsioPresentationError::Overflow)?;
        let error_numerator = base
            .checked_mul(factor_numerator)
            .ok_or(AsioPresentationError::Overflow)?;
        let error = ceil_ratio(error_numerator, source_span)
            .checked_add(1)
            .ok_or(AsioPresentationError::Overflow)?;
        let observation_error = Duration::from_nanos(
            i64::try_from(error).map_err(|_| AsioPresentationError::Overflow)?,
        );
        let mapper = AffineClockMapper::from_pairs(
            ClockPair {
                source: first.output,
                target: ClockPoint {
                    domain: first.host.before.domain,
                    timestamp: Timestamp::from_nanos(first_midpoint),
                },
            },
            ClockPair {
                source: second.output,
                target: ClockPoint {
                    domain: second.host.before.domain,
                    timestamp: Timestamp::from_nanos(second_midpoint),
                },
            },
            validity,
            extrapolation,
            CalibrationUncertainty {
                observation_error,
                residual_drift_error,
            },
        )?;
        Ok(Self {
            first,
            second,
            mapper,
        })
    }
    /// The actual finite output-to-host mapper, including inverse validity guards.
    pub const fn mapper(&self) -> &AffineClockMapper {
        &self.mapper
    }
    /// Estimated or Unknown quality; never an Exact physical clock assertion.
    pub fn quality(&self) -> ClockMappingQuality {
        self.mapper.quality()
    }
    /// Original output-domain frame-zero origin retained from both observations.
    pub const fn output_origin(&self) -> ClockPoint {
        self.first.output_origin
    }
    /// Original complete observations, preserving actual block and interval metadata.
    pub const fn observations(&self) -> (AsioPresentationObservation, AsioPresentationObservation) {
        (self.first, self.second)
    }
    /// Maps a host point into the actual output grid only within finite validity.
    pub fn map_host(&self, host: ClockPoint) -> Result<ClockPoint, AsioPresentationError> {
        Ok(ClockPoint {
            domain: self.first.output.domain,
            timestamp: self.mapper.map_checked(host, self.first.output.domain)?,
        })
    }
    /// Anchors song time at output frame zero using the measured inverse slope.
    /// Fails when frame zero is outside validity or the inverse Rate cannot fit.
    pub fn transport(&self, song_time: Timestamp) -> Result<Transport, AsioPresentationError> {
        let host = self
            .mapper
            .map_checked(self.first.output_origin, self.first.host.before.domain)?;
        let (target, source) = self.mapper.rate();
        let numerator = i64::try_from(source).map_err(|_| AsioPresentationError::Overflow)?;
        let rate = Rate::new(numerator, target).map_err(|_| AsioPresentationError::Overflow)?;
        Ok(Transport::new(host, song_time, rate))
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::audio::asio::multimedia_clock::MultimediaHostInterval;
    use beatkernel::{
        audio::{AudioCounters, RenderReport},
        time::ClockDomainId,
    };
    fn point(domain: u32, nanos: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(nanos),
        }
    }
    fn observation(frame: u64, before: i64, after: i64) -> AsioPresentationObservation {
        AsioPresentationObservation::from_render(
            RenderReport {
                start_frame: frame,
                frames: 1,
                playback_start_frame: frame,
                playback_frames: 1,
                paused: false,
                playback_end_physical_frame: None,
                active_voices: 0,
                pending_commands: 0,
                song_position: Timestamp::ZERO,
                producer_disconnected: false,
                counters: AudioCounters::default(),
            },
            1_000_000_000,
            MultimediaHostInterval {
                before: point(2, before),
                after: point(2, after),
            },
            0,
            0,
            point(1, 0),
        )
        .unwrap()
    }
    fn validity() -> ClockInterval {
        ClockInterval {
            start: Timestamp::ZERO,
            end: Timestamp::from_nanos(10),
        }
    }
    #[test]
    fn midpoint_relation_retains_unknown_drift_and_actual_observations() {
        let first = observation(0, 100, 104);
        let second = observation(10, 120, 124);
        let clock = AsioPresentationClock::from_observations(
            first,
            second,
            validity(),
            ExtrapolationPolicy::Forbid,
            None,
        )
        .unwrap();
        assert_eq!(clock.quality(), ClockMappingQuality::Unknown);
        assert_eq!(clock.observations(), (first, second));
        assert_eq!(clock.map_host(point(2, 112)).unwrap(), point(1, 5));
        assert_eq!(
            clock.transport(Timestamp::ZERO).unwrap().anchor().rate,
            Rate::new(1, 2).unwrap()
        );
    }
    #[test]
    fn radius_quantization_and_extrapolation_expand_error() {
        let clock = AsioPresentationClock::from_observations(
            observation(0, 100, 105),
            observation(10, 120, 125),
            ClockInterval {
                start: Timestamp::from_nanos(-10),
                end: Timestamp::from_nanos(20),
            },
            ExtrapolationPolicy::Bounded {
                before: Duration::from_nanos(10),
                after: Duration::from_nanos(10),
            },
            Some(Duration::ZERO),
        )
        .unwrap();
        assert_eq!(
            clock.mapper().uncertainty().unwrap().observation_error,
            Duration::from_nanos(16)
        );
        assert!(matches!(
            clock.quality(),
            ClockMappingQuality::Estimated { .. }
        ));
    }
    #[test]
    fn invalid_grid_rate_origin_and_overlap_reject() {
        let first = observation(0, 100, 104);
        let second = observation(10, 120, 124);
        let mut forged = second;
        forged.output.timestamp = Timestamp::from_nanos(11);
        assert!(
            AsioPresentationClock::from_observations(
                first,
                forged,
                validity(),
                ExtrapolationPolicy::Forbid,
                None
            )
            .is_err()
        );
        let mut changed = second;
        changed.sample_rate = 500_000_000;
        changed.output.timestamp = Timestamp::from_nanos(20);
        assert_eq!(
            AsioPresentationClock::from_observations(
                first,
                changed,
                validity(),
                ExtrapolationPolicy::Forbid,
                None
            ),
            Err(AsioPresentationError::RateChanged)
        );
        let mut overlapping = first;
        overlapping.render.frames = 11;
        assert!(
            AsioPresentationClock::from_observations(
                overlapping,
                second,
                validity(),
                ExtrapolationPolicy::Forbid,
                None
            )
            .is_err()
        );
    }
    #[test]
    fn origin_and_host_queries_obey_finite_validity() {
        let clock = AsioPresentationClock::from_observations(
            observation(1, 100, 100),
            observation(10, 118, 118),
            ClockInterval {
                start: Timestamp::from_nanos(1),
                end: Timestamp::from_nanos(10),
            },
            ExtrapolationPolicy::Forbid,
            Some(Duration::ZERO),
        )
        .unwrap();
        assert!(clock.transport(Timestamp::ZERO).is_err());
        assert!(clock.map_host(point(2, 119)).is_err());
        assert!(clock.map_host(point(3, 110)).is_err());
    }
}
