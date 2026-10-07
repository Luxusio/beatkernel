//! Freeze actual portable live completion without retaining gameplay or native owners.
use crate::{
    competition_presentation::CompetitionSnapshot,
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    step_gameplay::{StepGameplay, StepLocalGameplay},
    ui::results::{ResultsView, ResultDetails},
};

#[derive(Default)]
pub struct CompletedResultsPresentation {
    results: Option<Vec<(PlayerId, CompletedPlayResult)>>,
    view: Option<ResultsView>,
    error: Option<String>,
}
impl CompletedResultsPresentation {
    pub fn capture_solo(
        &mut self,
        game: &StepGameplay,
        competition: Option<&CompetitionSnapshot>,
    ) -> Result<bool, String> {
        if self.results.is_some() {
            return Ok(true);
        }
        let Some(result) = game.completed_result().copied() else {
            return Ok(false);
        };
        let player = PlayerId(1);
        self.freeze(
            vec![(player, result)],
            &[player],
            &[ResultDetails {
                player,
                score: game.score(),
                competition,
            }],
        )
    }
    pub fn capture_local(
        &mut self,
        game: &StepLocalGameplay,
        comparisons: &[(PlayerId, Option<&CompetitionSnapshot>)],
    ) -> Result<bool, String> {
        if self.results.is_some() {
            return Ok(true);
        }
        let mut results = Vec::new();
        results
            .try_reserve_exact(game.players().len())
            .map_err(|error| error.to_string())?;
        for player in game.players() {
            let Some(result) = game
                .completed_result(*player)
                .map_err(|error| error.to_string())?
                .copied()
            else {
                return Ok(false);
            };
            results.push((*player, result));
        }
        let mut details = Vec::new();
        if let Err(error) = details.try_reserve_exact(results.len()) {
            let error = error.to_string();
            self.results = Some(results);
            self.error = Some(error.clone());
            return Err(error);
        }
        if !comparisons.is_empty()
            && (comparisons.len() != results.len()
                || comparisons.iter().enumerate().any(|(index, (player, _))| {
                    !game.players().contains(player)
                        || comparisons[..index]
                            .iter()
                            .any(|(previous, _)| previous == player)
                }))
        {
            self.results = Some(results);
            let error = "completed comparison table differs from the actual roster".to_string();
            self.error = Some(error.clone());
            return Err(error);
        }
        for player in game.players() {
            details.push(ResultDetails {
                player: *player,
                score: game
                    .score(*player)
                    .ok_or("completed member score missing")?,
                competition: comparisons
                    .iter()
                    .find(|(id, _)| id == player)
                    .and_then(|(_, snapshot)| *snapshot),
            });
        }
        self.freeze(results, game.players(), &details)
    }
    fn freeze(
        &mut self,
        results: Vec<(PlayerId, CompletedPlayResult)>,
        roster: &[PlayerId],
        details: &[ResultDetails<'_>],
    ) -> Result<bool, String> {
        // Historical proof remains available even if its display projection refuses data.
        let view = ResultsView::new_with_details(&results, roster, details);
        self.results = Some(results);
        match view {
            Ok(view) => {
                self.view = Some(view);
                Ok(true)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    /// Export captured display values without exporting a live completion constructor.
    pub fn export_visual(&self) -> Result<Option<crate::ui::results::FrozenResultsModel>, String> {
        self.view.as_ref().map(ResultsView::export_visual).transpose()
    }
    pub fn results(&self) -> Option<&[(PlayerId, CompletedPlayResult)]> {
        self.results.as_deref()
    }
    pub fn view(&self) -> Option<&ResultsView> {
        self.view.as_ref()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
#[path = "completed_results_presentation_fixtures.rs"]
mod fixtures;
