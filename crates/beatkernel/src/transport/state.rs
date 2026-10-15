use super::{OutputPhaseBasis, OutputPhaseError, OutputPhasePoint, PositiveRatePhase, Rate};
use crate::time::{ClockPoint, Duration, Timestamp};
use std::fmt;

/// The beginning of one host-to-song mapping segment.
///
/// For host times in this segment, mapping is
/// `song_time + trunc((host - host_time) * rate)`, using wide integer
/// intermediates. The rightmost anchor wins when host timestamps are equal.
/// With opt-in preroll timing these fields remain integer snapshots, but the
/// mapping retains the unrounded whole preroll phase internally instead of
/// adding independently rounded elapsed time to this snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportAnchor {
    /// Start of the segment in one normalized monotonic host domain.
    pub host_time: Timestamp,
    /// Integer song position at the start of the segment.
    pub song_time: Timestamp,
    /// Playback rate active during the segment.
    pub rate: Rate,
}

/// An invalid transport query or command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    /// Opted output phase requires an explicit validated phase point.
    OutputPhaseRequired,
    /// A queried host time precedes the initial anchor.
    BeforeOrigin,
    /// A command precedes the last successful command's host time.
    NonMonotonicHost,
    /// The mapped song timestamp is outside the signed nanosecond range.
    Overflow,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputPhaseRequired => f.write_str("transport requires an output phase point"),
            Self::BeforeOrigin => f.write_str("host time precedes transport origin"),
            Self::NonMonotonicHost => f.write_str("command host time moved backward"),
            Self::Overflow => f.write_str("song position is outside the timestamp range"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Immutable original timing contract of an exact preroll transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrerollTransportSetup {
    /// Logical output timestamp at which active output elapsed is zero.
    pub logical_origin: Timestamp,
    /// Original song position reached when output preroll has elapsed.
    pub original_start: Timestamp,
    /// Nonnegative output duration before the original song start.
    pub output_preroll: Duration,
    /// Positive normalized rate applied to the whole signed preroll phase.
    pub positive_rate: Rate,
    /// Optional original-song upper cap, without audio completion authority.
    pub original_end: Option<Timestamp>,
}

/// Invalid cold exact-preroll setup, separate from legacy transport errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTransportSetupError {
    /// The output preroll duration was negative.
    NegativePreroll,
    /// The selected rate was zero or reverse.
    NonPositiveRate,
    /// The original finite end preceded the original start.
    InvalidEnd,
    /// The initial signed preroll mapping did not fit a song timestamp.
    Overflow,
}

impl fmt::Display for LiveTransportSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NegativePreroll => "output preroll must be nonnegative",
            Self::NonPositiveRate => "live playback rate must be positive",
            Self::InvalidEnd => "original end precedes original start",
            Self::Overflow => "initial song position is outside the timestamp range",
        })
    }
}

impl std::error::Error for LiveTransportSetupError {}

/// Invalid exact acknowledged pause/resume control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTransportControlError {
    /// Opted output phase requires an explicit validated phase control.
    OutputPhaseRequired,
    /// The current segment has no exact phase, including after a legacy reset.
    MissingPrerollSetup,
    /// A duplicate exact pause supplied a different frozen elapsed time.
    InvalidState,
    /// The logical cutoff preceded the last successful command timestamp.
    NonMonotonicHost,
    /// The supplied active output elapsed time was negative.
    NegativeElapsed,
    /// The supplied elapsed time preceded the latest successful command's phase.
    RegressingElapsed,
    /// The final song timestamp or required active elapsed time overflowed.
    Overflow,
}

impl fmt::Display for LiveTransportControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OutputPhaseRequired => "transport requires an output phase control",
            Self::MissingPrerollSetup => "transport has no active exact preroll phase",
            Self::InvalidState => "exact pause control differs from the frozen elapsed time",
            Self::NonMonotonicHost => "command host time moved backward",
            Self::NegativeElapsed => "active output elapsed time must be nonnegative",
            Self::RegressingElapsed => "active output elapsed time moved backward",
            Self::Overflow => "song position or active elapsed time overflow",
        })
    }
}

impl std::error::Error for LiveTransportControlError {}

