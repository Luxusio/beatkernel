//! Cold fixed-speed setup shared by live gameplay and output scheduling.

use crate::{
    replay_audio::{output_elapsed, output_time, ReplayAudioError},
    replay_speed::{ReplaySpeed, ReplaySpeedError},
};
use beatkernel::{
    time::{ClockPoint, Duration, Timestamp},
    transport::{LiveTransportSetupError, Transport},
};
use std::fmt;

/// Invalid live timing setup or an unrepresentable output schedule.
#[derive(Debug)]
pub enum LiveSpeedError {
    InvalidConfiguration(&'static str),
    Overflow,
    Speed(ReplaySpeedError),
    Transport(LiveTransportSetupError),
    Audio(ReplayAudioError),
}

impl fmt::Display for LiveSpeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(reason) => f.write_str(reason),
            Self::Overflow => f.write_str("live playback output timestamp overflow"),
            Self::Speed(error) => fmt::Display::fmt(error, f),
            Self::Transport(error) => fmt::Display::fmt(error, f),
            Self::Audio(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for LiveSpeedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Speed(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::Audio(error) => Some(error),
            _ => None,
        }
    }
}

/// Validated original-song coordinates and physical output preroll.
///
/// This immutable value owns no clock, output device or playback state. Sample
/// rates and input acquisition times retain their physical units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveSpeedSetup {
    start: Timestamp,
    preroll: Duration,
    speed: ReplaySpeed,
    end: Option<Timestamp>,
}

impl LiveSpeedSetup {
    pub fn new(
        start: Timestamp,
        preroll: Duration,
        speed: ReplaySpeed,
        end: Option<Timestamp>,
    ) -> Result<Self, LiveSpeedError> {
        if end.is_some_and(|end| end < start) {
            return Err(LiveSpeedError::InvalidConfiguration(
                "original live playback end precedes start",
            ));
        }
        // Validate the whole signed preroll phase before any output is created.
        speed
            .song_at_elapsed(start, preroll, 0, end)
            .map_err(LiveSpeedError::Speed)?;
        Ok(Self {
            start,
            preroll,
            speed,
            end,
        })
    }

    pub const fn start(self) -> Timestamp {
        self.start
    }

    pub const fn preroll(self) -> Duration {
        self.preroll
    }

    pub const fn speed(self) -> ReplaySpeed {
        self.speed
    }

    pub const fn end(self) -> Option<Timestamp> {
        self.end
    }

    /// Integer snapshot at active output elapsed zero, without a second clock.
    pub fn song_origin(self) -> Timestamp {
        // The i64 preroll and positive i64 numerator product fit i128.
        // Construction proves the final snapshot fits i64; the fields are
        // private and immutable. The end cannot be below this initial point.
        let rate = self.speed.rate();
        let preroll_song = i128::from(self.preroll.as_nanos()) * i128::from(rate.numerator())
            / i128::from(rate.denominator());
        Timestamp::from_nanos((i128::from(self.start.as_nanos()) - preroll_song) as i64)
    }

    /// Creates the one exact-phase transport in its actual logical domain.
    pub fn transport(self, logical_origin: Timestamp) -> Result<Transport, LiveSpeedError> {
        Transport::new_with_preroll(
            logical_origin,
            self.start,
            self.preroll,
            self.speed.rate(),
            self.end,
        )
        .map_err(LiveSpeedError::Transport)
    }

    /// Exclusive endpoint on a fresh physical output grid, rounded up once.
    /// A numeric fence supplies no actual execution or presentation authority.
    pub fn playback_end_frame(self, sample_rate: u32) -> Result<Option<u64>, LiveSpeedError> {
        if sample_rate == 0 || sample_rate > 1_000_000_000 {
            return Err(LiveSpeedError::InvalidConfiguration(
                "live playback requires a representable nonzero output frame grid",
            ));
        }
        let Some(end) = self.end else {
            return Ok(None);
        };
        let nanos = output_elapsed(
            i128::from(end.as_nanos()),
            self.start,
            self.preroll,
            self.speed.rate(),
        )
        .map_err(LiveSpeedError::Audio)?;
        if nanos > i128::from(i64::MAX) {
            return Err(LiveSpeedError::Overflow);
        }
        let scaled = nanos
            .checked_mul(i128::from(sample_rate))
            .ok_or(LiveSpeedError::Overflow)?;
        let frames = scaled / 1_000_000_000 + i128::from(scaled % 1_000_000_000 != 0);
        let frame_nanos = frames
            .checked_mul(1_000_000_000)
            .ok_or(LiveSpeedError::Overflow)?;
        let rounded_nanos = frame_nanos / i128::from(sample_rate)
            + i128::from(frame_nanos % i128::from(sample_rate) != 0);
        if rounded_nanos > i128::from(i64::MAX) {
            return Err(LiveSpeedError::Overflow);
        }
        Ok(Some(
            u64::try_from(frames).map_err(|_| LiveSpeedError::Overflow)?,
        ))
    }

    /// Maps an original sound point to the actual output domain once.
    /// Points beyond a finite end remain uncapped; the feeder owns admission.
    pub fn output_at(
        self,
        output_origin: ClockPoint,
        song: Timestamp,
    ) -> Result<ClockPoint, LiveSpeedError> {
        if song < self.start {
            return Err(LiveSpeedError::InvalidConfiguration(
                "live sound precedes original section start",
            ));
        }
        let timestamp = output_time(
            i128::from(song.as_nanos()),
            self.start,
            output_origin,
            self.preroll,
            self.speed.rate(),
        )
        .map_err(LiveSpeedError::Audio)?;
        Ok(ClockPoint {
            domain: output_origin.domain,
            timestamp,
        })
    }
}
