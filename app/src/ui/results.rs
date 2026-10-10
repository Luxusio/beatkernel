//! Retained final results from explicit immutable completion evidence.
use super::{
    atoms::text_clipped,
    layout::{LayoutUpdate, MountedLayout, Node, NodeId},
};
use crate::{
    competition::{OpponentKind, ScoreSummary},
    competition_presentation::{CompetitionSnapshot, NetworkStatus},
    gauge::{GaugeFailure, GAUGE_UNITS_PER_PERCENT},
    local_players::PlayerId,
    play_result::{CompletedPlayResult, PlayResultOutcome, PlayResultScope},
    scene::{ClipRect, GeometrySnapshot, Scene, UiComponentKey, MAX_UI_COMPONENTS},
    screen_lifecycle::ScreenInstanceId,
};

use std::{cell::RefCell, sync::Arc};

pub const PLAYERS_PER_PAGE: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Component {
    Scope,
    Card(usize),
    Footer,
}
type N = Node<'static, Component>;
// Shared by genuine completed Results and their immutable visual reconstruction.
// No actions live here: the existing presentation owner supplies page/mode intent.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([912, 7], Component::Scope).at(24, 100),
        N::column(
            [912, 430],
            10,
            &[
                N::leaf([912, 100], Component::Card(0)),
                N::leaf([912, 100], Component::Card(1)),
                N::leaf([912, 100], Component::Card(2)),
                N::leaf([912, 100], Component::Card(3)),
            ],
        )
        .clipped()
        .at(24, 140),
        N::leaf([912, 7], Component::Footer).at(24, 600),
    ],
)
.clipped();
const LABEL_COLOR: u32 = 0xf0f4ff;
const DETAIL_COLOR: u32 = 0x9bb1cf;

struct ResultLine {
    label: String,
    y: usize,
    scale: usize,
    color: u32,
}
struct PageContent {
    scope: String,
    cards: Vec<Vec<ResultLine>>,
    footer: String,
}

// Geometry from the ordinary header is retained separately from component-tagged
// Results. Only cold page/layout/target changes capture and bind source packets.
struct ComponentPage {
    owner: ScreenInstanceId,
    page: usize,
    comparisons: bool,
    revision: u64,
    nodes: Vec<NodeId>,
    identity: Arc<()>,
    prefix: GeometrySnapshot,
}

fn content_for<'a>(
    pages: &'a [PageContent],
    comparisons: &'a [PageContent],
    page: usize,
    comparison_mode: bool,
) -> Result<(&'a PageContent, bool), String> {
    let comparison_mode = comparison_mode && !comparisons.is_empty();
    let contents = if comparison_mode { comparisons } else { pages };
    Ok((
        contents.get(page).ok_or("Results page is out of range")?,
        comparison_mode,
    ))
}

fn displayed_nodes(layout: &MountedLayout<Component>, content: &PageContent) -> Vec<NodeId> {
    if layout.suspended() {
        return Vec::new();
    }
    layout
        .leaves()
        .iter()
        .filter(|leaf| match leaf.component {
            Component::Card(index) => index < content.cards.len(),
            Component::Scope | Component::Footer => true,
        })
        .map(|leaf| leaf.id)
        .collect()
}

fn paint_content(
    scene: &mut Scene,
    component: Component,
    bounds: super::interaction::Bounds,
    content: &PageContent,
    clip: ClipRect,
) -> Result<(), String> {
    let x = bounds.x as usize;
    let y = bounds.y as usize;
    match component {
        Component::Scope => text_clipped(scene, x, y, &content.scope, 1, DETAIL_COLOR, clip),
        Component::Card(index) => {
            if let Some(card) = content.cards.get(index) {
                for line in card {
                    text_clipped(
                        scene,
                        x,
                        y + line.y,
                        &line.label,
                        line.scale,
                        line.color,
                        clip,
                    )?;
                }
            }
            Ok(())
        }
        Component::Footer => text_clipped(scene, x, y, &content.footer, 1, DETAIL_COLOR, clip),
    }
}

fn clip(bounds: super::interaction::Bounds) -> Result<Option<ClipRect>, String> {
    if bounds.width == 0 || bounds.height == 0 {
        Ok(None)
    } else {
        ClipRect::new([bounds.x, bounds.y, bounds.width, bounds.height]).map(Some)
    }
}

