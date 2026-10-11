use super::Rate;
use crate::time::{Duration, Timestamp};
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

// Optional metadata is indexed by the existing authoritative anchor history.
// It holds no duplicate host/song/rate or independent command chronology.
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
/// Construction and mutation may allocate and belong on a control thread,
/// outside an audio callback.
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
        self.validate_command_host(host_time)
            .map_err(|_| LiveTransportControlError::NonMonotonicHost)
    }

    fn validate_command_host(&self, host_time: Timestamp) -> Result<(), TransportError> {
        if host_time < self.last_command_host {
            Err(TransportError::NonMonotonicHost)
        } else {
            Ok(())
        }
    }
}