// Optional exact metadata is indexed by the authoritative anchor history.
// Acknowledgements bound elapsed values; phases add no separate chronology.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputPhaseHistory {
    basis: OutputPhaseBasis,
    // Successful acknowledgements, including no-ops, retain their phase bounds.
    controls: Vec<(Timestamp, i128)>,
    // Present only for runtime-rate opt-in, indexed by authoritative anchors.
    runtime_phases: Option<Vec<PositiveRatePhase>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrerollHistory {
    setup: PrerollTransportSetup,
    active_elapsed: Vec<Option<i128>>,
}

impl PrerollTransportSetup {
    fn song_at_elapsed(self, active_elapsed: i128) -> Result<Timestamp, TransportError> {
        let elapsed = active_elapsed - i128::from(self.output_preroll.as_nanos());
        let song = self
            .positive_rate
            .checked_scale_wide(elapsed)
            .and_then(|scaled| scaled.checked_add(i128::from(self.original_start.as_nanos())));
        let song = match (song, self.original_end) {
            (Some(song), end) => end.map_or(song, |end| song.min(i128::from(end.as_nanos()))),
            // A positive wide overflow exceeds every possible finite end.
            (None, Some(end)) if elapsed >= 0 => return Ok(end),
            (None, _) => return Err(TransportError::Overflow),
        };
        i64::try_from(song)
            .map(Timestamp::from_nanos)
            .map_err(|_| TransportError::Overflow)
    }
}

/// An ordered history of host-to-song transport segments.
///
/// Host times must come from one normalized monotonic domain. Commands may
/// share a timestamp but cannot precede the last successful command, even
/// when that command was a no-op. Read-only queries do not advance chronology.
/// Errors leave all transport state intact. Negative song positions are valid.
///
/// Position lookup is allocation-free and O(log n). Actual changes append
/// anchors in amortized O(1) time and retain O(n) history for late input events.
/// Construction and unreserved mutation may allocate and belong on a control
/// thread, outside an audio callback. Opted output-phase controls can reserve
/// all required history capacity before application.
///
/// ```
/// use beatkernel::{time::Timestamp, transport::{Rate, Transport}};
/// let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
/// transport.set_rate(Timestamp::from_nanos(10), Rate::REVERSE)?;
/// assert_eq!(transport.position_at(Timestamp::from_nanos(25))?,
///            Timestamp::from_nanos(-5));
/// // Historical timestamps still use the earlier forward segment.
/// assert_eq!(transport.position_at(Timestamp::from_nanos(5))?,
///            Timestamp::from_nanos(5));
/// # Ok::<(), beatkernel::transport::TransportError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transport {
    anchors: Vec<TransportAnchor>,
    resume_rate: Rate,
    last_command_host: Timestamp,
    preroll: Option<PrerollHistory>,
    output_phase: Option<OutputPhaseHistory>,
}

impl Transport {
    /// Creates one initial anchor and initializes command chronology.
    ///
    /// If `rate` is zero, the first resume uses [`Rate::NORMAL`]. Otherwise
    /// pause/resume will remember this rate, including negative playback.
    pub fn new(host_time: Timestamp, song_time: Timestamp, rate: Rate) -> Self {
        Self {
            anchors: vec![TransportAnchor {
                host_time,
                song_time,
                rate,
            }],
            resume_rate: if rate == Rate::ZERO {
                Rate::NORMAL
            } else {
                rate
            },
            last_command_host: host_time,
            preroll: None,
            output_phase: None,
        }
    }

    /// Creates exact forward timing `start + trunc((E - preroll) * rate)`.
    ///
    /// Preroll is an output duration; E initially equals zero. A validated
    /// finite end caps positive overflow before narrowing, but does not prove
    /// audio completion. Exact pause/resume retains this signed phase.
    pub fn new_with_preroll(
        logical_origin: Timestamp,
        original_start: Timestamp,
        output_preroll: Duration,
        positive_rate: Rate,
        original_end: Option<Timestamp>,
    ) -> Result<Self, LiveTransportSetupError> {
        if output_preroll.as_nanos() < 0 {
            return Err(LiveTransportSetupError::NegativePreroll);
        }
        if positive_rate.numerator() <= 0 {
            return Err(LiveTransportSetupError::NonPositiveRate);
        }
        if original_end.is_some_and(|end| end < original_start) {
            return Err(LiveTransportSetupError::InvalidEnd);
        }
        let setup = PrerollTransportSetup {
            logical_origin,
            original_start,
            output_preroll,
            positive_rate,
            original_end,
        };
        let song_time = setup
            .song_at_elapsed(0)
            .map_err(|_| LiveTransportSetupError::Overflow)?;
        let mut transport = Self::new(logical_origin, song_time, positive_rate);
        transport.preroll = Some(PrerollHistory {
            setup,
            active_elapsed: vec![Some(0)],
        });
        Ok(transport)
    }

