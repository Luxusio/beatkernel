//! Retained saved and reported peer prefixes; never advances a comparison.
use crate::{
    multiplayer_protocol::{Progress, validate_progress},
    player::{CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus},
    saved_opponents::SavedOpponents,
};

/// Bounded Worker/control-side state shared with the existing competition view.
#[derive(Default)]
pub struct SavedOpponentHud {
    snapshot: Option<CompetitionSnapshot>,
    failed: bool,
    peer_failed: bool,
}

impl SavedOpponentHud {
    /// Copy actual summaries without changing their song frontier or judging.
    /// Failure hides saved rows and disables their updates, preserving the peer.
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
            Ok(ghosts)
        })();
        match result {
            Ok(ghosts) => {
                let network = self
                    .snapshot
                    .as_mut()
                    .and_then(|snapshot| snapshot.network.take());
                self.snapshot = (!ghosts.is_empty() || network.is_some())
                    .then_some(CompetitionSnapshot { ghosts, network });
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
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.ghosts.clear();
            if snapshot.network.is_none() {
                self.snapshot = None;
            }
        }
        self.failed = true;
    }

    /// Admit an actual reported peer prefix through the common protocol checks.
    /// Empty words change status only; invalid updates preserve the old display.
    /// Status is Waiting=0, Connected=1, Disconnected=2 or Stopped=3 and cannot
    /// regress. Ten words are low/high halves of song i64, hits, misses, combo
    /// and max-combo u64 values. Waiting does not admit a progress payload.
    pub fn update_peer(&mut self, status: u32, words: &[u32]) -> Result<(), String> {
        if self.peer_failed {
            return Err("peer presentation is disabled".into());
        }
        let next_status = match status {
            0 => NetworkStatus::Waiting,
            1 => NetworkStatus::Connected,
            2 => NetworkStatus::Disconnected,
            3 => NetworkStatus::Stopped,
            _ => return Err("invalid peer presentation status".into()),
        };
        if !words.is_empty() && (words.len() != 10 || status == 0) {
            return Err("peer progress requires ten words and a non-waiting status".into());
        }
        let previous = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.network.as_ref());
        if let Some(previous) = previous {
            let old_status = match previous.status {
                NetworkStatus::Waiting => 0,
                NetworkStatus::Connected => 1,
                NetworkStatus::Disconnected => 2,
                NetworkStatus::Stopped => 3,
            };
            if status < old_status {
                return Err("peer presentation status cannot regress or reconnect".into());
            }
        }
        let mut progress = previous.and_then(|peer| peer.progress);
        if !words.is_empty() {
            let value = |index| u64::from(words[index]) | (u64::from(words[index + 1]) << 32);
            let next = Progress {
                song_ns: value(0) as i64,
                hits: value(2),
                misses: value(4),
                combo: value(6),
                max_combo: value(8),
            };
            validate_progress(progress, next).map_err(|error| error.to_string())?;
            progress = Some(next);
        }
        let snapshot = self.snapshot.get_or_insert_with(|| CompetitionSnapshot {
            ghosts: Vec::new(),
            network: None,
        });
        snapshot.network = Some(NetworkSnapshot {
            status: next_status,
            progress,
        });
        Ok(())
    }

    pub const fn peer_failed(&self) -> bool {
        self.peer_failed
    }

    /// Peer display failure cannot discard independently valid saved prefixes.
    pub fn mark_peer_failed(&mut self) {
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.network = None;
            if snapshot.ghosts.is_empty() {
                self.snapshot = None;
            }
        }
        self.peer_failed = true;
    }
}