fn compose_components(
    scene: &mut Scene,
    layout: &MountedLayout<Component>,
    content: &PageContent,
    cache: &RefCell<Option<ComponentPage>>,
    owner: ScreenInstanceId,
    page: usize,
    comparisons: bool,
    animated: &[NodeId],
) -> Result<(), String> {
    let present = |node: NodeId| {
        !layout.suspended()
            && layout.leaves().iter().any(|leaf| {
                leaf.id == node
                    && match leaf.component {
                        Component::Card(index) => index < content.cards.len(),
                        Component::Scope | Component::Footer => true,
                    }
            })
    };
    if owner.0 == 0
        || animated.len() > MAX_UI_COMPONENTS
        || animated
            .iter()
            .enumerate()
            .any(|(i, node)| animated[..i].contains(node) || !present(*node))
    {
        return Err("invalid Results animated node set".into());
    }
    if !layout.suspended() && scene.logical_extent() != layout.extent() {
        return Err("Results component extent mismatch".into());
    }
    if scene
        .component_owner()
        .is_some_and(|current| current != owner)
    {
        return Err("Results component scene belongs to another owner".into());
    }
    let cached = cache.borrow();
    let same_scene = cached.as_ref().filter(|cached| {
        cached.owner == owner
            && Arc::ptr_eq(&cached.identity, scene.geometry_stamp().0)
            && scene.component_owner() == Some(owner)
    });
    if same_scene.is_some_and(|cached| {
        cached.page == page
            && cached.comparisons == comparisons
            && cached.revision == layout.revision()
            && cached.nodes == animated
    }) {
        return Ok(());
    }
    let prefix = if let Some(cached) = same_scene {
        cached.prefix.clone()
    } else {
        scene.static_geometry_snapshot()?
    };
    let mut next = scene.component_candidate();
    next.retain_component_keys(owner, animated);
    next.append_geometry(&prefix)?;
    if !layout.suspended() {
        let [width, height] = layout.extent();
        for leaf in layout.leaves() {
            if !present(leaf.id) {
                continue;
            }
            let moving = animated.contains(&leaf.id);
            let clips = layout
                .component_clips(leaf.id)
                .ok_or("Results mounted clip missing")?;
            let source = if moving {
                clips.source
            } else {
                leaf.geometry.clip
            };
            let mut packet = if moving {
                Scene::component_source(width, height, 256)
            } else {
                Scene::with_capacity(width, height, 256)
            };
            if let Some(source) = clip(source)? {
                paint_content(
                    &mut packet,
                    leaf.component,
                    leaf.geometry.bounds,
                    content,
                    source,
                )?;
            }
            let packet = packet.geometry_snapshot()?;
            let start = next.rectangles().len() as u32;
            let end = start + packet.rectangle_count() as u32;
            next.append_geometry(&packet)?;
            if moving {
                next.bind_component_clipped(
                    UiComponentKey {
                        screen: owner,
                        node: leaf.id,
                    },
                    &[start..end],
                    [leaf.geometry.bounds.x, leaf.geometry.bounds.y],
                    clip(clips.source)?,
                    clip(clips.inherited)?,
                )?;
            }
        }
    }
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(animated.len())
        .map_err(|error| error.to_string())?;
    nodes.extend_from_slice(animated);
    drop(cached);
    scene.publish_component_scene(next, owner);
    let identity = Arc::clone(scene.geometry_stamp().0);
    *cache.borrow_mut() = Some(ComponentPage {
        owner,
        page,
        comparisons,
        revision: layout.revision(),
        nodes,
        identity,
        prefix,
    });
    Ok(())
}