    /// Opts into relative active-source phase; the origin is unchanged chronology.
    /// Construction is mathematical validation, not native admission evidence.
    pub fn new_with_output_phase(
        basis: OutputPhaseBasis,
        logical_origin: ClockPoint,
        original_start: Timestamp,
        output_preroll: Duration,
        positive_rate: Rate,
        original_end: Option<Timestamp>,
    ) -> Result<Self, OutputPhaseError> {
        if logical_origin.domain != basis.domain() {
            return Err(OutputPhaseError::WrongDomain);
        }
        let mut transport = Self::new_with_preroll(
            logical_origin.timestamp,
            original_start,
            output_preroll,
            positive_rate,
            original_end,
        )
        .map_err(OutputPhaseError::Setup)?;
        transport.output_phase = Some(OutputPhaseHistory {
            basis,
            controls: vec![(logical_origin.timestamp, 0)],
            runtime_phases: None,
        });
        Ok(transport)
    }

    /// Opts into exact same-attempt positive rate changes at actual output cuts.
    ///
    /// The original setup remains immutable. This mathematical constructor does
    /// not establish presentation, native admission or clock-domain authority.
    pub fn new_with_runtime_output_phase(
        basis: OutputPhaseBasis,
        logical_origin: ClockPoint,
        original_start: Timestamp,
        output_preroll: Duration,
        positive_rate: Rate,
        original_end: Option<Timestamp>,
    ) -> Result<Self, OutputPhaseError> {
        let mut transport = Self::new_with_output_phase(
            basis,
            logical_origin,
            original_start,
            output_preroll,
            positive_rate,
            original_end,
        )?;
        let phase =
            PositiveRatePhase::new(original_start, output_preroll, positive_rate, original_end)
                .map_err(OutputPhaseError::RatePhase)?;
        transport
            .output_phase
            .as_mut()
            .expect("output phase constructor retains history")
            .runtime_phases = Some(vec![phase]);
        Ok(transport)
    }

    /// Returns the current positive runtime rate, including while paused.
    /// Fixed-only and scalar transports return `None`.
    pub fn runtime_output_rate(&self) -> Option<Rate> {
        self.output_phase
            .as_ref()?
            .runtime_phases
            .as_ref()?
            .last()
            .map(|phase| phase.rate())
    }

    /// Reserves history for additional output-phase controls on a cold thread.
    ///
    /// Subsequent controls within the reserved count do not allocate. Each
    /// control can append at most one anchor, elapsed value, exact phase and
    /// acknowledgement; unchanged controls need no new anchor or phase.
    pub fn try_reserve_output_phase_controls(
        &mut self,
        additional: usize,
    ) -> Result<(), OutputPhaseError> {
        self.reserve_output_phase_history(additional, additional)
    }

    /// Bound immutable attempt identity, independent of rounded song snapshots.
    pub fn output_phase_basis(&self) -> Option<OutputPhaseBasis> {
        self.output_phase.as_ref().map(|phase| phase.basis)
    }

