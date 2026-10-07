//! Retained final results from explicit immutable completion evidence.
use super::atoms::{text, text_clipped};
use crate::{
    gauge::{GaugeFailure, GAUGE_UNITS_PER_PERCENT},
    competition::{ScoreSummary, OpponentKind},
    competition_presentation::{CompetitionSnapshot, NetworkStatus},
    local_players::PlayerId,
    play_result::{CompletedPlayResult, PlayResultOutcome, PlayResultScope},
    scene::{GeometrySnapshot, Scene, ClipRect},
};

pub const PLAYERS_PER_PAGE: usize = 4;

pub struct ResultRow {
    pub player: PlayerId,
    pub result: CompletedPlayResult,
    pub identity_label: String,
    pub outcome_label: String,
    pub gauge_label: String,
}

pub struct ResultDetails<'a> {
    pub player: PlayerId,
    pub score: &'a ScoreSummary,
    pub competition: Option<&'a CompetitionSnapshot>,
}

pub struct FrozenResultDetails {
    pub player: PlayerId,
    pub score: ScoreSummary,
    pub competition: Option<CompetitionSnapshot>,
}

/// All labels and page packets are staged once before a view is accepted.
pub struct ResultsView {
    rows: Vec<ResultRow>,
    scope_label: String,
    pages: Vec<GeometrySnapshot>,
    details: Vec<FrozenResultDetails>,
    comparison_pages: Vec<GeometrySnapshot>,
}
impl ResultsView {
    pub fn new(
        results: &[(PlayerId, CompletedPlayResult)],
        roster: &[PlayerId],
    ) -> Result<Self, String> {
        Self::new_model(results, roster, true)
    }
    fn new_model(
        results: &[(PlayerId, CompletedPlayResult)],
        roster: &[PlayerId],
        simple: bool,
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
        let scope_label = result_scope_label(scope);
        let mut rows = Vec::new();
        rows.try_reserve_exact(roster.len())
            .map_err(|error| error.to_string())?;
        for player in roster {
            let result = results
                .iter()
                .find(|(id, _)| id == player)
                .ok_or("completed Results omitted a registered player")?
                .1;
            let archived = crate::result_archive::ArchivedResult { scope: result.scope(), outcome: result.outcome(), gauge: result.gauge() };
            let (identity_label, outcome_label, gauge_label) = result_labels(*player, archived);
            rows.push(ResultRow { player: *player, result, identity_label, outcome_label, gauge_label });
        }
        let mut pages = Vec::new();
        if simple {
            let display_rows: Vec<_> = rows.iter().map(|row| FrozenResultRow { player: row.player,
                result: crate::result_archive::ArchivedResult { scope: row.result.scope(), outcome: row.result.outcome(), gauge: row.result.gauge() } }).collect();
            pages = simple_result_pages(&scope_label, &display_rows)?;
        }
        Ok(Self {
            rows,
            scope_label,
            pages,
            details: Vec::new(),
            comparison_pages: Vec::new(),
        })
    }
    /// Freeze all supplied score and comparison prefixes before creating retained packets.
    pub fn new_with_details(
        results: &[(PlayerId, CompletedPlayResult)],
        roster: &[PlayerId],
        details: &[ResultDetails<'_>],
    ) -> Result<Self, String> {
        if details.len() != roster.len() {
            return Err("Results details require the entire roster".into());
        }
        for (index, detail) in details.iter().enumerate() {
            if !roster.contains(&detail.player)
                || details[..index]
                    .iter()
                    .any(|other| other.player == detail.player)
            {
                return Err("Results details contain foreign or duplicate identities".into());
            }
            if let Some(competition) = detail.competition {
                if competition.ghosts.len() > 8
                    || competition.ghosts.iter().any(|ghost| {
                        ghost.label.len() > 256
                            || ghost.label.chars().count() > 64
                            || ghost.label.chars().any(char::is_control)
                    })
                {
                    return Err("Results comparison exceeds supported ghost or label bounds".into());
                }
            }
        }
        let mut view = Self::new_model(results, roster, false)?;
        let mut detail_cards = Vec::new();
        let mut comparison_cards = Vec::new();
        view.details
            .try_reserve_exact(roster.len())
            .map_err(|error| error.to_string())?;
        for row in &view.rows {
            let detail = details
                .iter()
                .find(|detail| detail.player == row.player)
                .ok_or("Results details omitted a registered player")?;
            append_result_cards(&row.identity_label, &row.outcome_label, &row.gauge_label,
                detail.score.hits, detail.score.misses, detail.score.combo, detail.score.max_combo,
                crate::timing_display::summary(&detail.score.timing),
                detail.score.grades.iter().map(|(grade, count)| format!("GRADE G{grade} COUNT {count}")).collect(),
                detail.competition, &mut detail_cards, &mut comparison_cards);
            view.details.push(FrozenResultDetails {
                player: row.player,
                score: detail.score.clone(),
                competition: detail.competition.cloned(),
            });
        }
        view.pages = card_pages(&view.scope_label, &detail_cards, "DETAILS")?;
        view.comparison_pages = card_pages(&view.scope_label, &comparison_cards, "COMPARISONS")?;
        Ok(view)
    }
    pub fn export_visual(&self) -> Result<FrozenResultsModel, String> {
        let model = FrozenResultsModel {
            roster: self.rows.iter().map(|row| row.player).collect(),
            rows: self.rows.iter().map(|row| FrozenResultRow { player: row.player,
                result: crate::result_archive::ArchivedResult { scope: row.result.scope(), outcome: row.result.outcome(), gauge: row.result.gauge() } }).collect(),
            details: self.details.iter().map(|detail| Ok(FrozenScoreDetails { player: detail.player,
                score: crate::result_archive::ArchivedScore::from_summary(&detail.score).map_err(|error| error.to_string())?,
                competition: detail.competition.clone() })).collect::<Result<_, String>>()?,
        };
        model.validate()?;
        Ok(model)
    }
    pub fn details(&self) -> &[FrozenResultDetails] {
        &self.details
    }
    pub fn has_comparisons(&self) -> bool {
        !self.comparison_pages.is_empty()
    }
    pub fn page_count_for(&self, comparisons: bool) -> usize {
        if comparisons && self.has_comparisons() {
            self.comparison_pages.len()
        } else {
            self.pages.len()
        }
    }
    pub fn compose_mode(
        &self,
        scene: &mut Scene,
        page: usize,
        comparisons: bool,
    ) -> Result<(), String> {
        if comparisons && self.has_comparisons() {
            scene.append_geometry(
                self.comparison_pages
                    .get(page)
                    .ok_or("Results comparison page is out of range")?,
            )
        } else {
            self.compose(scene, page)
        }
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

fn counters(hits: u64, misses: u64, combo: u64, max_combo: u64) -> String {
    format!("HITS {hits} MISSES {misses} COMBO {combo} MAX COMBO {max_combo}")
}

fn card_pages(
    scope: &str,
    cards: &[Vec<String>],
    mode: &str,
) -> Result<Vec<GeometrySnapshot>, String> {
    let mut pages = Vec::new();
    pages
        .try_reserve_exact(cards.len().div_ceil(PLAYERS_PER_PAGE))
        .map_err(|error| error.to_string())?;
    for (page, visible) in cards.chunks(PLAYERS_PER_PAGE).enumerate() {
        let mut scene = Scene::with_capacity(960, 720, 1024);
        text(&mut scene, 24, 100, scope, 1, 0x9bb1cf);
        for (index, card) in visible.iter().enumerate() {
            let y = 140 + index * 110;
            let clip = ClipRect::new([24, y as i64, 912, 100])?;
            for (line, label) in card.iter().enumerate() {
                text_clipped(
                    &mut scene,
                    24,
                    y + line * 10,
                    label,
                    1,
                    if line == 0 { 0xf0f4ff } else { 0x9bb1cf },
                    clip,
                )?;
            }
        }
        text(
            &mut scene,
            24,
            600,
            &format!(
                "LOCAL {mode} PAGE {}/{} - PGUP/PGDN",
                page + 1,
                cards.len().div_ceil(PLAYERS_PER_PAGE)
            ),
            1,
            0x9bb1cf,
        );
        pages.push(scene.geometry_snapshot()?);
    }
    Ok(pages)
}

#[cfg(test)]
#[path = "results_detail_fixtures.rs"]
mod detail_fixtures;

#[cfg(test)]
#[path = "results_fixtures.rs"]
mod fixtures;

fn append_result_cards(
    identity_label: &String,
    outcome_label: &String,
    gauge_label: &String,
    hits: u64,
    misses: u64,
    combo: u64,
    max_combo: u64,
    timing: (String, String),
    grades: Vec<String>,
    competition: Option<&CompetitionSnapshot>,
    detail_cards: &mut Vec<Vec<String>>,
    comparison_cards: &mut Vec<Vec<String>>,
) {
    let (bias, absolute) = timing;
    detail_cards.push(vec![
        identity_label.clone(),
        outcome_label.clone(),
        gauge_label.clone(),
        counters(hits, misses, combo, max_combo),
        bias,
        absolute,
    ]);
    // Opaque grade identities are exact counts, with every supplied grade reachable.
    for chunk in grades.chunks(8) {
        let mut card = vec![format!("{} GRADE COUNTS", identity_label)];
        card.extend_from_slice(chunk);
        detail_cards.push(card);
    }
    if let Some(competition) = competition {
        for ghost in &competition.ghosts {
            comparison_cards.push(vec![
                identity_label.clone(),
                match ghost.kind {
                    OpponentKind::Own => "OWN RECORDED PREFIX",
                    OpponentKind::Other => "OTHER RECORDED PREFIX",
                }
                .into(),
                ghost.label.clone(),
                counters(ghost.hits, ghost.misses, ghost.combo, ghost.max_combo),
                ghost
                    .recorded_until
                    .map_or("RECORDED UNTIL UNKNOWN".into(), |time| {
                        format!("RECORDED UNTIL {} NS", time.as_nanos())
                    }),
            ]);
        }
        if let Some(network) = &competition.network {
            let mut card = vec![
                identity_label.clone(),
                "SELF-REPORTED PEER PREFIX".into(),
                match network.status {
                    NetworkStatus::Waiting => "NETWORK WAITING",
                    NetworkStatus::Connected => "NETWORK CONNECTED",
                    NetworkStatus::Disconnected => "NETWORK DISCONNECTED",
                    NetworkStatus::Stopped => "NETWORK STOPPED",
                }
                .into(),
            ];
            if let Some(progress) = network.progress {
                card.push(counters(
                    progress.hits,
                    progress.misses,
                    progress.combo,
                    progress.max_combo,
                ));
                card.push(format!("PREFIX SONG {} NS", progress.song_ns));
            } else {
                card.push("PEER PREFIX UNAVAILABLE".into());
            }
            comparison_cards.push(card);
        }
    }
}
fn result_scope_label(scope: PlayResultScope) -> String {
    match scope {
        PlayResultScope::FullSong => "WHOLE SONG".into(),
        PlayResultScope::PracticeSection { start, end } => match end {
            Some(end) => format!(
                "PRACTICE START {} NS END {} NS",
                start.as_nanos(),
                end.as_nanos()
            ),
            None => format!("PRACTICE START {} NS END UNBOUNDED", start.as_nanos()),
        },
    }
}
fn result_labels(
    player: PlayerId,
    result: crate::result_archive::ArchivedResult,
) -> (String, String, String) {
    let outcome = match result.outcome {
        PlayResultOutcome::Cleared if matches!(result.scope, PlayResultScope::FullSong) => {
            "CLEARED"
        }
        PlayResultOutcome::Cleared => "PRACTICE - CLEAR THRESHOLD MET",
        PlayResultOutcome::BelowClearThreshold => "BELOW CLEAR THRESHOLD",
        PlayResultOutcome::Failed(GaugeFailure::InstantDeath) => "FAILED - INSTANT DEATH",
        PlayResultOutcome::Failed(GaugeFailure::Depleted) => "FAILED - DEPLETED",
    };
    let level = result.gauge.level_units;
    (
        format!("PLAYER {}", player.0),
        outcome.into(),
        format!(
            "GAUGE {}.{:06}%",
            level / GAUGE_UNITS_PER_PERCENT,
            level % GAUGE_UNITS_PER_PERCENT
        ),
    )
}
/// Display data cannot be supplied to any live completion API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenResultRow {
    pub player: PlayerId,
    pub result: crate::result_archive::ArchivedResult,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenScoreDetails {
    pub player: PlayerId,
    pub score: crate::result_archive::ArchivedScore,
    pub competition: Option<CompetitionSnapshot>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenResultsModel {
    pub roster: Vec<PlayerId>,
    pub rows: Vec<FrozenResultRow>,
    pub details: Vec<FrozenScoreDetails>,
}
pub(crate) fn validate_frozen_result(
    result: &crate::result_archive::ArchivedResult,
) -> Result<(), String> {
    if result.gauge.level_units > crate::gauge::MAX_GAUGE_UNITS
        || match result.outcome {
            PlayResultOutcome::Failed(reason) => result.gauge.failure != Some(reason),
            _ => result.gauge.failure.is_some(),
        }
    {
        return Err("invalid frozen display result gauge or outcome".into());
    }
    if let PlayResultScope::PracticeSection { start, end } = result.scope {
        if start < beatkernel::time::Timestamp::ZERO
            || end.is_some_and(|end| end <= start)
            || (start == beatkernel::time::Timestamp::ZERO && end.is_none())
        {
            return Err("invalid frozen display result scope".into());
        }
    }
    Ok(())
}
impl FrozenResultsModel {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=64).contains(&self.roster.len())
            || self.rows.len() != self.roster.len()
            || (!self.details.is_empty() && self.details.len() != self.roster.len())
        {
            return Err("frozen Results require entire bounded roster".into());
        }
        for (index, player) in self.roster.iter().enumerate() {
            if player.0 == 0
                || self.roster[..index].contains(player)
                || self.rows[index].player != *player
                || self.rows[index].result.scope != self.rows[0].result.scope
            {
                return Err("frozen Results contain foreign, duplicate or mixed-scope rows".into());
            }
            validate_frozen_result(&self.rows[index].result)?;
            if let Some(detail) = self.details.get(index) {
                if detail.player != *player {
                    return Err("frozen Results detail identity mismatch".into());
                }
                detail.score.validate().map_err(|error| error.to_string())?;
                if let Some(snapshot) = &detail.competition {
                    crate::browser_render_state::validate_comparison(snapshot)?;
                }
            }
        }
        Ok(())
    }
    /// Rows48+score128+grade12; comparison labels256+64 metadata per ghost and network64.
    pub fn encoded_bytes(&self) -> Result<usize, String> {
        self.validate()?;
        Ok(64
            + self.roster.len() * 4
            + self.rows.len() * 48
            + self
                .details
                .iter()
                .map(|detail| {
                    128 + detail.score.grades.len() * 12
                        + detail.competition.as_ref().map_or(0, |snapshot| {
                            64 + snapshot
                                .ghosts
                                .iter()
                                .map(|ghost| 64 + ghost.label.len())
                                .sum::<usize>()
                        })
                })
                .sum::<usize>())
    }
}
/// Retained visual reconstruction uses common labels/cards, without creating completion evidence.
pub struct FrozenResultsView {
    model: FrozenResultsModel,
    pages: Vec<GeometrySnapshot>,
    comparison_pages: Vec<GeometrySnapshot>,
}
impl FrozenResultsView {
    pub fn from_model(model: FrozenResultsModel) -> Result<Self, String> {
        model.validate()?;
        let scope = result_scope_label(model.rows[0].result.scope);
        let mut detail_cards = Vec::new();
        let mut comparison_cards = Vec::new();
        let pages = if model.details.is_empty() {
            simple_result_pages(&scope, &model.rows)?
        } else {
            for (row, detail) in model.rows.iter().zip(&model.details) {
                let (identity, outcome, gauge) = result_labels(row.player, row.result);
                let timing = detail.score.timing;
                let bias = if timing.count == 0 {
                    None
                } else {
                    i64::try_from(timing.sum / i128::from(timing.count)).ok()
                };
                let absolute = if timing.count == 0 {
                    None
                } else {
                    u64::try_from(timing.absolute_sum / u128::from(timing.count)).ok()
                };
                append_result_cards(
                    &identity,
                    &outcome,
                    &gauge,
                    detail.score.hits,
                    detail.score.misses,
                    detail.score.combo,
                    detail.score.max_combo,
                    (
                        format!(
                            "BIAS {}",
                            bias.map_or("--".into(), crate::timing_display::signed_ms)
                        ),
                        format!(
                            "MEAN ABS {}",
                            absolute.map_or("--".into(), crate::timing_display::unsigned_ms)
                        ),
                    ),
                    detail
                        .score
                        .grades
                        .iter()
                        .map(|(grade, count)| format!("GRADE G{grade} COUNT {count}"))
                        .collect(),
                    detail.competition.as_ref(),
                    &mut detail_cards,
                    &mut comparison_cards,
                );
            }
            card_pages(&scope, &detail_cards, "DETAILS")?
        };
        let comparison_pages = card_pages(&scope, &comparison_cards, "COMPARISONS")?;
        Ok(Self {
            model,
            pages,
            comparison_pages,
        })
    }
    pub fn model(&self) -> &FrozenResultsModel {
        &self.model
    }
    pub fn has_comparisons(&self) -> bool {
        !self.comparison_pages.is_empty()
    }
    pub fn page_count_for(&self, comparisons: bool) -> usize {
        if comparisons && self.has_comparisons() {
            self.comparison_pages.len()
        } else {
            self.pages.len()
        }
    }
    pub fn compose_mode(
        &self,
        scene: &mut Scene,
        page: usize,
        comparisons: bool,
    ) -> Result<(), String> {
        let pages = if comparisons && self.has_comparisons() {
            &self.comparison_pages
        } else {
            &self.pages
        };
        scene.append_geometry(pages.get(page).ok_or("frozen Results page out of range")?)
    }
}

fn simple_result_pages(
    scope: &str,
    rows: &[FrozenResultRow],
) -> Result<Vec<GeometrySnapshot>, String> {
    let mut pages = Vec::new();
    pages
        .try_reserve_exact(rows.len().div_ceil(PLAYERS_PER_PAGE))
        .map_err(|error| error.to_string())?;
    for (page, visible) in rows.chunks(PLAYERS_PER_PAGE).enumerate() {
        let mut scene = Scene::with_capacity(960, 720, 512);
        text(&mut scene, 24, 100, scope, 1, 0x9bb1cf);
        for (index, row) in visible.iter().enumerate() {
            let (identity, outcome, gauge) = result_labels(row.player, row.result);
            let color = match row.result.outcome {
                PlayResultOutcome::Cleared => 0x74e5c5,
                PlayResultOutcome::BelowClearThreshold => 0xd8b36b,
                PlayResultOutcome::Failed(_) => 0xff8e8e,
            };
            let y = 140 + index * 110;
            text(&mut scene, 24, y, &identity, 2, 0xf0f4ff);
            text(&mut scene, 24, y + 28, &outcome, 2, color);
            text(&mut scene, 24, y + 55, &gauge, 2, 0x9bb1cf);
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
    Ok(pages)
}