fn page_geometry(
    layout: &MountedLayout<Component>,
    content: &PageContent,
) -> Result<GeometrySnapshot, String> {
    let [width, height] = layout.extent();
    let mut scene = Scene::with_capacity(width, height, 1024);
    for leaf in layout.leaves() {
        let geometry = leaf.geometry;
        if geometry.clip.width == 0 || geometry.clip.height == 0 {
            continue;
        }
        let clip = ClipRect::new([
            geometry.clip.x,
            geometry.clip.y,
            geometry.clip.width,
            geometry.clip.height,
        ])?;
        let x = geometry.bounds.x as usize;
        let y = geometry.bounds.y as usize;
        match leaf.component {
            Component::Scope => {
                text_clipped(&mut scene, x, y, &content.scope, 1, DETAIL_COLOR, clip)?
            }
            Component::Card(index) => {
                if let Some(card) = content.cards.get(index) {
                    for line in card {
                        text_clipped(
                            &mut scene,
                            x,
                            y + line.y,
                            &line.label,
                            line.scale,
                            line.color,
                            clip,
                        )?;
                    }
                }
            }
            Component::Footer => {
                text_clipped(&mut scene, x, y, &content.footer, 1, DETAIL_COLOR, clip)?
            }
        }
    }
    scene.geometry_snapshot()
}

fn stage_pages(
    layout: &MountedLayout<Component>,
    contents: &[PageContent],
) -> Result<Vec<GeometrySnapshot>, String> {
    let mut pages = Vec::new();
    pages
        .try_reserve_exact(contents.len())
        .map_err(|error| error.to_string())?;
    for content in contents {
        pages.push(page_geometry(layout, content)?);
    }
    Ok(pages)
}

fn publish_layout(
    layout: &mut MountedLayout<Component>,
    candidate: MountedLayout<Component>,
    pages: &mut [GeometrySnapshot],
    contents: &[PageContent],
    comparisons: &mut [GeometrySnapshot],
    comparison_contents: &[PageContent],
) -> Result<bool, String> {
    if !candidate.suspended() {
        // Stage both modes before replacing any packet or publishing geometry.
        let staged = stage_pages(&candidate, contents)?;
        let comparison_staged = stage_pages(&candidate, comparison_contents)?;
        for (old, next) in pages.iter_mut().zip(staged) {
            *old = next;
        }
        for (old, next) in comparisons.iter_mut().zip(comparison_staged) {
            *old = next;
        }
    }
    *layout = candidate;
    Ok(true)
}

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

