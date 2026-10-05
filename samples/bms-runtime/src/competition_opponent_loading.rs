//! Saved-opponent admission through an opaque, statically dispatched loader.

use crate::competition::{Competition, CompetitionError, OpponentKind};
use beatkernel::replay::codec::{ReplayCodecLimits, ReplayFile};
use beatkernel_bms::BmsChart;

pub struct LoadedOpponent {
    pub file: ReplayFile,
    pub label: String,
}

pub trait OpponentReplayPort {
    type Key: ?Sized;
    type Error;
    fn load(
        &mut self,
        key: &Self::Key,
        limits: ReplayCodecLimits,
    ) -> Result<LoadedOpponent, Self::Error>;
}

pub struct OpponentRequest<'a, K: ?Sized> {
    pub kind: OpponentKind,
    pub key: &'a K,
}

pub enum OpponentLoadError<E> {
    Load(E),
    Competition(CompetitionError),
}

/// Reject the whole requested count before resource acquisition. Later refusals
/// retain earlier accepted opponents; each individual admission stays atomic.
pub fn load_opponents<P: OpponentReplayPort>(
    port: &mut P,
    source: &BmsChart,
    competition: &mut Competition,
    requests: &[OpponentRequest<'_, P::Key>],
    limits: ReplayCodecLimits,
) -> Result<(), OpponentLoadError<P::Error>> {
    if requests.len() > competition.remaining_opponent_capacity() {
        return Err(OpponentLoadError::Competition(
            CompetitionError::TooManyOpponents,
        ));
    }
    for request in requests {
        let loaded = port
            .load(request.key, limits)
            .map_err(OpponentLoadError::Load)?;
        competition
            .add_replay(source, loaded.file, limits, request.kind, loaded.label)
            .map_err(OpponentLoadError::Competition)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "competition_opponent_loading_fixtures.rs"]
mod fixtures;
