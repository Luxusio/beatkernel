//! Checked observed or nominal projection of a session start into a native physical frame.
//! ClockPair supplies no drift/error bounds; this is not a physical timing proof.
use beatkernel::time::{ClockPair, ClockPoint, Timestamp};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartProjectionError {
    Chronology,
    StaleAnchor,
    Domains,
    InvalidRate,
    Overflow,
    TooClose,
    Slope,
}
impl fmt::Display for StartProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StartProjectionError {}

/// A native host observation bracketed by reads of the session monotonic clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionHostBracket {
    before: i64,
    after: i64,
    host: ClockPoint,
}
impl SessionHostBracket {
    pub fn new(before: i64, host: ClockPoint, after: i64) -> Result<Self, StartProjectionError> {
        if before < 0 || after < before {
            return Err(StartProjectionError::Chronology);
        }
        Ok(Self {
            before,
            after,
            host,
        })
    }
    /// Preserves the capture bracket under an explicitly nominal unit clock slope.
    pub fn deadline_at(
        self,
        target: i64,
        now: i64,
        max_age_ns: u64,
    ) -> Result<HostStartWindow, StartProjectionError> {
        self.deadline_window_at(target, target, now, max_age_ns)
    }
    /// Expands a committed midpoint by a conservative integer half-width.
    pub fn deadline_for_schedule(
        self,
        schedule: crate::multiplayer_start::StartSchedule,
        now: i64,
        max_age_ns: u64,
    ) -> Result<HostStartWindow, StartProjectionError> {
        let radius = (i128::from(schedule.uncertainty_ns) + 1) / 2;
        let earliest = i64::try_from(i128::from(schedule.target_ns) - radius)
            .map_err(|_| StartProjectionError::Overflow)?;
        let latest = i64::try_from(i128::from(schedule.target_ns) + radius)
            .map_err(|_| StartProjectionError::Overflow)?;
        self.deadline_window_at(earliest, latest, now, max_age_ns)
    }
    pub fn deadline_window_at(
        self,
        earliest: i64,
        latest: i64,
        now: i64,
        max_age_ns: u64,
    ) -> Result<HostStartWindow, StartProjectionError> {
        if earliest > latest {
            return Err(StartProjectionError::Chronology);
        }
        if now < self.after {
            return Err(StartProjectionError::Chronology);
        }
        if i128::from(now) - i128::from(self.after) > i128::from(max_age_ns) {
            return Err(StartProjectionError::StaleAnchor);
        }
        if earliest <= now {
            return Err(StartProjectionError::TooClose);
        }
        let point = |target: i64, observed: i64| -> Result<ClockPoint, StartProjectionError> {
            let ns = i128::from(self.host.timestamp.as_nanos()) + i128::from(target)
                - i128::from(observed);
            Ok(ClockPoint {
                domain: self.host.domain,
                timestamp: Timestamp::from_nanos(
                    i64::try_from(ns).map_err(|_| StartProjectionError::Overflow)?,
                ),
            })
        };
        Ok(HostStartWindow {
            earliest: point(earliest, self.after)?,
            latest: point(latest, self.before)?,
        })
    }
}

/// Retained native-host deadline endpoints; neither endpoint is treated as exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostStartWindow {
    earliest: ClockPoint,
    latest: ClockPoint,
}
impl HostStartWindow {
    pub fn earliest(self) -> ClockPoint {
        self.earliest
    }
    pub fn latest(self) -> ClockPoint {
        self.latest
    }
}

