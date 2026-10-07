//! Native-frontier pause acknowledgement and bounded keyboard reconciliation.
//! Boundary interpolation has unknown physical mapping error; no wall clock is read.
use crate::native_start::{HostStartWindow, StartInterval};
use beatkernel::{
    audio::RenderReport,
    input::{ButtonEvent, ButtonState, DeviceId, PhysicalControlId, PhysicalInputEvent},
    time::{ClockDomainId, ClockPair, ClockPoint, Timestamp},
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseError(pub &'static str);
impl std::fmt::Display for PauseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for PauseError {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PausePhase {
    Running,
    Pausing,
    Paused,
    Resuming,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseBoundary {
    pub paused: bool,
    pub host: ClockPoint,
    pub playback_frame: u64,
}
/// Original actual render block and its assessed presentation interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseIntervalObservation {
    pub output_origin: ClockPoint,
    pub sample_rate: u32,
    pub render: RenderReport,
    pub clock: StartInterval,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalPauseBoundary {
    pub paused: bool,
    pub host: HostStartWindow,
    pub physical_frame: u64,
    pub playback_frame: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvidenceKind {
    Point,
    Interval,
}
#[derive(Clone, Copy, Debug)]
struct PendingBoundary {
    physical: u64,
    playback: u64,
    gap: u64,
}
#[derive(Clone, Debug)]
pub struct NativePause {
    epoch: u64,
    min_physical_frame: u64,
    min_host: Option<ClockPoint>,
    origin: ClockPoint,
    host: ClockDomainId,
    rate: u32,
    phase: PausePhase,
    gap: u64,
    frozen: u64,
    reference: Option<ClockPair>,
    last_pair: Option<ClockPair>,
    last_report: Option<RenderReport>,
    boundary: Option<PendingBoundary>,
    playback_end: Option<u64>,
    end_marker: Option<u64>,
    setup_locked: bool,
    start_frame: u64,
    start_configured: bool,
    evidence_kind: Option<EvidenceKind>,
    interval_reference: Option<PauseIntervalObservation>,
    last_interval: Option<PauseIntervalObservation>,
    interval_window: Option<HostStartWindow>,
    last_now: Option<ClockPoint>,
}
impl NativePause {
    pub fn new(
        output_origin: ClockPoint,
        host_domain: ClockDomainId,
        sample_rate: u32,
    ) -> Result<Self, PauseError> {
        if output_origin.domain == host_domain || sample_rate == 0 || sample_rate > 1_000_000_000 {
            return Err(PauseError(
                "pause requires distinct domains and a representable nonzero frame grid",
            ));
        }
        Ok(Self {
            epoch: 0,
            min_physical_frame: 0,
            min_host: None,
            origin: output_origin,
            host: host_domain,
            rate: sample_rate,
            phase: PausePhase::Running,
            gap: 0,
            frozen: 0,
            reference: None,
            last_pair: None,
            last_report: None,
            boundary: None,
            playback_end: None,
            end_marker: None,
            setup_locked: false,
            start_frame: 0,
            start_configured: false,
            evidence_kind: None,
            interval_reference: None,
            last_interval: None,
            interval_window: None,
            last_now: None,
        })
    }
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Validates a fully observed candidate without changing either pause owner.
    pub fn validate_replacement(
        &self,
        candidate: &Self,
        basis: beatkernel::audio::OutputFrameBasis,
        pair: ClockPair,
    ) -> Result<(), PauseError> {
        if self.phase != PausePhase::Paused
            || candidate.phase != PausePhase::Paused
            || candidate.epoch <= self.epoch
            || self.end_marker.is_some()
            || candidate.end_marker.is_some()
            || self.origin != candidate.origin
            || self.host != candidate.host
            || self.rate != candidate.rate
            || self.gap != candidate.gap
            || self.frozen != candidate.frozen
            || self.start_frame != candidate.start_frame
            || self.start_configured != candidate.start_configured
            || self.playback_end != candidate.playback_end
            || candidate.boundary.is_some()
            || basis.origin() != self.origin
            || basis.sample_rate() != self.rate
            || candidate.min_physical_frame != basis.start_physical_frame()
        {
            return Err(PauseError(
                "replacement changed acknowledged pause identity",
            ));
        }
        self.check_pair(pair)?;
        candidate.check_pair(pair)?;
        let report = candidate
            .last_report
            .ok_or(PauseError("replacement lacks paused render evidence"))?;
        candidate.check_report(report)?;
        self.check_report(report)?;
        if self.playback_end.is_some_and(|end| self.frozen >= end) {
            return Err(PauseError("replacement cannot reopen reached endpoint"));
        }
        if !report.paused
            || report.frames == 0
            || report.playback_frames != 0
            || report.playback_start_frame != self.frozen
        {
            return Err(PauseError("replacement changed frozen playback"));
        }
        match candidate.evidence_kind {
            Some(EvidenceKind::Point) if candidate.last_pair == Some(pair) => {}
            Some(EvidenceKind::Interval) => {
                let interval = candidate
                    .last_interval
                    .ok_or(PauseError("replacement lacks original interval evidence"))?;
                if interval.render != report
                    || interval.clock.output != pair.source
                    || pair.target.domain != interval.clock.before.domain
                    || pair.target.timestamp < interval.clock.before.timestamp
                    || pair.target.timestamp > interval.clock.after.timestamp
                {
                    return Err(PauseError(
                        "replacement pair differs from original interval",
                    ));
                }
            }
            _ => return Err(PauseError("replacement lacks accepted pause observation")),
        }
        Ok(())
    }
    /// Rebinds a newer output owner after the caller proves native retirement.
    /// Frozen playback and acknowledged pause gap remain unchanged until resume.
    pub fn rebind_output(
        &mut self,
        epoch: u64,
        mixer: &beatkernel::audio::Mixer,
    ) -> Result<(), PauseError> {
        if epoch <= self.epoch {
            return Err(PauseError("pause output epoch must increase"));
        }
        if self.phase != PausePhase::Paused || !mixer.is_paused() {
            return Err(PauseError(
                "pause output rebind requires acknowledged paused state",
            ));
        }
        let config = mixer.config();
        if config.domain() != self.origin.domain
            || config.origin() != self.origin.timestamp
            || config.format().sample_rate() != self.rate
            || config.playback_end_frame() != self.playback_end
        {
            return Err(PauseError(
                "pause output rebind changed the immutable frame grid",
            ));
        }
        match mixer.start_gate_frame() {
            None if self.start_frame == 0 => {}
            Some(Some(frame))
                if frame == self.start_frame && mixer.applied_start_frame() == Some(frame) => {}
            _ => {
                return Err(PauseError(
                    "pause output rebind changed or has unresolved startup",
                ));
            }
        }
        if mixer.playback_frame_cursor() != self.frozen {
            return Err(PauseError("pause output rebind changed frozen playback"));
        }
        if self.end_marker.is_some()
            || self
                .playback_end
                .is_some_and(|end| mixer.playback_frame_cursor() >= end)
        {
            return Err(PauseError(
                "pause output rebind cannot reopen a reached endpoint",
            ));
        }
        let physical = mixer.frame_cursor();
        let previous_end = self
            .last_report
            .map(|report| {
                report
                    .start_frame
                    .checked_add(
                        u64::try_from(report.frames)
                            .map_err(|_| PauseError("render extent overflow"))?,
                    )
                    .ok_or(PauseError("previous physical extent overflow"))
            })
            .transpose()?;
        if physical < self.min_physical_frame || previous_end.is_some_and(|end| physical < end) {
            return Err(PauseError(
                "pause output rebind physical frontier regressed",
            ));
        }
        self.point(physical)?;
        let mut min_host = self.min_host;
        for host in [self.last_pair.map(|pair| pair.target), self.last_now]
            .into_iter()
            .flatten()
        {
            if min_host.is_none_or(|old| host.timestamp > old.timestamp) {
                min_host = Some(host);
            }
        }
        self.epoch = epoch;
        self.min_physical_frame = physical;
        self.min_host = min_host;
        self.reference = None;
        self.last_pair = None;
        self.last_report = None;
        self.boundary = None;
        self.evidence_kind = None;
        self.interval_reference = None;
        self.last_interval = None;
        self.interval_window = None;
        self.setup_locked = true;
        Ok(())
    }
    pub fn request_in_epoch(
        &mut self,
        epoch: u64,
        paused: bool,
        reference: ClockPair,
    ) -> Result<bool, PauseError> {
        if epoch != self.epoch {
            return Err(PauseError("pause output epoch mismatch"));
        }
        self.request(paused, reference)
    }
    pub fn observe_in_epoch(
        &mut self,
        epoch: u64,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<PauseBoundary>, PauseError> {
        if epoch != self.epoch {
            return Err(PauseError("pause output epoch mismatch"));
        }
        self.observe(report, pair)
    }
    pub fn request_interval_in_epoch(
        &mut self,
        epoch: u64,
        paused: bool,
        reference: PauseIntervalObservation,
    ) -> Result<bool, PauseError> {
        if epoch != self.epoch {
            return Err(PauseError("pause output epoch mismatch"));
        }
        self.request_interval(paused, reference)
    }
    pub fn observe_interval_in_epoch(
        &mut self,
        epoch: u64,
        observation: Option<PauseIntervalObservation>,
        now: ClockPoint,
    ) -> Result<Option<IntervalPauseBoundary>, PauseError> {
        if epoch != self.epoch {
            return Err(PauseError("pause output epoch mismatch"));
        }
        self.observe_interval(observation, now)
    }
    /// Configures initial silent physical frames before any request/observation.
    /// Playback scheduling remains relative to logical frame zero.
    pub fn with_start_frame(mut self, frame: u64) -> Result<Self, PauseError> {
        if self.setup_locked
            || self.last_pair.is_some()
            || self.last_report.is_some()
            || self.start_configured
        {
            return Err(PauseError(
                "pause startup is already configured or observed",
            ));
        }
        let physical_end = frame
            .checked_add(self.playback_end.unwrap_or(0))
            .ok_or(PauseError("startup endpoint overflow"))?;
        self.point(physical_end)?;
        self.start_frame = frame;
        self.start_configured = true;
        self.gap = frame;
        Ok(self)
    }
    /// Opts into one immutable endpoint before any request or clock/report
    /// observation. Unlimited owners retain strict manual-pause validation.
    pub fn with_playback_end_frame(mut self, end: u64) -> Result<Self, PauseError> {
        if self.setup_locked
            || self.last_pair.is_some()
            || self.last_report.is_some()
            || self.playback_end.is_some()
        {
            return Err(PauseError(
                "finite pause setup is already configured or observed",
            ));
        }
        self.point(
            self.start_frame
                .checked_add(end)
                .ok_or(PauseError("startup endpoint overflow"))?,
        )?;
        self.playback_end = Some(end);
        Ok(self)
    }
    pub fn phase(&self) -> PausePhase {
        self.phase
    }
    /// Latest validated render evidence, retained across transient unavailable
    /// telemetry reads. This never advances a cursor without a report.
    pub fn last_render_report(&self) -> Option<RenderReport> {
        self.last_report
    }
    fn check_pair(&self, pair: ClockPair) -> Result<(), PauseError> {
        if pair.source.domain != self.origin.domain
            || pair.target.domain != self.host
            || pair.source.timestamp < self.point(self.min_physical_frame)?.timestamp
            || self
                .min_host
                .is_some_and(|host| pair.target.timestamp < host.timestamp)
        {
            return Err(PauseError(
                "pause clock pair has wrong domain or precedes output origin",
            ));
        }
        if self.last_pair.is_some_and(|old| {
            pair.source.timestamp < old.source.timestamp
                || pair.target.timestamp < old.target.timestamp
        }) {
            return Err(PauseError("pause clock observations regressed"));
        }
        Ok(())
    }
    fn bind(&mut self, kind: EvidenceKind) -> Result<(), PauseError> {
        if self.evidence_kind.is_some_and(|old| old != kind) {
            return Err(PauseError("pause evidence kind changed"));
        }
        self.evidence_kind = Some(kind);
        self.setup_locked = true;
        Ok(())
    }
    pub fn request(&mut self, paused: bool, reference: ClockPair) -> Result<bool, PauseError> {
        let mut next = self.clone();
        next.bind(EvidenceKind::Point)?;
        let accepted = next.request_point_inner(paused, reference)?;
        *self = next;
        Ok(accepted)
    }
    fn request_point_inner(
        &mut self,
        paused: bool,
        reference: ClockPair,
    ) -> Result<bool, PauseError> {
        self.check_pair(reference)?;
        self.setup_locked = true;
        if self.end_marker.is_some() {
            return Ok(false);
        }
        let next = match (self.phase, paused) {
            (PausePhase::Running, true) => PausePhase::Pausing,
            (PausePhase::Paused, false) => PausePhase::Resuming,
            _ => return Ok(false),
        };
        self.phase = next;
        self.reference = Some(reference);
        self.last_pair = Some(reference);
        self.boundary = None;
        Ok(true)
    }
    fn admit_interval(
        &mut self,
        observation: PauseIntervalObservation,
    ) -> Result<PauseIntervalObservation, PauseError> {
        observation
            .clock
            .validate()
            .map_err(|_| PauseError("invalid pause presentation interval"))?;
        if observation.output_origin != self.origin
            || observation.sample_rate != self.rate
            || observation.clock.before.domain != self.host
            || observation.clock.after.domain != self.host
            || observation.clock.output != self.point(observation.render.start_frame)?
            || observation.render.frames == 0
        {
            return Err(PauseError(
                "pause interval metadata differs from the output grid",
            ));
        }
        let physical_end = observation
            .render
            .start_frame
            .checked_add(
                u64::try_from(observation.render.frames)
                    .map_err(|_| PauseError("render extent overflow"))?,
            )
            .ok_or(PauseError("physical frame overflow"))?;
        if observation.render.counters.rendered_frames != physical_end {
            return Err(PauseError(
                "pause interval render counters precede its block",
            ));
        }
        if let Some(old) = self.last_interval {
            if observation.render.start_frame == old.render.start_frame {
                if observation.render != old.render {
                    return Err(PauseError(
                        "pause interval changed an existing render block",
                    ));
                }
                return Ok(old); // Refreshed bounds cannot move original evidence.
            }
            if observation.clock.output.timestamp < old.clock.output.timestamp
                || observation.clock.before.timestamp < old.clock.before.timestamp
                || observation.clock.after.timestamp < old.clock.after.timestamp
            {
                return Err(PauseError("pause intervals regressed"));
            }
        }
        self.adopt_report(observation.render)?;
        self.last_interval = Some(observation);
        Ok(observation)
    }
    pub fn request_interval(
        &mut self,
        paused: bool,
        reference: PauseIntervalObservation,
    ) -> Result<bool, PauseError> {
        let mut next = self.clone();
        next.bind(EvidenceKind::Interval)?;
        let reference = next.admit_interval(reference)?;
        next.fix_interval_window()?;
        let phase = match (next.phase, paused, next.end_marker.is_none()) {
            (PausePhase::Running, true, true) => Some(PausePhase::Pausing),
            (PausePhase::Paused, false, true) => Some(PausePhase::Resuming),
            _ => None,
        };
        if let Some(phase) = phase {
            next.phase = phase;
            next.interval_reference = Some(reference);
            next.interval_window = None;
            next.boundary = None;
        }
        *self = next;
        Ok(phase.is_some())
    }
    pub fn observe_interval(
        &mut self,
        observation: Option<PauseIntervalObservation>,
        now: ClockPoint,
    ) -> Result<Option<IntervalPauseBoundary>, PauseError> {
        let mut next = self.clone();
        next.bind(EvidenceKind::Interval)?;
        if now.domain != next.host
            || next
                .min_host
                .is_some_and(|host| now.timestamp < host.timestamp)
            || next
                .last_now
                .is_some_and(|old| now.timestamp < old.timestamp)
        {
            return Err(PauseError(
                "pause host arrival observation regressed or changed domain",
            ));
        }
        if let Some(observation) = observation {
            next.admit_interval(observation)?;
        }
        next.last_now = Some(now);
        let result = next.interval_boundary(now)?;
        *self = next;
        Ok(result)
    }
    fn fix_interval_window(&mut self) -> Result<(), PauseError> {
        let Some(boundary) = self.boundary else {
            return Ok(());
        };
        if self.interval_window.is_none() {
            let reference = self
                .interval_reference
                .ok_or(PauseError("pause interval lacks its request bracket"))?;
            let output = self.point(boundary.physical)?;
            if reference.clock.output.timestamp > output.timestamp {
                return Err(PauseError(
                    "pause boundary precedes its interval request bracket",
                ));
            }
            let current = self
                .last_interval
                .ok_or(PauseError("pause interval lacks native evidence"))?;
            if current.clock.output.timestamp < output.timestamp {
                return Ok(());
            }
            let (earliest, latest) = if reference.clock.output == output {
                (reference.clock.before, reference.clock.after)
            } else if current.clock.output == output {
                (current.clock.before, current.clock.after)
            } else {
                (reference.clock.before, current.clock.after)
            };
            self.interval_window = Some(
                HostStartWindow::new(earliest, latest)
                    .map_err(|_| PauseError("pause boundary interval is invalid"))?,
            );
        }
        Ok(())
    }
    fn interval_boundary(
        &mut self,
        now: ClockPoint,
    ) -> Result<Option<IntervalPauseBoundary>, PauseError> {
        let Some(boundary) = self.boundary else {
            return Ok(None);
        };
        self.fix_interval_window()?;
        let Some(host) = self.interval_window else {
            return Ok(None);
        };
        if now.timestamp < host.latest().timestamp {
            return Ok(None);
        }
        let paused = self.commit_boundary(boundary);
        Ok(Some(IntervalPauseBoundary {
            paused,
            host,
            physical_frame: boundary.physical,
            playback_frame: boundary.playback,
        }))
    }
    fn point(&self, frame: u64) -> Result<ClockPoint, PauseError> {
        let nanos = i128::from(frame) * 1_000_000_000 / i128::from(self.rate);
        let value = i128::from(self.origin.timestamp.as_nanos())
            .checked_add(nanos)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(PauseError("pause frame timestamp overflow"))?;
        Ok(ClockPoint {
            domain: self.origin.domain,
            timestamp: Timestamp::from_nanos(value),
        })
    }
    fn startup_prefix(&self, report: RenderReport) -> Result<u64, PauseError> {
        Ok(u64::try_from(report.frames)
            .map_err(|_| PauseError("render extent overflow"))?
            .min(self.start_frame.saturating_sub(report.start_frame)))
    }
    fn report_gap(&self, report: RenderReport) -> Result<u64, PauseError> {
        report
            .start_frame
            .checked_add(self.startup_prefix(report)?)
            .and_then(|frame| frame.checked_sub(report.playback_start_frame))
            .ok_or(PauseError("playback grid exceeds physical grid"))
    }
    fn check_report(&self, report: RenderReport) -> Result<(u64, u64, u64), PauseError> {
        if report.start_frame < self.min_physical_frame {
            return Err(PauseError(
                "render report precedes rebound physical frontier",
            ));
        }
        let physical = report
            .start_frame
            .checked_add(
                u64::try_from(report.frames).map_err(|_| PauseError("render extent overflow"))?,
            )
            .ok_or(PauseError("physical frame overflow"))?;
        let playback = report
            .playback_start_frame
            .checked_add(
                u64::try_from(report.playback_frames)
                    .map_err(|_| PauseError("playback extent overflow"))?,
            )
            .ok_or(PauseError("playback frame overflow"))?;
        let prefix = self.startup_prefix(report)?;
        let frames =
            u64::try_from(report.frames).map_err(|_| PauseError("render extent overflow"))?;
        let gap = self.report_gap(report)?;
        let startup_held = self.start_frame > 0
            && physical <= self.start_frame
            && report.start_frame <= self.start_frame;
        if report.playback_frames as u64 > frames - prefix
            || (!report.paused && report.playback_frames as u64 != frames - prefix)
            || (report.start_frame < self.start_frame && report.playback_start_frame != 0)
            || (startup_held && (playback != 0 || (frames > 0 && !report.paused)))
            || (!startup_held && gap < self.start_frame)
            || (self.start_configured
                && self.start_frame > 0
                && report.playback_start_frame == 0
                && report.playback_frames > 0
                && gap != self.start_frame)
            || (prefix > 0
                && prefix < frames
                && report.paused
                && report.playback_end_physical_frame.is_none())
        {
            return Err(PauseError("render pause extent is inconsistent"));
        }
        self.point(physical)?;
        self.point(playback)?;
        match (self.playback_end, report.playback_end_physical_frame) {
            (None, Some(_)) => {
                return Err(PauseError("endpoint evidence requires finite pause setup"));
            }
            (Some(expected), marker) => {
                if playback > expected {
                    return Err(PauseError("render playback passed its configured endpoint"));
                }
                if let Some(marker) = marker {
                    let manual_gap = marker
                        .checked_sub(expected)
                        .ok_or(PauseError("physical endpoint precedes playback endpoint"))?;
                    let prefix_end = report
                        .start_frame
                        .checked_add(prefix)
                        .and_then(|frame| frame.checked_add(report.playback_frames as u64))
                        .ok_or(PauseError("endpoint prefix overflow"))?;
                    if !report.paused
                        || playback != expected
                        || marker > physical
                        || manual_gap < self.gap
                        || (report.playback_frames > 0 && marker != prefix_end)
                        || (report.playback_frames == 0 && marker > report.start_frame + prefix)
                        || self.end_marker.is_some_and(|old| old != marker)
                    {
                        return Err(PauseError(
                            "inconsistent or changed physical endpoint evidence",
                        ));
                    }
                    self.point(marker)?;
                } else if self.end_marker.is_some()
                    || (report.frames > 0 && !startup_held && playback == expected)
                {
                    return Err(PauseError("reached endpoint lost its physical marker"));
                }
            }
            (None, None) => {}
        }
        if let Some(old) = self.last_report {
            let old_end = old
                .start_frame
                .checked_add(old.frames as u64)
                .ok_or(PauseError("previous physical extent overflow"))?;
            let old_play_end = old
                .playback_start_frame
                .checked_add(old.playback_frames as u64)
                .ok_or(PauseError("previous playback extent overflow"))?;
            if report.start_frame < old.start_frame
                || physical < old_end
                || playback < old_play_end
                || gap < self.report_gap(old)?
            {
                return Err(PauseError("render report grid regressed"));
            }
        }
        Ok((physical, playback, gap))
    }
    /// Returns the playback end cursor on the scheduling clock grid.
    pub fn scheduling_point(&self, report: RenderReport) -> Result<ClockPoint, PauseError> {
        let (_, playback, _) = self.check_report(report)?;
        self.point(playback)
    }
    pub(crate) fn resumed_presentation_point(&self) -> Result<ClockPoint, PauseError> {
        self.point(
            self.frozen
                .checked_add(self.gap)
                .ok_or(PauseError("resume presentation frame overflow"))?,
        )
    }
    /// Host clock domain retained across output replacement.
    pub const fn host_domain(&self) -> ClockDomainId {
        self.host
    }
    /// Offset a new presentation anchor while preserving cumulative gap rounding.
    /// Wide terms are combined before the one final timestamp narrowing.
    pub fn song_origin_for_presentation(
        &self,
        original: Timestamp,
        playback_origin: ClockPoint,
    ) -> Result<Timestamp, PauseError> {
        let startup = self.point(self.start_frame)?;
        if playback_origin.domain != self.origin.domain
            || playback_origin.timestamp < startup.timestamp
        {
            return Err(PauseError(
                "presentation origin precedes startup or changed domain",
            ));
        }
        let manual_gap = self
            .gap
            .checked_sub(self.start_frame)
            .ok_or(PauseError("manual pause gap precedes startup"))?;
        let gap = i128::from(manual_gap) * 1_000_000_000 / i128::from(self.rate);
        let shift = i128::from(playback_origin.timestamp.as_nanos())
            - i128::from(startup.timestamp.as_nanos());
        let value = i128::from(original.as_nanos())
            .checked_sub(gap)
            .and_then(|value| value.checked_add(shift))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(PauseError("presentation song origin overflow"))?;
        Ok(Timestamp::from_nanos(value))
    }
    /// Applies the cumulative gap once, avoiding per-pause rounding drift.
    pub fn song_origin_after_pause(&self, original: Timestamp) -> Result<Timestamp, PauseError> {
        let manual_gap = self
            .gap
            .checked_sub(self.start_frame)
            .ok_or(PauseError("manual pause gap precedes startup"))?;
        let gap = i128::from(manual_gap) * 1_000_000_000 / i128::from(self.rate);
        let value = i128::from(original.as_nanos())
            .checked_sub(gap)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(PauseError("paused song origin overflow"))?;
        Ok(Timestamp::from_nanos(value))
    }
    fn adopt_report(&mut self, report: RenderReport) -> Result<(), PauseError> {
        let (physical, playback, gap) = self.check_report(report)?;
        let startup_held = self.start_frame > 0
            && report.start_frame <= self.start_frame
            && physical <= self.start_frame;
        let terminal_marker = report.playback_end_physical_frame;
        if matches!(self.phase, PausePhase::Running | PausePhase::Pausing)
            && !report.paused
            && !startup_held
            && gap != self.gap
        {
            return Err(PauseError(
                "active render changed the acknowledged pause gap",
            ));
        }
        if self.phase == PausePhase::Paused
            && (playback != self.frozen
                || (report.playback_frames > 0 && self.last_report != Some(report)))
        {
            return Err(PauseError(
                "paused render changed the frozen playback frame",
            ));
        }
        match self.phase {
            PausePhase::Running if report.paused && terminal_marker.is_none() && !startup_held => {
                return Err(PauseError("unexpected paused render while running"));
            }
            PausePhase::Paused if !report.paused => {
                return Err(PauseError("unexpected active render while paused"));
            }
            PausePhase::Pausing
                if report.paused
                    && report.frames > 0
                    && !startup_held
                    && self.boundary.is_none() =>
            {
                if gap < self.gap {
                    return Err(PauseError("pause gap regressed"));
                }
                self.boundary = Some(PendingBoundary {
                    physical: if let Some(marker) = terminal_marker {
                        marker
                    } else {
                        playback
                            .checked_add(self.gap)
                            .ok_or(PauseError("pause boundary overflow"))?
                    },
                    playback,
                    gap: self.gap,
                });
            }
            PausePhase::Resuming
                if (!report.paused || terminal_marker.is_some())
                    && report.frames > 0
                    && self.boundary.is_none() =>
            {
                let gap = if let Some(marker) = terminal_marker {
                    marker
                        .checked_sub(
                            self.playback_end
                                .ok_or(PauseError("finite endpoint unavailable"))?,
                        )
                        .ok_or(PauseError("endpoint manual gap underflow"))?
                } else {
                    gap
                };
                if report.playback_start_frame < self.frozen || gap <= self.gap {
                    return Err(PauseError(
                        "resume report precedes the frozen playback frontier",
                    ));
                }
                self.boundary = Some(PendingBoundary {
                    physical: self
                        .frozen
                        .checked_add(gap)
                        .ok_or(PauseError("resume boundary overflow"))?,
                    playback: self.frozen,
                    gap,
                });
            }
            _ => {}
        }
        if terminal_marker.is_some() {
            self.end_marker = terminal_marker;
        }
        self.last_report = Some(report);
        Ok(())
    }
    fn commit_boundary(&mut self, boundary: PendingBoundary) -> bool {
        let paused = self.phase == PausePhase::Pausing;
        self.phase = if paused {
            self.frozen = boundary.playback;
            PausePhase::Paused
        } else {
            self.gap = boundary.gap;
            PausePhase::Running
        };
        self.boundary = None;
        self.reference = None;
        self.interval_reference = None;
        self.interval_window = None;
        paused
    }
    pub fn observe(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<PauseBoundary>, PauseError> {
        let mut next = self.clone();
        next.bind(EvidenceKind::Point)?;
        let result = next.observe_inner(report, pair)?;
        *self = next;
        Ok(result)
    }
    fn observe_inner(
        &mut self,
        report: Option<RenderReport>,
        pair: ClockPair,
    ) -> Result<Option<PauseBoundary>, PauseError> {
        self.check_pair(pair)?;
        if let Some(report) = report {
            self.adopt_report(report)?;
        }
        self.last_pair = Some(pair);
        let Some(boundary) = self.boundary else {
            return Ok(None);
        };
        let output = self.point(boundary.physical)?;
        let lower = self
            .reference
            .ok_or(PauseError("pause boundary lacks a native lower bracket"))?;
        if output.timestamp < lower.source.timestamp {
            return Err(PauseError(
                "pause boundary precedes its native lower bracket",
            ));
        }
        if pair.source.timestamp < output.timestamp {
            return Ok(None);
        }
        let source_delta = i128::from(pair.source.timestamp.as_nanos())
            - i128::from(lower.source.timestamp.as_nanos());
        let host_delta = i128::from(pair.target.timestamp.as_nanos())
            - i128::from(lower.target.timestamp.as_nanos());
        if source_delta <= 0 || host_delta <= 0 {
            return Err(PauseError(
                "native boundary interpolation requires progress in both clocks",
            ));
        }
        let offset =
            i128::from(output.timestamp.as_nanos()) - i128::from(lower.source.timestamp.as_nanos());
        let host = offset
            .checked_mul(host_delta)
            .map(|value| value / source_delta)
            .and_then(|value| value.checked_add(i128::from(lower.target.timestamp.as_nanos())))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(PauseError("native boundary interpolation overflow"))?;
        let paused = self.commit_boundary(boundary);
        Ok(Some(PauseBoundary {
            paused,
            host: ClockPoint {
                domain: self.host,
                timestamp: Timestamp::from_nanos(host),
            },
            playback_frame: boundary.playback,
        }))
    }
}

const MAX_CONTROLS: usize = 65536;
#[derive(Clone)]
struct Key {
    suppressed: bool,
    down: bool,
    event: ButtonEvent,
    ordinal: u64,
}
/// Tracks accepted button levels; unknown repeats never become new presses.
pub struct PauseKeyboard {
    keys: HashMap<(DeviceId, PhysicalControlId), Key>,
    domain: Option<ClockDomainId>,
    next: u64,
}
impl Default for PauseKeyboard {
    fn default() -> Self {
        Self::new()
    }
}
impl PauseKeyboard {
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            domain: None,
            next: 0,
        }
    }
    fn button(&self, event: &PhysicalInputEvent) -> Result<ButtonEvent, PauseError> {
        let PhysicalInputEvent::Button(button) = event else {
            return Err(PauseError("pause reconciliation supports only buttons"));
        };
        if self
            .domain
            .is_some_and(|domain| domain != button.meta.clock_domain)
        {
            return Err(PauseError("pause keyboard clock domain changed"));
        }
        Ok(button.clone())
    }
    fn insert(&mut self, button: &ButtonEvent, suppressed: bool) -> Result<(), PauseError> {
        if self.keys.len() >= MAX_CONTROLS {
            return Err(PauseError("pause keyboard control capacity exceeded"));
        }
        let next = self
            .next
            .checked_add(1)
            .ok_or(PauseError("pause keyboard ordinal exhausted"))?;
        self.keys
            .try_reserve(1)
            .map_err(|_| PauseError("pause keyboard allocation failed"))?;
        self.keys.insert(
            (button.meta.source, button.control),
            Key {
                suppressed,
                down: true,
                event: button.clone(),
                ordinal: self.next,
            },
        );
        self.next = next;
        Ok(())
    }
    pub fn accept(&mut self, event: &PhysicalInputEvent) -> Result<bool, PauseError> {
        let button = self.button(event)?;
        let id = (button.meta.source, button.control);
        let accepted = match self.keys.get(&id) {
            Some(key) if key.suppressed => {
                if button.state == ButtonState::Up {
                    self.keys.remove(&id);
                }
                false
            }
            Some(_) => {
                if button.state == ButtonState::Up {
                    self.keys.remove(&id);
                }
                true
            }
            None if button.state == ButtonState::Down => {
                self.insert(&button, false)?;
                true
            }
            // Preserve ordinary runtime handling of unpaired releases/repeats.
            // Neither event establishes a tracked held key.
            None => true,
        };
        self.domain = Some(button.meta.clock_domain);
        Ok(accepted)
    }
    pub fn observe_paused(&mut self, event: PhysicalInputEvent) -> Result<(), PauseError> {
        let button = self.button(&event)?;
        let id = (button.meta.source, button.control);
        if let Some(key) = self.keys.get_mut(&id) {
            if key.suppressed && button.state == ButtonState::Up {
                self.keys.remove(&id);
            } else {
                key.down = button.state != ButtonState::Up;
                key.event = button.clone();
            }
        } else if button.state == ButtonState::Down {
            self.insert(&button, true)?;
        }
        self.domain = Some(button.meta.clock_domain);
        Ok(())
    }
    /// Reconciles releases only. New paused presses remain suppressed until Up.
    pub fn resume(&mut self, at: ClockPoint) -> Result<Vec<PhysicalInputEvent>, PauseError> {
        if self.domain.is_some_and(|domain| domain != at.domain) {
            return Err(PauseError("pause keyboard resume clock domain changed"));
        }
        let mut releases = Vec::new();
        releases
            .try_reserve_exact(self.keys.len())
            .map_err(|_| PauseError("pause release allocation failed"))?;
        for (id, key) in &self.keys {
            if !key.suppressed && !key.down {
                let mut event = key.event.clone();
                if event.meta.timestamp > at.timestamp {
                    return Err(PauseError("paused release follows resume frontier"));
                }
                event.state = ButtonState::Up;
                event.meta.original_clock_point =
                    event.meta.original_clock_point.or(Some(ClockPoint {
                        domain: event.meta.clock_domain,
                        timestamp: event.meta.timestamp,
                    }));
                event.meta.clock_domain = at.domain;
                event.meta.timestamp = at.timestamp;
                releases.push((*id, key.ordinal, event));
            }
        }
        releases
            .sort_by_key(|(_, ordinal, event)| (event.meta.source, event.meta.sequence, *ordinal));
        let mut output = Vec::new();
        output
            .try_reserve_exact(releases.len())
            .map_err(|_| PauseError("pause release output allocation failed"))?;
        for (id, _, event) in releases {
            self.keys.remove(&id);
            output.push(PhysicalInputEvent::Button(event));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        audio::AudioCounters,
        input::{AxisEvent, AxisMode, BackendId, EventMeta, NativeEventMeta},
    };
    fn point(domain: u32, ns: i64) -> ClockPoint {
        ClockPoint {
            domain: ClockDomainId(domain),
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn pair(ns: i64) -> ClockPair {
        ClockPair {
            source: point(1, ns),
            target: point(2, ns + 10_000),
        }
    }
    fn report(physical: u64, playback: u64, frames: usize, paused: bool) -> RenderReport {
        RenderReport {
            start_frame: physical,
            frames,
            playback_start_frame: playback,
            playback_frames: if paused { 0 } else { frames },
            paused,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        }
    }
    #[test]
    fn straddling_pause_uses_prefix_end_and_repeated_cached_prefix_cannot_advance_frozen_time() {
        let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        pause.request(true, pair(0)).unwrap();
        let mut partial = report(0, 0, 10, true);
        partial.playback_frames = 4;
        assert_eq!(pause.observe(Some(partial), pair(3_000_000)).unwrap(), None);
        let boundary = pause.observe(None, pair(4_000_000)).unwrap().unwrap();
        assert_eq!(boundary.host, point(2, 4_010_000));
        assert_eq!(boundary.playback_frame, 4);
        assert_eq!(
            pause.scheduling_point(partial).unwrap(),
            point(1, 4_000_000)
        );
        assert_eq!(pause.observe(Some(partial), pair(9_000_000)).unwrap(), None);
        assert_eq!(
            pause
                .observe(Some(report(10, 4, 5, true)), pair(14_000_000))
                .unwrap(),
            None
        );
        let before = pause.clone();
        let mut advancing = report(15, 4, 2, true);
        advancing.playback_frames = 1;
        assert!(pause.observe(Some(advancing), pair(16_000_000)).is_err());
        assert_eq!(pause.last_render_report(), before.last_render_report());
        assert_eq!(pause.phase(), PausePhase::Paused);
        let mut rewound_prefix = report(15, 0, 4, true);
        rewound_prefix.playback_frames = 4;
        assert!(
            pause
                .observe(Some(rewound_prefix), pair(16_000_000))
                .is_err()
        );
        assert_eq!(pause.last_render_report(), before.last_render_report());
        let mut overlong = partial;
        overlong.playback_frames = 11;
        assert!(pause.scheduling_point(overlong).is_err());
        let mut incomplete_active = partial;
        incomplete_active.paused = false;
        assert!(pause.scheduling_point(incomplete_active).is_err());
    }
    #[test]
    fn coalesced_reports_wait_for_native_crossing_and_recover_first_boundaries() {
        let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        assert!(pause.request(true, pair(0)).unwrap());
        assert!(!pause.request(false, pair(0)).unwrap());
        assert_eq!(
            pause
                .observe(Some(report(12, 10, 4, true)), pair(8_000_000))
                .unwrap(),
            None
        );
        let boundary = pause.observe(None, pair(12_000_000)).unwrap().unwrap();
        assert_eq!(
            boundary,
            PauseBoundary {
                paused: true,
                host: point(2, 10_010_000),
                playback_frame: 10
            }
        );
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert!(pause.request(false, pair(14_000_000)).unwrap());
        let resumed = pause
            .observe(Some(report(20, 12, 2, false)), pair(20_000_000))
            .unwrap()
            .unwrap();
        assert_eq!(
            resumed,
            PauseBoundary {
                paused: false,
                host: point(2, 18_010_000),
                playback_frame: 10
            }
        );
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(
            pause.scheduling_point(report(20, 12, 2, false)).unwrap(),
            point(1, 14_000_000)
        );
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::from_nanos(-8_000_000)
        );
    }
    #[test]
    fn repeated_gap_rounding_is_cumulative_and_malformed_observations_are_atomic() {
        let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 3).unwrap();
        let mut reference = pair(0);
        for index in 0..3u64 {
            let play = index + 1;
            let physical = play + index;
            assert!(pause.request(true, reference).unwrap());
            let crossing = pair(pause.point(physical).unwrap().timestamp.as_nanos() + 10);
            pause
                .observe(Some(report(physical, play, 1, true)), crossing)
                .unwrap()
                .unwrap();
            assert!(pause.request(false, crossing).unwrap());
            reference = pair(pause.point(physical + 1).unwrap().timestamp.as_nanos() + 10);
            pause
                .observe(Some(report(physical + 1, play, 1, false)), reference)
                .unwrap()
                .unwrap();
        }
        assert_eq!(
            pause
                .song_origin_after_pause(Timestamp::ZERO)
                .unwrap()
                .as_nanos(),
            -1_000_000_000
        );
        let before = pause.clone();
        assert!(
            pause
                .observe(Some(report(9, 6, 1, true)), reference)
                .is_err()
        );
        assert_eq!(pause.phase(), before.phase());
        assert_eq!(pause.last_report, before.last_report);
        assert_eq!(pause.gap, before.gap);
        let wrong = ClockPair {
            source: point(9, 0),
            target: point(2, 0),
        };
        assert!(pause.request(true, wrong).is_err());
        assert_eq!(pause.phase(), PausePhase::Running);
        assert!(
            pause
                .scheduling_point(report(u64::MAX, u64::MAX, 1, false))
                .is_err()
        );
        assert!(NativePause::new(point(1, 0), ClockDomainId(1), 3).is_err());
        assert!(NativePause::new(point(1, 0), ClockDomainId(2), 1_000_000_001).is_err());
        let mut bracket = NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        bracket.request(true, pair(20_000_000)).unwrap();
        assert!(
            bracket
                .observe(Some(report(12, 10, 4, true)), pair(21_000_000))
                .is_err()
        );
        assert_eq!(bracket.phase(), PausePhase::Pausing);
        assert!(bracket.last_report.is_none());
        let mut stagnant = NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        stagnant.request(true, pair(0)).unwrap();
        assert!(
            stagnant
                .observe(Some(report(0, 0, 1, true)), pair(0))
                .is_err()
        );
    }
    fn finite_mixer(end: u64) -> (beatkernel::audio::CommandProducer, beatkernel::audio::Mixer) {
        mixer_queue(end, false)
    }
    fn mixer_queue(
        end: u64,
        gated: bool,
    ) -> (beatkernel::audio::CommandProducer, beatkernel::audio::Mixer) {
        use beatkernel::audio::*;
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = AudioLimits::new(8, 2, 8, 32, 8).unwrap();
        let pcm_limits = PcmLimits::new(4096, 8192, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 16], pcm_limits).unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = if gated {
            command_queue_with_start_gate(8)
        } else {
            command_queue(8)
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
        let config = MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits)
            .with_playback_end_frame(end);
        (producer, Mixer::new(config, bank, consumer).unwrap())
    }
    #[test]
    fn gated_start_hold_crossing_and_manual_pause_keep_logical_schedule() {
        let (mut producer, mut mixer) = mixer_queue(16, true);
        let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 1000)
            .unwrap()
            .with_playback_end_frame(16)
            .unwrap()
            .with_start_frame(4)
            .unwrap();
        pause.observe(None, pair(0)).unwrap();
        let empty = mixer.render(&mut []).unwrap();
        assert_eq!(pause.observe(Some(empty), pair(0)).unwrap(), None);
        let held = mixer.render(&mut [99.0; 2]).unwrap();
        assert!(held.paused);
        assert_eq!(pause.observe(Some(held), pair(1_000_000)).unwrap(), None);
        assert_eq!(pause.phase(), PausePhase::Running);
        producer.schedule_start_at(4).unwrap();
        let mut output = [99.0; 4];
        let active = mixer.render(&mut output).unwrap();
        assert_eq!(output, [0.0, 0.0, 0.25, 0.25]);
        assert!(!active.paused);
        assert_eq!(pause.observe(Some(active), pair(5_000_000)).unwrap(), None);
        assert_eq!(pause.scheduling_point(active).unwrap(), point(1, 2_000_000));
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::ZERO
        );
        let mut default = NativePause::new(point(1, 0), ClockDomainId(2), 1000)
            .unwrap()
            .with_playback_end_frame(16)
            .unwrap();
        assert!(default.observe(Some(active), pair(5_000_000)).is_err());
        assert!(default.last_report.is_none());
        pause.request(true, pair(5_000_000)).unwrap();
        producer.request_pause(true);
        let frozen = mixer.render(&mut [99.0; 2]).unwrap();
        assert_eq!(
            pause
                .observe(Some(frozen), pair(6_000_000))
                .unwrap()
                .unwrap()
                .playback_frame,
            2
        );
        pause.request(false, pair(7_000_000)).unwrap();
        producer.request_pause(false);
        let resumed = mixer.render(&mut [99.0; 2]).unwrap();
        let boundary = pause
            .observe(Some(resumed), pair(8_000_000))
            .unwrap()
            .unwrap();
        assert!(!boundary.paused);
        assert_eq!(boundary.host, point(2, 8_010_000));
        assert_eq!(boundary.playback_frame, 2);
        assert_eq!(
            pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
            Timestamp::from_nanos(-2_000_000)
        );
        assert_eq!(
            pause.scheduling_point(resumed).unwrap(),
            point(1, 4_000_000)
        );
    }
    #[test]
    fn gated_finite_first_prefix_and_zero_endpoint_preserve_strict_setup() {
        for end in [0, 3] {
            let (mut producer, mut mixer) = mixer_queue(end, true);
            let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 1000)
                .unwrap()
                .with_start_frame(4)
                .unwrap()
                .with_playback_end_frame(end)
                .unwrap();
            let held = mixer.render(&mut [99.0; 2]).unwrap();
            assert_eq!(pause.observe(Some(held), pair(0)).unwrap(), None);
            producer.schedule_start_at(4).unwrap();
            let crossed = mixer.render(&mut [99.0; 6]).unwrap();
            assert_eq!(crossed.playback_end_physical_frame, Some(4 + end));
            assert_eq!(pause.observe(Some(crossed), pair(4_000_000)).unwrap(), None);
            assert_eq!(pause.phase(), PausePhase::Running);
            let before = pause.clone();
            let mut malformed = crossed;
            malformed.playback_end_physical_frame = Some(3 + end);
            assert!(pause.observe(Some(malformed), pair(5_000_000)).is_err());
            assert_eq!(pause.last_report, before.last_report);
        }
        let fresh = || NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        assert!(fresh().with_start_frame(u64::MAX).is_err());
        assert!(
            fresh()
                .with_start_frame(0)
                .unwrap()
                .with_start_frame(0)
                .is_err()
        );
        let mut observed = fresh();
        observed.observe(None, pair(0)).unwrap();
        assert!(observed.with_start_frame(4).is_err());
        let mut requested = fresh();
        requested.request(false, pair(0)).unwrap();
        assert!(requested.with_start_frame(4).is_err());
        // At the exact target, an empty telemetry call still cannot open the gate.
        let (mut producer, mut mixer) = mixer_queue(0, true);
        producer.schedule_start_at(4).unwrap();
        let mut at_target = fresh()
            .with_start_frame(4)
            .unwrap()
            .with_playback_end_frame(0)
            .unwrap();
        let held = mixer.render(&mut [99.0; 4]).unwrap();
        at_target.observe(Some(held), pair(0)).unwrap();
        let empty = mixer.render(&mut []).unwrap();
        assert_eq!(
            at_target.observe(Some(empty), pair(1_000_000)).unwrap(),
            None
        );
        assert_eq!(at_target.phase(), PausePhase::Running);
        let finite = fresh().with_playback_end_frame(1).unwrap();
        assert!(finite.with_start_frame(u64::MAX).is_err());
    }
    #[test]
    fn finite_running_and_pending_pause_use_actual_retained_marker() {
        let (_producer, mut mixer) = finite_mixer(3);
        let mut output = [9.0; 5];
        let prefix = mixer.render(&mut output).unwrap();
        assert_eq!(output, [0.25, 0.25, 0.25, 0.0, 0.0]);
        assert_eq!(prefix.playback_end_physical_frame, Some(3));
        let latest = mixer.render(&mut output).unwrap();
        assert_eq!(latest.playback_frames, 0);
        let mut running = NativePause::new(point(1, 0), ClockDomainId(2), 1000)
            .unwrap()
            .with_playback_end_frame(3)
            .unwrap();
        assert_eq!(
            running.observe(Some(prefix), pair(2_000_000)).unwrap(),
            None
        );
        assert_eq!(running.phase(), PausePhase::Running);
        assert_eq!(
            running.observe(Some(latest), pair(6_000_000)).unwrap(),
            None
        );
        assert_eq!(running.phase(), PausePhase::Running);
        assert_eq!(
            running.scheduling_point(latest).unwrap(),
            point(1, 3_000_000)
        );
        let mut pending = NativePause::new(point(1, 0), ClockDomainId(2), 1000)
            .unwrap()
            .with_playback_end_frame(3)
            .unwrap();
        pending.request(true, pair(0)).unwrap();
        assert_eq!(
            pending.observe(Some(latest), pair(2_000_000)).unwrap(),
            None
        );
        let boundary = pending.observe(None, pair(4_000_000)).unwrap().unwrap();
        assert_eq!(
            boundary,
            PauseBoundary {
                paused: true,
                host: point(2, 3_010_000),
                playback_frame: 3
            }
        );
        assert_eq!(pending.phase(), PausePhase::Paused);
        assert_eq!(
            pending.observe(Some(latest), pair(5_000_000)).unwrap(),
            None
        );
        assert!(!pending.request(false, pair(5_000_000)).unwrap());
        assert_eq!(pending.phase(), PausePhase::Paused);
    }
    #[test]
    fn short_resume_reaching_end_recovers_manual_gap_from_prefix_or_coalesced_silence() {
        for coalesced in [false, true] {
            let (mut producer, mut mixer) = finite_mixer(3);
            let mut pause = NativePause::new(point(1, 0), ClockDomainId(2), 1000)
                .unwrap()
                .with_playback_end_frame(3)
                .unwrap();
            let active = mixer.render(&mut [0.0; 2]).unwrap();
            pause.observe(Some(active), pair(1_000_000)).unwrap();
            pause.request(true, pair(1_000_000)).unwrap();
            producer.request_pause(true);
            let paused = mixer.render(&mut [0.0; 3]).unwrap();
            assert_eq!(paused.playback_end_physical_frame, None);
            let boundary = pause
                .observe(Some(paused), pair(3_000_000))
                .unwrap()
                .unwrap();
            assert_eq!(boundary.playback_frame, 2);
            assert_eq!(boundary.host, point(2, 2_010_000));
            pause.request(false, pair(4_000_000)).unwrap();
            producer.request_pause(false);
            let mut output = [9.0; 4];
            let terminal_prefix = mixer.render(&mut output).unwrap();
            assert_eq!(output, [0.25, 0.0, 0.0, 0.0]);
            assert!(terminal_prefix.paused);
            assert_eq!(terminal_prefix.playback_frames, 1);
            assert_eq!(terminal_prefix.playback_end_physical_frame, Some(6));
            let latest = mixer.render(&mut output).unwrap();
            assert_eq!(latest.playback_end_physical_frame, Some(6));
            let evidence = if coalesced { latest } else { terminal_prefix };
            assert_eq!(
                pause.observe(Some(evidence), pair(4_500_000)).unwrap(),
                None
            );
            assert_eq!(pause.phase(), PausePhase::Resuming);
            let resumed = pause.observe(None, pair(7_000_000)).unwrap().unwrap();
            assert_eq!(
                resumed,
                PauseBoundary {
                    paused: false,
                    host: point(2, 5_010_000),
                    playback_frame: 2
                }
            );
            assert_eq!(pause.phase(), PausePhase::Running);
            assert_eq!(
                pause.song_origin_after_pause(Timestamp::ZERO).unwrap(),
                Timestamp::from_nanos(-3_000_000)
            );
            assert_eq!(pause.observe(Some(latest), pair(8_000_000)).unwrap(), None);
            assert_eq!(pause.scheduling_point(latest).unwrap(), point(1, 3_000_000));
            assert_eq!(pause.gap, 3); // Silent terminal suffix is never counted as manual pause.
        }
    }
    #[test]
    fn finite_marker_identity_setup_and_malformed_evidence_are_atomic() {
        let (_producer, mut mixer) = finite_mixer(3);
        let terminal = mixer.render(&mut [0.0; 5]).unwrap();
        let latest = mixer.render(&mut [0.0; 5]).unwrap();
        let mut unlimited = NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        assert!(unlimited.observe(Some(terminal), pair(4_000_000)).is_err());
        assert!(unlimited.last_render_report().is_none());
        assert_eq!(unlimited.phase(), PausePhase::Running);
        let fresh = || NativePause::new(point(1, 0), ClockDomainId(2), 1000).unwrap();
        assert!(fresh().with_playback_end_frame(u64::MAX).is_err());
        assert!(fresh().with_playback_end_frame(0).is_ok());
        assert!(
            fresh()
                .with_playback_end_frame(3)
                .unwrap()
                .with_playback_end_frame(3)
                .is_err()
        );
        let mut requested = fresh();
        assert!(!requested.request(false, pair(0)).unwrap());
        assert!(requested.with_playback_end_frame(3).is_err());
        let mut observed = fresh();
        observed.observe(None, pair(0)).unwrap();
        assert!(observed.with_playback_end_frame(3).is_err());
        let mut wrong_endpoint = fresh().with_playback_end_frame(4).unwrap();
        assert!(
            wrong_endpoint
                .observe(Some(terminal), pair(4_000_000))
                .is_err()
        );
        assert!(wrong_endpoint.last_render_report().is_none());
        let mut finite = fresh().with_playback_end_frame(3).unwrap();
        finite.observe(Some(terminal), pair(4_000_000)).unwrap();
        for malformed in [
            RenderReport {
                playback_end_physical_frame: Some(4),
                ..latest
            },
            RenderReport {
                playback_end_physical_frame: None,
                ..latest
            },
            RenderReport {
                paused: false,
                ..latest
            },
            RenderReport {
                playback_end_physical_frame: Some(11),
                ..latest
            },
        ] {
            let before = finite.clone();
            assert!(finite.observe(Some(malformed), pair(6_000_000)).is_err());
            assert_eq!(finite.last_report, before.last_report);
            assert_eq!(finite.last_pair, before.last_pair);
            assert_eq!(finite.end_marker, before.end_marker);
            assert_eq!(finite.phase(), before.phase());
        }
        assert_eq!(finite.observe(Some(latest), pair(6_000_000)).unwrap(), None);
    }
    fn button(
        device: u64,
        key: u16,
        state: ButtonState,
        time: i64,
        sequence: u64,
    ) -> PhysicalInputEvent {
        let mut meta = EventMeta::new(DeviceId(device), point(2, time), sequence);
        meta.native = Some(NativeEventMeta {
            backend: BackendId(7),
            code: Some(key as u32),
            timestamp: Some(point(9, time - 1)),
        });
        PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(key),
            state,
        })
    }
    #[test]
    fn held_release_reconciliation_preserves_native_provenance_and_suppresses_new_keys() {
        let mut keys = PauseKeyboard::new();
        assert!(keys.accept(&button(2, 4, ButtonState::Down, 1, 1)).unwrap());
        assert!(keys.accept(&button(1, 5, ButtonState::Down, 1, 1)).unwrap());
        let released = button(2, 4, ButtonState::Up, 4, 4);
        keys.observe_paused(released.clone()).unwrap();
        keys.observe_paused(button(1, 5, ButtonState::Up, 3, 3))
            .unwrap();
        keys.observe_paused(button(3, 6, ButtonState::Down, 5, 5))
            .unwrap();
        keys.observe_paused(button(4, 7, ButtonState::Repeat, 6, 6))
            .unwrap();
        let reconciled = keys.resume(point(2, 10)).unwrap();
        assert_eq!(reconciled.len(), 2);
        assert_eq!(reconciled[0].meta().source, DeviceId(1));
        assert_eq!(reconciled[1].meta().source, DeviceId(2));
        assert_eq!(reconciled[1].meta().timestamp, Timestamp::from_nanos(10));
        assert_eq!(reconciled[1].meta().native, released.meta().native);
        assert_eq!(reconciled[1].meta().original_clock_point, Some(point(2, 4)));
        assert!(
            !keys
                .accept(&button(3, 6, ButtonState::Repeat, 11, 6))
                .unwrap()
        );
        assert!(!keys.accept(&button(3, 6, ButtonState::Up, 12, 7)).unwrap());
        assert!(
            keys.accept(&button(3, 6, ButtonState::Down, 13, 8))
                .unwrap()
        );
        assert!(
            keys.accept(&button(4, 7, ButtonState::Repeat, 14, 9))
                .unwrap()
        );
        assert!(keys.resume(point(2, 15)).unwrap().is_empty());
    }
    #[test]
    fn held_levels_restore_without_new_presses_and_bad_resume_is_atomic() {
        let mut keys = PauseKeyboard::new();
        keys.accept(&button(1, 4, ButtonState::Down, 1, 1)).unwrap();
        keys.observe_paused(button(1, 4, ButtonState::Up, 2, 2))
            .unwrap();
        keys.observe_paused(button(1, 4, ButtonState::Repeat, 3, 3))
            .unwrap();
        assert!(keys.resume(point(2, 4)).unwrap().is_empty());
        let mut up = button(1, 4, ButtonState::Up, 5, 5);
        up.meta_mut().original_clock_point = Some(point(8, 50));
        keys.observe_paused(up).unwrap();
        assert!(keys.resume(point(1, 10)).is_err());
        assert!(keys.resume(point(2, 4)).is_err());
        let nonbutton = PhysicalInputEvent::Axis(AxisEvent {
            meta: EventMeta::new(DeviceId(1), point(2, 10), 6),
            control: PhysicalControlId::keyboard(4),
            value: 0.0,
            mode: AxisMode::Absolute,
        });
        assert!(keys.accept(&nonbutton).is_err());
        let up = keys.resume(point(2, 10)).unwrap();
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].meta().original_clock_point, Some(point(8, 50)));
        assert!(keys.accept(&button(1, 4, ButtonState::Up, 11, 6)).unwrap());
    }
}

#[cfg(test)]
#[path = "playback_pause_interval_fixtures.rs"]
mod interval_fixtures;

#[cfg(test)]
#[path = "playback_pause_rebind_fixtures.rs"]
mod playback_pause_rebind_fixtures;

#[cfg(test)]
#[path = "playback_presentation_origin_fixtures.rs"]
mod playback_presentation_origin_fixtures;