/// Labels freeze before admission. Page packets reuse them only on explicit
/// mounted geometry edits; ordinary composition borrows the existing caches.
pub struct ResultsView {
    rows: Vec<ResultRow>,
    scope_label: String,
    pages: Vec<GeometrySnapshot>,
    details: Vec<FrozenResultDetails>,
    comparison_pages: Vec<GeometrySnapshot>,
    layout: MountedLayout<Component>,
    page_contents: Vec<PageContent>,
    comparison_contents: Vec<PageContent>,
    component_page: RefCell<Option<ComponentPage>>,
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
            let archived = crate::result_archive::ArchivedResult {
                scope: result.scope(),
                outcome: result.outcome(),
                gauge: result.gauge(),
            };
            let (identity_label, outcome_label, gauge_label) = result_labels(*player, archived);
            rows.push(ResultRow {
                player: *player,
                result,
                identity_label,
                outcome_label,
                gauge_label,
            });
        }
        let layout = MountedLayout::mount(SCREEN)?;
        let mut pages = Vec::new();
        let mut page_contents = Vec::new();
        if simple {
            let display_rows: Vec<_> = rows
                .iter()
                .map(|row| FrozenResultRow {
                    player: row.player,
                    result: crate::result_archive::ArchivedResult {
                        scope: row.result.scope(),
                        outcome: row.result.outcome(),
                        gauge: row.result.gauge(),
                    },
                })
                .collect();
            (pages, page_contents) = simple_result_pages(&layout, &scope_label, &display_rows)?;
        }
        Ok(Self {
            rows,
            scope_label,
            pages,
            details: Vec::new(),
            comparison_pages: Vec::new(),
            layout,
            page_contents,
            comparison_contents: Vec::new(),
            component_page: RefCell::new(None),
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
            append_result_cards(
                &row.identity_label,
                &row.outcome_label,
                &row.gauge_label,
                detail.score.hits,
                detail.score.misses,
                detail.score.combo,
                detail.score.max_combo,
                crate::timing_display::summary(&detail.score.timing),
                detail
                    .score
                    .grades
                    .iter()
                    .map(|(grade, count)| format!("GRADE G{grade} COUNT {count}"))
                    .collect(),
                detail.competition,
                &mut detail_cards,
                &mut comparison_cards,
            );
            view.details.push(FrozenResultDetails {
                player: row.player,
                score: detail.score.clone(),
                competition: detail.competition.cloned(),
            });
        }
        (view.pages, view.page_contents) =
            card_pages(&view.layout, &view.scope_label, &detail_cards, "DETAILS")?;
        (view.comparison_pages, view.comparison_contents) = card_pages(
            &view.layout,
            &view.scope_label,
            &comparison_cards,
            "COMPARISONS",
        )?;
        Ok(view)
    }
    pub fn export_visual(&self) -> Result<FrozenResultsModel, String> {
        let model = FrozenResultsModel {
            roster: self.rows.iter().map(|row| row.player).collect(),
            rows: self
                .rows
                .iter()
                .map(|row| FrozenResultRow {
                    player: row.player,
                    result: crate::result_archive::ArchivedResult {
                        scope: row.result.scope(),
                        outcome: row.result.outcome(),
                        gauge: row.result.gauge(),
                    },
                })
                .collect(),
            details: self
                .details
                .iter()
                .map(|detail| {
                    Ok(FrozenScoreDetails {
                        player: detail.player,
                        score: crate::result_archive::ArchivedScore::from_summary(&detail.score)
                            .map_err(|error| error.to_string())?,
                        competition: detail.competition.clone(),
                    })
                })
                .collect::<Result<_, String>>()?,
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
    /// Readonly mounted targets on the selected page; absent card slots are excluded.
    pub fn displayed_nodes(&self, page: usize, comparisons: bool) -> Result<Vec<NodeId>, String> {
        let (content, _) = content_for(
            &self.page_contents,
            &self.comparison_contents,
            page,
            comparisons,
        )?;
        Ok(displayed_nodes(&self.layout, content))
    }
    /// Cold capture binds node-local source geometry; repeated motion frames reuse it.
    pub fn compose_components_mode(
        &self,
        scene: &mut Scene,
        owner: ScreenInstanceId,
        page: usize,
        comparisons: bool,
        animated_nodes: &[NodeId],
    ) -> Result<(), String> {
        let (content, comparisons) = content_for(
            &self.page_contents,
            &self.comparison_contents,
            page,
            comparisons,
        )?;
        compose_components(
            scene,
            &self.layout,
            content,
            &self.component_page,
            owner,
            page,
            comparisons,
            animated_nodes,
        )
    }
    pub fn compose_mode(
        &self,
        scene: &mut Scene,
        page: usize,
        comparisons: bool,
    ) -> Result<(), String> {
        if comparisons && self.has_comparisons() {
            let packet = self
                .comparison_pages
                .get(page)
                .ok_or("Results comparison page is out of range")?;
            if self.layout.suspended() {
                return Ok(());
            }
            scene.append_geometry(packet)
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
        if self.layout.suspended() {
            return Ok(());
        }
        scene.append_geometry(packet)
    }
    /// Resize existing page allocations without changing frozen result evidence.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        let mut candidate = self.layout.clone();
        if !candidate.resize([width, height])? {
            return Ok(false);
        }
        self.publish_layout(candidate)
    }
    /// Explicit child flow/clip edits address the mounted hierarchy directly.
    pub fn update_layout(&mut self, updates: &[LayoutUpdate]) -> Result<bool, String> {
        let mut candidate = self.layout.clone();
        if !candidate.update(updates)? {
            return Ok(false);
        }
        self.publish_layout(candidate)
    }
    fn publish_layout(&mut self, candidate: MountedLayout<Component>) -> Result<bool, String> {
        publish_layout(
            &mut self.layout,
            candidate,
            &mut self.pages,
            &self.page_contents,
            &mut self.comparison_pages,
            &self.comparison_contents,
        )
    }
}

fn counters(hits: u64, misses: u64, combo: u64, max_combo: u64) -> String {
    format!("HITS {hits} MISSES {misses} COMBO {combo} MAX COMBO {max_combo}")
}

fn card_pages(
    layout: &MountedLayout<Component>,
    scope: &str,
    cards: &[Vec<String>],
    mode: &str,
) -> Result<(Vec<GeometrySnapshot>, Vec<PageContent>), String> {
    let mut contents = Vec::new();
    contents
        .try_reserve_exact(cards.len().div_ceil(PLAYERS_PER_PAGE))
        .map_err(|error| error.to_string())?;
    for (page, visible) in cards.chunks(PLAYERS_PER_PAGE).enumerate() {
        contents.push(PageContent {
            scope: scope.to_owned(),
            cards: visible
                .iter()
                .map(|card| {
                    card.iter()
                        .enumerate()
                        .map(|(line, label)| ResultLine {
                            label: label.clone(),
                            y: line * 10,
                            scale: 1,
                            color: if line == 0 { LABEL_COLOR } else { DETAIL_COLOR },
                        })
                        .collect()
                })
                .collect(),
            footer: format!(
                "LOCAL {mode} PAGE {}/{} - PGUP/PGDN",
                page + 1,
                cards.len().div_ceil(PLAYERS_PER_PAGE)
            ),
        });
    }
    Ok((stage_pages(layout, &contents)?, contents))
}

#[cfg(test)]
#[path = "results_detail_fixtures.rs"]
mod detail_fixtures;

#[cfg(test)]
#[path = "results_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "results_declarative_fixtures.rs"]
mod declarative_fixtures;

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
    crate::result_archive::validate_frozen_result(result)
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
    layout: MountedLayout<Component>,
    page_contents: Vec<PageContent>,
    comparison_contents: Vec<PageContent>,
    component_page: RefCell<Option<ComponentPage>>,
}
impl FrozenResultsView {
    pub fn from_model(model: FrozenResultsModel) -> Result<Self, String> {
        model.validate()?;
        let scope = result_scope_label(model.rows[0].result.scope);
        let layout = MountedLayout::mount(SCREEN)?;
        let mut detail_cards = Vec::new();
        let mut comparison_cards = Vec::new();
        let (pages, page_contents) = if model.details.is_empty() {
            simple_result_pages(&layout, &scope, &model.rows)?
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
            card_pages(&layout, &scope, &detail_cards, "DETAILS")?
        };
        let (comparison_pages, comparison_contents) =
            card_pages(&layout, &scope, &comparison_cards, "COMPARISONS")?;
        Ok(Self {
            model,
            pages,
            comparison_pages,
            layout,
            page_contents,
            comparison_contents,
            component_page: RefCell::new(None),
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
    /// Readonly mounted targets on the selected page; absent card slots are excluded.
    pub fn displayed_nodes(&self, page: usize, comparisons: bool) -> Result<Vec<NodeId>, String> {
        let (content, _) = content_for(
            &self.page_contents,
            &self.comparison_contents,
            page,
            comparisons,
        )?;
        Ok(displayed_nodes(&self.layout, content))
    }
    /// Cold capture binds node-local source geometry; repeated motion frames reuse it.
    pub fn compose_components_mode(
        &self,
        scene: &mut Scene,
        owner: ScreenInstanceId,
        page: usize,
        comparisons: bool,
        animated_nodes: &[NodeId],
    ) -> Result<(), String> {
        let (content, comparisons) = content_for(
            &self.page_contents,
            &self.comparison_contents,
            page,
            comparisons,
        )?;
        compose_components(
            scene,
            &self.layout,
            content,
            &self.component_page,
            owner,
            page,
            comparisons,
            animated_nodes,
        )
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
        let packet = pages.get(page).ok_or("frozen Results page out of range")?;
        if self.layout.suspended() {
            return Ok(());
        }
        scene.append_geometry(packet)
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        let mut candidate = self.layout.clone();
        if !candidate.resize([width, height])? {
            return Ok(false);
        }
        self.publish_layout(candidate)
    }
    pub fn update_layout(&mut self, updates: &[LayoutUpdate]) -> Result<bool, String> {
        let mut candidate = self.layout.clone();
        if !candidate.update(updates)? {
            return Ok(false);
        }
        self.publish_layout(candidate)
    }
    fn publish_layout(&mut self, candidate: MountedLayout<Component>) -> Result<bool, String> {
        publish_layout(
            &mut self.layout,
            candidate,
            &mut self.pages,
            &self.page_contents,
            &mut self.comparison_pages,
            &self.comparison_contents,
        )
    }
}

fn simple_result_pages(
    layout: &MountedLayout<Component>,
    scope: &str,
    rows: &[FrozenResultRow],
) -> Result<(Vec<GeometrySnapshot>, Vec<PageContent>), String> {
    let mut contents = Vec::new();
    contents
        .try_reserve_exact(rows.len().div_ceil(PLAYERS_PER_PAGE))
        .map_err(|error| error.to_string())?;
    for (page, visible) in rows.chunks(PLAYERS_PER_PAGE).enumerate() {
        let mut cards = Vec::new();
        for row in visible {
            let (identity, outcome, gauge) = result_labels(row.player, row.result);
            let color = match row.result.outcome {
                PlayResultOutcome::Cleared => 0x74e5c5,
                PlayResultOutcome::BelowClearThreshold => 0xd8b36b,
                PlayResultOutcome::Failed(_) => 0xff8e8e,
            };
            cards.push(vec![
                ResultLine {
                    label: identity,
                    y: 0,
                    scale: 2,
                    color: LABEL_COLOR,
                },
                ResultLine {
                    label: outcome,
                    y: 28,
                    scale: 2,
                    color,
                },
                ResultLine {
                    label: gauge,
                    y: 55,
                    scale: 2,
                    color: DETAIL_COLOR,
                },
            ]);
        }
        contents.push(PageContent {
            scope: scope.to_owned(),
            cards,
            footer: format!(
                "LOCAL RESULTS PAGE {}/{} - PGUP/PGDN",
                page + 1,
                rows.len().div_ceil(PLAYERS_PER_PAGE)
            ),
        });
    }
    Ok((stage_pages(layout, &contents)?, contents))
}

#[cfg(test)]
mod component_geometry_tests {
    use super::*;
    use crate::{gauge::BmsGauge, scene::UiTransform, texture::TextureId};
    use beatkernel::time::Timestamp;

    enum View {
        Genuine(ResultsView),
        Frozen(FrozenResultsView),
    }
    impl View {
        fn new(frozen: bool) -> Self {
            let result =
                CompletedPlayResult::from_completed(Timestamp::ZERO, None, &BmsGauge::default());
            let roster: Vec<_> = (1..=5).map(PlayerId).collect();
            let rows: Vec<_> = roster.iter().map(|&id| (id, result)).collect();
            let view = ResultsView::new(&rows, &roster).unwrap();
            if frozen {
                Self::Frozen(FrozenResultsView::from_model(view.export_visual().unwrap()).unwrap())
            } else {
                Self::Genuine(view)
            }
        }
        fn nodes(&self, page: usize) -> Vec<NodeId> {
            match self {
                Self::Genuine(v) => v.displayed_nodes(page, false).unwrap(),
                Self::Frozen(v) => v.displayed_nodes(page, false).unwrap(),
            }
        }
        fn compose(&self, scene: &mut Scene, page: usize, nodes: &[NodeId]) {
            match self {
                Self::Genuine(v) => {
                    v.compose_components_mode(scene, ScreenInstanceId(81), page, false, nodes)
                }
                Self::Frozen(v) => {
                    v.compose_components_mode(scene, ScreenInstanceId(81), page, false, nodes)
                }
            }
            .unwrap();
        }
        fn ordinary(&self, scene: &mut Scene) {
            match self {
                Self::Genuine(v) => v.compose_mode(scene, 0, false),
                Self::Frozen(v) => v.compose_mode(scene, 0, false),
            }
            .unwrap();
        }
        fn move_card_beyond_ancestor(&mut self) -> NodeId {
            let layout = match self {
                Self::Genuine(v) => &v.layout,
                Self::Frozen(v) => &v.layout,
            };
            let card0 = layout
                .leaves()
                .iter()
                .find(|leaf| leaf.component == Component::Card(0))
                .unwrap()
                .id;
            let card = layout
                .leaves()
                .iter()
                .find(|leaf| leaf.component == Component::Card(1))
                .unwrap()
                .id;
            let updates = [LayoutUpdate {
                id: card0,
                change: super::super::layout::LayoutChange::Size([912, 420]),
            }];
            match self {
                Self::Genuine(v) => v.update_layout(&updates),
                Self::Frozen(v) => v.update_layout(&updates),
            }
            .unwrap();
            card
        }
    }

    fn bytes(scene: &Scene) -> Vec<u8> {
        bytemuck::cast_slice::<_, u8>(scene.rectangles()).to_vec()
    }

    #[test]
    fn genuine_and_frozen_preserve_actual_header_triangles_and_painter_order_on_page_rebind() {
        for frozen in [false, true] {
            let view = View::new(frozen);
            let mut scene = Scene::new(960, 720);
            scene.rect(2, 3, 9, 11, 0x123456);
            scene.rect(15, 17, 5, 7, 0xabcdef);
            let prefix = bytes(&scene);
            let node = view.nodes(0)[1];
            view.compose(&mut scene, 0, &[node]);
            assert_eq!(&bytes(&scene)[..prefix.len()], prefix.as_slice());
            let first = &scene.batches()[0];
            assert_eq!(first.texture, TextureId::WHITE);
            assert_eq!((first.first, first.count, first.component), (0, 2, 0));
            assert!(scene.batches()[1..].iter().all(|batch| batch.first >= 2));
            let id = scene
                .component_id(UiComponentKey {
                    screen: ScreenInstanceId(81),
                    node,
                })
                .unwrap();
            scene
                .set_component_transforms(&[(
                    id,
                    UiTransform::new([8.0, 4.0], [1.0, 1.0], 1.0).unwrap(),
                )])
                .unwrap();
            let triangles = bytes(&scene);
            let identity = Arc::clone(scene.geometry_stamp().0);
            let epoch = scene.geometry_stamp().1;
            view.compose(&mut scene, 0, &[node]);
            assert_eq!(bytes(&scene), triangles);
            assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
            assert_eq!(scene.geometry_stamp().1, epoch);
            view.compose(&mut scene, 1, &[node]);
            assert_eq!(&bytes(&scene)[..prefix.len()], prefix.as_slice());
            assert_eq!(scene.batches()[0].count, 2);
            assert_eq!(scene.component_id(id.key()), Some(id));
            assert!(scene.geometry_stamp().1 > epoch);
        }
    }

    #[test]
    fn genuine_and_frozen_keep_ancestor_hidden_glyph_triangles_for_transform_reveal() {
        for frozen in [false, true] {
            let mut view = View::new(frozen);
            let card = view.move_card_beyond_ancestor();
            let mut ordinary = Scene::new(960, 720);
            view.ordinary(&mut ordinary);
            // Enlarging the first card pushes the next PLAYER glyph beyond
            // the fixed column bottom (y=570); the ordinary page crops it.
            let glyph = [24.0, 570.0, 10.0, 14.0];
            let uv = crate::font::glyph_uv('P');
            assert!(!ordinary
                .rectangles()
                .iter()
                .any(|r| r.bounds == glyph && r.uv == uv));
            let mut scene = Scene::new(960, 720);
            view.compose(&mut scene, 0, &[card]);
            let index = scene
                .rectangles()
                .iter()
                .position(|r| r.bounds == glyph && r.uv == uv)
                .expect("node-local capture must retain the whole hidden PLAYER glyph");
            assert!(scene.batches().iter().any(|batch| batch.component != 0
                && (batch.first as usize..(batch.first + batch.count) as usize).contains(&index)));
            let id = scene
                .component_id(UiComponentKey {
                    screen: ScreenInstanceId(81),
                    node: card,
                })
                .unwrap();
            assert_eq!(scene.project_component_point(id, (26.0, 572.0)), None);
            let source = bytes(&scene);
            scene
                .set_component_transforms(&[(
                    id,
                    UiTransform::new([0.0, -100.0], [1.0, 1.0], 1.0).unwrap(),
                )])
                .unwrap();
            // This point lies inside the actual retained glyph after its
            // transform and inside the fixed ancestor clip, not merely a box.
            assert_eq!(
                scene.project_component_point(id, (26.0, 472.0)),
                Some((26.0, 572.0))
            );
            assert_eq!(bytes(&scene), source);
            view.compose(&mut scene, 0, &[card]);
            assert_eq!(bytes(&scene), source);
            assert_eq!(
                scene.project_component_point(id, (26.0, 472.0)),
                Some((26.0, 572.0))
            );
        }
    }
}
