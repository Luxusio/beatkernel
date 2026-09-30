//! Explicit rendered-block presentation intervals, without native position inference.

use super::multimedia_clock::{MultimediaClockError, MultimediaHostInterval};
use beatkernel::{
    audio::RenderReport,
    time::{ClockPoint, Timestamp},
};

/// One actual rendered block paired with a caller-bounded host presentation interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioPresentationObservation {
    /// Output-grid point of the actual block's first rendered frame.
    pub output: ClockPoint,
    /// Inclusive host interval after explicit driver latency and error accounting.
    pub host: MultimediaHostInterval,
    /// Actual successful Mixer report identifying the prepared block.
    pub render: RenderReport,
}

/// Invalid, unavailable or unrepresentable ASIO presentation relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioPresentationError {
    /// No coherent successful native callback observation is available.
    Unavailable,
    /// Rate, block extent, domains or host interval is invalid.
    Malformed,
    /// Native rate changed or differs from the established Mixer rate.
    RateChanged,
    /// Checked frame or timestamp arithmetic cannot be represented.
    Overflow,
    /// Explicit multimedia-clock relation rejected the native reading.
    Clock(MultimediaClockError),
}

impl std::fmt::Display for AsioPresentationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("ASIO presentation observation unavailable"),
            Self::Malformed => formatter.write_str("malformed ASIO presentation observation"),
            Self::RateChanged => formatter.write_str("ASIO presentation sample rate changed"),
            Self::Overflow => formatter.write_str("ASIO presentation extent or timestamp overflow"),
            Self::Clock(error) => write!(formatter, "ASIO presentation clock: {error}"),
        }
    }
}
impl std::error::Error for AsioPresentationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Clock(error) => Some(error),
            _ => None,
        }
    }
}
impl From<MultimediaClockError> for AsioPresentationError {
    fn from(error: MultimediaClockError) -> Self {
        Self::Clock(error)
    }
}

impl AsioPresentationObservation {
    /// Maps an actual rendered frame using an explicitly established switch interval.
    ///
    /// Output time floors the rational frame time, losing less than one nanosecond.
    /// Host latency floors the lower endpoint and ceils the upper endpoint, then
    /// expands both by the caller's assessed latency error. These supplied bounds
    /// do not establish physical precision. Native sample position is not used.
    pub fn from_render(
        render: RenderReport,
        sample_rate: u32,
        switch_host: MultimediaHostInterval,
        output_latency_frames: u32,
        latency_error_ns: u64,
        output_origin: ClockPoint,
    ) -> Result<Self, AsioPresentationError> {
        if sample_rate == 0
            || render.frames == 0
            || switch_host.before.domain != switch_host.after.domain
            || switch_host.before.timestamp > switch_host.after.timestamp
            || output_origin.domain == switch_host.before.domain
        {
            return Err(AsioPresentationError::Malformed);
        }
        render
            .start_frame
            .checked_add(u64::try_from(render.frames).map_err(|_| AsioPresentationError::Overflow)?)
            .ok_or(AsioPresentationError::Overflow)?;
        let rate = i128::from(sample_rate);
        let frame_ns = i128::from(render.start_frame) * 1_000_000_000 / rate;
        let output_ns = i128::from(output_origin.timestamp.as_nanos()) + frame_ns;
        let latency_numerator = i128::from(output_latency_frames) * 1_000_000_000;
        let latency_floor = latency_numerator / rate;
        let latency_ceil = latency_floor + i128::from(latency_numerator % rate != 0);
        let before_ns = i128::from(switch_host.before.timestamp.as_nanos()) + latency_floor
            - i128::from(latency_error_ns);
        let after_ns = i128::from(switch_host.after.timestamp.as_nanos())
            + latency_ceil
            + i128::from(latency_error_ns);
        let narrow = |value| {
            i64::try_from(value)
                .map(Timestamp::from_nanos)
                .map_err(|_| AsioPresentationError::Overflow)
        };
        Ok(Self {
            output: ClockPoint {
                domain: output_origin.domain,
                timestamp: narrow(output_ns)?,
            },
            host: MultimediaHostInterval {
                before: ClockPoint {
                    domain: switch_host.before.domain,
                    timestamp: narrow(before_ns)?,
                },
                after: ClockPoint {
                    domain: switch_host.after.domain,
                    timestamp: narrow(after_ns)?,
                },
            },
            render,
        })
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{audio::AudioCounters, time::ClockDomainId};
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn report(start_frame: u64) -> RenderReport {
        RenderReport {
            start_frame,
            frames: 2,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        }
    }
    fn interval() -> MultimediaHostInterval {
        MultimediaHostInterval {
            before: point(1, 100),
            after: point(1, 110),
        }
    }
    #[test]
    fn actual_render_frame_defines_output_identity() {
        let render = report(12);
        let observation =
            AsioPresentationObservation::from_render(render, 4, interval(), 0, 0, point(2, -5))
                .unwrap();
        assert_eq!(observation.output.timestamp.as_nanos(), 2_999_999_995);
        assert_eq!(observation.host, interval());
        assert_eq!(observation.render, render);
    }
    #[test]
    fn nonintegral_latency_expands_outward() {
        let observation =
            AsioPresentationObservation::from_render(report(1), 3, interval(), 1, 7, point(2, 0))
                .unwrap();
        assert_eq!(observation.output.timestamp.as_nanos(), 333_333_333);
        assert_eq!(observation.host.before.timestamp.as_nanos(), 333_333_426);
        assert_eq!(observation.host.after.timestamp.as_nanos(), 333_333_451);
    }
    #[test]
    fn overflow_rejects_frame_and_timestamp_extents() {
        assert_eq!(
            AsioPresentationObservation::from_render(
                report(u64::MAX),
                1,
                interval(),
                0,
                0,
                point(2, 0)
            ),
            Err(AsioPresentationError::Overflow)
        );
        assert_eq!(
            AsioPresentationObservation::from_render(
                report(1),
                1,
                interval(),
                0,
                0,
                point(2, i64::MAX)
            ),
            Err(AsioPresentationError::Overflow)
        );
        assert_eq!(
            AsioPresentationObservation::from_render(
                report(0),
                1,
                interval(),
                0,
                u64::MAX,
                point(2, 0)
            ),
            Err(AsioPresentationError::Overflow)
        );
    }
    #[test]
    fn malformed_rate_domains_interval_and_empty_block_reject() {
        assert_eq!(
            AsioPresentationObservation::from_render(report(0), 0, interval(), 0, 0, point(2, 0)),
            Err(AsioPresentationError::Malformed)
        );
        for invalid in [
            MultimediaHostInterval {
                before: point(1, 111),
                after: point(1, 110),
            },
            MultimediaHostInterval {
                before: point(1, 100),
                after: point(3, 110),
            },
        ] {
            assert_eq!(
                AsioPresentationObservation::from_render(report(0), 1, invalid, 0, 0, point(2, 0)),
                Err(AsioPresentationError::Malformed)
            );
        }
        assert_eq!(
            AsioPresentationObservation::from_render(report(0), 1, interval(), 0, 0, point(1, 0)),
            Err(AsioPresentationError::Malformed)
        );
        let mut empty = report(0);
        empty.frames = 0;
        assert_eq!(
            AsioPresentationObservation::from_render(empty, 1, interval(), 0, 0, point(2, 0)),
            Err(AsioPresentationError::Malformed)
        );
    }
}
