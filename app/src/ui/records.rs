//! Retained saved-record chooser presentation; metadata and preview work stay external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field_value, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText,
    record_model::{RecordCatalog, RecordPreview},
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use beatkernel::time::Timestamp;
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};

const DIRECTORY: Bounds = Bounds {
    x: 160,
    y: 108,
    width: 770,
    height: 34,
};
/// Shared button geometry in painter order, including conditionally visible pages.
pub const BUTTONS: [(ControlId, Bounds, &'static str); 12] = [
    (
        ControlId(56),
        Bounds {
            x: 620,
            y: 475,
            width: 150,
            height: 30,
        },
        "PREVIOUS",
    ),
    (
        ControlId(57),
        Bounds {
            x: 780,
            y: 475,
            width: 150,
            height: 30,
        },
        "NEXT",
    ),
    (
        ControlId(50),
        Bounds {
            x: 24,
            y: 620,
            width: 130,
            height: 34,
        },
        "SCAN",
    ),
    (
        ControlId(51),
        Bounds {
            x: 164,
            y: 620,
            width: 130,
            height: 34,
        },
        "PREVIEW",
    ),
    (
        ControlId(52),
        Bounds {
            x: 304,
            y: 620,
            width: 130,
            height: 34,
        },
        "ADD OWN",
    ),
    (
        ControlId(53),
        Bounds {
            x: 444,
            y: 620,
            width: 140,
            height: 34,
        },
        "ADD OTHER",
    ),
    (
        ControlId(54),
        Bounds {
            x: 594,
            y: 620,
            width: 140,
            height: 34,
        },
        "CLEAR ALL",
    ),
    (
        ControlId(55),
        Bounds {
            x: 754,
            y: 620,
            width: 176,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(59),
        Bounds {
            x: 754,
            y: 575,
            width: 176,
            height: 34,
        },
        "WATCH (W)",
    ),
    (
        ControlId(60),
        Bounds {
            x: 430,
            y: 575,
            width: 140,
            height: 34,
        },
        "REMOVE OWN",
    ),
    (
        ControlId(61),
        Bounds {
            x: 580,
            y: 575,
            width: 164,
            height: 34,
        },
        "REMOVE OTHER",
    ),
    (
        ControlId(66),
        Bounds {
            x: 304,
            y: 575,
            width: 110,
            height: 34,
        },
        "DETAILS (D)",
    ),
];
pub const DETAIL_BUTTONS: [(ControlId, Bounds, &str); 3] = [
    (
        ControlId(66),
        Bounds {
            x: 754,
            y: 620,
            width: 176,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(67),
        Bounds {
            x: 430,
            y: 575,
            width: 140,
            height: 34,
        },
        "PREVIOUS",
    ),
    (
        ControlId(68),
        Bounds {
            x: 580,
            y: 575,
            width: 164,
            height: 34,
        },
        "NEXT",
    ),
];
fn grade_pages(frame: &RecordsFrame<'_>) -> usize {
    frame.preview.map_or(1, |preview| {
        crate::historical_record_presentation::historical_page_count(
            preview.historical_score.as_deref(),
            preview.historical_comparison.as_deref(),
        )
    })
}
fn detail_available(id: ControlId, page: usize, pages: usize) -> bool {
    match id.0 {
        66 => true,
        67 => page > 0,
        68 => page.saturating_add(1) < pages,
        _ => false,
    }
}
pub struct RecordsFrame<'a> {
    pub directory: &'a LineEditor,
    pub directory_focused: bool,
    pub catalog: Option<&'a RecordCatalog>,
    pub selected: Option<usize>,
    pub first: usize,
    /// The coordinator supplies only the current compatible selected preview.
    pub preview: Option<&'a RecordPreview>,
    pub pending: bool,
    pub details: bool,
    pub grade_page: usize,
    pub opponents: usize,
    /// Exact selected path occurrences, ordered own/other, in the parent draft.
    pub selected_opponents: [usize; 2],
    pub message: Option<&'a str>,
    pub error: Option<&'a str>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
fn bounds(slot: usize) -> Bounds {
    Bounds {
        x: 24,
        y: 170 + slot as i64 * 30,
        width: 906,
        height: 28,
    }
}
fn available(frame: &RecordsFrame<'_>, id: ControlId) -> bool {
    if frame.pending {
        return false;
    }
    if frame.details {
        return detail_available(id, frame.grade_page, grade_pages(frame));
    }
    let count = frame.catalog.map_or(0, |catalog| catalog.entries.len());
    match id.0 {
        56 => frame.catalog.is_some() && frame.first > 0,
        57 => frame.catalog.is_some() && frame.first.saturating_add(10) < count,
        51 => frame.selected.is_some_and(|index| index < count),
        52 | 53 => frame.preview.is_some() && frame.opponents < 8,
        59 => frame.preview.is_some(),
        66 => frame
            .preview
            .is_some_and(|preview| preview.historical.is_some()),
        60 => frame.selected_opponents[0] > 0,
        61 => frame.selected_opponents[1] > 0,
        50 | 54 | 55 => true,
        _ => false,
    }
}
/// Hit admission from current borrowed state, independent of the desktop hit cache.
/// Reverse painter order matches composed geometry. Invalid or pending frames admit none.
pub fn hit(frame: &RecordsFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    let point = point?;
    if frame.pending || validate_frame(frame).is_err() {
        return None;
    }
    if frame.details {
        return DETAIL_BUTTONS.iter().rev().find_map(|(id, bounds, _)| {
            (available(frame, *id) && bounds.contains(point)).then_some(*id)
        });
    }
    for (id, bounds, _) in BUTTONS.iter().rev() {
        if available(frame, *id) && bounds.contains(point) {
            return Some(*id);
        }
    }
    let count = frame.catalog.map_or(0, |catalog| catalog.entries.len());
    for slot in (0..10).rev() {
        let index = frame.first + slot;
        if index < count && bounds(slot).contains(point) {
            return Some(ControlId(50000 + index as u64));
        }
    }
    DIRECTORY.contains(point).then_some(ControlId(58))
}
fn validate_frame(frame: &RecordsFrame<'_>) -> Result<(), String> {
    if (!frame.details && frame.grade_page != 0)
        || (frame.details && frame.grade_page >= grade_pages(frame))
    {
        return Err("Records stored grade page is out of range".into());
    }
    if frame.details
        && (frame.pending
            || frame
                .preview
                .is_none_or(|preview| preview.historical.is_none()))
    {
        return Err("Records details require idle associated historical metadata".into());
    }
    let count = frame.catalog.map_or(0, |catalog| catalog.entries.len());
    if count > 256
        || frame.opponents > crate::settings::MAX_FIELDS
        || frame
            .selected_opponents
            .iter()
            .any(|&count| count > crate::settings::MAX_FIELDS)
        || frame.selected_opponents.iter().sum::<usize>() > frame.opponents
        || (frame.selected.is_none() && frame.selected_opponents != [0; 2])
        || frame.first.checked_add(10).is_none()
        || (count == 0 && frame.first != 0)
        || (count > 0 && frame.first >= count)
        || frame.selected.is_some_and(|index| index >= count)
    {
        return Err("Records frame exceeds catalog or index bounds".into());
    }
    if let Some(preview) = frame.preview {
        if let Some(classes) = preview.bms_score {
            classes
                .validate_for(preview.score.hits, preview.score.misses)
                .map_err(|error| error.to_string())?;
        }
        if let Some(classes) = preview.historical_bms_score {
            let score = preview
                .historical_score
                .as_deref()
                .filter(|_| preview.historical.is_some())
                .ok_or("stored class score requires associated historical counts")?;
            classes
                .validate_for(score.hits, score.misses)
                .map_err(|error| error.to_string())?;
        }
        let selected = frame
            .selected
            .and_then(|index| frame.catalog?.entries.get(index));
        if selected != Some(&preview.path) {
            return Err("Records preview does not match the selected path".into());
        }
    }
    Ok(())
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    label: String,
    selected: bool,
    pending: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Preview {
    bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    records: usize,
    start: Timestamp,
    end: Option<Timestamp>,
    historical: Option<crate::record_model::HistoricalRecordValue>,
    archive_failed: bool,
    until: Option<Timestamp>,
    hits: u64,
    misses: u64,
    combo: u64,
    max_combo: u64,
    timing: crate::timing::TimingSummary,
}
impl From<&RecordPreview> for Preview {
    fn from(value: &RecordPreview) -> Self {
        Self {
            bms_score: value.bms_score,
            records: value.records,
            start: value.start,
            end: value.end,
            historical: value.historical,
            archive_failed: value.archive_error.is_some(),
            until: value.recorded_until,
            hits: value.score.hits,
            misses: value.score.misses,
            combo: value.score.combo,
            max_combo: value.score.max_combo,
            timing: value.score.timing,
        }
    }
}
/// Fixed node tree per Records instance, with only visible row labels retained.
struct DetailCache {
    bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    value: crate::record_model::HistoricalRecordValue,
    score: Option<Arc<crate::result_archive::ArchivedScore>>,
    comparison: Option<Arc<Option<crate::competition_presentation::CompetitionSnapshot>>>,
    presentation: crate::historical_record_presentation::HistoricalRecordPresentation,
    geometry: crate::scene::GeometrySnapshot,
    grade_page: usize,
    grade_geometry: crate::scene::GeometrySnapshot,
}
fn detail_geometry(
    presentation: &crate::historical_record_presentation::HistoricalRecordPresentation,
    grade_geometry: &crate::scene::GeometrySnapshot,
    page: usize,
    hovered: Option<ControlId>,
    armed: Option<ControlId>,
) -> Result<crate::scene::GeometrySnapshot, String> {
    let mut scene = Scene::with_capacity(960, 720, 1024);
    rect(&mut scene, 0, 0, 960, 720, 0x10151e);
    presentation.compose_body_for_page(page, &mut scene)?;
    scene.append_geometry(grade_geometry)?;
    for (id, bounds, label) in DETAIL_BUTTONS {
        if detail_available(id, page, presentation.grade_page_count()) {
            button(
                &mut scene,
                bounds,
                label,
                hovered == Some(id),
                armed == Some(id),
            );
        }
    }
    scene.geometry_snapshot()
}
pub struct RecordsView {
    details: Cell<bool>,
    detail_dirty: Cell<bool>,
    detail_hovered: Cell<Option<ControlId>>,
    detail_armed: Cell<Option<ControlId>>,
    detail_cache: RefCell<Option<DetailCache>>,
    id: ScreenInstanceId,
    scope: Scope,
    directory: RwSignal<LineEditor>,
    directory_focused: RwSignal<bool>,
    input_font: RwSignal<Option<FontText>>,
    rows: [RwSignal<Option<Row>>; 10],
    summary: RwSignal<Option<(usize, bool)>>,
    preview: RwSignal<Option<Preview>>,
    opponents: RwSignal<usize>,
    selected_opponents: RwSignal<[usize; 2]>,
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    gates: [RwSignal<bool>; 12],
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
}
impl RecordsView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let nodes = RetainedNodes::new(width, height)?;
        let directory = LineEditor::new("", 4096)?;
        let scope = Scope::new();
        let mut view = Self {
            details: Cell::new(false),
            detail_dirty: Cell::new(false),
            detail_hovered: Cell::new(None),
            detail_armed: Cell::new(None),
            detail_cache: RefCell::new(None),
            id,
            scope,
            directory: scope.create_rw_signal(directory),
            directory_focused: scope.create_rw_signal(true),
            input_font: scope.create_rw_signal(None),
            rows: std::array::from_fn(|_| scope.create_rw_signal(None)),
            summary: scope.create_rw_signal(None),
            preview: scope.create_rw_signal(None),
            opponents: scope.create_rw_signal(0),
            selected_opponents: scope.create_rw_signal([0; 2]),
            message: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            gates: std::array::from_fn(|_| scope.create_rw_signal(false)),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
        };
        view.nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x10151e);
            text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
            text(scene, 24, 65, "RECORDS - RECORDED PREFIX", 2, 0x9bb1cf);
        });
        let directory = view.directory;
        let focused = view.directory_focused;
        let pending = view.pending;
        let input_font = view.input_font;
        let memo = scope.create_memo(move |_| {
            (
                directory.get(),
                focused.get(),
                pending.get(),
                input_font.get(),
            )
        });
        view.nodes.bind(
            scope,
            memo,
            |(directory, focused, pending, font), scene, hits| {
                text(scene, 24, 120, "DIRECTORY", 1, 0xf0f4ff);
                text_field_with_font(
                    scene,
                    &directory,
                    DIRECTORY,
                    focused && !pending,
                    font.as_ref(),
                );
                if !pending {
                    hits.push((ControlId(58), DIRECTORY));
                }
            },
        );
        view.nodes.static_node(|scene, _| {
            text(
                scene,
                24,
                151,
                "TAB DIRECTORY/LIST   ENTER SCAN/PREVIEW   PGUP/PGDN PAGE",
                1,
                0x9bb1cf,
            )
        });
        for (slot, row) in view.rows.iter().copied().enumerate() {
            let memo = scope.create_memo(move |_| row.get());
            view.nodes.bind(scope, memo, move |row, scene, hits| {
                if let Some(row) = row {
                    let bounds = bounds(slot);
                    rect(
                        scene,
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        if row.selected { 0x29475e } else { 0x1d2734 },
                    );
                    text_field_value(
                        scene,
                        &row.label,
                        Bounds {
                            height: 30,
                            ..bounds
                        },
                    );
                    if row.selected {
                        rect(scene, bounds.x, bounds.y, 4, bounds.height, 0x74e5c5);
                    }
                    if !row.pending {
                        hits.push((ControlId(50000 + row.index as u64), bounds));
                    }
                }
            });
        }
        let summary = view.summary;
        let memo = scope.create_memo(move |_| summary.get());
        view.nodes.bind(scope, memo, |summary, scene, _| {
            if let Some((count, truncated)) = summary {
                if count == 0 {
                    text(scene, 24, 180, "NO DIRECT .BKR RECORDS", 1, 0x9bb1cf);
                }
                text(
                    scene,
                    24,
                    472,
                    &format!(
                        "{} RECORDS{}",
                        count,
                        if truncated {
                            " - DIRECTORY LIMIT REACHED"
                        } else {
                            ""
                        }
                    ),
                    1,
                    0xd8b36b,
                );
            }
        });
        for index in 0..2 {
            view.button_node(index);
        }
        let preview = view.preview;
        let pending = view.pending;
        let memo = scope.create_memo(move |_| {
            if pending.get() {
                (true, None)
            } else {
                (false, preview.get())
            }
        });
        view.nodes
            .bind(scope, memo, |(pending, preview), scene, _| {
                if pending {
                    text(scene, 24, 520, "LOADING RECORDS", 2, 0xd8b36b);
                } else if let Some(preview) = preview {
                    if let Some(classes) = preview.bms_score {
                        for (y, label) in [
                            (482, format!("PREFIX EX {}", classes.ex_score)),
                            (
                                492,
                                format!("PGREAT {} GREAT {}", classes.pgreat, classes.great),
                            ),
                            (
                                502,
                                format!(
                                    "GOOD {} BAD {} POOR {}",
                                    classes.good, classes.bad, classes.poor
                                ),
                            ),
                        ] {
                            text(scene, 24, y, &label, 1, 0xd8b36b);
                        }
                    } else {
                        text(
                            scene,
                            24,
                            482,
                            "PREFIX CLASS SCORE UNAVAILABLE",
                            1,
                            0x9bb1cf,
                        );
                    }
                    text(
                        scene,
                        24,
                        514,
                        &format!(
                            "OPERATIONS {}   START {:.3} S",
                            preview.records,
                            preview.start.as_nanos() as f64 / 1e9
                        ),
                        1,
                        0xb6cce6,
                    );
                    text(
                        scene,
                        24,
                        524,
                        &format!(
                            "UNTIL {}",
                            preview.until.map_or("UNKNOWN".into(), |at| format!(
                                "{:.3} S",
                                at.as_nanos() as f64 / 1e9
                            ))
                        ),
                        1,
                        0xb6cce6,
                    );
                    text(
                        scene,
                        24,
                        534,
                        &format!("HITS {} MISSES {}", preview.hits, preview.misses),
                        1,
                        0x9bb1cf,
                    );
                    text(
                        scene,
                        24,
                        544,
                        &format!("COMBO {} MAX {}", preview.combo, preview.max_combo),
                        1,
                        0x9bb1cf,
                    );
                    let (bias, absolute) = crate::timing_display::summary(&preview.timing);
                    text(scene, 24, 554, &bias, 1, 0x9bb1cf);
                    text(scene, 24, 564, &absolute, 1, 0x9bb1cf);
                    let end = preview.end.map_or_else(
                        || "END UNLIMITED".into(),
                        |end| format!("END {} NS", end.as_nanos()),
                    );
                    text(scene, 560, 514, &end, 1, 0xb6cce6);
                    if let Some((player, result)) = preview.historical {
                        use crate::play_result::{PlayResultScope, PlayResultOutcome};
                        use crate::gauge::GaugeFailure;
                        text(
                            scene,
                            560,
                            524,
                            &format!("HISTORICAL PLAYER {}", player.0),
                            1,
                            0xd8b36b,
                        );
                        text(
                            scene,
                            560,
                            534,
                            match result.scope {
                                PlayResultScope::FullSong => "STORED SCOPE FULL SONG",
                                PlayResultScope::PracticeSection { .. } => {
                                    "STORED SCOPE PRACTICE SECTION"
                                }
                            },
                            1,
                            0xd8b36b,
                        );
                        text(
                            scene,
                            560,
                            544,
                            match result.outcome {
                                PlayResultOutcome::Cleared => "STORED OUTCOME CLEARED",
                                PlayResultOutcome::BelowClearThreshold => {
                                    "STORED OUTCOME BELOW CLEAR"
                                }
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
                            scene,
                            560,
                            554,
                            &format!("STORED GAUGE {} UNITS", result.gauge.level_units),
                            1,
                            0xd8b36b,
                        );
                        if preview.archive_failed {
                            text(scene, 560, 564, "ARCHIVE DIAGNOSTIC", 1, 0xf07878);
                        }
                    } else {
                        text(
                            scene,
                            560,
                            524,
                            if preview.archive_failed {
                                "HISTORICAL ARCHIVE UNAVAILABLE"
                            } else {
                                "NO HISTORICAL ARCHIVE"
                            },
                            1,
                            if preview.archive_failed {
                                0xf07878
                            } else {
                                0x9bb1cf
                            },
                        );
                    }
                } else {
                    text(
                        scene,
                        24,
                        520,
                        "PREVIEW A COMPATIBLE RECORD BEFORE WATCH / ADD",
                        1,
                        0x9bb1cf,
                    );
                }
            });
        let opponents = view.opponents;
        let memo = scope.create_memo(move |_| opponents.get());
        view.nodes.bind(scope, memo, |opponents, scene, _| {
            text(
                scene,
                24,
                592,
                &format!("SAVED GHOSTS {opponents}/8 - DRAFT ONLY"),
                1,
                0x9bb1cf,
            )
        });
        let selected_opponents = view.selected_opponents;
        let memo = scope.create_memo(move |_| selected_opponents.get());
        view.nodes.bind(scope, memo, |counts, scene, _| {
            text(
                scene,
                24,
                576,
                &format!("SELECTED OWN {} / OTHER {}", counts[0], counts[1]),
                1,
                0x9bb1cf,
            );
        });
        for index in 2..BUTTONS.len() {
            view.button_node(index);
        }
        let message = view.message;
        let memo = scope.create_memo(move |_| message.get());
        view.nodes.bind(scope, memo, |message, scene, _| {
            if let Some(message) = message {
                text(scene, 24, 665, &message, 1, 0x74e5c5);
            }
        });
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind(scope, memo, |error, scene, _| {
            if let Some(error) = error {
                text_field_value(
                    scene,
                    &error,
                    Bounds {
                        x: 24,
                        y: 682,
                        width: 906,
                        height: 30,
                    },
                );
            }
        });
        view.nodes.validate()?;
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn set_input_font(&self, font: Option<FontText>) {
        if self.input_font.get_untracked() != font {
            self.input_font.set(font);
        }
    }
    pub fn update(&self, frame: RecordsFrame<'_>) -> Result<(), String> {
        validate_frame(&frame)?;
        let mut staged = None;
        let mut staged_geometry = None;
        if frame.details {
            let preview = frame.preview.expect("validated detail preview");
            let value = preview.historical.expect("validated historical value");
            let same = self.detail_cache.borrow().as_ref().is_some_and(|cache| {
                cache.value == value
                    && cache.bms_score == preview.historical_bms_score
                    && match (&cache.score, &preview.historical_score) {
                        (None, None) => true,
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        _ => false,
                    }
                    && match (&cache.comparison, &preview.historical_comparison) {
                        (None, None) => true,
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        _ => false,
                    }
            });
            if !same {
                let presentation = crate::historical_record_presentation::HistoricalRecordPresentation::from_record_with_class_score(value, preview.historical_score.as_deref(), preview.historical_comparison.as_deref(), preview.historical_bms_score)?;
                let grade_geometry = presentation.prepare_grade_page(frame.grade_page)?;
                let geometry = detail_geometry(
                    &presentation,
                    &grade_geometry,
                    frame.grade_page,
                    frame.hovered,
                    frame.armed,
                )?;
                staged = Some(DetailCache {
                    bms_score: preview.historical_bms_score,
                    value,
                    score: preview.historical_score.clone(),
                    comparison: preview.historical_comparison.clone(),
                    presentation,
                    geometry,
                    grade_page: frame.grade_page,
                    grade_geometry,
                });
            } else if !self.details.get()
                || self
                    .detail_cache
                    .borrow()
                    .as_ref()
                    .is_some_and(|cache| cache.grade_page != frame.grade_page)
                || self.detail_hovered.get() != frame.hovered
                || self.detail_armed.get() != frame.armed
            {
                let cache = self.detail_cache.borrow();
                let cache = cache.as_ref().expect("matching detail cache");
                let grade_geometry = if cache.grade_page == frame.grade_page {
                    cache.grade_geometry.clone()
                } else {
                    cache.presentation.prepare_grade_page(frame.grade_page)?
                };
                let geometry = detail_geometry(
                    &cache.presentation,
                    &grade_geometry,
                    frame.grade_page,
                    frame.hovered,
                    frame.armed,
                )?;
                staged_geometry = Some((frame.grade_page, grade_geometry, geometry));
            }
        }
        if let Some(cache) = staged {
            *self.detail_cache.borrow_mut() = Some(cache);
            self.detail_dirty.set(true);
        }
        if let Some((page, grade_geometry, geometry)) = staged_geometry {
            let mut cache = self.detail_cache.borrow_mut();
            let cache = cache.as_mut().expect("prepared detail cache");
            cache.grade_page = page;
            cache.grade_geometry = grade_geometry;
            cache.geometry = geometry;
            self.detail_dirty.set(true);
        }
        if self.details.replace(frame.details) != frame.details {
            self.detail_dirty.set(true);
        }
        if frame.details {
            if self.detail_hovered.get() != frame.hovered || self.detail_armed.get() != frame.armed
            {
                self.detail_dirty.set(true);
            }
            if self.detail_hovered.get() != frame.hovered {
                self.detail_hovered.set(frame.hovered);
            }
            if self.detail_armed.get() != frame.armed {
                self.detail_armed.set(frame.armed);
            }
            return Ok(());
        }
        if !self.directory.with_untracked(|old| old == frame.directory) {
            self.directory.set(frame.directory.clone());
        }
        if self.directory_focused.get_untracked() != frame.directory_focused {
            self.directory_focused.set(frame.directory_focused);
        }
        for (slot, signal) in self.rows.iter().enumerate() {
            let index = frame.first + slot;
            let path = frame.catalog.and_then(|catalog| catalog.entries.get(index));
            if let Some(path) = path {
                let label = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                let selected = frame.selected == Some(index);
                if !signal.with_untracked(|old| {
                    old.as_ref().is_some_and(|old| {
                        old.index == index
                            && old.label == label
                            && old.selected == selected
                            && old.pending == frame.pending
                    })
                }) {
                    signal.set(Some(Row {
                        index,
                        label: label.into_owned(),
                        selected,
                        pending: frame.pending,
                    }));
                }
            } else if signal.with_untracked(Option::is_some) {
                signal.set(None);
            }
        }
        let summary = frame
            .catalog
            .map(|catalog| (catalog.entries.len(), catalog.truncated));
        if self.summary.get_untracked() != summary {
            self.summary.set(summary);
        }
        let preview = frame.preview.map(Preview::from);
        if self.preview.get_untracked() != preview {
            self.preview.set(preview);
        }
        if self.opponents.get_untracked() != frame.opponents {
            self.opponents.set(frame.opponents);
        }
        if self.selected_opponents.get_untracked() != frame.selected_opponents {
            self.selected_opponents.set(frame.selected_opponents);
        }
        if !self
            .message
            .with_untracked(|old| old.as_deref() == frame.message)
        {
            self.message.set(frame.message.map(str::to_owned));
        }
        if !self
            .error
            .with_untracked(|old| old.as_deref() == frame.error)
        {
            self.error.set(frame.error.map(str::to_owned));
        }
        if self.pending.get_untracked() != frame.pending {
            self.pending.set(frame.pending);
        }
        for (signal, (id, _, _)) in self.gates.iter().zip(BUTTONS) {
            let gate = available(&frame, id);
            if signal.get_untracked() != gate {
                signal.set(gate);
            }
        }
        if self.hovered.get_untracked() != frame.hovered {
            self.hovered.set(frame.hovered);
        }
        if self.armed.get_untracked() != frame.armed {
            self.armed.set(frame.armed);
        }
        Ok(())
    }
    pub fn dirty(&self) -> bool {
        self.detail_dirty.get() || (!self.details.get() && self.nodes.dirty())
    }
    pub fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        if self.details.get() {
            let cache = self.detail_cache.borrow();
            let cache = cache.as_ref().ok_or("Records details cache unavailable")?;
            scene.clear();
            hits.clear();
            scene.append_geometry(&cache.geometry)?;
            for (id, bounds, _) in DETAIL_BUTTONS {
                if detail_available(id, cache.grade_page, cache.presentation.grade_page_count()) {
                    hits.push((id, bounds));
                }
            }
        } else {
            self.nodes.compose(scene, hits)?;
        }
        self.detail_dirty.set(false);
        Ok(())
    }
    fn button_node(&mut self, index: usize) {
        let (id, bounds, label) = BUTTONS[index];
        let gate = self.gates[index];
        let hovered = self.hovered;
        let armed = self.armed;
        let memo = self.scope.create_memo(move |_| {
            if gate.get() {
                (true, hovered.get() == Some(id), armed.get() == Some(id))
            } else {
                (false, false, false)
            }
        });
        self.nodes.bind(
            self.scope,
            memo,
            move |(available, hovered, armed), scene, hits| {
                if index < 2 && !available {
                    return;
                }
                button(scene, bounds, label, hovered, armed);
                if available {
                    hits.push((id, bounds));
                }
            },
        );
    }
}
impl Drop for RecordsView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::competition::ScoreSummary;
    fn catalog(count: usize) -> RecordCatalog {
        RecordCatalog {
            entries: (0..count)
                .map(|index| format!("records/{index}.bkr").into())
                .collect(),
            truncated: false,
        }
    }
    fn frame<'a>(directory: &'a LineEditor, catalog: &'a RecordCatalog) -> RecordsFrame<'a> {
        RecordsFrame {
            directory,
            directory_focused: true,
            catalog: Some(catalog),
            selected: (!catalog.entries.is_empty()).then_some(0),
            first: 0,
            preview: None,
            pending: false,
            details: false,
            grade_page: 0,
            opponents: 0,
            selected_opponents: [0; 2],
            message: None,
            error: None,
            hovered: None,
            armed: None,
        }
    }
    #[test]
    fn selective_membership_gates_survive_overfull_drafts_without_repainting_rows() {
        let view = RecordsView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let catalog = catalog(1);
        let directory = LineEditor::new("records", 4096).unwrap();
        let mut value = frame(&directory, &catalog);
        value.opponents = 3;
        value.selected_opponents = [2, 1];
        assert_eq!(hit(&value, Some((450.0, 580.0))), Some(ControlId(60)));
        assert_eq!(hit(&value, Some((600.0, 580.0))), Some(ControlId(61)));
        view.update(value).unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.iter().any(|(id, _)| id.0 == 60));
        assert!(hits.iter().any(|(id, _)| id.0 == 61));
        let before = view.nodes.paints();
        let mut value = frame(&directory, &catalog);
        value.opponents = 2;
        value.selected_opponents = [1, 1];
        view.update(value).unwrap();
        assert_eq!(&view.nodes.paints()[3..13], &before[3..13]);
        let same = view.nodes.paints();
        let mut value = frame(&directory, &catalog);
        value.opponents = 2;
        value.selected_opponents = [1, 1];
        view.update(value).unwrap();
        assert_eq!(view.nodes.paints(), same);
        let mut overfull = frame(&directory, &catalog);
        overfull.opponents = 9;
        overfull.selected_opponents = [9, 0];
        assert_eq!(hit(&overfull, Some((450.0, 580.0))), Some(ControlId(60)));
        assert!(!available(&overfull, ControlId(52)));
        view.update(overfull).unwrap();
        let before = view.nodes.paints();
        for counts in [[usize::MAX, 0], [9, 1]] {
            let mut invalid = frame(&directory, &catalog);
            invalid.opponents = 9;
            invalid.selected_opponents = counts;
            assert!(view.update(invalid).is_err());
            assert_eq!(view.nodes.paints(), before);
        }
        let mut pending = frame(&directory, &catalog);
        pending.opponents = 1;
        pending.selected_opponents = [1, 0];
        pending.pending = true;
        assert_eq!(hit(&pending, Some((450.0, 580.0))), None);
        view.update(pending).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
    }
    #[test]
    fn no_op_error_directory_and_selection_updates_repaint_only_dependencies() {
        let view = RecordsView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let catalog = catalog(25);
        let mut directory = LineEditor::new("records", 4096).unwrap();
        view.update(frame(&directory, &catalog)).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = view.nodes.paints();
        view.update(frame(&directory, &catalog)).unwrap();
        assert_eq!(view.nodes.paints(), before);
        assert!(!view.dirty());
        let mut update = frame(&directory, &catalog);
        update.error = Some("ERROR");
        view.update(update).unwrap();
        let error = view.nodes.paints();
        let error_node = before.len() - 1;
        assert_eq!(&error[..error_node], &before[..error_node]);
        assert_eq!(error[error_node], before[error_node] + 1);
        directory.left();
        let mut update = frame(&directory, &catalog);
        update.error = Some("ERROR");
        view.update(update).unwrap();
        let cursor = view.nodes.paints();
        assert_eq!(cursor[1], error[1] + 1);
        assert_eq!(&cursor[..1], &error[..1]);
        assert_eq!(&cursor[2..], &error[2..]);
        let mut update = frame(&directory, &catalog);
        update.selected = Some(1);
        update.error = Some("ERROR");
        view.update(update).unwrap();
        let selected = view.nodes.paints();
        assert_eq!(selected[3], cursor[3] + 1);
        assert_eq!(selected[4], cursor[4] + 1);
        assert_eq!(&selected[..3], &cursor[..3]);
        assert_eq!(&selected[5..], &cursor[5..]);
    }
    #[test]
    fn paging_preview_pending_and_hit_projection_match_actual_painter_order() {
        let view = RecordsView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let catalog = catalog(25);
        let directory = LineEditor::new("records", 4096).unwrap();
        let mut update = frame(&directory, &catalog);
        update.first = 10;
        update.selected = Some(10);
        let preview = RecordPreview {
            path: catalog.entries[10].clone(),
            records: 3,
            recorded_until: Some(Timestamp::from_nanos(200_000_000)),
            start: Timestamp::ZERO,
            end: None,
            historical: None,
            historical_score: None,
            bms_score: None,
            historical_bms_score: None,
            historical_comparison: None,
            archive_error: None,
            score: ScoreSummary {
                hits: 2,
                misses: 1,
                combo: 0,
                max_combo: 2,
                ..ScoreSummary::default()
            },
        };
        update.preview = Some(&preview);
        update.opponents = 7;
        assert_eq!(hit(&update, Some((30.0, 175.0))), Some(ControlId(50010)));
        assert_eq!(hit(&update, Some((630.0, 480.0))), Some(ControlId(56)));
        assert_eq!(hit(&update, Some((790.0, 480.0))), Some(ControlId(57)));
        assert_eq!(hit(&update, Some((760.0, 580.0))), Some(ControlId(59)));
        assert_eq!(hit(&update, Some((170.0, 115.0))), Some(ControlId(58)));
        view.update(update).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            std::iter::once(58)
                .chain(50010..50020)
                .chain([56, 57, 50, 51, 52, 53, 54, 55, 59])
                .collect::<Vec<_>>()
        );
        let mut capped = frame(&directory, &catalog);
        capped.first = 10;
        capped.selected = Some(10);
        capped.preview = Some(&preview);
        capped.opponents = 8;
        assert!(!available(&capped, ControlId(52)));
        assert!(!available(&capped, ControlId(53)));
        view.update(capped).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.iter().all(|(id, _)| !matches!(id.0, 52 | 53)));
        let mut pending = frame(&directory, &catalog);
        pending.pending = true;
        pending.first = 10;
        pending.selected = Some(10);
        pending.preview = Some(&preview);
        pending.hovered = Some(ControlId(59));
        pending.armed = Some(ControlId(59));
        assert_eq!(hit(&pending, Some((760.0, 580.0))), None);
        view.update(pending).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
        let mut tail = frame(&directory, &catalog);
        tail.first = 20;
        tail.selected = Some(20);
        view.update(tail).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(!hits.iter().any(|(id, _)| matches!(id.0, 57 | 52 | 53 | 59)));
        assert!(hits.iter().any(|(id, _)| id.0 == 56));
    }
    #[test]
    fn timing_only_preview_updates_one_retained_node_and_unchanged_preview_stays_cached() {
        use beatkernel::{
            chart::ObjectId,
            judge::{JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage},
            time::Duration,
        };
        let view = RecordsView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let catalog = catalog(1);
        let directory = LineEditor::new("records", 4096).unwrap();
        let mut preview = RecordPreview {
            path: catalog.entries[0].clone(),
            records: 1,
            recorded_until: Some(Timestamp::ZERO),
            start: Timestamp::ZERO,
            end: None,
            historical: None,
            historical_score: None,
            bms_score: None,
            historical_bms_score: None,
            historical_comparison: None,
            archive_error: None,
            score: ScoreSummary::default(),
        };
        let mut update = frame(&directory, &catalog);
        update.selected = Some(0);
        update.preview = Some(&preview);
        view.update(update).unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = view.nodes.paints();
        preview
            .score
            .timing
            .observe(&[JudgeEvent {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: Duration::from_nanos(-1_234_567),
                },
                at: Timestamp::ZERO,
                input: None,
            }])
            .unwrap();
        let mut update = frame(&directory, &catalog);
        update.selected = Some(0);
        update.preview = Some(&preview);
        view.update(update).unwrap();
        let after = view.nodes.paints();
        assert_eq!(
            after
                .iter()
                .zip(&before)
                .filter(|(new, old)| new != old)
                .count(),
            1
        );
        scene.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(scene.rectangles().iter().any(|rect| rect.bounds[0] >= 24.0
            && rect.bounds[0] < 400.0
            && rect.bounds[1] >= 554.0
            && rect.bounds[1] < 561.0));
        let mut same = frame(&directory, &catalog);
        same.selected = Some(0);
        same.preview = Some(&preview);
        view.update(same).unwrap();
        assert_eq!(view.nodes.paints(), after);
        assert!(!view.dirty());
    }
    #[test]
    fn invalid_frames_are_atomic_and_restore_disposal_keeps_shared_node_contract() {
        let view = RecordsView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let catalog = catalog(2);
        let directory = LineEditor::new("records", 4096).unwrap();
        view.update(frame(&directory, &catalog)).unwrap();
        let before = view.nodes.paints();
        let identities = view.nodes.identities();
        let mut invalid = frame(&directory, &catalog);
        invalid.selected = Some(2);
        invalid.error = Some("MUST NOT CHANGE");
        assert!(view.update(invalid).is_err());
        let mut invalid = frame(&directory, &catalog);
        invalid.first = usize::MAX;
        assert!(view.update(invalid).is_err());
        let huge = self::catalog(257);
        assert!(view.update(frame(&directory, &huge)).is_err());
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.error.get_untracked(), None);
        let stale = RecordPreview {
            path: "different.bkr".into(),
            records: 0,
            recorded_until: None,
            start: Timestamp::ZERO,
            end: None,
            historical: None,
            historical_score: None,
            bms_score: None,
            historical_bms_score: None,
            historical_comparison: None,
            archive_error: None,
            score: ScoreSummary::default(),
        };
        let mut invalid = frame(&directory, &catalog);
        invalid.preview = Some(&stale);
        assert!(view.update(invalid).is_err());
        assert_eq!(view.nodes.paints(), before);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let count = scene.rectangles().len();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.nodes.identities(), identities);
        assert!(!view.dirty());
        let weak = view.nodes.weak_dirty();
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(RecordsView::new(ScreenInstanceId(8), 800, 600).is_err());
    }
    #[test]
    fn excessive_opponent_draft_still_displays_count_and_allows_clear_and_back() {
        let view = RecordsView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let catalog = catalog(0);
        let directory = LineEditor::new("records", 4096).unwrap();
        let mut update = frame(&directory, &catalog);
        update.opponents = 9;
        assert_eq!(hit(&update, Some((600.0, 625.0))), Some(ControlId(54)));
        assert_eq!(hit(&update, Some((760.0, 625.0))), Some(ControlId(55)));
        view.update(update).unwrap();
        assert_eq!(view.opponents.get_untracked(), 9);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.iter().any(|(id, _)| *id == ControlId(54)));
        assert!(hits.iter().any(|(id, _)| *id == ControlId(55)));
        // This node displays the actual draft count, including9/8, so invalid
        // repeated settings remain editable rather than disabling the screen.
        assert!(
            scene
                .rectangles()
                .iter()
                .any(|rect| rect.bounds[1] == 592.0)
        );
    }
}

#[cfg(test)]
#[path = "records_archive_fixtures.rs"]
mod archive_fixtures;

#[cfg(test)]
#[path = "records_stored_score_fixtures.rs"]
mod records_stored_score_fixtures;

#[cfg(test)]
#[path = "records_grade_page_fixtures.rs"]
mod records_grade_page_fixtures;

#[cfg(test)]
#[path = "records_comparison_page_fixtures.rs"]
mod records_comparison_page_fixtures;
