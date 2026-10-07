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

pub const GRADE_ROWS_PER_PAGE: usize = 4;

pub fn historical_page_count(
    score: Option<&crate::result_archive::ArchivedScore>,
    comparisons: Option<&Option<crate::competition_presentation::CompetitionSnapshot>>,
) -> usize {
    let grades = score.map_or(1, |score| {
        score.grades.len().div_ceil(GRADE_ROWS_PER_PAGE).max(1)
    });
    grades
        + comparisons.map_or(0, |snapshot| {
            snapshot.as_ref().map_or(1, |snapshot| {
                (snapshot.ghosts.len() + usize::from(snapshot.network.is_some())).max(1)
            })
        })
}

pub struct HistoricalRecordPresentation {
    value: HistoricalRecordValue,
    start: Timestamp,
    end: Option<Timestamp>,
    score: Option<crate::result_archive::ArchivedScore>,
    bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    geometry: GeometrySnapshot,
    comparison_body: GeometrySnapshot,
    comparison: Option<Option<crate::competition_presentation::CompetitionSnapshot>>,
    comparison_pages: Vec<GeometrySnapshot>,
    grade_page: usize,
    grade_geometry: GeometrySnapshot,
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
        let comparison = archive.comparisons().and_then(|rows| {
            rows.iter()
                .find(|(id, _)| *id == entry.player)
                .map(|(_, snapshot)| snapshot)
        });
        Self::from_record_with_class_score(
            (entry.player, entry.result),
            entry.score.as_ref(),
            comparison,
            entry.bms_score().map_err(|error| error.to_string())?,
        )
        .map(Some)
    }
    pub fn from_record(
        value: HistoricalRecordValue,
        stored_score: Option<&crate::result_archive::ArchivedScore>,
    ) -> Result<Self, String> {
        Self::from_record_with_comparisons(value, stored_score, None)
    }
    pub fn from_record_with_comparisons(
        value: HistoricalRecordValue,
        stored_score: Option<&crate::result_archive::ArchivedScore>,
        comparisons: Option<&Option<crate::competition_presentation::CompetitionSnapshot>>,
    ) -> Result<Self, String> {
        Self::from_record_with_class_score(value, stored_score, comparisons, None)
    }
    pub fn from_record_with_class_score(
        value: HistoricalRecordValue,
        stored_score: Option<&crate::result_archive::ArchivedScore>,
        comparisons: Option<&Option<crate::competition_presentation::CompetitionSnapshot>>,
        bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    ) -> Result<Self, String> {
        if value.0.0 == 0 {
            return Err("historical player ID must be nonzero".into());
        }
        if let Some(score) = stored_score {
            score.validate().map_err(|error| error.to_string())?;
        }
        if let Some(classes) = bms_score {
            let score = stored_score.ok_or("stored class score requires stored counts")?;
            classes
                .validate_for(score.hits, score.misses)
                .map_err(|error| error.to_string())?;
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
        let comparison = comparisons
            .map(|snapshot| {
                snapshot
                    .as_ref()
                    .map(crate::result_archive::copy_comparison)
                    .transpose()
            })
            .transpose()
            .map_err(|error| error.to_string())?;
        let comparison_pages = build_comparison_pages(comparison.as_ref())?;
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
        text(
            &mut scene,
            500,
            286,
            if comparison.is_some() {
                "STORED COMPARISON METADATA"
            } else {
                "STORED COMPARISONS UNAVAILABLE"
            },
            1,
            0x9bb1cf,
        );
        let comparison_body = scene.geometry_snapshot()?;
        let mut scene = Scene::with_capacity(960, 720, 1024);
        scene.append_geometry(&comparison_body)?;
        if let Some(classes) = bms_score {
            for (y, name, value) in [
                (322, "EX", classes.ex_score),
                (346, "PGREAT", classes.pgreat),
                (370, "GREAT", classes.great),
                (394, "GOOD", classes.good),
                (418, "BAD", classes.bad),
                (442, "POOR", classes.poor),
            ] {
                text(
                    &mut scene,
                    500,
                    y,
                    &format!("STORED {name} {value}"),
                    1,
                    0xd8b36b,
                );
            }
        } else {
            text(
                &mut scene,
                500,
                322,
                "STORED CLASS SCORE UNAVAILABLE",
                1,
                0x9bb1cf,
            );
        }
        if let Some(score) = stored_score {
            let score_clip = crate::scene::ClipRect::new([24, 300, 452, 200])?;
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
                crate::ui::atoms::text_clipped(&mut scene, 24, y, &label, 1, 0xb6cce6, score_clip)?;
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
                crate::ui::atoms::text_clipped(&mut scene, 24, y, &label, 1, 0xb6cce6, score_clip)?;
            }
        } else {
            text(&mut scene, 24, 322, "STORED SCORE UNAVAILABLE", 1, 0x9bb1cf);
        }
        let score = stored_score
            .map(crate::result_archive::ArchivedScore::try_copy)
            .transpose()
            .map_err(|error| error.to_string())?;
        let grade_geometry = build_grade_geometry(score.as_ref(), 0)?;
        Ok(Self {
            comparison,
            comparison_pages,
            comparison_body,
            grade_page: 0,
            grade_geometry,
            value,
            start,
            end,
            score,
            bms_score,
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
    pub const fn bms_score(&self) -> Option<crate::judgment_policy::BmsScoreSummary> {
        self.bms_score
    }
    pub const fn grade_page(&self) -> usize {
        self.grade_page
    }
    pub fn grade_page_count(&self) -> usize {
        historical_page_count(self.score.as_ref(), self.comparison.as_ref())
    }
    pub fn comparison(
        &self,
    ) -> Option<&Option<crate::competition_presentation::CompetitionSnapshot>> {
        self.comparison.as_ref()
    }
    fn score_pages(&self) -> usize {
        historical_page_count(self.score.as_ref(), None)
    }
    pub(crate) fn prepare_grade_page(&self, page: usize) -> Result<GeometrySnapshot, String> {
        if page >= self.grade_page_count() {
            return Err("stored grade page is out of range".into());
        }
        if page == self.grade_page {
            return Ok(self.grade_geometry.clone());
        }
        if page >= self.score_pages() {
            return Ok(self.comparison_pages[page - self.score_pages()].clone());
        }
        build_grade_geometry(self.score.as_ref(), page)
    }
    pub fn set_grade_page(&mut self, page: usize) -> Result<bool, String> {
        if page >= self.grade_page_count() {
            return Err("stored grade page is out of range".into());
        }
        if page == self.grade_page {
            return Ok(false);
        }
        let geometry = self.prepare_grade_page(page)?;
        self.grade_geometry = geometry;
        self.grade_page = page;
        Ok(true)
    }
    pub(crate) fn compose_body(&self, scene: &mut Scene) -> Result<(), String> {
        scene.append_geometry(&self.geometry)
    }
    pub(crate) fn compose_body_for_page(
        &self,
        page: usize,
        scene: &mut Scene,
    ) -> Result<(), String> {
        if page >= self.grade_page_count() {
            return Err("stored detail page is out of range".into());
        }
        if page >= self.score_pages() {
            scene.append_geometry(&self.comparison_body)
        } else {
            self.compose_body(scene)
        }
    }
    pub fn compose(&self, scene: &mut Scene) -> Result<(), String> {
        self.compose_body_for_page(self.grade_page, scene)?;
        scene.append_geometry(&self.grade_geometry)
    }
}
fn build_comparison_pages(
    comparison: Option<&Option<crate::competition_presentation::CompetitionSnapshot>>,
) -> Result<Vec<GeometrySnapshot>, String> {
    use crate::{ui::atoms::text, competition::OpponentKind, competition_presentation::NetworkStatus};
    let mut pages = Vec::new();
    let Some(comparison) = comparison else {
        return Ok(pages);
    };
    let count = comparison.as_ref().map_or(1, |snapshot| {
        (snapshot.ghosts.len() + usize::from(snapshot.network.is_some())).max(1)
    });
    pages
        .try_reserve_exact(count)
        .map_err(|error| error.to_string())?;
    if let Some(snapshot) = comparison {
        for ghost in &snapshot.ghosts {
            let mut scene = Scene::with_capacity(960, 720, 512);
            text(
                &mut scene,
                24,
                322,
                "SAVED REPLAY OPERATION PREFIX",
                1,
                0xd8b36b,
            );
            text(
                &mut scene,
                24,
                346,
                match ghost.kind {
                    OpponentKind::Own => "STORED OWNER OWN",
                    OpponentKind::Other => "STORED OWNER OTHER",
                },
                1,
                0xd8b36b,
            );
            text(&mut scene, 24, 370, &ghost.label, 1, 0xb6cce6);
            text(
                &mut scene,
                24,
                394,
                &format!("STORED HITS {} MISSES {}", ghost.hits, ghost.misses),
                1,
                0xb6cce6,
            );
            text(
                &mut scene,
                24,
                418,
                &format!("STORED COMBO {} MAX COMBO {}", ghost.combo, ghost.max_combo),
                1,
                0xb6cce6,
            );
            let frontier = ghost.recorded_until.map_or_else(
                || "RECORDED UNTIL UNAVAILABLE".into(),
                |time| format!("RECORDED UNTIL {} NS", time.as_nanos()),
            );
            text(&mut scene, 24, 442, &frontier, 1, 0xb6cce6);
            text(
                &mut scene,
                24,
                466,
                "PREFIX DOES NOT PROVE WHOLE-SONG COMPLETION",
                1,
                0x9bb1cf,
            );
            text(
                &mut scene,
                24,
                514,
                &format!("STORED COMPARISONS PAGE {} / {}", pages.len() + 1, count),
                1,
                0x9bb1cf,
            );
            pages.push(scene.geometry_snapshot()?);
        }
        if let Some(network) = &snapshot.network {
            let mut scene = Scene::with_capacity(960, 720, 512);
            text(
                &mut scene,
                24,
                322,
                "PEER-REPORTED NOT FINAL RANKING",
                1,
                0xd8b36b,
            );
            text(
                &mut scene,
                24,
                346,
                match network.status {
                    NetworkStatus::Waiting => "NETWORK STATUS WAITING",
                    NetworkStatus::Connected => "NETWORK STATUS CONNECTED",
                    NetworkStatus::Disconnected => "NETWORK STATUS DISCONNECTED",
                    NetworkStatus::Stopped => "NETWORK STATUS STOPPED",
                },
                1,
                0xd8b36b,
            );
            if let Some(progress) = network.progress {
                text(
                    &mut scene,
                    24,
                    370,
                    &format!("REPORTED SONG {} NS", progress.song_ns),
                    1,
                    0xb6cce6,
                );
                text(
                    &mut scene,
                    24,
                    394,
                    &format!("REPORTED HITS {} MISSES {}", progress.hits, progress.misses),
                    1,
                    0xb6cce6,
                );
                text(
                    &mut scene,
                    24,
                    418,
                    &format!(
                        "REPORTED COMBO {} MAX COMBO {}",
                        progress.combo, progress.max_combo
                    ),
                    1,
                    0xb6cce6,
                );
            } else {
                text(
                    &mut scene,
                    24,
                    370,
                    "PEER PROGRESS UNAVAILABLE",
                    1,
                    0x9bb1cf,
                );
            }
            text(
                &mut scene,
                24,
                514,
                &format!("STORED COMPARISONS PAGE {} / {}", pages.len() + 1, count),
                1,
                0x9bb1cf,
            );
            pages.push(scene.geometry_snapshot()?);
        }
    }
    if pages.is_empty() {
        let mut scene = Scene::with_capacity(960, 720, 256);
        text(
            &mut scene,
            24,
            322,
            if comparison.is_none() {
                "NO SELECTED COMPARISONS"
            } else {
                "STORED COMPARISON SNAPSHOT EMPTY"
            },
            1,
            0x9bb1cf,
        );
        text(
            &mut scene,
            24,
            514,
            "STORED COMPARISONS PAGE 1 / 1",
            1,
            0x9bb1cf,
        );
        pages.push(scene.geometry_snapshot()?);
    }
    Ok(pages)
}
fn build_grade_geometry(
    score: Option<&crate::result_archive::ArchivedScore>,
    page: usize,
) -> Result<GeometrySnapshot, String> {
    use crate::ui::atoms::text;
    let mut scene = Scene::with_capacity(960, 720, 512);
    match score {
        None => text(
            &mut scene,
            24,
            514,
            "STORED GRADES UNAVAILABLE",
            1,
            0x9bb1cf,
        ),
        Some(score) if score.grades.is_empty() => {
            text(&mut scene, 24, 514, "STORED GRADES EMPTY", 1, 0x9bb1cf)
        }
        Some(score) => {
            let pages = score.grades.len().div_ceil(GRADE_ROWS_PER_PAGE);
            text(
                &mut scene,
                24,
                514,
                &format!("STORED GRADES PAGE {} / {}", page + 1, pages),
                1,
                0x9bb1cf,
            );
            let start = page * GRADE_ROWS_PER_PAGE;
            for (index, (grade, count)) in score.grades
                [start..score.grades.len().min(start + GRADE_ROWS_PER_PAGE)]
                .iter()
                .enumerate()
            {
                text(
                    &mut scene,
                    24,
                    538 + index * 16,
                    &format!("STORED GRADE {grade} COUNT {count}"),
                    1,
                    0xb6cce6,
                );
            }
        }
    }
    scene.geometry_snapshot()
}
#[cfg(test)]
#[path = "historical_record_presentation_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "historical_grade_page_fixtures.rs"]
mod historical_grade_page_fixtures;

#[cfg(test)]
#[path = "historical_comparison_page_fixtures.rs"]
mod historical_comparison_page_fixtures;