    /// Allocation-free historical query with one whole signed preroll phase.
    pub fn position_at_output_phase(
        &self,
        point: OutputPhasePoint,
    ) -> Result<Timestamp, OutputPhaseError> {
        let phase = self
            .output_phase
            .as_ref()
            .ok_or(OutputPhaseError::MissingPhase)?;
        if point.logical().domain != phase.basis.domain() {
            return Err(OutputPhaseError::WrongDomain);
        }
        if point.basis() != phase.basis {
            return Err(OutputPhaseError::WrongBasis);
        }
        let mut end = self
            .anchors
            .partition_point(|anchor| anchor.host_time <= point.logical().timestamp);
        if end == 0 {
            return Err(OutputPhaseError::BeforeOrigin);
        }
        let history = self
            .preroll
            .as_ref()
            .expect("phase constructor retains preroll history");
        let elapsed = point.elapsed_nanos();
        // Several actual cuts can share a rounded logical nanosecond.
        // Within that timestamp the exact active elapsed value selects the
        // segment, so late input before a cut keeps its previous phase.
        let equal_start = self.anchors[..end]
            .partition_point(|anchor| anchor.host_time < point.logical().timestamp);
        end = equal_start
            + history.active_elapsed[equal_start..end].partition_point(|base| {
                base.expect("phase controls retain exact elapsed") <= elapsed
            });
        if end == 0 {
            return Err(OutputPhaseError::BeforeOrigin);
        }
        let base = history.active_elapsed[end - 1].expect("phase controls retain exact elapsed");
        let control_end = phase
            .controls
            .partition_point(|control| *control <= (point.logical().timestamp, elapsed));
        // The initial acknowledgement precedes every admitted query. Future
        // acknowledgements constrain historical queries as well as no-op bounds.
        let lower = phase.controls[control_end - 1].1;
        let upper = phase.controls.get(control_end).map(|(_, elapsed)| *elapsed);
        if elapsed < base || elapsed < lower || upper.is_some_and(|upper| elapsed > upper) {
            return Err(OutputPhaseError::RegressingElapsed);
        }
        if self.anchors[end - 1].rate == Rate::ZERO && elapsed != base {
            return Err(OutputPhaseError::InvalidState);
        }
        if let Some(runtime_phases) = &phase.runtime_phases {
            return runtime_phases[end - 1]
                .song_at_elapsed(elapsed)
                .map_err(OutputPhaseError::RatePhase);
        }
        history
            .setup
            .song_at_elapsed(elapsed)
            .map_err(OutputPhaseError::Transport)
    }

    /// Changes an opted positive runtime rate at an actual qualified active cut.
    ///
    /// The same attempt and fractional phase survive. A same-rate control only
    /// advances control chronology; it does not add or reanchor a phase segment.
    /// Paused output cannot establish an active application cut.
    pub fn set_rate_at_output_phase(
        &mut self,
        point: OutputPhasePoint,
        rate: Rate,
    ) -> Result<(), OutputPhaseError> {
        let current_phase = self
            .output_phase
            .as_ref()
            .and_then(|history| history.runtime_phases.as_ref())
            .and_then(|phases| phases.last())
            .copied()
            .ok_or(OutputPhaseError::RuntimeRateRequired)?;
        let song_time = self.validate_output_phase_control(point)?;
        if self.is_paused() {
            return Err(OutputPhaseError::InvalidState);
        }
        let candidate = current_phase
            .with_rate_at_elapsed(point.elapsed_nanos(), rate)
            .map_err(OutputPhaseError::RatePhase)?;
        let changed = candidate.rate() != current_phase.rate();
        let acknowledgement = (point.logical().timestamp, point.elapsed_nanos());
        let append_control = self
            .output_phase
            .as_ref()
            .expect("validated runtime phase")
            .controls
            .last()
            .copied()
            != Some(acknowledgement);
        self.reserve_output_phase_history(usize::from(changed), usize::from(append_control))?;
        if changed {
            self.anchors.push(TransportAnchor {
                host_time: point.logical().timestamp,
                song_time,
                rate,
            });
            self.preroll
                .as_mut()
                .expect("validated runtime phase")
                .active_elapsed
                .push(Some(point.elapsed_nanos()));
            self.output_phase
                .as_mut()
                .expect("validated runtime phase")
                .runtime_phases
                .as_mut()
                .expect("runtime opt-in retains phases")
                .push(candidate);
            self.resume_rate = rate;
        }
        self.last_command_host = point.logical().timestamp;
        if append_control {
            self.output_phase
                .as_mut()
                .expect("validated runtime phase")
                .controls
                .push(acknowledgement);
        }
        Ok(())
    }

    /// Freezes at an actual caller-qualified active phase; no timestamp subtraction.
    pub fn pause_at_output_phase(
        &mut self,
        point: OutputPhasePoint,
    ) -> Result<(), OutputPhaseError> {
        self.control_output_phase(point, true)
    }

