//! Native saved-record acquisition and display-path formatting.

use crate::{
    competition_opponent_loading::{LoadedOpponent, OpponentReplayPort},
    replay_playback::read_replay,
};
use beatkernel::replay::codec::ReplayCodecLimits;
use std::{fs::File, path::Path};

pub(crate) struct NativeOpponentReplayPort;

impl OpponentReplayPort for NativeOpponentReplayPort {
    type Key = Path;
    type Error = Box<dyn std::error::Error>;

    fn load(
        &mut self,
        key: &Path,
        limits: ReplayCodecLimits,
    ) -> Result<LoadedOpponent, Self::Error> {
        let file = read_replay(&mut File::open(key)?, limits)?;
        Ok(LoadedOpponent {
            file,
            label: key.display().to_string(),
        })
    }
}