/// Checked physical-frame interval with an explicit render-ahead margin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputStartPlan {
    earliest_frame: u64,
    latest_frame: u64,
    selected_output: ClockPoint,
}
impl OutputStartPlan {
    /// Uses a nominal unit-slope pair; selects the latest ceil-quantized endpoint.
    /// Caller must separately establish native clock quality, age and drift limits.
    pub fn from_pair(
        window: HostStartWindow,
        pair: ClockPair,
        origin: ClockPoint,
        sample_rate: u32,
        rendered_end: u64,
        minimum_ahead_frames: u64,
    ) -> Result<Self, StartProjectionError> {
        Self::project(
            window,
            pair,
            origin,
            sample_rate,
            rendered_end,
            minimum_ahead_frames,
            1,
            1,
        )
    }
    /// Projects through an observed positive slope, rejecting caller-bounded ppm.
    /// Two native observations still do not establish physical measurement error.
    pub fn from_pairs(
        window: HostStartWindow,
        first: ClockPair,
        second: ClockPair,
        origin: ClockPoint,
        sample_rate: u32,
        rendered_end: u64,
        minimum_ahead_frames: u64,
        maximum_rate_error_ppm: u32,
    ) -> Result<Self, StartProjectionError> {
        if first.source.domain != second.source.domain
            || first.target.domain != second.target.domain
        {
            return Err(StartProjectionError::Domains);
        }
        let source_span = i128::from(second.source.timestamp.as_nanos())
            - i128::from(first.source.timestamp.as_nanos());
        let host_span = i128::from(second.target.timestamp.as_nanos())
            - i128::from(first.target.timestamp.as_nanos());
        if source_span <= 0 || host_span <= 0 {
            return Err(StartProjectionError::Chronology);
        }
        if maximum_rate_error_ppm >= 1_000_000
            || (source_span - host_span).abs() * 1_000_000
                > host_span * i128::from(maximum_rate_error_ppm)
        {
            return Err(StartProjectionError::Slope);
        }
        let mut a = source_span;
        let mut b = host_span;
        while b != 0 {
            let next = a % b;
            a = b;
            b = next;
        }
        Self::project(
            window,
            second,
            origin,
            sample_rate,
            rendered_end,
            minimum_ahead_frames,
            source_span / a,
            host_span / a,
        )
    }
    fn project(
        window: HostStartWindow,
        pair: ClockPair,
        origin: ClockPoint,
        sample_rate: u32,
        rendered_end: u64,
        minimum_ahead_frames: u64,
        slope_numerator: i128,
        slope_denominator: i128,
    ) -> Result<Self, StartProjectionError> {
        if pair.source.domain != origin.domain || pair.target.domain != window.earliest.domain {
            return Err(StartProjectionError::Domains);
        }
        if sample_rate == 0 {
            return Err(StartProjectionError::InvalidRate);
        }
        let frame_at = |host: ClockPoint| -> Result<u64, StartProjectionError> {
            let base = i128::from(pair.source.timestamp.as_nanos())
                - i128::from(origin.timestamp.as_nanos());
            let host_delta = i128::from(host.timestamp.as_nanos())
                - i128::from(pair.target.timestamp.as_nanos());
            let scaled = base
                .checked_mul(slope_denominator)
                .and_then(|value| {
                    host_delta
                        .checked_mul(slope_numerator)
                        .and_then(|offset| value.checked_add(offset))
                })
                .ok_or(StartProjectionError::Overflow)?;
            if scaled < 0 {
                return Err(StartProjectionError::TooClose);
            }
            let numerator = scaled
                .checked_mul(i128::from(sample_rate))
                .ok_or(StartProjectionError::Overflow)?;
            let denominator = slope_denominator
                .checked_mul(1_000_000_000)
                .ok_or(StartProjectionError::Overflow)?;
            let frame = numerator / denominator + i128::from(numerator % denominator != 0);
            u64::try_from(frame).map_err(|_| StartProjectionError::Overflow)
        };
        let earliest_frame = frame_at(window.earliest)?;
        let latest_frame = frame_at(window.latest)?;
        let minimum = rendered_end
            .checked_add(minimum_ahead_frames)
            .ok_or(StartProjectionError::Overflow)?;
        if earliest_frame < minimum {
            return Err(StartProjectionError::TooClose);
        }
        let ns = i128::from(origin.timestamp.as_nanos())
            + i128::from(latest_frame) * 1_000_000_000 / i128::from(sample_rate);
        let selected_output = ClockPoint {
            domain: origin.domain,
            timestamp: Timestamp::from_nanos(
                i64::try_from(ns).map_err(|_| StartProjectionError::Overflow)?,
            ),
        };
        Ok(Self {
            earliest_frame,
            latest_frame,
            selected_output,
        })
    }
    pub fn earliest_frame(self) -> u64 {
        self.earliest_frame
    }
    pub fn latest_frame(self) -> u64 {
        self.latest_frame
    }
    pub fn selected_frame(self) -> u64 {
        self.latest_frame
    }
    pub fn selected_output(self) -> ClockPoint {
        self.selected_output
    }
}

