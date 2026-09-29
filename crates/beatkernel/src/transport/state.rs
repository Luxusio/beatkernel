use super::Rate;
use crate::time::Timestamp;
use std::fmt;

/// The beginning of one host-to-song mapping segment.
///
/// For host times in this segment, mapping is
/// `song_time + trunc((host - host_time) * rate)`, using wide integer
/// intermediates. The rightmost anchor wins when host timestamps are equal.
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
        }
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
        self.last_command_host = host_time;
        Ok(())
    }

    fn validate_command_host(&self, host_time: Timestamp) -> Result<(), TransportError> {
        if host_time < self.last_command_host {
            Err(TransportError::NonMonotonicHost)
        } else {
            Ok(())
        }
    }
}