    /// Resumes at the acknowledged frozen phase, retaining the original basis.
    pub fn resume_at_output_phase(
        &mut self,
        point: OutputPhasePoint,
    ) -> Result<(), OutputPhaseError> {
        self.control_output_phase(point, false)
    }

    fn control_output_phase(
        &mut self,
        point: OutputPhasePoint,
        paused: bool,
    ) -> Result<(), OutputPhaseError> {
        let song_time = self.validate_output_phase_control(point)?;
        let runtime_phase = self
            .output_phase
            .as_ref()
            .expect("validated phase")
            .runtime_phases
            .as_ref()
            .and_then(|phases| phases.last())
            .copied();
        let rate = if paused {
            Rate::ZERO
        } else {
            runtime_phase.map_or_else(
                || {
                    self.preroll
                        .as_ref()
                        .expect("validated phase")
                        .setup
                        .positive_rate
                },
                |phase| phase.rate(),
            )
        };
        let changed = self.anchor().rate != rate;
        let acknowledgement = (point.logical().timestamp, point.elapsed_nanos());
        let append_control = self
            .output_phase
            .as_ref()
            .expect("validated phase")
            .controls
            .last()
            .copied()
            != Some(acknowledgement);
        self.reserve_output_phase_history(usize::from(changed), usize::from(append_control))?;
        if changed {
            self.anchors.push(TransportAnchor {
                host_time: point.logical().timestamp,
                song_time,
                rate,
            });
            self.preroll
                .as_mut()
                .expect("validated phase")
                .active_elapsed
                .push(Some(point.elapsed_nanos()));
            if let Some(runtime_phase) = runtime_phase {
                self.output_phase
                    .as_mut()
                    .expect("validated phase")
                    .runtime_phases
                    .as_mut()
                    .expect("runtime opt-in retains phases")
                    .push(runtime_phase);
            }
        }
        self.last_command_host = point.logical().timestamp;
        if append_control {
            self.output_phase
                .as_mut()
                .expect("validated phase")
                .controls
                .push(acknowledgement);
        }
        Ok(())
    }

    fn validate_output_phase_control(
        &self,
        point: OutputPhasePoint,
    ) -> Result<Timestamp, OutputPhaseError> {
        if point.logical().timestamp < self.last_command_host {
            return Err(OutputPhaseError::Transport(
                TransportError::NonMonotonicHost,
            ));
        }
        let song_time = self.position_at_output_phase(point)?;
        let history = self.output_phase.as_ref().expect("validated phase");
        if point.elapsed_nanos() < history.controls.last().expect("phase has an origin").1 {
            return Err(OutputPhaseError::RegressingElapsed);
        }
        Ok(song_time)
    }

    fn reserve_output_phase_history(
        &mut self,
        anchors: usize,
        controls: usize,
    ) -> Result<(), OutputPhaseError> {
        let history = self
            .output_phase
            .as_mut()
            .ok_or(OutputPhaseError::MissingPhase)?;
        self.anchors
            .try_reserve(anchors)
            .map_err(|_| OutputPhaseError::AllocationFailed)?;
        self.preroll
            .as_mut()
            .expect("output phase retains preroll history")
            .active_elapsed
            .try_reserve(anchors)
            .map_err(|_| OutputPhaseError::AllocationFailed)?;
        history
            .controls
            .try_reserve(controls)
            .map_err(|_| OutputPhaseError::AllocationFailed)?;
        if let Some(phases) = &mut history.runtime_phases {
            phases
                .try_reserve(anchors)
                .map_err(|_| OutputPhaseError::AllocationFailed)?;
        }
        Ok(())
    }

    /// Returns the original cold setup while the current segment retains it.
    /// Explicit seeks and real legacy rate changes discard the current exact
    /// phase; historical segments retain their original mapping.
    pub fn preroll_setup(&self) -> Option<PrerollTransportSetup> {
        self.preroll.as_ref().and_then(|history| {
            history
                .active_elapsed
                .last()
                .copied()
                .flatten()
                .map(|_| history.setup)
        })
    }

    /// Returns the latest anchor in O(1) time.
    pub fn anchor(&self) -> TransportAnchor {
        self.anchors[self.anchors.len() - 1]
    }

