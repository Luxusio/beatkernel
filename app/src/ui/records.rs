//! Retained saved-record chooser presentation; metadata and preview work stay external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    layout::{LayoutGeometry, LayoutUpdate, MountedLayout, Node, NodeId},
    molecules::{button, text_field_value, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText,
    record_model::{FrozenRecordPreview, RecordCatalog, RecordPreview},
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use beatkernel::time::Timestamp;
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Component {
    Background,
    Title,
    Subtitle,
    DirectoryLabel,
    Directory,
    Help,
    Row(usize),
    Summary,
    Preview,
    Opponents,
    SelectedOpponents,
    Button(usize),
    Message,
    Error,
    DetailBody,
    DetailButton(usize),
}
type N = Node<'static, Component>;
/// One mounted hierarchy; state updates address its existing rows and actions.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([960, 720], Component::Background)
            .fill([true, true])
            .at(0, 0),
        N::column(
            [936, 59],
            24,
            &[
                N::leaf([936, 21], Component::Title),
                N::leaf([936, 14], Component::Subtitle),
            ],
        )
        .at(24, 20),
        N::layer(
            [906, 34],
            &[
                N::leaf([136, 7], Component::DirectoryLabel).at(0, 12),
                N::leaf([770, 34], Component::Directory).at(136, 0),
            ],
        )
        .at(24, 108),
        N::leaf([906, 7], Component::Help).at(24, 151),
        N::column(
            [906, 298],
            2,
            &[
                N::leaf([906, 28], Component::Row(0)),
                N::leaf([906, 28], Component::Row(1)),
                N::leaf([906, 28], Component::Row(2)),
                N::leaf([906, 28], Component::Row(3)),
                N::leaf([906, 28], Component::Row(4)),
                N::leaf([906, 28], Component::Row(5)),
                N::leaf([906, 28], Component::Row(6)),
                N::leaf([906, 28], Component::Row(7)),
                N::leaf([906, 28], Component::Row(8)),
                N::leaf([906, 28], Component::Row(9)),
            ],
        )
        .clipped()
        .at(24, 170),
        N::leaf([906, 309], Component::Summary).at(24, 170),
        N::leaf([906, 92], Component::Preview).at(24, 482),
        N::leaf([280, 7], Component::SelectedOpponents).at(24, 576),
        N::leaf([280, 7], Component::Opponents).at(24, 592),
        N::leaf([906, 7], Component::Message).at(24, 665),
        N::leaf([906, 30], Component::Error).at(24, 682),
        N::leaf([150, 30], Component::Button(0)).at(620, 475),
        N::leaf([150, 30], Component::Button(1)).at(780, 475),
        N::row(
            [906, 34],
            10,
            &[
                N::leaf([130, 34], Component::Button(2)),
                N::leaf([130, 34], Component::Button(3)),
                N::leaf([130, 34], Component::Button(4)),
                N::leaf([140, 34], Component::Button(5)),
                N::layer(
                    [150, 34],
                    &[N::leaf([140, 34], Component::Button(6)).at(0, 0)],
                ),
                N::leaf([176, 34], Component::Button(7)),
            ],
        )
        .at(24, 620),
        N::leaf([176, 34], Component::Button(8)).at(754, 575),
        N::leaf([140, 34], Component::Button(9)).at(430, 575),
        N::leaf([164, 34], Component::Button(10)).at(580, 575),
        N::leaf([110, 34], Component::Button(11)).at(304, 575),
        N::leaf([960, 720], Component::DetailBody)
            .fill([true, true])
            .at(0, 0),
        N::leaf([176, 34], Component::DetailButton(0)).at(754, 620),
        N::leaf([140, 34], Component::DetailButton(1)).at(430, 575),
        N::leaf([164, 34], Component::DetailButton(2)).at(580, 575),
    ],
)
.clipped();
/// Retained painters report conversion refusal so staged layout publication stays atomic.
fn paint_text(
    scene: &mut Scene,
    bounds: Bounds,
    offset: [i64; 2],
    value: &str,
    scale: usize,
    color: u32,
) {
    let origin = bounds
        .x
        .checked_add(offset[0])
        .zip(bounds.y.checked_add(offset[1]));
    let origin = origin.and_then(|(x, y)| usize::try_from(x).ok().zip(usize::try_from(y).ok()));
    match origin {
        Some((x, y)) => text(scene, x, y, value, scale, color),
        None => scene.reject("Records text origin exceeds the supported coordinate range".into()),
    }
}
fn node_id(layout: &MountedLayout<Component>, component: Component) -> NodeId {
    layout
        .leaves()
        .iter()
        .find(|leaf| leaf.component == component)
        .expect("mounted Records component")
        .id
}
fn grade_pages<P: RecordInfo>(frame: &RecordsFrame<'_, P>) -> usize {
    frame.preview.map_or(1, |preview| {
        let preview = preview.project();
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
pub struct RecordsFrame<'a, P = RecordPreview> {
    pub directory: &'a LineEditor,
    pub directory_focused: bool,
    pub catalog: Option<&'a RecordCatalog>,
    pub selected: Option<usize>,
    pub first: usize,
    /// The coordinator supplies only the current compatible selected preview.
    pub preview: Option<&'a P>,
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
pub type VisualRecordsFrame<'a> = RecordsFrame<'a, FrozenRecordPreview>;
fn bounds(slot: usize) -> Bounds {
    Bounds {
        x: 24,
        y: 170 + slot as i64 * 30,
        width: 906,
        height: 28,
    }
}
fn available<P: RecordInfo>(frame: &RecordsFrame<'_, P>, id: ControlId) -> bool {
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
            .is_some_and(|preview| preview.project().historical.is_some()),
        60 => frame.selected_opponents[0] > 0,
        61 => frame.selected_opponents[1] > 0,
        50 | 54 | 55 => true,
        _ => false,
    }
}
/// Hit admission from current borrowed state, independent of the desktop hit cache.
/// Reverse painter order matches composed geometry. Invalid or pending frames admit none.
pub fn hit(frame: &RecordsFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    hit_projected(frame, point)
}
pub fn hit_visual(frame: &VisualRecordsFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    if frame
        .preview
        .is_some_and(|preview| preview.validate().is_err())
    {
        return None;
    }
    hit_projected(frame, point)
}
fn hit_projected<P: RecordInfo>(
    frame: &RecordsFrame<'_, P>,
    point: Option<(f64, f64)>,
) -> Option<ControlId> {
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
fn validate_frame<P: RecordInfo>(frame: &RecordsFrame<'_, P>) -> Result<(), String> {
    if (!frame.details && frame.grade_page != 0)
        || (frame.details && frame.grade_page >= grade_pages(frame))
    {
        return Err("Records stored grade page is out of range".into());
    }
    if frame.details
        && (frame.pending
            || frame
                .preview
                .is_none_or(|preview| preview.project().historical.is_none()))
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
        let preview = preview.project();
        if let Some(classes) = preview.bms_score {
            classes
                .validate_for(preview.hits, preview.misses)
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
        if selected != Some(preview.path) {
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
    timing: crate::timing::TimingRecord,
}
struct RecordProjection<'a> {
    path: &'a std::path::PathBuf,
    bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    historical_bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    records: usize,
    recorded_until: Option<Timestamp>,
    start: Timestamp,
    end: Option<Timestamp>,
    historical: Option<crate::record_model::HistoricalRecordValue>,
    historical_score: &'a Option<Arc<crate::result_archive::ArchivedScore>>,
    historical_comparison:
        &'a Option<Arc<Option<crate::competition_presentation::CompetitionSnapshot>>>,
    archive_error: &'a Option<String>,
    hits: u64,
    misses: u64,
    combo: u64,
    max_combo: u64,
    timing: crate::timing::TimingRecord,
}
trait RecordInfo {
    fn project(&self) -> RecordProjection<'_>;
}
impl RecordInfo for RecordPreview {
    fn project(&self) -> RecordProjection<'_> {
        RecordProjection {
            path: &self.path,
            bms_score: self.bms_score,
            historical_bms_score: self.historical_bms_score,
            records: self.records,
            recorded_until: self.recorded_until,
            start: self.start,
            end: self.end,
            historical: self.historical,
            historical_score: &self.historical_score,
            historical_comparison: &self.historical_comparison,
            archive_error: &self.archive_error,
            hits: self.score.hits,
            misses: self.score.misses,
            combo: self.score.combo,
            max_combo: self.score.max_combo,
            timing: self.score.timing.record(),
        }
    }
}
impl RecordInfo for FrozenRecordPreview {
    fn project(&self) -> RecordProjection<'_> {
        RecordProjection {
            path: &self.path,
            bms_score: self.bms_score,
            historical_bms_score: self.historical_bms_score,
            records: self.records,
            recorded_until: self.recorded_until,
            start: self.start,
            end: self.end,
            historical: self.historical,
            historical_score: &self.historical_score,
            historical_comparison: &self.historical_comparison,
            archive_error: &self.archive_error,
            hits: self.score.hits,
            misses: self.score.misses,
            combo: self.score.combo,
            max_combo: self.score.max_combo,
            timing: self.score.timing,
        }
    }
}
impl<P: RecordInfo> From<&P> for Preview {
    fn from(value: &P) -> Self {
        let value = value.project();
        Self {
            bms_score: value.bms_score,
            records: value.records,
            start: value.start,
            end: value.end,
            historical: value.historical,
            archive_failed: value.archive_error.is_some(),
            until: value.recorded_until,
            hits: value.hits,
            misses: value.misses,
            combo: value.combo,
            max_combo: value.max_combo,
            timing: value.timing,
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
    geometry: Option<crate::scene::GeometrySnapshot>,
    grade_page: usize,
    grade_geometry: crate::scene::GeometrySnapshot,
}
fn detail_geometry(
    layout: &MountedLayout<Component>,
    presentation: &crate::historical_record_presentation::HistoricalRecordPresentation,
    grade_geometry: &crate::scene::GeometrySnapshot,
    page: usize,
    hovered: Option<ControlId>,
    armed: Option<ControlId>,
) -> Result<Option<crate::scene::GeometrySnapshot>, String> {
    let extent = layout.extent();
    if layout.suspended() {
        return Ok(None);
    }
    let mut scene = Scene::with_capacity(extent[0], extent[1], 1024);
    let background = layout
        .geometry(node_id(layout, Component::Background))
        .unwrap();
    let mut background_scene = Scene::with_capacity(extent[0], extent[1], 1);
    rect(
        &mut background_scene,
        background.bounds.x,
        background.bounds.y,
        background.bounds.width,
        background.bounds.height,
        0x10151e,
    );
    append_clipped(&mut scene, &background_scene, background, [0, 0])?;
    let body = layout
        .geometry(node_id(layout, Component::DetailBody))
        .unwrap();
    let mut original = Scene::with_capacity(960, 720, 1024);
    presentation.compose_body_for_page(page, &mut original)?;
    original.append_geometry(grade_geometry)?;
    append_clipped(&mut scene, &original, body, [body.bounds.x, body.bounds.y])?;
    for (index, (id, _, label)) in DETAIL_BUTTONS.into_iter().enumerate() {
        if detail_available(id, page, presentation.grade_page_count()) {
            let geometry = layout
                .geometry(node_id(layout, Component::DetailButton(index)))
                .unwrap();
            let mut part = Scene::with_capacity(extent[0], extent[1], 64);
            button(
                &mut part,
                geometry.bounds,
                label,
                hovered == Some(id),
                armed == Some(id),
            );
            append_clipped(&mut scene, &part, geometry, [0, 0])?;
        }
    }
    scene.geometry_snapshot().map(Some)
}
/// Cached historical geometry is projected through the same mounted clip as its actions.
fn append_clipped(
    scene: &mut Scene,
    source: &Scene,
    geometry: LayoutGeometry,
    offset: [i64; 2],
) -> Result<(), String> {
    if geometry.clip.width == 0 || geometry.clip.height == 0 {
        return Ok(());
    }
    source.status()?;
    let clip = crate::scene::ClipRect::new([
        geometry.clip.x,
        geometry.clip.y,
        geometry.clip.width,
        geometry.clip.height,
    ])?;
    for batch in source.batches() {
        for rectangle in
            &source.rectangles()[batch.first as usize..(batch.first + batch.count) as usize]
        {
            let color = rectangle
                .color
                .map(|channel| (channel * 255.0).round() as u8);
            let mut bounds = rectangle.bounds.map(|n| n as i64);
            bounds[0] = bounds[0]
                .checked_add(offset[0])
                .ok_or("Records detail x overflow")?;
            bounds[1] = bounds[1]
                .checked_add(offset[1])
                .ok_or("Records detail y overflow")?;
            scene.sprite_clipped_alpha(
                batch.texture,
                bounds,
                rectangle.uv,
                (u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2]),
                color[3],
                clip,
            )?;
        }
    }
    Ok(())
}
fn clipped_bounds(geometry: LayoutGeometry) -> Option<Bounds> {
    let bounds = geometry.bounds;
    let clip = geometry.clip;
    let x = bounds.x.max(clip.x);
    let y = bounds.y.max(clip.y);
    let width = (bounds.x + bounds.width).min(clip.x + clip.width) - x;
    let height = (bounds.y + bounds.height).min(clip.y + clip.height) - y;
    (width > 0 && height > 0).then_some(Bounds {
        x,
        y,
        width,
        height,
    })
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
    layout: RefCell<MountedLayout<Component>>,
}
impl RecordsView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let mut layout = MountedLayout::mount(SCREEN)?;
        layout.resize([width, height])?;
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
            layout: RefCell::new(layout),
        };
        let layout = view.layout.borrow().clone();
        let header_ids = [Component::Background, Component::Title, Component::Subtitle]
            .map(|component| node_id(&layout, component));
        view.nodes
            .static_layout_node(&layout, &header_ids, move |id, geometry, scene, _| {
                let bounds = geometry.bounds;
                if id == header_ids[0] {
                    rect(
                        scene,
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        0x10151e,
                    );
                } else if id == header_ids[1] {
                    paint_text(scene, bounds, [0, 0], "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
                } else {
                    paint_text(
                        scene,
                        bounds,
                        [0, 0],
                        "RECORDS - RECORDED PREFIX",
                        2,
                        0x9bb1cf,
                    );
                }
            })?;
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
        let directory_ids = [
            node_id(&layout, Component::DirectoryLabel),
            node_id(&layout, Component::Directory),
        ];
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &directory_ids,
            move |(directory, focused, pending, font), id, geometry, scene, hits| {
                if id == directory_ids[0] {
                    paint_text(scene, geometry.bounds, [0, 0], "DIRECTORY", 1, 0xf0f4ff);
                } else {
                    text_field_with_font(
                        scene,
                        &directory,
                        geometry.bounds,
                        focused && !pending,
                        font.as_ref(),
                    );
                    if !pending {
                        hits.push((ControlId(58), geometry.bounds));
                    }
                }
            },
        )?;
        view.nodes.static_layout_node(
            &layout,
            &[node_id(&layout, Component::Help)],
            |_, geometry, scene, _| {
                paint_text(
                    scene,
                    geometry.bounds,
                    [0, 0],
                    "TAB DIRECTORY/LIST   ENTER SCAN/PREVIEW   PGUP/PGDN PAGE",
                    1,
                    0x9bb1cf,
                )
            },
        )?;
        for (slot, row) in view.rows.iter().copied().enumerate() {
            let memo = scope.create_memo(move |_| row.get());
            view.nodes.bind_layout(
                scope,
                memo,
                &layout,
                &[node_id(&layout, Component::Row(slot))],
                move |row, _, geometry, scene, hits| {
                    if let Some(row) = row {
                        let bounds = geometry.bounds;
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
                },
            )?;
        }
        let summary = view.summary;
        let memo = scope.create_memo(move |_| summary.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, Component::Summary)],
            |summary, _, geometry, scene, _| {
                let bounds = geometry.bounds;
                if let Some((count, truncated)) = summary {
                    if count == 0 {
                        paint_text(
                            scene,
                            bounds,
                            [0, 10],
                            "NO DIRECT .BKR RECORDS",
                            1,
                            0x9bb1cf,
                        );
                    }
                    paint_text(
                        scene,
                        bounds,
                        [0, 302],
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
            },
        )?;
        for index in 0..2 {
            view.button_node(index, &layout)?;
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
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, Component::Preview)],
            |(pending, preview), _, geometry, scene, _| {
                let bounds = geometry.bounds;
                if pending {
                    paint_text(scene, bounds, [0, 38], "LOADING RECORDS", 2, 0xd8b36b);
                } else if let Some(preview) = preview {
                    if let Some(classes) = preview.bms_score {
                        for (y, label) in [
                            (0, format!("PREFIX EX {}", classes.ex_score)),
                            (
                                10,
                                format!("PGREAT {} GREAT {}", classes.pgreat, classes.great),
                            ),
                            (
                                20,
                                format!(
                                    "GOOD {} BAD {} POOR {}",
                                    classes.good, classes.bad, classes.poor
                                ),
                            ),
                        ] {
                            paint_text(scene, bounds, [0, y], &label, 1, 0xd8b36b);
                        }
                    } else {
                        paint_text(
                            scene,
                            bounds,
                            [0, 0],
                            "PREFIX CLASS SCORE UNAVAILABLE",
                            1,
                            0x9bb1cf,
                        );
                    }
                    paint_text(
                        scene,
                        bounds,
                        [0, 32],
                        &format!(
                            "OPERATIONS {}   START {:.3} S",
                            preview.records,
                            preview.start.as_nanos() as f64 / 1e9
                        ),
                        1,
                        0xb6cce6,
                    );
                    paint_text(
                        scene,
                        bounds,
                        [0, 42],
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
                    paint_text(
                        scene,
                        bounds,
                        [0, 52],
                        &format!("HITS {} MISSES {}", preview.hits, preview.misses),
                        1,
                        0x9bb1cf,
                    );
                    paint_text(
                        scene,
                        bounds,
                        [0, 62],
                        &format!("COMBO {} MAX {}", preview.combo, preview.max_combo),
                        1,
                        0x9bb1cf,
                    );
                    let (bias, absolute) = crate::timing_display::record(&preview.timing);
                    paint_text(scene, bounds, [0, 72], &bias, 1, 0x9bb1cf);
                    paint_text(scene, bounds, [0, 82], &absolute, 1, 0x9bb1cf);
                    let end = preview.end.map_or_else(
                        || "END UNLIMITED".into(),
                        |end| format!("END {} NS", end.as_nanos()),
                    );
                    paint_text(scene, bounds, [536, 32], &end, 1, 0xb6cce6);
                    if let Some((player, result)) = preview.historical {
                        use crate::gauge::GaugeFailure;
                        use crate::play_result::{PlayResultOutcome, PlayResultScope};
                        paint_text(
                            scene,
                            bounds,
                            [536, 42],
                            &format!("HISTORICAL PLAYER {}", player.0),
                            1,
                            0xd8b36b,
                        );
                        paint_text(
                            scene,
                            bounds,
                            [536, 52],
                            match result.scope {
                                PlayResultScope::FullSong => "STORED SCOPE FULL SONG",
                                PlayResultScope::PracticeSection { .. } => {
                                    "STORED SCOPE PRACTICE SECTION"
                                }
                            },
                            1,
                            0xd8b36b,
                        );
                        paint_text(
                            scene,
                            bounds,
                            [536, 62],
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
                        paint_text(
                            scene,
                            bounds,
                            [536, 72],
                            &format!("STORED GAUGE {} UNITS", result.gauge.level_units),
                            1,
                            0xd8b36b,
                        );
                        if preview.archive_failed {
                            paint_text(scene, bounds, [536, 82], "ARCHIVE DIAGNOSTIC", 1, 0xf07878);
                        }
                    } else {
                        paint_text(
                            scene,
                            bounds,
                            [536, 42],
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
                    paint_text(
                        scene,
                        bounds,
                        [0, 38],
                        "PREVIEW A COMPATIBLE RECORD BEFORE WATCH / ADD",
                        1,
                        0x9bb1cf,
                    );
                }
            },
        )?;
        let opponents = view.opponents;
        let memo = scope.create_memo(move |_| opponents.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, Component::Opponents)],
            |opponents, _, geometry, scene, _| {
                let bounds = geometry.bounds;
                paint_text(
                    scene,
                    bounds,
                    [0, 0],
                    &format!("SAVED GHOSTS {opponents}/8 - DRAFT ONLY"),
                    1,
                    0x9bb1cf,
                )
            },
        )?;
        let selected_opponents = view.selected_opponents;
        let memo = scope.create_memo(move |_| selected_opponents.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, Component::SelectedOpponents)],
            |counts, _, geometry, scene, _| {
                let bounds = geometry.bounds;
                paint_text(
                    scene,
                    bounds,
                    [0, 0],
                    &format!("SELECTED OWN {} / OTHER {}", counts[0], counts[1]),
                    1,
                    0x9bb1cf,
                );
            },
        )?;
        for index in 2..BUTTONS.len() {
            view.button_node(index, &layout)?;
        }
        let message = view.message;
        let memo = scope.create_memo(move |_| message.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, Component::Message)],
            |message, _, geometry, scene, _| {
                let bounds = geometry.bounds;
                if let Some(message) = message {
                    paint_text(scene, bounds, [0, 0], &message, 1, 0x74e5c5);
                }
            },
        )?;
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, Component::Error)],
            |error, _, geometry, scene, _| {
                let bounds = geometry.bounds;
                if let Some(error) = error {
                    text_field_value(scene, &error, bounds);
                }
            },
        )?;
        view.nodes.validate()?;
        drop(layout);
        Ok(view)
    }
    /// Resize the existing logical surface; fixed record coordinates crop rather than remount.
    pub fn resize(&self, width: u32, height: u32) -> Result<bool, String> {
        let mut candidate = self.layout.borrow().clone();
        if !candidate.resize([width, height])? {
            return Ok(false);
        }
        self.publish_layout(candidate)
    }
    /// Direct child size/position/clip changes retain row and editor identities.
    pub fn update_layout(&self, updates: &[LayoutUpdate]) -> Result<bool, String> {
        let mut candidate = self.layout.borrow().clone();
        if !candidate.update(updates)? {
            return Ok(false);
        }
        self.publish_layout(candidate)
    }
    fn publish_layout(&self, candidate: MountedLayout<Component>) -> Result<bool, String> {
        let detail_changed = self.layout.borrow().extent() != candidate.extent()
            || [
                Component::Background,
                Component::DetailBody,
                Component::DetailButton(0),
                Component::DetailButton(1),
                Component::DetailButton(2),
            ]
            .iter()
            .any(|&component| {
                candidate
                    .changed_nodes()
                    .contains(&node_id(&candidate, component))
            });
        let staged = if detail_changed {
            self.detail_cache
                .borrow()
                .as_ref()
                .map(|cache| {
                    detail_geometry(
                        &candidate,
                        &cache.presentation,
                        &cache.grade_geometry,
                        cache.grade_page,
                        self.detail_hovered.get(),
                        self.detail_armed.get(),
                    )
                })
                .transpose()?
        } else {
            None
        };
        self.nodes.relayout(&candidate)?;
        *self.layout.borrow_mut() = candidate;
        if let Some(geometry) = staged {
            self.detail_cache.borrow_mut().as_mut().unwrap().geometry = geometry;
        }
        if self.details.get() && detail_changed {
            self.detail_dirty.set(true);
        }
        Ok(true)
    }
    /// Paint and hits use the same currently published geometry in both modes.
    pub fn hit(&self, point: (f64, f64)) -> Option<ControlId> {
        let layout = self.layout.borrow();
        if layout.suspended() {
            return None;
        }
        if !self.details.get() {
            return self.nodes.hit(point);
        }
        let cache = self.detail_cache.borrow();
        let cache = cache.as_ref()?;
        DETAIL_BUTTONS
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, (id, _, _))| {
                let geometry = layout.geometry(node_id(&layout, Component::DetailButton(index)))?;
                (detail_available(*id, cache.grade_page, cache.presentation.grade_page_count())
                    && clipped_bounds(geometry).is_some_and(|bounds| bounds.contains(point)))
                .then_some(*id)
            })
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
        self.update_projected(frame)
    }
    pub fn update_visual(&self, frame: VisualRecordsFrame<'_>) -> Result<(), String> {
        if let Some(preview) = frame.preview {
            preview.validate()?;
        }
        self.update_projected(frame)
    }
    fn update_projected<P: RecordInfo>(&self, frame: RecordsFrame<'_, P>) -> Result<(), String> {
        validate_frame(&frame)?;
        let mut staged = None;
        let mut staged_geometry = None;
        if frame.details {
            let preview = frame.preview.expect("validated detail preview").project();
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
                    &self.layout.borrow(),
                    &presentation,
                    &grade_geometry,
                    frame.grade_page,
                    frame.hovered,
                    frame.armed,
                )?;
                staged = Some(DetailCache {
                    bms_score: preview.historical_bms_score,
                    value,
                    score: (*preview.historical_score).clone(),
                    comparison: (*preview.historical_comparison).clone(),
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
                    &self.layout.borrow(),
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
        if self.layout.borrow().suspended() {
            scene.clear();
            hits.clear();
            self.detail_dirty.set(false);
            return Ok(());
        }
        if self.details.get() {
            let cache = self.detail_cache.borrow();
            let cache = cache.as_ref().ok_or("Records details cache unavailable")?;
            let geometry = cache
                .geometry
                .as_ref()
                .ok_or("Records details geometry unavailable")?;
            scene.clear();
            hits.clear();
            scene.append_geometry(geometry)?;
            let layout = self.layout.borrow();
            for (index, (id, _, _)) in DETAIL_BUTTONS.into_iter().enumerate() {
                if detail_available(id, cache.grade_page, cache.presentation.grade_page_count()) {
                    if let Some(bounds) = clipped_bounds(
                        layout
                            .geometry(node_id(&layout, Component::DetailButton(index)))
                            .unwrap(),
                    ) {
                        hits.push((id, bounds));
                    }
                }
            }
        } else {
            self.nodes.compose(scene, hits)?;
        }
        self.detail_dirty.set(false);
        Ok(())
    }
    fn button_node(
        &mut self,
        index: usize,
        layout: &MountedLayout<Component>,
    ) -> Result<(), String> {
        let (id, _, label) = BUTTONS[index];
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
        self.nodes.bind_layout(
            self.scope,
            memo,
            layout,
            &[node_id(layout, Component::Button(index))],
            move |(available, hovered, armed), _, geometry, scene, hits| {
                let bounds = geometry.bounds;
                if index < 2 && !available {
                    return;
                }
                button(scene, bounds, label, hovered, armed);
                if available {
                    hits.push((id, bounds));
                }
            },
        )?;
        Ok(())
    }
}
impl Drop for RecordsView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
#[path = "records_visual_fixtures.rs"]
mod visual_fixtures;

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
        assert!(RecordsView::new(ScreenInstanceId(8), 800, 600).is_ok());
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
        assert!(scene
            .rectangles()
            .iter()
            .any(|rect| rect.bounds[1] == 592.0));
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

#[cfg(test)]
#[path = "records_declarative_fixtures.rs"]
mod records_declarative_fixtures;
