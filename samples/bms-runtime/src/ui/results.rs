//! Retained final results from explicit immutable completion evidence.
use super::atoms::text;
use crate::{
    gauge::{GaugeFailure, GAUGE_UNITS_PER_PERCENT},
    local_players::PlayerId,
    play_result::{CompletedPlayResult, PlayResultOutcome, PlayResultScope},
    scene::{GeometrySnapshot, Scene},
};

pub const PLAYERS_PER_PAGE: usize = 4;

pub struct ResultRow {
    pub player: PlayerId,
    pub result: CompletedPlayResult,
    pub identity_label: String,
    pub outcome_label: String,
    pub gauge_label: String,
}

/// All labels and page packets are staged once before a view is accepted.
pub struct ResultsView {
    rows: Vec<ResultRow>,
    scope_label: String,
    pages: Vec<GeometrySnapshot>,
}
impl ResultsView {
    pub fn new(
        results: &[(PlayerId, CompletedPlayResult)],
        roster: &[PlayerId],
    ) -> Result<Self, String> {
        if !(1..=64).contains(&roster.len()) || results.len() != roster.len() {
            return Err("completed Results require the entire 1..64 player roster".into());
        }
        let scope = results[0].1.scope();
        for (index, player) in roster.iter().enumerate() {
            if player.0 == 0 || roster[..index].contains(player) {
                return Err("completed Results require unique original player identities".into());
            }
        }
        for (index, (player, result)) in results.iter().enumerate() {
            if !roster.contains(player)
                || results[..index]
                    .iter()
                    .any(|(previous, _)| previous == player)
                || result.scope() != scope
            {
                return Err(
                    "completed Results contain foreign, duplicate or mixed-scope rows".into(),
                );
            }
        }
        let scope_label = match scope {
            PlayResultScope::FullSong => "WHOLE SONG".into(),
            PlayResultScope::PracticeSection { start, end } => match end {
                Some(end) => format!(
                    "PRACTICE START {} NS END {} NS",
                    start.as_nanos(),
                    end.as_nanos()
                ),
                None => format!("PRACTICE START {} NS END UNBOUNDED", start.as_nanos()),
            },
        };
        let mut rows = Vec::new();
        rows.try_reserve_exact(roster.len())
            .map_err(|error| error.to_string())?;
        for player in roster {
            let result = results
                .iter()
                .find(|(id, _)| id == player)
                .ok_or("completed Results omitted a registered player")?
                .1;
            let outcome_label = match result.outcome() {
                PlayResultOutcome::Cleared if matches!(scope, PlayResultScope::FullSong) => {
                    "CLEARED"
                }
                PlayResultOutcome::Cleared => "PRACTICE - CLEAR THRESHOLD MET",
                PlayResultOutcome::BelowClearThreshold => "BELOW CLEAR THRESHOLD",
                PlayResultOutcome::Failed(GaugeFailure::InstantDeath) => "FAILED - INSTANT DEATH",
                PlayResultOutcome::Failed(GaugeFailure::Depleted) => "FAILED - DEPLETED",
            }
            .into();
            let level = result.gauge().level_units;
            rows.push(ResultRow {
                player: *player,
                result,
                identity_label: format!("PLAYER {}", player.0),
                outcome_label,
                gauge_label: format!(
                    "GAUGE {}.{:06}%",
                    level / GAUGE_UNITS_PER_PERCENT,
                    level % GAUGE_UNITS_PER_PERCENT
                ),
            });
        }
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(rows.len().div_ceil(PLAYERS_PER_PAGE))
            .map_err(|error| error.to_string())?;
        for (page, visible) in rows.chunks(PLAYERS_PER_PAGE).enumerate() {
            let mut scene = Scene::with_capacity(960, 720, 512);
            text(&mut scene, 24, 100, &scope_label, 1, 0x9bb1cf);
            for (index, row) in visible.iter().enumerate() {
                let y = 140 + index * 110;
                text(&mut scene, 24, y, &row.identity_label, 2, 0xf0f4ff);
                let color = match row.result.outcome() {
                    PlayResultOutcome::Cleared => 0x74e5c5,
                    PlayResultOutcome::BelowClearThreshold => 0xd8b36b,
                    PlayResultOutcome::Failed(_) => 0xff8e8e,
                };
                text(&mut scene, 24, y + 28, &row.outcome_label, 2, color);
                text(&mut scene, 24, y + 55, &row.gauge_label, 2, 0x9bb1cf);
            }
            text(
                &mut scene,
                24,
                600,
                &format!(
                    "LOCAL RESULTS PAGE {}/{} - PGUP/PGDN",
                    page + 1,
                    rows.len().div_ceil(PLAYERS_PER_PAGE)
                ),
                1,
                0x9bb1cf,
            );
            pages.push(scene.geometry_snapshot()?);
        }
        Ok(Self {
            rows,
            scope_label,
            pages,
        })
    }
    pub fn rows(&self) -> &[ResultRow] {
        &self.rows
    }
    pub fn scope_label(&self) -> &str {
        &self.scope_label
    }
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    /// Append the retained page without rebuilding labels, notes or gauge classification.
    pub fn compose(&self, scene: &mut Scene, page: usize) -> Result<(), String> {
        let packet = self
            .pages
            .get(page)
            .ok_or("completed Results page is out of range")?;
        scene.append_geometry(packet)
    }
}

#[cfg(test)]
#[path = "results_fixtures.rs"]
mod fixtures;
