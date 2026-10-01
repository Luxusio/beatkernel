//! Retained saved-record chooser presentation; metadata and preview work stay external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field, text_field_value},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    record_catalog::{RecordCatalog, RecordPreview},
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use beatkernel::time::Timestamp;
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};

const DIRECTORY: Bounds = Bounds {
    x: 160,
    y: 108,
    width: 770,
    height: 34,
};
/// Shared button geometry in painter order, including conditionally visible pages.
pub const BUTTONS: [(ControlId, Bounds, &'static str); 9] = [
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
];
pub struct RecordsFrame<'a> {
    pub directory: &'a LineEditor,
    pub directory_focused: bool,
    pub catalog: Option<&'a RecordCatalog>,
    pub selected: Option<usize>,
    pub first: usize,
    /// The coordinator supplies only the current compatible selected preview.
    pub preview: Option<&'a RecordPreview>,
    pub pending: bool,
    pub opponents: usize,
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
    let count = frame.catalog.map_or(0, |catalog| catalog.entries.len());
    match id.0 {
        56 => frame.catalog.is_some() && frame.first > 0,
        57 => frame.catalog.is_some() && frame.first.saturating_add(10) < count,
        51 => frame.selected.is_some_and(|index| index < count),
        52 | 53 | 59 => frame.preview.is_some(),
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
    let count = frame.catalog.map_or(0, |catalog| catalog.entries.len());
    if count > 256
        || frame.first.checked_add(10).is_none()
        || (count == 0 && frame.first != 0)
        || (count > 0 && frame.first >= count)
        || frame.selected.is_some_and(|index| index >= count)
    {
        return Err("Records frame exceeds catalog or index bounds".into());
    }
    if let Some(preview) = frame.preview {
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
    records: usize,
    start: Timestamp,
    until: Option<Timestamp>,
    hits: u64,
    misses: u64,
    combo: u64,
    max_combo: u64,
}
impl From<&RecordPreview> for Preview {
    fn from(value: &RecordPreview) -> Self {
        Self {
            records: value.records,
            start: value.start,
            until: value.recorded_until,
            hits: value.score.hits,
            misses: value.score.misses,
            combo: value.score.combo,
            max_combo: value.score.max_combo,
        }
    }
}
/// Fixed node tree per Records instance, with only visible row labels retained.
pub struct RecordsView {
    id: ScreenInstanceId,
    scope: Scope,
    directory: RwSignal<LineEditor>,
    directory_focused: RwSignal<bool>,
    rows: [RwSignal<Option<Row>>; 10],
    summary: RwSignal<Option<(usize, bool)>>,
    preview: RwSignal<Option<Preview>>,
    opponents: RwSignal<usize>,
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    gates: [RwSignal<bool>; 9],
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
            id,
            scope,
            directory: scope.create_rw_signal(directory),
            directory_focused: scope.create_rw_signal(true),
            rows: std::array::from_fn(|_| scope.create_rw_signal(None)),
            summary: scope.create_rw_signal(None),
            preview: scope.create_rw_signal(None),
            opponents: scope.create_rw_signal(0),
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
        let memo = scope.create_memo(move |_| (directory.get(), focused.get(), pending.get()));
        view.nodes
            .bind(scope, memo, |(directory, focused, pending), scene, hits| {
                text(scene, 24, 120, "DIRECTORY", 1, 0xf0f4ff);
                text_field(scene, &directory, DIRECTORY, focused && !pending);
                if !pending {
                    hits.push((ControlId(58), DIRECTORY));
                }
            });
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
                    485,
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
                    text(
                        scene,
                        24,
                        518,
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
                        534,
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
                        550,
                        &format!("HITS {} MISSES {}", preview.hits, preview.misses),
                        1,
                        0x9bb1cf,
                    );
                    text(
                        scene,
                        24,
                        566,
                        &format!("COMBO {} MAX {}", preview.combo, preview.max_combo),
                        1,
                        0x9bb1cf,
                    );
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
        for index in 2..9 {
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
    pub fn update(&self, frame: RecordsFrame<'_>) -> Result<(), String> {
        validate_frame(&frame)?;
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
        self.nodes.dirty()
    }
    pub fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        self.nodes.compose(scene, hits)
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
            opponents: 0,
            message: None,
            error: None,
            hovered: None,
            armed: None,
        }
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
        assert_eq!(&error[..26], &before[..26]);
        assert_eq!(error[26], before[26] + 1);
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
            score: ScoreSummary {
                hits: 2,
                misses: 1,
                combo: 0,
                max_combo: 2,
                ..ScoreSummary::default()
            },
        };
        update.preview = Some(&preview);
        update.opponents = 8;
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
