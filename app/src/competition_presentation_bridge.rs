//! Native display clock and player UI effects selected at compatibility edges.
use crate::{
    competition_presentation::{
        CompetitionPresentationHost, CompetitionSnapshot, GhostSnapshot, NetworkSnapshot,
        PresentationResult,
    },
    local_players::PlayerId,
    player,
};
use std::{sync::OnceLock, time::Instant};

pub(crate) struct NativeCompetitionPresentation;
impl CompetitionPresentationHost for NativeCompetitionPresentation {
    fn attached(&self) -> bool {
        player::attached()
    }
    fn now_ns(&mut self) -> PresentationResult<u64> {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        Ok(u64::try_from(
            ORIGIN.get_or_init(Instant::now).elapsed().as_nanos(),
        )?)
    }
    fn publish_saved(
        &mut self,
        player: PlayerId,
        ghosts: Vec<GhostSnapshot>,
    ) -> PresentationResult<()> {
        player::publish_saved_competition(player, ghosts)
    }
    fn publish_solo(
        &mut self,
        player: PlayerId,
        snapshot: CompetitionSnapshot,
    ) -> PresentationResult<()> {
        player::publish_competition(player, snapshot)
    }
    fn publish_group(&mut self, rows: &[(PlayerId, NetworkSnapshot)]) -> PresentationResult<()> {
        player::publish_networks(rows)
    }
}
