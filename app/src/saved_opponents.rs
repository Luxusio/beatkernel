//! Bounded saved-record comparisons, independent of live scoring and platform I/O.
use crate::competition::{Competition, CompetitionError, GhostOpponent, OpponentKind};
use beatkernel::{
    replay::{
        ReplayHeader,
        codec::{decode_replay, ReplayCodecError, ReplayCodecLimits},
    },
    time::Timestamp,
};
use beatkernel_bms::BmsChart;
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum SavedOpponentsError {
    InvalidLimits,
    InvalidLabel,
    Capacity,
    ByteLimit,
    Codec(ReplayCodecError),
    Competition(CompetitionError),
}
impl fmt::Display for SavedOpponentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => {
                f.write_str("saved opponents require 1..8 slots and 1..67108864 encoded bytes")
            }
            Self::InvalidLabel => {
                f.write_str("saved opponent label requires 1..256 UTF-8 bytes without controls")
            }
            Self::Capacity => f.write_str("saved opponent capacity reached"),
            Self::ByteLimit => f.write_str("saved opponent encoded-byte quota exceeded"),
            Self::Codec(error) => write!(f, "saved opponent replay: {error}"),
            Self::Competition(error) => write!(f, "saved opponent comparison: {error}"),
        }
    }
}
impl Error for SavedOpponentsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Codec(error) => Some(error),
            Self::Competition(error) => Some(error),
            _ => None,
        }
    }
}

/// Retains validated recorded prefixes; it never observes local judgment events.
/// Own/Other is caller-selected display metadata, not authenticated identity.
pub struct SavedOpponents {
    competition: Competition,
    limits: ReplayCodecLimits,
    max_opponents: usize,
    max_encoded_bytes: usize,
    encoded_bytes: usize,
}
impl SavedOpponents {
    pub fn new(
        header: ReplayHeader,
        limits: ReplayCodecLimits,
        max_opponents: usize,
        max_encoded_bytes: usize,
    ) -> Result<Self, SavedOpponentsError> {
        if !(1..=8).contains(&max_opponents) || !(1..=64 * 1024 * 1024).contains(&max_encoded_bytes)
        {
            return Err(SavedOpponentsError::InvalidLimits);
        }
        let competition =
            Competition::new(header, max_opponents).map_err(SavedOpponentsError::Competition)?;
        Ok(Self {
            competition,
            limits,
            max_opponents,
            max_encoded_bytes,
            encoded_bytes: 0,
        })
    }

    /// Preflight label, membership and aggregate bytes before canonical decode.
    /// Reconstruction and admission use the original source and actual judge.
    /// Every refusal preserves earlier opponents, their display and charged bytes.
    pub fn add(
        &mut self,
        source: &BmsChart,
        encoded: &[u8],
        kind: OpponentKind,
        label: &str,
    ) -> Result<usize, SavedOpponentsError> {
        if label.is_empty() || label.len() > 256 || label.chars().any(char::is_control) {
            return Err(SavedOpponentsError::InvalidLabel);
        }
        if self.count() >= self.max_opponents {
            return Err(SavedOpponentsError::Capacity);
        }
        let charged = self
            .encoded_bytes
            .checked_add(encoded.len())
            .filter(|bytes| *bytes <= self.max_encoded_bytes)
            .ok_or(SavedOpponentsError::ByteLimit)?;
        let file = decode_replay(encoded, self.limits).map_err(SavedOpponentsError::Codec)?;
        let index = self
            .competition
            .add_replay(source, file, self.limits, kind, label)
            .map_err(SavedOpponentsError::Competition)?;
        self.encoded_bytes = charged;
        Ok(index)
    }

    /// Advance only saved recorded operations through an actual local song frontier.
    /// The shared Competition performs atomic regression and score validation.
    pub fn advance_to(&mut self, song_time: Timestamp) -> Result<(), SavedOpponentsError> {
        self.competition
            .observe(&[], song_time)
            .map_err(SavedOpponentsError::Competition)
    }

    /// Return displays to pristine prefixes, retaining recordings and byte charges.
    pub fn reset(&mut self) {
        self.competition.reset();
    }
    pub fn opponents(&self) -> &[GhostOpponent] {
        self.competition.opponents()
    }
    pub fn count(&self) -> usize {
        self.opponents().len()
    }
    pub const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }
    pub fn song_time(&self) -> Option<Timestamp> {
        self.competition.song_time()
    }
    pub fn expected_header(&self) -> &ReplayHeader {
        self.competition.expected_header()
    }
}