/// Interpolates only after actual native observations bracket the applied output.
/// The returned host coordinate retains Unknown physical measurement accuracy.
pub fn presented_output(
    output: ClockPoint,
    lower: ClockPair,
    upper: ClockPair,
) -> Result<ClockPoint, StartProjectionError> {
    if output.domain != lower.source.domain
        || output.domain != upper.source.domain
        || lower.target.domain != upper.target.domain
    {
        return Err(StartProjectionError::Domains);
    }
    let source_span = i128::from(upper.source.timestamp.as_nanos())
        - i128::from(lower.source.timestamp.as_nanos());
    let host_span = i128::from(upper.target.timestamp.as_nanos())
        - i128::from(lower.target.timestamp.as_nanos());
    let offset =
        i128::from(output.timestamp.as_nanos()) - i128::from(lower.source.timestamp.as_nanos());
    if source_span <= 0 || host_span <= 0 || offset < 0 || offset > source_span {
        return Err(StartProjectionError::Chronology);
    }
    let host = offset
        .checked_mul(host_span)
        .map(|value| value / source_span)
        .and_then(|value| value.checked_add(i128::from(lower.target.timestamp.as_nanos())))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(StartProjectionError::Overflow)?;
    Ok(ClockPoint {
        domain: lower.target.domain,
        timestamp: Timestamp::from_nanos(host),
    })
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::time::ClockDomainId;
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    #[test]
    fn projected_frame_arms_real_mixer_after_silent_calibration_blocks() {
        use beatkernel::audio::*;
        let format = AudioFormat::new(1_000, 1).unwrap();
        let limits = AudioLimits::new(4, 1, 4, 16, 4).unwrap();
        let pcm_limits = PcmLimits::new(16, 64, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue_with_start_gate(4).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(2), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        let mut calibration = [1.0; 2];
        let report = mixer.render(&mut calibration).unwrap();
        assert_eq!(calibration, [0.0, 0.0]);
        assert_eq!(report.playback_frames, 0);
        assert_eq!(producer.applied_start_frame(), None);
        let window = SessionHostBracket::new(0, point(1, 0), 0)
            .unwrap()
            .deadline_at(3_000_000, 0, 0)
            .unwrap();
        let plan = OutputStartPlan::from_pair(
            window,
            ClockPair {
                source: point(2, 0),
                target: point(1, 0),
            },
            point(2, 0),
            1_000,
            2,
            1,
        )
        .unwrap();
        producer.schedule_start_at(plan.selected_frame()).unwrap();
        let mut playback = [1.0; 4];
        let report = mixer.render(&mut playback).unwrap();
        assert_eq!(playback, [0.0, 0.25, 0.5, 0.0]);
        assert_eq!(
            (
                report.start_frame,
                report.playback_start_frame,
                report.playback_frames
            ),
            (2, 0, 3)
        );
        assert_eq!(producer.applied_start_frame(), Some(3));
        assert_eq!(plan.selected_output(), point(2, 3_000_000));
    }
    #[test]
    fn measured_slope_and_actual_crossing_have_literal_coordinates() {
        let window = SessionHostBracket::new(0, point(1, 0), 0)
            .unwrap()
            .deadline_at(2_000_000_000, 0, 0)
            .unwrap();
        let first = ClockPair {
            source: point(2, 0),
            target: point(1, 0),
        };
        let second = ClockPair {
            source: point(2, 1_000_500_000),
            target: point(1, 1_000_000_000),
        };
        let plan = OutputStartPlan::from_pairs(
            window,
            first,
            second,
            point(2, 0),
            48_000,
            48_000,
            256,
            1000,
        )
        .unwrap();
        assert_eq!(plan.selected_frame(), 96_048);
        assert_eq!(plan.selected_output(), point(2, 2_001_000_000));
        assert_eq!(
            OutputStartPlan::from_pairs(window, first, second, point(2, 0), 48_000, 0, 0, 0),
            Err(StartProjectionError::Slope)
        );
        assert_eq!(
            OutputStartPlan::from_pairs(window, second, first, point(2, 0), 48_000, 0, 0, 1000),
            Err(StartProjectionError::Chronology)
        );
        let lower = ClockPair {
            source: point(2, 2_000_000_000),
            target: point(1, 1_999_000_000),
        };
        let upper = ClockPair {
            source: point(2, 2_002_000_000),
            target: point(1, 2_001_000_000),
        };
        assert_eq!(
            presented_output(plan.selected_output(), lower, upper).unwrap(),
            point(1, 2_000_000_000)
        );
        assert_eq!(
            presented_output(point(3, 2_001_000_000), lower, upper),
            Err(StartProjectionError::Domains)
        );
        assert_eq!(
            presented_output(point(2, 2_003_000_000), lower, upper),
            Err(StartProjectionError::Chronology)
        );
        assert_eq!(
            presented_output(plan.selected_output(), upper, lower),
            Err(StartProjectionError::Chronology)
        );
    }
    #[test]
    fn bracket_interval_and_conservative_physical_frame_are_literal() {
        let bridge = SessionHostBracket::new(1_000_000, point(1, 5_000_000), 2_000_000).unwrap();
        let window = bridge.deadline_at(10_000_000, 2_000_000, 0).unwrap();
        assert_eq!(window.earliest(), point(1, 13_000_000));
        assert_eq!(window.latest(), point(1, 14_000_000));
        let schedule = crate::multiplayer_start::StartSchedule {
            target_ns: 10_000_000,
            song_target_ns: 12_000_000,
            uncertainty_ns: 3,
        };
        let uncertain = bridge
            .deadline_for_schedule(schedule, 2_000_000, 0)
            .unwrap();
        assert_eq!(uncertain.earliest(), point(1, 12_999_998));
        assert_eq!(uncertain.latest(), point(1, 14_000_002));
        let pair = ClockPair {
            source: point(2, 2_000_000),
            target: point(1, 5_000_000),
        };
        let plan = OutputStartPlan::from_pair(window, pair, point(2, 0), 1_000, 3, 2).unwrap();
        assert_eq!(
            (
                plan.earliest_frame(),
                plan.latest_frame(),
                plan.selected_frame()
            ),
            (10, 11, 11)
        );
        assert_eq!(plan.selected_output(), point(2, 11_000_000));
        assert_eq!(
            OutputStartPlan::from_pair(window, pair, point(2, 0), 1_000, 10, 1),
            Err(StartProjectionError::TooClose)
        );
        assert_eq!(
            OutputStartPlan::from_pair(window, pair, point(3, 0), 1_000, 0, 0),
            Err(StartProjectionError::Domains)
        );
        assert_eq!(
            OutputStartPlan::from_pair(window, pair, point(2, 0), 0, 0, 0),
            Err(StartProjectionError::InvalidRate)
        );
    }
    #[test]
    fn chronology_age_range_and_quantization_are_checked() {
        assert_eq!(
            SessionHostBracket::new(-1, point(1, 0), 0),
            Err(StartProjectionError::Chronology)
        );
        assert_eq!(
            SessionHostBracket::new(2, point(1, 0), 1),
            Err(StartProjectionError::Chronology)
        );
        let bridge = SessionHostBracket::new(0, point(1, 0), 1).unwrap();
        assert_eq!(
            bridge.deadline_at(100, 0, 0),
            Err(StartProjectionError::Chronology)
        );
        assert_eq!(
            bridge.deadline_at(100, 2, 0),
            Err(StartProjectionError::StaleAnchor)
        );
        assert_eq!(
            bridge.deadline_at(1, 1, 0),
            Err(StartProjectionError::TooClose)
        );
        let window = bridge.deadline_at(2, 1, 0).unwrap();
        let pair = ClockPair {
            source: point(2, 0),
            target: point(1, 0),
        };
        let plan = OutputStartPlan::from_pair(window, pair, point(2, 0), 48_000, 0, 1).unwrap();
        assert_eq!((plan.earliest_frame(), plan.latest_frame()), (1, 1));
        assert_eq!(plan.selected_output(), point(2, 20_833));
        assert_eq!(
            OutputStartPlan::from_pair(window, pair, point(2, 0), 48_000, u64::MAX, 1),
            Err(StartProjectionError::Overflow)
        );
        assert_eq!(
            SessionHostBracket::new(0, point(1, i64::MAX), 0)
                .unwrap()
                .deadline_at(1, 0, 0),
            Err(StartProjectionError::Overflow)
        );
        for span in [
            20 * 60 * 60 * 1_000_000_000i64,
            7 * 24 * 60 * 60 * 1_000_000_000,
        ] {
            let bridge = SessionHostBracket::new(span, point(1, span), span).unwrap();
            let window = bridge.deadline_at(span + 1_000_000, span, 0).unwrap();
            let plan = OutputStartPlan::from_pair(window, pair, point(2, 0), 1_000, 0, 0).unwrap();
            assert_eq!(plan.selected_frame(), span as u64 / 1_000_000 + 1);
        }
    }
}
