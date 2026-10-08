//! Checked observed or nominal projection of a session start into a native physical frame.
//! ClockPair supplies no drift/error bounds; this is not a physical timing proof.
use beatkernel::time::{ClockPair, ClockPoint, Timestamp};
use std::fmt;

pub mod interval;
pub use interval::StartInterval;

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
    /// Preserve an already assessed inclusive host interval without selecting a point.
    pub fn new(earliest: ClockPoint, latest: ClockPoint) -> Result<Self, StartProjectionError> {
        if earliest.domain != latest.domain {
            return Err(StartProjectionError::Domains);
        }
        if earliest.timestamp > latest.timestamp {
            return Err(StartProjectionError::Chronology);
        }
        Ok(Self { earliest, latest })
    }
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

/// Startup runs on the game owner; native adapters retain acquisition and cleanup.
pub type NativeStartResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Shared acquisition bound; adapters retain original native input values.
pub const MAX_START_INPUT_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub enum NativeStartTiming {
    Point(ClockPair),
    Interval(StartInterval),
}
impl NativeStartTiming {
    /// Point-only adapters must not discard an interval to obtain a pair.
    pub fn point(self) -> Result<ClockPair, StartProjectionError> {
        match self {
            Self::Point(pair) => Ok(pair),
            Self::Interval(_) => Err(StartProjectionError::Domains),
        }
    }
    fn bounds(self) -> StartInterval {
        match self {
            Self::Point(pair) => StartInterval {
                output: pair.source,
                before: pair.target,
                after: pair.target,
            },
            Self::Interval(interval) => interval,
        }
    }
}
impl From<ClockPair> for NativeStartTiming {
    fn from(pair: ClockPair) -> Self {
        Self::Point(pair)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeStartObservation<E> {
    pub timing: NativeStartTiming,
    /// Unmodified native evidence belonging to these exact observations/bounds.
    pub evidence: E,
}

pub trait NativeStartDevice {
    type Evidence: Copy;
    fn start(&mut self) -> NativeStartResult<()>;
    /// Service acquisition/cancellation; retain original selected input after arming.
    /// Native collector compositions keep the same input buffer/owner through gameplay.
    fn service_input(&mut self, retain: bool) -> NativeStartResult<bool>;
    fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<Self::Evidence>>>;
    fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>>;
    fn buffer_frames(&self) -> NativeStartResult<u32>;
    fn host_now(&self) -> NativeStartResult<ClockPoint>;
    /// Seed the cloned finite observer before arming. Interval adapters retain
    /// their native report/grid evidence through their own typed observation API.
    fn seed_end(
        &mut self,
        end: &mut crate::native_end::NativeEnd,
        observation: &NativeStartObservation<Self::Evidence>,
    ) -> NativeStartResult<()> {
        end.observe(None, observation.timing.point()?)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeStartConfig {
    pub output_origin: ClockPoint,
    pub sample_rate: u32,
    pub playback_end_frame: Option<u64>,
    pub setup_timeout: std::time::Duration,
    pub max_clock_age_ns: u64,
    pub max_rate_error_ppm: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeStarted<E, P = OutputStartPlan> {
    pub plan: P,
    pub observation: NativeStartObservation<E>,
    /// Retained arrival uncertainty; readiness waits for its upper endpoint.
    pub host_window: HostStartWindow,
    /// Nominal transport anchor only. Interval evidence remains in host_window.
    pub host_origin: ClockPoint,
}

/// The real protocol owner and transport-free fixtures use the same startup flow.
pub trait NativeStartAgreement {
    fn await_commit(
        &mut self,
        service: &mut dyn FnMut() -> NativeStartResult<bool>,
    ) -> NativeStartResult<bool>;
    fn committed_schedule(&self) -> NativeStartResult<crate::multiplayer_start::StartSchedule>;
    fn host_bracket(
        &self,
        sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
    ) -> NativeStartResult<SessionHostBracket>;
    fn clock_now_ns(&self) -> NativeStartResult<i64>;
}

impl NativeStartAgreement for crate::competition_live::LiveCompetition {
    fn await_commit(
        &mut self,
        service: &mut dyn FnMut() -> NativeStartResult<bool>,
    ) -> NativeStartResult<bool> {
        self.await_network_commit(service)
    }
    fn committed_schedule(&self) -> NativeStartResult<crate::multiplayer_start::StartSchedule> {
        self.committed_start_schedule()
            .ok_or_else(|| "committed start missing".into())
    }
    fn host_bracket(
        &self,
        sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
    ) -> NativeStartResult<SessionHostBracket> {
        self.native_host_bracket(sample)?
            .ok_or_else(|| "session clock missing".into())
    }
    fn clock_now_ns(&self) -> NativeStartResult<i64> {
        self.network_clock_now_ns()?
            .ok_or_else(|| "session clock missing".into())
    }
}

fn startup_observation<D: NativeStartDevice>(
    device: &mut D,
    origin: ClockPoint,
    previous: &mut Option<NativeStartTiming>,
) -> NativeStartResult<Option<NativeStartObservation<D::Evidence>>> {
    let Some(observation) = device.observe()? else {
        return Ok(None);
    };
    let bounds = observation.timing.bounds();
    bounds.validate()?;
    if bounds.output.domain != origin.domain {
        return Err(StartProjectionError::Domains.into());
    }
    if bounds.output.timestamp < origin.timestamp {
        return Err(StartProjectionError::Chronology.into());
    }
    if let Some(last) = previous {
        if std::mem::discriminant(last) != std::mem::discriminant(&observation.timing) {
            return Err("native startup timing source changed".into());
        }
        let last = last.bounds();
        if bounds.before.domain != last.before.domain {
            return Err(StartProjectionError::Domains.into());
        }
        if bounds.output.timestamp < last.output.timestamp
            || bounds.before.timestamp < last.before.timestamp
            || bounds.after.timestamp < last.after.timestamp
        {
            return Err(StartProjectionError::Chronology.into());
        }
    }
    *previous = Some(observation.timing);
    Ok(Some(observation))
}

/// Bounded time for two separated observations on the actual callback frame grid.
pub(crate) fn native_calibration_timeout(
    buffer_frames: u32,
    sample_rate: u32,
) -> Result<std::time::Duration, Box<dyn std::error::Error>> {
    if buffer_frames == 0 || sample_rate == 0 {
        return Err("native calibration requires a nonzero frame grid".into());
    }
    let numerator = u128::from(buffer_frames) * 1_000_000_000;
    let quantum = numerator.div_ceil(u128::from(sample_rate));
    let budget = quantum
        .checked_mul(4)
        .and_then(|v| v.checked_add(100_000_000))
        .ok_or("native calibration timeout overflow")?
        .max(2_000_000_000);
    Ok(std::time::Duration::from_nanos(
        u64::try_from(budget).map_err(|_| "native calibration timeout exceeds nanoseconds")?,
    ))
}

/// Converted committed startup keeps immutable creation identity and one coherent auxiliary tuple.
pub trait NativeTargetStartDevice: NativeStartDevice {
    fn target_identity(&self) -> NativeStartResult<(u64, beatkernel::audio::TargetFrameBasis)>;
    fn target_output_telemetry(
        &mut self,
    ) -> NativeStartResult<Option<crate::gameplay::output::ports::TargetOutputTelemetry>>;
}
pub type TargetNativeStarted<E> = NativeStarted<E, TargetOutputStartPlan>;

trait CommittedStartPolicy<D: NativeStartDevice> {
    type Plan: Copy;
    fn calibration_rate(&self, config: NativeStartConfig) -> u32;
    fn project(
        &self,
        device: &mut D,
        window: HostStartWindow,
        first: NativeStartTiming,
        latest: NativeStartTiming,
        buffer: u32,
        config: NativeStartConfig,
    ) -> NativeStartResult<Option<Self::Plan>>;
    fn source_frame(&self, plan: Self::Plan) -> u64;
    fn output(&self, plan: Self::Plan) -> NativeStartResult<ClockPoint>;
    fn stage(
        &self,
        device: &mut D,
        plan: Self::Plan,
        latest: &NativeStartObservation<D::Evidence>,
        pause: &crate::playback_pause::NativePause,
        end: &Option<crate::native_end::NativeEnd>,
    ) -> NativeStartResult<(
        crate::playback_pause::NativePause,
        Option<crate::native_end::NativeEnd>,
    )>;
    fn confirmed(
        &self,
        device: &mut D,
        plan: Self::Plan,
        report: Option<beatkernel::audio::RenderReport>,
        applied: Option<u64>,
        config: NativeStartConfig,
    ) -> NativeStartResult<bool>;
}
struct SourceStartPolicy;
impl<D: NativeStartDevice> CommittedStartPolicy<D> for SourceStartPolicy {
    type Plan = OutputStartPlan;
    fn calibration_rate(&self, config: NativeStartConfig) -> u32 {
        config.sample_rate
    }
    fn project(
        &self,
        device: &mut D,
        window: HostStartWindow,
        first: NativeStartTiming,
        latest: NativeStartTiming,
        buffer: u32,
        config: NativeStartConfig,
    ) -> NativeStartResult<Option<Self::Plan>> {
        let report = device
            .render_report()?
            .ok_or("calibration render frontier missing")?;
        let frames = u64::try_from(report.frames).map_err(|_| StartProjectionError::Overflow)?;
        let rendered_end = report
            .start_frame
            .checked_add(frames)
            .ok_or(StartProjectionError::Overflow)?;
        let plan = match (first, latest) {
            (NativeStartTiming::Point(first), NativeStartTiming::Point(latest)) => {
                OutputStartPlan::from_pairs(
                    window,
                    first,
                    latest,
                    config.output_origin,
                    config.sample_rate,
                    rendered_end,
                    u64::from(buffer),
                    config.max_rate_error_ppm,
                )?
            }
            (NativeStartTiming::Interval(first), NativeStartTiming::Interval(latest)) => {
                interval::project(
                    window,
                    first,
                    latest,
                    config.output_origin,
                    config.sample_rate,
                    rendered_end,
                    u64::from(buffer),
                    config.max_rate_error_ppm,
                )?
            }
            _ => return Err("native startup timing source changed".into()),
        };
        Ok(Some(plan))
    }
    fn source_frame(&self, plan: Self::Plan) -> u64 {
        plan.selected_frame()
    }
    fn output(&self, plan: Self::Plan) -> NativeStartResult<ClockPoint> {
        Ok(plan.selected_output())
    }
    fn stage(
        &self,
        device: &mut D,
        plan: Self::Plan,
        latest: &NativeStartObservation<D::Evidence>,
        pause: &crate::playback_pause::NativePause,
        end: &Option<crate::native_end::NativeEnd>,
    ) -> NativeStartResult<(
        crate::playback_pause::NativePause,
        Option<crate::native_end::NativeEnd>,
    )> {
        let pause = pause.clone().with_start_frame(plan.selected_frame())?;
        let end = end
            .clone()
            .map(|observer| -> NativeStartResult<_> {
                let mut observer = observer.with_start_frame(plan.selected_frame())?;
                device.seed_end(&mut observer, latest)?;
                Ok(observer)
            })
            .transpose()?;
        Ok((pause, end))
    }
    fn confirmed(
        &self,
        _: &mut D,
        plan: Self::Plan,
        report: Option<beatkernel::audio::RenderReport>,
        applied: Option<u64>,
        config: NativeStartConfig,
    ) -> NativeStartResult<bool> {
        if applied.is_some_and(|frame| frame != plan.selected_frame()) {
            return Err("native applied frame differs from committed frame".into());
        }
        Ok(applied.is_some()
            || (config.playback_end_frame == Some(0)
                && report.is_some_and(|report| {
                    report.playback_end_physical_frame == Some(plan.selected_frame())
                })))
    }
}
struct TargetStartPolicy {
    epoch: u64,
    basis: beatkernel::audio::TargetFrameBasis,
}
impl TargetStartPolicy {
    fn telemetry<D: NativeTargetStartDevice>(
        &self,
        device: &mut D,
        config: NativeStartConfig,
    ) -> NativeStartResult<Option<crate::gameplay::output::ports::TargetOutputTelemetry>> {
        if device.target_identity()? != (self.epoch, self.basis)
            || self.basis.origin() != config.output_origin
        {
            return Err("target startup creation identity changed".into());
        }
        let Some(tuple) = device.target_output_telemetry()? else {
            return Ok(None);
        };
        if tuple.facts.origin != Some(self.basis.origin())
            || tuple.facts.source_rate != config.sample_rate
        {
            return Err("target startup source identity differs".into());
        }
        if let Some(report) = tuple.converted {
            if report.source_rate != config.sample_rate
                || report.target_rate != self.basis.sample_rate()
                || report.state != beatkernel::audio::ConvertedOutputState::Active
                || report.target_end_time.seconds() < self.basis.start_time().seconds()
                || (report.target_end_time.seconds() == self.basis.start_time().seconds()
                    && u128::from(report.target_end_time.numerator())
                        * u128::from(self.basis.start_time().denominator())
                        < u128::from(self.basis.start_time().numerator())
                            * u128::from(report.target_end_time.denominator()))
                || report
                    .target_start_time
                    .checked_add_frames(report.target_frames as u64, report.target_rate)?
                    != report.target_end_time
            {
                return Err("target startup conversion interpretation differs".into());
            }
        }
        Ok(Some(tuple))
    }
}
impl<D: NativeTargetStartDevice> CommittedStartPolicy<D> for TargetStartPolicy {
    type Plan = TargetOutputStartPlan;
    fn calibration_rate(&self, _: NativeStartConfig) -> u32 {
        self.basis.sample_rate()
    }
    fn project(
        &self,
        device: &mut D,
        window: HostStartWindow,
        first: NativeStartTiming,
        latest: NativeStartTiming,
        buffer: u32,
        config: NativeStartConfig,
    ) -> NativeStartResult<Option<Self::Plan>> {
        let first = first.point()?;
        let latest = latest.point()?;
        let Some(tuple) = self.telemetry(device, config)? else {
            return Ok(None);
        };
        let Some(report) = tuple.converted else {
            return Ok(None);
        };
        let snapshot = TargetStartSnapshot {
            basis: self.basis,
            generated_time: report.target_end_time,
            source_position: report.source_position,
            pulled_source_frame: report.pulled_source_frame_cursor,
            source_rate: report.source_rate,
        };
        Ok(Some(TargetOutputStartPlan::from_snapshot(
            window,
            first,
            latest,
            snapshot,
            u64::from(buffer),
            config.max_rate_error_ppm,
        )?))
    }
    fn source_frame(&self, plan: Self::Plan) -> u64 {
        plan.selected_source_frame()
    }
    fn output(&self, plan: Self::Plan) -> NativeStartResult<ClockPoint> {
        Ok(plan.selected_output()?)
    }
    fn stage(
        &self,
        device: &mut D,
        plan: Self::Plan,
        latest: &NativeStartObservation<D::Evidence>,
        pause: &crate::playback_pause::NativePause,
        end: &Option<crate::native_end::NativeEnd>,
    ) -> NativeStartResult<(
        crate::playback_pause::NativePause,
        Option<crate::native_end::NativeEnd>,
    )> {
        if device.target_identity()? != (self.epoch, self.basis) {
            return Err("target startup creation identity changed".into());
        }
        let pair = latest.timing.point()?;
        let pause = pause
            .clone()
            .with_start_frame(plan.selected_source_frame())?
            .with_target_basis(self.epoch, self.basis)?;
        let end = end
            .clone()
            .map(|observer| -> NativeStartResult<_> {
                let mut observer = observer
                    .with_start_frame(plan.selected_source_frame())?
                    .with_target_basis(self.epoch, self.basis)?;
                observer.prime_target_clock(self.epoch, self.basis, pair)?;
                Ok(observer)
            })
            .transpose()?;
        Ok((pause, end))
    }
    fn confirmed(
        &self,
        device: &mut D,
        plan: Self::Plan,
        _: Option<beatkernel::audio::RenderReport>,
        applied: Option<u64>,
        config: NativeStartConfig,
    ) -> NativeStartResult<bool> {
        if applied.is_some_and(|frame| frame != plan.selected_source_frame()) {
            return Err("native applied source frame differs from committed frame".into());
        }
        let Some(tuple) = self.telemetry(device, config)? else {
            return Ok(false);
        };
        if tuple.facts.startup.is_none() {
            return Ok(false);
        }
        plan.validate_startup(tuple.facts)?;
        let empty = config.playback_end_frame == Some(0)
            && tuple.source.is_some_and(|source| {
                source.playback_end_physical_frame == Some(plan.selected_source_frame())
            });
        Ok(applied.is_some() || empty)
    }
}

/// Calibrate a silent device, arm one committed source frame, and await native presentation.
/// Cancellation keeps native stop/input cleanup with the caller; clock evidence is not physical sync proof.
pub fn start_committed<D: NativeStartDevice, A: NativeStartAgreement>(
    device: &mut D,
    agreement: &mut A,
    producer: &mut beatkernel::audio::CommandProducer,
    pause: &mut crate::playback_pause::NativePause,
    end: &mut Option<crate::native_end::NativeEnd>,
    config: NativeStartConfig,
    feed: impl FnMut(
        Option<beatkernel::audio::RenderReport>,
        &mut beatkernel::audio::CommandProducer,
    ) -> NativeStartResult<()>,
) -> NativeStartResult<Option<NativeStarted<D::Evidence>>> {
    start_committed_with(
        device,
        agreement,
        producer,
        pause,
        end,
        config,
        feed,
        SourceStartPolicy,
    )
}
/// Arms the source gate and confirms its actual mapped target onset before native readiness.
/// Pause/end must be cold source observers; target identity is staged before queue admission.
pub fn start_target_committed<D: NativeTargetStartDevice, A: NativeStartAgreement>(
    device: &mut D,
    agreement: &mut A,
    producer: &mut beatkernel::audio::CommandProducer,
    pause: &mut crate::playback_pause::NativePause,
    end: &mut Option<crate::native_end::NativeEnd>,
    config: NativeStartConfig,
    feed: impl FnMut(
        Option<beatkernel::audio::RenderReport>,
        &mut beatkernel::audio::CommandProducer,
    ) -> NativeStartResult<()>,
) -> NativeStartResult<Option<TargetNativeStarted<D::Evidence>>> {
    let (epoch, basis) = device.target_identity()?;
    start_committed_with(
        device,
        agreement,
        producer,
        pause,
        end,
        config,
        feed,
        TargetStartPolicy { epoch, basis },
    )
}

fn start_committed_with<
    D: NativeStartDevice,
    A: NativeStartAgreement,
    P: CommittedStartPolicy<D>,
>(
    device: &mut D,
    agreement: &mut A,
    producer: &mut beatkernel::audio::CommandProducer,
    pause: &mut crate::playback_pause::NativePause,
    end: &mut Option<crate::native_end::NativeEnd>,
    config: NativeStartConfig,
    mut feed: impl FnMut(
        Option<beatkernel::audio::RenderReport>,
        &mut beatkernel::audio::CommandProducer,
    ) -> NativeStartResult<()>,
    policy: P,
) -> NativeStartResult<Option<NativeStarted<D::Evidence, P::Plan>>> {
    use std::time::{Duration, Instant};
    if config.sample_rate == 0
        || config.sample_rate > 1_000_000_000
        || config.max_rate_error_ppm >= 1_000_000
        || config.setup_timeout.is_zero()
    {
        return Err("invalid native startup configuration".into());
    }
    let buffer = device.buffer_frames()?;
    if buffer == 0 {
        return Err("native startup buffer is empty".into());
    }
    let calibration_deadline = Instant::now()
        .checked_add(native_calibration_timeout(
            buffer,
            policy.calibration_rate(config),
        )?)
        .ok_or("native calibration deadline overflow")?;
    device.start()?;
    let mut previous = None;
    let mut first = None;
    let (first, mut latest) = loop {
        if !device.service_input(false)? {
            return Ok(None);
        }
        if Instant::now() >= calibration_deadline {
            return Err("native startup calibration timed out".into());
        }
        let observation = startup_observation(device, config.output_origin, &mut previous)?;
        feed(device.render_report()?, producer)?;
        if let Some(observation) = observation {
            let lower = *first.get_or_insert(observation.timing);
            let current = observation.timing.bounds();
            let initial = lower.bounds();
            if current.output.timestamp > initial.output.timestamp
                && i128::from(current.before.timestamp.as_nanos())
                    - i128::from(initial.after.timestamp.as_nanos())
                    >= 100_000_000
            {
                break (lower, observation);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    if !agreement.await_commit(&mut || {
        if !device.service_input(false)? {
            return Ok(false);
        }
        if let Some(observation) = startup_observation(device, config.output_origin, &mut previous)?
        {
            latest = observation;
        }
        feed(device.render_report()?, producer)?;
        Ok(true)
    })? {
        return Ok(None);
    }
    let schedule = agreement.committed_schedule()?;
    let mut sampled_host = None;
    let bracket = agreement.host_bracket(&mut || {
        let host = device.host_now()?;
        if host.domain != latest.timing.bounds().before.domain {
            return Err(StartProjectionError::Domains.into());
        }
        sampled_host = Some(host);
        Ok(host)
    })?;
    let mut last_host = sampled_host.ok_or("session bracket did not sample native host clock")?;
    let window = bracket.deadline_for_schedule(
        schedule,
        agreement.clock_now_ns()?,
        config.max_clock_age_ns,
    )?;
    let projection_deadline = Instant::now()
        .checked_add(config.setup_timeout)
        .ok_or("native projection deadline overflow")?;
    let plan = loop {
        if let Some(plan) = policy.project(device, window, first, latest.timing, buffer, config)? {
            break plan;
        }
        if Instant::now() >= projection_deadline {
            return Err("native target projection telemetry timed out".into());
        }
        if !device.service_input(false)? {
            return Ok(None);
        }
        if let Some(observation) = startup_observation(device, config.output_origin, &mut previous)?
        {
            latest = observation;
        }
        feed(device.render_report()?, producer)?;
        let host = device.host_now()?;
        if host.domain != last_host.domain {
            return Err(StartProjectionError::Domains.into());
        }
        if host.timestamp < last_host.timestamp {
            return Err(StartProjectionError::Chronology.into());
        }
        last_host = host;
        std::thread::sleep(Duration::from_millis(1));
    };
    // Reject observer setup failures before changing either the queue or live observers.
    let (next_pause, next_end) = policy.stage(device, plan, &latest, pause, end)?;
    let selected_source = policy.source_frame(plan);
    let selected_output = policy.output(plan)?;
    let crossing_deadline = Instant::now()
        .checked_add(config.setup_timeout)
        .ok_or("native presentation deadline overflow")?;
    producer.schedule_start_at(selected_source)?;
    *pause = next_pause;
    *end = next_end;
    let mut lower = latest.timing;
    let mut pending = None;
    loop {
        if !device.service_input(true)? {
            return Ok(None);
        }
        if Instant::now() >= crossing_deadline {
            return Err("native applied-start presentation timed out".into());
        }
        let observation = startup_observation(device, config.output_origin, &mut previous)?;
        let report = device.render_report()?;
        feed(report, producer)?;
        if let Some(observation) = observation.filter(|_| pending.is_none()) {
            if observation.timing.bounds().output.timestamp < selected_output.timestamp {
                lower = observation.timing;
            } else {
                if policy.confirmed(device, plan, report, producer.applied_start_frame(), config)? {
                    let host_window = match (lower, observation.timing) {
                        (NativeStartTiming::Point(lower), NativeStartTiming::Point(upper)) => {
                            let host = presented_output(selected_output, lower, upper)?;
                            HostStartWindow {
                                earliest: host,
                                latest: host,
                            }
                        }
                        (
                            NativeStartTiming::Interval(lower),
                            NativeStartTiming::Interval(upper),
                        ) => interval::crossing(
                            selected_output,
                            lower,
                            upper,
                            config.max_rate_error_ppm,
                        )?,
                        _ => return Err("native startup timing source changed".into()),
                    };
                    let midpoint = i128::from(host_window.earliest.timestamp.as_nanos())
                        + (i128::from(host_window.latest.timestamp.as_nanos())
                            - i128::from(host_window.earliest.timestamp.as_nanos()))
                            / 2;
                    let host_origin = ClockPoint {
                        domain: host_window.earliest.domain,
                        timestamp: Timestamp::from_nanos(i64::try_from(midpoint)?),
                    };
                    pending = Some(NativeStarted {
                        plan,
                        observation,
                        host_window,
                        host_origin,
                    });
                }
            }
        }
        // Presentation metadata can legitimately describe a future host instant.
        // Preserve the first crossing evidence while servicing acquisition until
        // the actual normalized host clock arrives, under the same deadline.
        let host = device.host_now()?;
        if host.domain != last_host.domain {
            return Err(StartProjectionError::Domains.into());
        }
        if host.timestamp < last_host.timestamp {
            return Err(StartProjectionError::Chronology.into());
        }
        last_host = host;
        if let Some(started) = pending {
            if host.timestamp >= started.host_window.latest.timestamp {
                return Ok(Some(started));
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
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
    struct Device {
        mixer: beatkernel::audio::Mixer,
        report: Option<beatkernel::audio::RenderReport>,
        pcm: [f32; 100],
        frame: u64,
        starts: usize,
        stages: Vec<bool>,
        cancel_retained: bool,
        invalid_domain: bool,
        future_host: bool,
        arrival_cancel: bool,
        arrival_domain: bool,
        arrival_regression: bool,
        buffer: u32,
    }
    impl NativeStartDevice for Device {
        type Evidence = u64;
        fn start(&mut self) -> NativeStartResult<()> {
            self.starts += 1;
            Ok(())
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            self.stages.push(retain);
            Ok(!(retain && (self.cancel_retained || (self.arrival_cancel && self.frame >= 1_100))))
        }
        fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<u64>>> {
            self.report = Some(self.mixer.render(&mut self.pcm)?);
            self.frame += 100;
            let ns = i64::try_from(self.frame * 1_000_000).unwrap();
            Ok(Some(NativeStartObservation {
                timing: NativeStartTiming::Point(ClockPair {
                    source: point(if self.invalid_domain { 3 } else { 2 }, ns),
                    target: point(1, ns),
                }),
                evidence: self.frame,
            }))
        }
        fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
            Ok(self.report)
        }
        fn buffer_frames(&self) -> NativeStartResult<u32> {
            Ok(self.buffer)
        }
        fn host_now(&self) -> NativeStartResult<ClockPoint> {
            if self.arrival_domain && self.frame >= 1_200 {
                return Ok(point(3, 1_000_000_000));
            }
            if self.arrival_regression && self.frame >= 1_200 {
                return Ok(point(1, 1));
            }
            let ns = if self.future_host && self.frame > 300 {
                300_000_000 + (self.frame as i64 - 300) * 500_000
            } else {
                self.frame as i64 * 1_000_000
            };
            Ok(point(1, ns))
        }
    }
    struct Agreement {
        cancel: bool,
        target: i64,
    }
    impl NativeStartAgreement for Agreement {
        fn await_commit(
            &mut self,
            service: &mut dyn FnMut() -> NativeStartResult<bool>,
        ) -> NativeStartResult<bool> {
            if !service()? {
                return Ok(false);
            }
            Ok(!self.cancel)
        }
        fn committed_schedule(&self) -> NativeStartResult<crate::multiplayer_start::StartSchedule> {
            Ok(crate::multiplayer_start::StartSchedule {
                target_ns: self.target,
                song_target_ns: self.target,
                uncertainty_ns: 0,
            })
        }
        fn host_bracket(
            &self,
            sample: &mut dyn FnMut() -> NativeStartResult<ClockPoint>,
        ) -> NativeStartResult<SessionHostBracket> {
            Ok(SessionHostBracket::new(
                300_000_000,
                sample()?,
                300_000_000,
            )?)
        }
        fn clock_now_ns(&self) -> NativeStartResult<i64> {
            Ok(300_000_000)
        }
    }
    fn device(
        gated: bool,
        finite: Option<u64>,
    ) -> (
        Device,
        beatkernel::audio::CommandProducer,
        crate::playback_pause::NativePause,
        Option<crate::native_end::NativeEnd>,
    ) {
        use beatkernel::audio::*;
        let format = AudioFormat::new(1_000, 1).unwrap();
        let limits = AudioLimits::new(4, 1, 4, 128, 4).unwrap();
        let pcm_limits = PcmLimits::new(16, 64, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25, 0.5], pcm_limits).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = if gated {
            command_queue_with_start_gate(4)
        } else {
            command_queue(4)
        }
        .unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
        let mut config = MixerConfig::new(format, ClockDomainId(2), Timestamp::ZERO, limits);
        if let Some(end) = finite {
            config = config.with_playback_end_frame(end);
        }
        let mixer = Mixer::new(config, bank, consumer).unwrap();
        let mut pause =
            crate::playback_pause::NativePause::new(point(2, 0), ClockDomainId(1), 1_000).unwrap();
        if let Some(end) = finite {
            pause = pause.with_playback_end_frame(end).unwrap();
        }
        let end = finite.map(|end| {
            crate::native_end::NativeEnd::new(point(2, 0), ClockDomainId(1), 1_000, end).unwrap()
        });
        (
            Device {
                mixer,
                report: None,
                pcm: [0.0; 100],
                frame: 0,
                starts: 0,
                stages: Vec::new(),
                cancel_retained: false,
                invalid_domain: false,
                future_host: false,
                arrival_cancel: false,
                arrival_domain: false,
                arrival_regression: false,
                buffer: 100,
            },
            producer,
            pause,
            end,
        )
    }
    fn config(finite: Option<u64>) -> NativeStartConfig {
        NativeStartConfig {
            output_origin: point(2, 0),
            sample_rate: 1_000,
            playback_end_frame: finite,
            setup_timeout: std::time::Duration::from_secs(1),
            max_clock_age_ns: 0,
            max_rate_error_ppm: 1_000,
        }
    }
    // Synthetic native telemetry around actual Mixer blocks, without a driver.
    struct IntervalDevice {
        inner: Device,
        sounds: Vec<(u64, f32)>,
        seeded: Option<beatkernel_platform::audio::asio::AsioPresentationObservation>,
    }
    impl NativeStartDevice for IntervalDevice {
        type Evidence = beatkernel_platform::audio::asio::AsioPresentationObservation;
        fn start(&mut self) -> NativeStartResult<()> {
            self.inner.start()
        }
        fn service_input(&mut self, retain: bool) -> NativeStartResult<bool> {
            self.inner.service_input(retain)
        }
        fn observe(&mut self) -> NativeStartResult<Option<NativeStartObservation<Self::Evidence>>> {
            use beatkernel_platform::audio::asio::MultimediaHostInterval;
            let report = self.inner.mixer.render(&mut self.inner.pcm)?;
            self.inner.report = Some(report);
            self.inner.frame += 100;
            self.sounds.extend(
                self.inner
                    .pcm
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        (*value != 0.0).then_some((report.start_frame + index as u64, *value))
                    }),
            );
            let ns = i64::try_from(report.start_frame * 1_000_000)?;
            let evidence = Self::Evidence::from_render(
                report,
                1_000,
                MultimediaHostInterval {
                    before: point(1, ns - 1_000_000),
                    after: point(1, ns + 1_000_000),
                },
                0,
                0,
                point(2, 0),
            )?;
            Ok(Some(NativeStartObservation {
                timing: NativeStartTiming::Interval(StartInterval::new(
                    evidence.output,
                    evidence.host.before,
                    evidence.host.after,
                )?),
                evidence,
            }))
        }
        fn render_report(&mut self) -> NativeStartResult<Option<beatkernel::audio::RenderReport>> {
            self.inner.render_report()
        }
        fn buffer_frames(&self) -> NativeStartResult<u32> {
            self.inner.buffer_frames()
        }
        fn host_now(&self) -> NativeStartResult<ClockPoint> {
            self.inner.host_now()
        }
        fn seed_end(
            &mut self,
            end: &mut crate::native_end::NativeEnd,
            observation: &NativeStartObservation<Self::Evidence>,
        ) -> NativeStartResult<()> {
            end.observe_asio(observation.evidence)?;
            self.seeded = Some(observation.evidence);
            Ok(())
        }
    }
    #[test]
    fn interval_start_retains_native_evidence_and_waits_upper_arrival_for_finite_sections() {
        for finite in [None, Some(2), Some(0)] {
            let (mut inner, mut producer, mut pause, mut end) = device(true, finite);
            inner.future_host = true;
            let mut device = IntervalDevice {
                inner,
                sounds: Vec::new(),
                seeded: None,
            };
            let mut agreement = Agreement {
                cancel: false,
                target: 1_000_000_000,
            };
            let mut feeds = 0;
            let started = start_committed(
                &mut device,
                &mut agreement,
                &mut producer,
                &mut pause,
                &mut end,
                config(finite),
                |_, _| {
                    feeds += 1;
                    Ok(())
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(device.inner.starts, 1);
            assert_eq!(started.plan.selected_frame(), 1_052);
            assert!(started.plan.earliest_frame() < started.plan.latest_frame());
            let evidence = started.observation.evidence;
            let NativeStartTiming::Interval(timing) = started.observation.timing else {
                panic!("interval must remain an interval");
            };
            assert_eq!(timing.output, evidence.output);
            assert_eq!(timing.before, evidence.host.before);
            assert_eq!(timing.after, evidence.host.after);
            assert!(started.observation.timing.point().is_err());
            assert!(started.host_window.earliest().timestamp < started.host_origin.timestamp);
            assert!(started.host_origin.timestamp < started.host_window.latest().timestamp);
            assert!(device.host_now().unwrap().timestamp >= started.host_window.latest().timestamp);
            assert_eq!(evidence.render.start_frame, 1_100);
            assert!(evidence.render.start_frame < device.inner.report.unwrap().start_frame);
            assert_eq!(&device.inner.stages[..4], &[false; 4]);
            assert!(device.inner.stages[4..].iter().all(|retain| *retain));
            assert_eq!(feeds, device.inner.stages.len());
            let report = device.inner.report.unwrap();
            let logical_end = report.playback_start_frame + report.playback_frames as u64;
            assert_eq!(
                pause.scheduling_point(report).unwrap(),
                point(2, logical_end as i64 * 1_000_000),
            );
            assert!(logical_end < report.start_frame + report.frames as u64);
            if finite == Some(0) {
                assert_eq!(producer.applied_start_frame(), None);
                assert!(device.sounds.is_empty());
                assert_eq!(evidence.render.playback_end_physical_frame, Some(1_052));
            } else {
                assert_eq!(producer.applied_start_frame(), Some(1_052));
                assert_eq!(device.sounds, [(1_052, 0.25), (1_053, 0.5)]);
            }
            if let Some(end) = &mut end {
                let seed = device.seeded.unwrap();
                assert_eq!(seed.render.start_frame, 300);
                assert!(seed.render.paused);
                let boundary = end.observe_asio(evidence).unwrap().unwrap();
                assert_eq!(boundary.physical_frame, 1_052 + finite.unwrap());
                assert_eq!(boundary.host, evidence.host.after);
            } else {
                assert!(device.seeded.is_none());
            }
        }
    }
    #[test]
    fn interval_start_cancellation_keeps_cleanup_with_caller() {
        for after_gate in [false, true] {
            let (mut inner, mut producer, mut pause, mut end) = device(true, None);
            inner.cancel_retained = after_gate;
            let mut device = IntervalDevice {
                inner,
                sounds: Vec::new(),
                seeded: None,
            };
            let mut agreement = Agreement {
                cancel: !after_gate,
                target: 1_000_000_000,
            };
            assert!(start_committed(
                &mut device,
                &mut agreement,
                &mut producer,
                &mut pause,
                &mut end,
                config(None),
                |_, _| Ok(()),
            )
            .unwrap()
            .is_none());
            assert_eq!(device.inner.starts, 1);
            assert_eq!(device.inner.stages.last(), Some(&after_gate));
            assert_eq!(producer.applied_start_frame(), None);
            assert!(device.sounds.is_empty());
        }
    }
    #[test]
    fn common_owner_uses_actual_gate_and_preserves_crossing_evidence() {
        for finite in [None, Some(2), Some(0)] {
            let (mut device, mut producer, mut pause, mut end) = device(true, finite);
            let mut agreement = Agreement {
                cancel: false,
                target: 1_000_000_000,
            };
            let mut feeds = 0;
            let started = start_committed(
                &mut device,
                &mut agreement,
                &mut producer,
                &mut pause,
                &mut end,
                config(finite),
                |_, _| {
                    feeds += 1;
                    Ok(())
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(device.starts, 1);
            assert_eq!(started.plan.selected_frame(), 1_000);
            assert_eq!(started.host_origin, point(1, 1_000_000_000));
            assert_eq!(started.observation.evidence, device.frame);
            assert_eq!(
                started.observation.timing.point().unwrap().source,
                point(2, device.frame as i64 * 1_000_000)
            );
            assert_eq!(&device.stages[..3], &[false, false, false]);
            assert!(device.stages[3..].iter().all(|retain| *retain));
            assert!(feeds >= device.stages.len());
            if finite == Some(0) {
                assert_eq!(producer.applied_start_frame(), None);
                assert!(device.pcm.iter().all(|value| *value == 0.0));
                assert_eq!(
                    device.report.unwrap().playback_end_physical_frame,
                    Some(1_000)
                );
            } else {
                assert_eq!(producer.applied_start_frame(), Some(1_000));
                assert_eq!(&device.pcm[..3], &[0.25, 0.5, 0.0]);
            }
            if let Some(end) = &mut end {
                let boundary = end
                    .observe(device.report, started.observation.timing.point().unwrap())
                    .unwrap()
                    .unwrap();
                assert_eq!(boundary.physical_frame, 1_000 + finite.unwrap());
            }
        }
    }
    #[test]
    fn common_owner_cancellation_before_and_after_gate_keeps_cleanup_with_caller() {
        for after_gate in [false, true] {
            let (mut device, mut producer, mut pause, mut end) = device(true, None);
            device.cancel_retained = after_gate;
            let mut agreement = Agreement {
                cancel: !after_gate,
                target: 1_000_000_000,
            };
            assert!(start_committed(
                &mut device,
                &mut agreement,
                &mut producer,
                &mut pause,
                &mut end,
                config(None),
                |_, _| Ok(())
            )
            .unwrap()
            .is_none());
            assert_eq!(device.starts, 1);
            assert_eq!(producer.applied_start_frame(), None);
            assert_eq!(device.stages.last(), Some(&after_gate));
            if !after_gate {
                assert!(pause.clone().with_start_frame(10).is_ok());
            }
        }
    }
    #[test]
    fn malformed_capability_and_clock_fail_before_gate_and_observer_adoption() {
        for invalid_clock in [false, true] {
            let (mut device, mut producer, mut pause, mut end) = device(true, None);
            device.invalid_domain = invalid_clock;
            if !invalid_clock {
                device.buffer = 0;
            }
            let mut agreement = Agreement {
                cancel: false,
                target: 1_000_000_000,
            };
            assert!(start_committed(
                &mut device,
                &mut agreement,
                &mut producer,
                &mut pause,
                &mut end,
                config(None),
                |_, _| Ok(())
            )
            .is_err());
            assert_eq!(producer.applied_start_frame(), None);
            assert!(pause.clone().with_start_frame(10).is_ok());
            assert_eq!(device.starts, usize::from(invalid_clock));
            assert!(!device.stages.contains(&true));
        }
        let (mut device, mut producer, mut pause, mut end) = device(false, None);
        let mut agreement = Agreement {
            cancel: false,
            target: 1_000_000_000,
        };
        assert!(start_committed(
            &mut device,
            &mut agreement,
            &mut producer,
            &mut pause,
            &mut end,
            config(None),
            |_, _| Ok(())
        )
        .is_err());
        assert!(pause.with_start_frame(10).is_ok());
        assert!(!device.stages.contains(&true));
    }
    #[test]
    fn observer_configuration_failure_does_not_arm_gate_or_adopt_other_observer() {
        let (mut device, mut producer, mut pause, mut end) = device(true, Some(2));
        end = Some(end.take().unwrap().with_start_frame(0).unwrap());
        let mut agreement = Agreement {
            cancel: false,
            target: 1_000_000_000,
        };
        assert!(start_committed(
            &mut device,
            &mut agreement,
            &mut producer,
            &mut pause,
            &mut end,
            config(Some(2)),
            |_, _| Ok(())
        )
        .is_err());
        assert!(pause.with_start_frame(42).is_ok());
        assert!(producer.schedule_start_at(1_000).is_ok());
        assert!(!device.stages.contains(&true));
    }
    #[test]
    fn future_presentation_waits_for_host_and_keeps_first_crossing_evidence() {
        let (mut device, mut producer, mut pause, mut end) = device(true, None);
        device.future_host = true;
        let mut agreement = Agreement {
            cancel: false,
            target: 1_000_000_000,
        };
        let mut feeds = 0;
        let started = start_committed(
            &mut device,
            &mut agreement,
            &mut producer,
            &mut pause,
            &mut end,
            config(None),
            |_, _| {
                feeds += 1;
                Ok(())
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(started.host_origin, point(1, 1_000_000_000));
        assert_eq!(started.observation.evidence, 1_100);
        assert_eq!(
            started.observation.timing.point().unwrap().source,
            point(2, 1_100_000_000)
        );
        assert_eq!(device.frame, 1_700);
        assert_eq!(device.host_now().unwrap(), started.host_origin);
        assert_eq!(producer.applied_start_frame(), Some(1_000));
        assert_eq!(feeds, device.stages.len());
        assert!(device.stages[3..].iter().all(|retain| *retain));
    }
    #[test]
    fn host_arrival_wait_services_cancel_and_rejects_domain_or_time_regression() {
        for failure in 0..3 {
            let (mut device, mut producer, mut pause, mut end) = device(true, None);
            device.future_host = true;
            device.arrival_cancel = failure == 0;
            device.arrival_domain = failure == 1;
            device.arrival_regression = failure == 2;
            let mut agreement = Agreement {
                cancel: false,
                target: 1_000_000_000,
            };
            let result = start_committed(
                &mut device,
                &mut agreement,
                &mut producer,
                &mut pause,
                &mut end,
                config(None),
                |_, _| Ok(()),
            );
            if failure == 0 {
                assert!(result.unwrap().is_none());
                assert_eq!(device.frame, 1_100);
            } else {
                assert!(result.is_err());
                assert_eq!(device.frame, 1_200);
            }
            assert_eq!(producer.applied_start_frame(), Some(1_000));
            assert!(device.stages[3..].iter().all(|retain| *retain));
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
        let exact = HostStartWindow::new(point(1, 7), point(1, 7)).unwrap();
        assert_eq!(exact.earliest(), point(1, 7));
        assert_eq!(exact.latest(), point(1, 7));
        let full = HostStartWindow::new(point(1, i64::MIN), point(1, i64::MAX)).unwrap();
        assert_eq!(full.earliest(), point(1, i64::MIN));
        assert_eq!(full.latest(), point(1, i64::MAX));
        assert_eq!(
            HostStartWindow::new(point(1, 8), point(1, 7)),
            Err(StartProjectionError::Chronology)
        );
        assert_eq!(
            HostStartWindow::new(point(1, 7), point(2, 8)),
            Err(StartProjectionError::Domains)
        );
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

/// Exact conversion state at a generated frontier and its original native creation basis.
/// Source pull is the immutable arming safety frontier, never a playback timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetStartSnapshot {
    pub basis: beatkernel::audio::TargetFrameBasis,
    pub generated_time: beatkernel::audio::TargetTime,
    pub source_position: beatkernel::audio::SourcePosition,
    pub pulled_source_frame: u64,
    pub source_rate: u32,
}
impl TargetStartSnapshot {
    pub fn from_owner(owner: &beatkernel_platform::audio::ConvertedNativeOutputState) -> Self {
        Self {
            basis: owner.target_frame_basis(),
            generated_time: owner.converter_owner().target_time(),
            source_position: owner.converter_owner().source_position(),
            pulled_source_frame: owner.converter_owner().pulled_source_frame_cursor(),
            source_rate: owner.mixer().config().format().sample_rate(),
        }
    }
}
/// Source gate selected from original native clock associations and exact target time.
/// The predicted output must still be confirmed by the actual mapped startup fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetOutputStartPlan {
    source_frame: u64,
    source_rate: u32,
    target_time: beatkernel::audio::TargetTime,
    basis: beatkernel::audio::TargetFrameBasis,
}
impl TargetOutputStartPlan {
    pub fn from_owner(
        window: HostStartWindow,
        first: ClockPair,
        second: ClockPair,
        owner: &beatkernel_platform::audio::ConvertedNativeOutputState,
        target_buffer_frames: u64,
        maximum_rate_error_ppm: u32,
    ) -> Result<Self, StartProjectionError> {
        Self::from_snapshot(
            window,
            first,
            second,
            TargetStartSnapshot::from_owner(owner),
            target_buffer_frames,
            maximum_rate_error_ppm,
        )
    }
    pub fn from_snapshot(
        window: HostStartWindow,
        first: ClockPair,
        second: ClockPair,
        snapshot: TargetStartSnapshot,
        target_buffer_frames: u64,
        maximum_rate_error_ppm: u32,
    ) -> Result<Self, StartProjectionError> {
        if first.source.domain != snapshot.basis.origin().domain
            || first.source.domain != second.source.domain
            || first.target.domain != second.target.domain
            || second.target.domain != window.earliest().domain
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
        let position = snapshot.source_position;
        if snapshot.source_rate == 0
            || target_buffer_frames == 0
            || position.denominator == 0
            || position.numerator >= position.denominator
            || snapshot.pulled_source_frame < position.frame
        {
            return Err(StartProjectionError::InvalidRate);
        }
        let target_rate = u128::from(snapshot.basis.sample_rate());
        let project = |host: ClockPoint| -> Result<u64, StartProjectionError> {
            let physical_ns = (i128::from(second.source.timestamp.as_nanos())
                - i128::from(snapshot.basis.origin().timestamp.as_nanos()))
            .checked_mul(host_span)
            .and_then(|value| {
                (i128::from(host.timestamp.as_nanos())
                    - i128::from(second.target.timestamp.as_nanos()))
                .checked_mul(source_span)
                .and_then(|offset| value.checked_add(offset))
            })
            .ok_or(StartProjectionError::Overflow)?;
            let duration = snapshot.generated_time;
            let start = i128::from(duration.seconds())
                .checked_mul(i128::from(duration.denominator()))
                .and_then(|value| value.checked_add(i128::from(duration.numerator())))
                .and_then(|value| value.checked_mul(1_000_000_000))
                .and_then(|value| value.checked_mul(host_span))
                .ok_or(StartProjectionError::Overflow)?;
            let delta = physical_ns
                .checked_mul(i128::from(duration.denominator()))
                .and_then(|value| value.checked_sub(start))
                .ok_or(StartProjectionError::Overflow)?;
            if delta < 0 {
                return Err(StartProjectionError::TooClose);
            }
            let denominator = (host_span as u128)
                .checked_mul(u128::from(duration.denominator()))
                .and_then(|value| value.checked_mul(1_000_000_000))
                .ok_or(StartProjectionError::Overflow)?;
            // Quantize the source gate once. Rounding to a target frame first
            // can unnecessarily skip a source frame that already maps to the
            // same admissible first target sample.
            let scaled = (delta as u128)
                .checked_mul(u128::from(snapshot.source_rate))
                .ok_or(StartProjectionError::Overflow)?;
            let mut a = denominator;
            let mut b = u128::from(position.denominator);
            while b != 0 {
                let remainder = a % b;
                a = b;
                b = remainder;
            }
            let common = (denominator / a)
                .checked_mul(u128::from(position.denominator))
                .ok_or(StartProjectionError::Overflow)?;
            let fraction = (scaled % denominator)
                .checked_mul(common / denominator)
                .and_then(|value| {
                    u128::from(position.numerator)
                        .checked_mul(common / u128::from(position.denominator))
                        .and_then(|phase| value.checked_add(phase))
                })
                .ok_or(StartProjectionError::Overflow)?;
            let advance = (scaled / denominator)
                .checked_add(fraction / common)
                .and_then(|value| value.checked_add(u128::from(fraction % common != 0)))
                .ok_or(StartProjectionError::Overflow)?;
            position
                .frame
                .checked_add(u64::try_from(advance).map_err(|_| StartProjectionError::Overflow)?)
                .ok_or(StartProjectionError::Overflow)
        };
        let earliest = project(window.earliest())?;
        let source_buffer = u128::from(target_buffer_frames) * u128::from(snapshot.source_rate);
        let source_buffer =
            source_buffer / target_rate + u128::from(source_buffer % target_rate != 0);
        let minimum = snapshot
            .pulled_source_frame
            .checked_add(u64::try_from(source_buffer).map_err(|_| StartProjectionError::Overflow)?)
            .ok_or(StartProjectionError::Overflow)?;
        if earliest < minimum {
            return Err(StartProjectionError::TooClose);
        }
        let source_frame = project(window.latest())?;
        let distance = u128::from(source_frame - position.frame) * u128::from(position.denominator)
            - u128::from(position.numerator);
        let numerator = distance
            .checked_mul(target_rate)
            .ok_or(StartProjectionError::Overflow)?;
        let denominator = u128::from(position.denominator) * u128::from(snapshot.source_rate);
        let target_frames = numerator / denominator + u128::from(numerator % denominator != 0);
        let target_time = snapshot
            .generated_time
            .checked_add_frames(
                u64::try_from(target_frames).map_err(|_| StartProjectionError::Overflow)?,
                snapshot.basis.sample_rate(),
            )
            .map_err(|_| StartProjectionError::Overflow)?;
        target_time
            .point(snapshot.basis.origin())
            .map_err(|_| StartProjectionError::Overflow)?;
        Ok(Self {
            source_frame,
            source_rate: snapshot.source_rate,
            target_time,
            basis: snapshot.basis,
        })
    }
    pub const fn selected_source_frame(self) -> u64 {
        self.source_frame
    }
    pub fn selected_output(self) -> Result<ClockPoint, StartProjectionError> {
        self.target_time
            .point(self.basis.origin())
            .map_err(|_| StartProjectionError::Overflow)
    }
    pub const fn target_basis(self) -> beatkernel::audio::TargetFrameBasis {
        self.basis
    }
    /// Actual gate adoption must agree with the source identity and generated target boundary.
    pub fn validate_startup(
        self,
        facts: beatkernel_platform::audio::ConvertedBoundaryFacts,
    ) -> Result<ClockPoint, StartProjectionError> {
        let boundary = facts.startup.ok_or(StartProjectionError::TooClose)?;
        if facts.origin != Some(self.basis.origin())
            || facts.source_rate != self.source_rate
            || boundary.source_frame != self.source_frame
            || boundary.target_time != self.target_time
        {
            return Err(StartProjectionError::Chronology);
        }
        boundary
            .target_time
            .point(self.basis.origin())
            .map_err(|_| StartProjectionError::Overflow)
    }
}
