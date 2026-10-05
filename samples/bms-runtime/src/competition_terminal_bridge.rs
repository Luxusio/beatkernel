//! Native ownership copying and endpoint cleanup at the terminal boundary.

use crate::{
    competition_terminal::CompetitionTerminalPort,
    multiplayer::{MultiplayerError, MultiplayerEvent, MultiplayerNotice},
    multiplayer_group::MemberProgress,
    native_competition_network::NativeCompetitionNetwork,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) struct NativeTerminalPort<'a> {
    network: &'a mut NativeCompetitionNetwork,
}

impl<'a> NativeTerminalPort<'a> {
    pub(crate) fn new(network: &'a mut NativeCompetitionNetwork) -> Self {
        Self { network }
    }
}

impl CompetitionTerminalPort for NativeTerminalPort<'_> {
    type Error = Box<dyn std::error::Error>;

    fn deliver(&mut self, members: &[MemberProgress]) -> Result<()> {
        let mut owned = Vec::new();
        owned.try_reserve_exact(members.len())?;
        owned.extend_from_slice(members);
        Ok(self.network.finish_delivery(owned)?)
    }

    fn cleanup(&mut self) -> Result<()> {
        Ok(self.network.stop()?)
    }

    fn drain(&mut self) -> Result<()> {
        if self.network.is_room() {
            return Ok(());
        }
        let mut first = None;
        for notice in self.network.poll() {
            if let MultiplayerNotice::Session(MultiplayerEvent::Disconnected(error)) = notice {
                if !matches!(error, MultiplayerError::Closed) && first.is_none() {
                    first = Some(error);
                }
            }
        }
        match first {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }
}
