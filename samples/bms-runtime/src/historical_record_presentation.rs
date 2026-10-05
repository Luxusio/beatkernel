//! Cached presentation of decoded historical data, without live completion evidence.
use crate::{
    record_model::HistoricalRecordValue,
    local_players::PlayerId,
    play_result::{PlayResultScope, PlayResultOutcome},
    gauge::GaugeFailure,
    scene::{Scene, GeometrySnapshot},
};
use beatkernel::{
    time::Timestamp,
    replay::codec::{ReplayCodecLimits, decode_replay},
    input::CodecLimits,
};

pub struct HistoricalRecordPresentation {
    value: HistoricalRecordValue,
    start: Timestamp,
    end: Option<Timestamp>,
    score: Option<crate::result_archive::ArchivedScore>,
    geometry: GeometrySnapshot,
}
impl HistoricalRecordPresentation {
    pub fn new(
        replay: &[u8],
        archive: Option<&[u8]>,
        player: Option<PlayerId>,
    ) -> Result<Option<Self>, String> {
        if player.is_some_and(|player| player.0 == 0) {
            return Err("historical player ID must be nonzero".into());
        }
        let limits = ReplayCodecLimits::new(
            64 * 1024 * 1024,
            1_000_000,
            4096,
            CodecLimits::new(65536, 32768).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let replay = decode_replay(replay, limits).map_err(|error| error.to_string())?;
        let Some(archive) = archive else {
            return Ok(None);
        };
        let archive =
            crate::result_archive::decode_archive(archive).map_err(|error| error.to_string())?;
        let entry = crate::record_association::associate(&archive, &replay.header, player)
            .map_err(|error| error.to_string())?;
        Self::from_record((entry.player, entry.result), entry.score.as_ref()).map(Some)
    }
    pub fn from_record(
        value: HistoricalRecordValue,
        stored_score: Option<&crate::result_archive::ArchivedScore>,
    ) -> Result<Self, String> {
        if value.0.0 == 0 {
            return Err("historical player ID must be nonzero".into());
        }
        if let Some(score) = stored_score {
            score.validate().map_err(|error| error.to_string())?;
        }
        let (start, end) = match value.1.scope {
            PlayResultScope::FullSong => (Timestamp::ZERO, None),
            PlayResultScope::PracticeSection { start, end } => {
                if start < Timestamp::ZERO
                    || end.is_some_and(|end| end <= start)
                    || (start == Timestamp::ZERO && end.is_none())
                {
                    return Err("invalid historical extent".into());
                }
                (start, end)
            }
        };
        let mut scene = Scene::with_capacity(960, 720, 1024);
        use crate::ui::atoms::text;
        text(&mut scene, 24, 65, "STORED HISTORICAL RECORD", 2, 0x9bb1cf);
        text(
            &mut scene,
            24,
            130,
            &format!("HISTORICAL PLAYER {}", value.0.0),
            1,
            0xd8b36b,
        );
        text(
            &mut scene,
            24,
            154,
            match value.1.scope {
                PlayResultScope::FullSong => "STORED SCOPE FULL SONG",
                PlayResultScope::PracticeSection { .. } => "STORED SCOPE PRACTICE SECTION",
            },
            1,
            0xd8b36b,
        );
        text(
            &mut scene,
            24,
            178,
            &format!("START {} NS", start.as_nanos()),
            1,
            0xb6cce6,
        );
        let end_label = end.map_or_else(
            || "END UNLIMITED".into(),
            |end| format!("END {} NS", end.as_nanos()),
        );
        text(&mut scene, 24, 202, &end_label, 1, 0xb6cce6);
        text(
            &mut scene,
            24,
            226,
            match value.1.outcome {
                PlayResultOutcome::Cleared => "STORED OUTCOME CLEARED",
                PlayResultOutcome::BelowClearThreshold => "STORED OUTCOME BELOW CLEAR",
                PlayResultOutcome::Failed(GaugeFailure::InstantDeath) => {
                    "STORED OUTCOME FAILED INSTANT DEATH"
                }
                PlayResultOutcome::Failed(GaugeFailure::Depleted) => {
                    "STORED OUTCOME FAILED DEPLETED"
                }
            },
            1,
            0xd8b36b,
        );
        text(
            &mut scene,
            24,
            250,
            &format!("STORED GAUGE {} UNITS", value.1.gauge.level_units),
            1,
            0xd8b36b,
        );
        text(&mut scene, 24, 286, "STORED HISTORICAL DATA", 1, 0x9bb1cf);
        if let Some(score) = stored_score {
            for (y, label) in [
                (
                    322,
                    format!("STORED HITS {} MISSES {}", score.hits, score.misses),
                ),
                (
                    346,
                    format!("STORED COMBO {} MAX COMBO {}", score.combo, score.max_combo),
                ),
                (
                    370,
                    format!(
                        "TIMING COUNT {} EARLY {} LATE {} EXACT {}",
                        score.timing.count,
                        score.timing.early,
                        score.timing.late,
                        score.timing.exact
                    ),
                ),
                (394, format!("TIMING SUM {} NS", score.timing.sum)),
                (
                    418,
                    format!("TIMING ABSOLUTE SUM {} NS", score.timing.absolute_sum),
                ),
            ] {
                text(&mut scene, 24, y, &label, 1, 0xb6cce6);
            }
            for (y, name, value) in [
                (442, "LAST", score.timing.last),
                (466, "MIN", score.timing.min),
                (490, "MAX", score.timing.max),
            ] {
                let label = value.map_or_else(
                    || format!("TIMING {name} UNAVAILABLE"),
                    |value| format!("TIMING {name} {value} NS"),
                );
                text(&mut scene, 24, y, &label, 1, 0xb6cce6);
            }
        } else {
            text(&mut scene, 24, 322, "STORED SCORE UNAVAILABLE", 1, 0x9bb1cf);
        }
        let score = stored_score
            .map(crate::result_archive::ArchivedScore::try_copy)
            .transpose()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            value,
            start,
            end,
            score,
            geometry: scene.geometry_snapshot()?,
        })
    }
    pub const fn value(&self) -> HistoricalRecordValue {
        self.value
    }
    pub const fn start(&self) -> Timestamp {
        self.start
    }
    pub const fn end(&self) -> Option<Timestamp> {
        self.end
    }
    pub fn score(&self) -> Option<&crate::result_archive::ArchivedScore> {
        self.score.as_ref()
    }
    pub fn compose(&self, scene: &mut Scene) -> Result<(), String> {
        scene.append_geometry(&self.geometry)
    }
}
#[cfg(test)]
#[path = "historical_record_presentation_fixtures.rs"]
mod fixtures;