    /// Borrows the complete history, in nondecreasing host-time order.
    pub fn anchors(&self) -> &[TransportAnchor] {
        &self.anchors
    }

    /// Returns whether the active rate is zero.
    pub fn is_paused(&self) -> bool {
        self.anchor().rate == Rate::ZERO
    }

    /// Maps a host timestamp through its historical segment in O(log n).
    ///
    /// Equal-time anchors resolve to the last appended anchor. Host
    /// subtraction, rate scaling, and song addition use `i128`; only the
    /// final song timestamp must fit `i64`. Division truncates toward zero.
    pub fn position_at(&self, host_time: Timestamp) -> Result<Timestamp, TransportError> {
        if self.output_phase.is_some() {
            return Err(TransportError::OutputPhaseRequired);
        }
        let end = self
            .anchors
            .partition_point(|anchor| anchor.host_time <= host_time);
        if end == 0 {
            return Err(TransportError::BeforeOrigin);
        }
        let anchor = self.anchors[end - 1];
        if let Some(history) = &self.preroll {
            if let Some(active_elapsed) = history.active_elapsed[end - 1] {
                let elapsed = if anchor.rate == Rate::ZERO {
                    Some(active_elapsed)
                } else {
                    active_elapsed.checked_add(
                        i128::from(host_time.as_nanos()) - i128::from(anchor.host_time.as_nanos()),
                    )
                };
                return match elapsed {
                    Some(elapsed) => history.setup.song_at_elapsed(elapsed),
                    None => history.setup.original_end.ok_or(TransportError::Overflow),
                };
            }
        }
        let elapsed = i128::from(host_time.as_nanos()) - i128::from(anchor.host_time.as_nanos());
        let song = i128::from(anchor.song_time.as_nanos()) + anchor.rate.scale_wide(elapsed);
        i64::try_from(song)
            .map(Timestamp::from_nanos)
            .map_err(|_| TransportError::Overflow)
    }

    /// Changes rate continuously at the supplied host timestamp.
    ///
    /// Zero pauses and preserves the last nonzero resume rate. A nonzero
    /// rate starts immediately and becomes the remembered resume rate.
    /// Setting the same rate validates the current mapping and advances
    /// command chronology without adding an anchor or rounding it again.
    /// A real change quantizes the old mapped position to a new integer anchor.
    pub fn set_rate(&mut self, host_time: Timestamp, rate: Rate) -> Result<(), TransportError> {
        self.validate_command_host(host_time)?;
        let song_time = self.position_at(host_time)?;
        if self.anchor().rate != rate {
            self.anchors.push(TransportAnchor {
                host_time,
                song_time,
                rate,
            });
            if let Some(history) = &mut self.preroll {
                history.active_elapsed.push(None);
            }
            if rate != Rate::ZERO {
                self.resume_rate = rate;
            }
        }
        self.last_command_host = host_time;
        Ok(())
    }

    /// Pauses while remembering the last nonzero rate.
    ///
    /// Repeated pause is a validated no-op that advances command chronology
    /// without appending an anchor. Errors leave all state intact.
    pub fn pause(&mut self, host_time: Timestamp) -> Result<(), TransportError> {
        self.set_rate(host_time, Rate::ZERO)
    }

    /// Resumes the remembered nonzero rate, including reverse playback.
    ///
    /// Resume while already playing is a validated no-op that advances
    /// chronology without reanchoring. An initially paused transport resumes
    /// at normal speed. Errors leave all state intact.
    pub fn resume(&mut self, host_time: Timestamp) -> Result<(), TransportError> {
        let rate = if self.is_paused() {
            self.resume_rate
        } else {
            self.anchor().rate
        };
        self.set_rate(host_time, rate)
    }

    /// Appends a new song position while retaining active and resume rates.
    ///
    /// This intentionally does not query the old mapping: seek can recover
    /// from an overflowing trajectory. Seeking while paused keeps it paused.
    /// Equal-time seeks take precedence over earlier anchors at that time.
    /// Errors leave all state intact.
    pub fn seek(
        &mut self,
        host_time: Timestamp,
        song_time: Timestamp,
    ) -> Result<(), TransportError> {
        self.validate_command_host(host_time)?;
        self.anchors.push(TransportAnchor {
            host_time,
            song_time,
            rate: self.anchor().rate,
        });
        if let Some(history) = &mut self.preroll {
            history.active_elapsed.push(None);
        }
        self.last_command_host = host_time;
        Ok(())
    }

