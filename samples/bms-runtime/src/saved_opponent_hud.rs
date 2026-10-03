//! Retained presentation of actual saved prefixes; never advances a comparison.
use crate::{
    player::{CompetitionSnapshot, GhostSnapshot},
    saved_opponents::SavedOpponents,
};

/// Bounded Worker/control-side state shared with the existing competition view.
#[derive(Default)]
pub struct SavedOpponentHud {
    snapshot: Option<CompetitionSnapshot>,
    failed: bool,
}

impl SavedOpponentHud {
    /// Copy actual summaries without changing their song frontier or judging.
    /// Failure hides the old snapshot and permanently disables this HUD owner.
    pub fn update(&mut self, opponents: &SavedOpponents) -> Result<(), String> {
        if self.failed {
            return Err("saved opponent presentation is disabled".into());
        }
        let result = (|| {
            if opponents.count() > 8 {
                return Err("saved opponent presentation exceeds eight records".into());
            }
            let mut ghosts = Vec::new();
            ghosts
                .try_reserve_exact(opponents.count())
                .map_err(|_| "allocate saved opponent presentation".to_string())?;
            for opponent in opponents.opponents() {
                let label = opponent.label();
                let score = opponent.score();
                if label.is_empty()
                    || label.len() > 256
                    || label.chars().any(char::is_control)
                    || score.combo > score.max_combo
                    || score.max_combo > score.hits
                {
                    return Err("invalid saved opponent presentation".into());
                }
                let mut retained_label = String::new();
                retained_label
                    .try_reserve_exact(label.len())
                    .map_err(|_| "allocate saved opponent label".to_string())?;
                retained_label.push_str(label);
                ghosts.push(GhostSnapshot {
                    kind: opponent.kind(),
                    label: retained_label,
                    hits: score.hits,
                    misses: score.misses,
                    combo: score.combo,
                    max_combo: score.max_combo,
                    recorded_until: opponent.recorded_until(),
                });
            }
            Ok((!ghosts.is_empty()).then_some(CompetitionSnapshot {
                ghosts,
                network: None,
            }))
        })();
        match result {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                Ok(())
            }
            Err(error) => {
                self.mark_failed();
                Err(error)
            }
        }
    }

    pub fn snapshot(&self) -> Option<&CompetitionSnapshot> {
        self.snapshot.as_ref()
    }

    pub const fn failed(&self) -> bool {
        self.failed
    }

    /// Comparison failure does not change local scoring, audio or capture.
    pub fn mark_failed(&mut self) {
        self.snapshot = None;
        self.failed = true;
    }
}