    /// Freezes the exact phase at an acknowledged active output elapsed time.
    ///
    /// The supplied E can lag the logical cutoff, but cannot precede the
    /// active elapsed time at the latest successful command. The caller owns
    /// verification of the physical output acknowledgement and clock domain.
    /// Duplicate pauses require the identical frozen E. Refusals are atomic.
    pub fn pause_at_output_elapsed(
        &mut self,
        logical_cutoff: Timestamp,
        active_elapsed_ns: i128,
    ) -> Result<(), LiveTransportControlError> {
        self.validate_exact_host(logical_cutoff)?;
        if active_elapsed_ns < 0 {
            return Err(LiveTransportControlError::NegativeElapsed);
        }
        let history = self
            .preroll
            .as_ref()
            .filter(|_| self.preroll_setup().is_some())
            .ok_or(LiveTransportControlError::MissingPrerollSetup)?;
        let base = history
            .active_elapsed
            .last()
            .copied()
            .flatten()
            .expect("active exact phase has an elapsed value");
        if self.is_paused() {
            if active_elapsed_ns != base {
                return Err(LiveTransportControlError::InvalidState);
            }
        } else {
            let last_elapsed = base
                .checked_add(
                    i128::from(self.last_command_host.as_nanos())
                        - i128::from(self.anchor().host_time.as_nanos()),
                )
                .ok_or(LiveTransportControlError::Overflow)?;
            if active_elapsed_ns < last_elapsed {
                return Err(LiveTransportControlError::RegressingElapsed);
            }
        }
        let song_time = history
            .setup
            .song_at_elapsed(active_elapsed_ns)
            .map_err(|_| LiveTransportControlError::Overflow)?;
        if !self.is_paused() {
            self.anchors.push(TransportAnchor {
                host_time: logical_cutoff,
                song_time,
                rate: Rate::ZERO,
            });
            self.preroll
                .as_mut()
                .expect("validated exact phase")
                .active_elapsed
                .push(Some(active_elapsed_ns));
        }
        self.last_command_host = logical_cutoff;
        Ok(())
    }

    /// Resumes the same exact preroll phase after an acknowledged output gap.
    /// Running resume is a validated no-op and does not re-round the phase.
    pub fn resume_preroll(
        &mut self,
        logical_cutoff: Timestamp,
    ) -> Result<(), LiveTransportControlError> {
        self.validate_exact_host(logical_cutoff)?;
        let setup = self
            .preroll_setup()
            .ok_or(LiveTransportControlError::MissingPrerollSetup)?;
        let song_time = self
            .position_at(logical_cutoff)
            .map_err(|_| LiveTransportControlError::Overflow)?;
        if self.is_paused() {
            let active_elapsed = self
                .preroll
                .as_ref()
                .expect("validated exact phase")
                .active_elapsed
                .last()
                .copied()
                .flatten()
                .expect("active exact phase has an elapsed value");
            self.anchors.push(TransportAnchor {
                host_time: logical_cutoff,
                song_time,
                rate: setup.positive_rate,
            });
            self.preroll
                .as_mut()
                .expect("validated exact phase")
                .active_elapsed
                .push(Some(active_elapsed));
        }
        self.last_command_host = logical_cutoff;
        Ok(())
    }

    fn validate_exact_host(&self, host_time: Timestamp) -> Result<(), LiveTransportControlError> {
        if self.output_phase.is_some() {
            return Err(LiveTransportControlError::OutputPhaseRequired);
        }
        self.validate_command_host(host_time)
            .map_err(|_| LiveTransportControlError::NonMonotonicHost)
    }

    fn validate_command_host(&self, host_time: Timestamp) -> Result<(), TransportError> {
        if self.output_phase.is_some() {
            return Err(TransportError::OutputPhaseRequired);
        }
        if host_time < self.last_command_host {
            Err(TransportError::NonMonotonicHost)
        } else {
            Ok(())
        }
    }
}
