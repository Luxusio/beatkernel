//! Retained bounded device metadata presentation; selection admission stays external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    layout::{LayoutUpdate, MountedLayout, Node, NodeId, TextStyle},
    molecules::button,
    retained::RetainedNodes,
};
use crate::{
    device_catalog::{DeviceCatalog, MAX_DEVICES},
    local_players::PlayerId,
    local_setup::{validate_browser_sources, BrowserInputKind, BrowserInputSource},
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
pub const BUTTONS: [(ControlId, Bounds, &'static str); 5] = [
    (
        ControlId(20),
        Bounds {
            x: 24,
            y: 620,
            width: 170,
            height: 34,
        },
        "USE DEVICE",
    ),
    (
        ControlId(21),
        Bounds {
            x: 212,
            y: 620,
            width: 170,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(22),
        Bounds {
            x: 400,
            y: 620,
            width: 170,
            height: 34,
        },
        "REFRESH",
    ),
    (
        ControlId(23),
        Bounds {
            x: 588,
            y: 620,
            width: 170,
            height: 34,
        },
        "PREV",
    ),
    (
        ControlId(24),
        Bounds {
            x: 776,
            y: 620,
            width: 170,
            height: 34,
        },
        "NEXT",
    ),
];

const TITLE: TextStyle = TextStyle {
    scale: 3,
    color: 0xf0f4ff,
};
const LABEL: TextStyle = TextStyle {
    scale: 1,
    color: 0xf0f4ff,
};
const HELP: TextStyle = TextStyle {
    scale: 1,
    color: 0x9bb1cf,
};
const NOTICE: TextStyle = TextStyle {
    scale: 1,
    color: 0xd8b36b,
};
const PLAYER: TextStyle = TextStyle {
    scale: 1,
    color: 0x74e5c5,
};
const ERROR: TextStyle = TextStyle {
    scale: 1,
    color: 0xff8e8e,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Component {
    Background,
    Title,
    Subtitle,
    Player,
    Count,
    Row(usize),
    Empty,
    DeviceId,
    Detail,
    Action(usize),
    Pending,
    Error,
}
type N = Node<'static, Component>;
// One hierarchy for actual native metadata and honest browser capability
// projections. Rows never enumerate devices or decide assignment themselves.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([960, 720], Component::Background)
            .fill([true, true])
            .at(0, 0),
        N::leaf([936, 21], Component::Title).at(24, 20),
        N::leaf([666, 7], Component::Subtitle).at(24, 65),
        N::leaf([246, 7], Component::Player).at(690, 65),
        N::leaf([906, 7], Component::Count).at(24, 91),
        N::column(
            [906, 385],
            5,
            &[
                N::leaf([906, 34], Component::Row(0)),
                N::leaf([906, 34], Component::Row(1)),
                N::leaf([906, 34], Component::Row(2)),
                N::leaf([906, 34], Component::Row(3)),
                N::leaf([906, 34], Component::Row(4)),
                N::leaf([906, 34], Component::Row(5)),
                N::leaf([906, 34], Component::Row(6)),
                N::leaf([906, 34], Component::Row(7)),
                N::leaf([906, 34], Component::Row(8)),
                N::leaf([906, 34], Component::Row(9)),
            ],
        )
        .clipped()
        .at(24, 120),
        N::leaf([906, 14], Component::Empty).at(24, 145),
        N::column(
            [906, 34],
            20,
            &[
                N::leaf([906, 7], Component::DeviceId),
                N::leaf([906, 7], Component::Detail),
            ],
        )
        .at(24, 558),
        N::row(
            [922, 34],
            18,
            &[
                N::leaf([170, 34], Component::Action(0)),
                N::leaf([170, 34], Component::Action(1)),
                N::leaf([170, 34], Component::Action(2)),
                N::leaf([170, 34], Component::Action(3)),
                N::leaf([170, 34], Component::Action(4)),
            ],
        )
        .at(24, 620),
        N::leaf([906, 7], Component::Pending).at(24, 665),
        N::leaf([906, 7], Component::Error).at(24, 690),
    ],
)
.clipped();
fn node_id(layout: &MountedLayout<Component>, component: Component) -> NodeId {
    layout
        .leaves()
        .iter()
        .find(|leaf| leaf.component == component)
        .unwrap()
        .id
}
fn paint_text(scene: &mut Scene, bounds: Bounds, value: &str, style: TextStyle) {
    text(
        scene,
        bounds.x as usize,
        bounds.y as usize,
        value,
        style.scale,
        style.color,
    );
}
pub struct DevicesFrame<'a> {
    pub catalog: &'a DeviceCatalog,
    pub player: Option<PlayerId>,
    pub selected: Option<usize>,
    pub first: usize,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
pub struct BrowserDevicesFrame<'a> {
    pub sources: &'a [BrowserInputSource<'a>],
    pub can_assign: bool,
    pub can_refresh: bool,
    pub player: Option<PlayerId>,
    pub selected: Option<usize>,
    pub first: usize,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
fn validate_browser(frame: &BrowserDevicesFrame<'_>) -> Result<(), String> {
    validate_browser_sources(frame.sources)?;
    let count = frame.sources.len();
    if frame.first.checked_add(10).is_none()
        || (count == 0 && frame.first != 0)
        || (count > 0 && frame.first >= count)
        || frame.selected.is_some_and(|index| index >= count)
        || frame.player.is_some_and(|player| player.0 == 0)
    {
        return Err("browser Devices frame exceeds identity/index bounds".into());
    }
    Ok(())
}
pub fn hit_browser(
    frame: &BrowserDevicesFrame<'_>,
    point: Option<(f64, f64)>,
) -> Option<ControlId> {
    validate_browser(frame).ok()?;
    let point = point?;
    if frame.pending {
        return None;
    }
    let selected = frame
        .selected
        .is_some_and(|index| frame.sources[index].selectable)
        && frame.can_assign;
    let gates = [
        selected,
        true,
        frame.can_refresh,
        frame.first > 0,
        frame.first + 10 < frame.sources.len(),
    ];
    for ((id, bounds, _), enabled) in BUTTONS.iter().zip(gates).rev() {
        if enabled && bounds.contains(point) {
            return Some(*id);
        }
    }
    if frame.can_assign {
        for slot in (0..10).rev() {
            let index = frame.first + slot;
            if frame
                .sources
                .get(index)
                .is_some_and(|source| source.selectable)
                && bounds(slot).contains(point)
            {
                return Some(ControlId(10000 + index as u64));
            }
        }
    }
    None
}
fn bounds(slot: usize) -> Bounds {
    Bounds {
        x: 24,
        y: 120 + slot as i64 * 39,
        width: 906,
        height: 34,
    }
}
fn validate(frame: &DevicesFrame<'_>) -> Result<(), String> {
    let count = frame.catalog.choices().len();
    if count > MAX_DEVICES
        || frame.first.checked_add(10).is_none()
        || (count == 0 && frame.first != 0)
        || (count > 0 && frame.first >= count)
        || frame.selected.is_some_and(|index| index >= count)
    {
        return Err("Devices frame exceeds catalog or index bounds".into());
    }
    Ok(())
}
fn enabled(frame: &DevicesFrame<'_>, id: ControlId) -> bool {
    !frame.pending && (id.0 != 20 || frame.selected.is_some())
}
/// Current-state hit admission; nonselectable rows stay visible but have no hit.
pub fn hit(frame: &DevicesFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    let point = point?;
    if frame.pending || validate(frame).is_err() {
        return None;
    }
    for (id, bounds, _) in BUTTONS.iter().rev() {
        if enabled(frame, *id) && bounds.contains(point) {
            return Some(*id);
        }
    }
    for slot in (0..10).rev() {
        let index = frame.first + slot;
        if frame
            .catalog
            .choices()
            .get(index)
            .is_some_and(|choice| choice.selectable)
            && bounds(slot).contains(point)
        {
            return Some(ControlId(10000 + index as u64));
        }
    }
    None
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    label: String,
    source_id: String,
    kind: Option<BrowserInputKind>,
    selected: bool,
    selectable: bool,
    pending: bool,
}
struct DeviceStatus<'a> {
    count: usize,
    first: usize,
    selected: Option<usize>,
    pending: bool,
    keyboard: bool,
    player: Option<PlayerId>,
    browser: bool,
    can_use: bool,
    can_refresh: bool,
    pages: (bool, bool),
    error: Option<&'a str>,
    hovered: Option<ControlId>,
    armed: Option<ControlId>,
}
pub struct DevicesView {
    id: ScreenInstanceId,
    scope: Scope,
    rows: [RwSignal<Option<Row>>; 10],
    header: RwSignal<(bool, Option<PlayerId>)>,
    browser: RwSignal<bool>,
    can_refresh: RwSignal<bool>,
    pages: RwSignal<(bool, bool)>,
    count: RwSignal<usize>,
    details: RwSignal<Option<(String, String)>>,
    pending: RwSignal<bool>,
    selected: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
    layout: MountedLayout<Component>,
}
impl DevicesView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let nodes = RetainedNodes::new(width, height)?;
        let mut layout = MountedLayout::mount(SCREEN)?;
        layout.resize([width, height])?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            rows: std::array::from_fn(|_| scope.create_rw_signal(None)),
            header: scope.create_rw_signal((false, None)),
            browser: scope.create_rw_signal(false),
            can_refresh: scope.create_rw_signal(true),
            pages: scope.create_rw_signal((true, true)),
            count: scope.create_rw_signal(0),
            details: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            selected: scope.create_rw_signal(false),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
            layout,
        };
        let background = node_id(&view.layout, Component::Background);
        let title = node_id(&view.layout, Component::Title);
        view.nodes.static_layout_node(
            &view.layout,
            &[background, title],
            move |id, geometry, scene, _| {
                let b = geometry.bounds;
                if id == background {
                    rect(scene, b.x, b.y, b.width, b.height, 0x10151e);
                } else {
                    paint_text(scene, b, "BEATKERNEL BMS PLAYER", TITLE);
                }
            },
        )?;
        let subtitle = node_id(&view.layout, Component::Subtitle);
        let player_id = node_id(&view.layout, Component::Player);
        let header = view.header;
        let browser = view.browser;
        let memo = scope.create_memo(move |_| (header.get(), browser.get()));
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[subtitle, player_id],
            move |((keyboard, player), browser), id, geometry, scene, _| {
                if id == subtitle {
                    paint_text(
                        scene,
                        geometry.bounds,
                        if browser {
                            "BROWSER INPUT SOURCES - ENTER USE - ESC BACK"
                        } else if keyboard {
                            "KEYBOARD DEVICES - UP/DOWN SELECT - ENTER USE - ESC BACK"
                        } else {
                            "AUDIO OUTPUT DEVICES - UP/DOWN SELECT - ENTER USE - ESC BACK"
                        },
                        HELP,
                    );
                } else if let Some(player) = player {
                    paint_text(
                        scene,
                        geometry.bounds,
                        &format!("FOR P{}", player.0),
                        PLAYER,
                    );
                }
            },
        )?;
        let count = view.count;
        let memo = scope.create_memo(move |_| count.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[node_id(&view.layout, Component::Count)],
            |count, _, geometry, scene, _| {
                paint_text(
                    scene,
                    geometry.bounds,
                    &format!("{} DEVICE ENTRIES - NO AUTOMATIC SELECTION", count),
                    NOTICE,
                )
            },
        )?;
        for (slot, row) in view.rows.iter().copied().enumerate() {
            let memo = scope.create_memo(move |_| row.get());
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &[node_id(&view.layout, Component::Row(slot))],
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
                        let label = row.kind.map(|kind| {
                            format!("{} {} ({})", kind.label(), row.label, row.source_id)
                        });
                        text(
                            scene,
                            bounds.x as usize + 8,
                            bounds.y as usize + 9,
                            label.as_deref().unwrap_or(&row.label),
                            2,
                            if row.selectable { 0xf0f4ff } else { 0x687485 },
                        );
                        if !row.pending && row.selectable {
                            hits.push((ControlId(10000 + row.index as u64), bounds));
                        }
                    }
                },
            )?;
        }
        let count = view.count;
        let memo = scope.create_memo(move |_| count.get() == 0);
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[node_id(&view.layout, Component::Empty)],
            |empty, _, geometry, scene, _| {
                if empty {
                    paint_text(
                        scene,
                        geometry.bounds,
                        "NO DEVICES REPORTED",
                        TextStyle { scale: 2, ..HELP },
                    );
                }
            },
        )?;
        let details = view.details;
        let memo = scope.create_memo(move |_| details.get());
        let device_id = node_id(&view.layout, Component::DeviceId);
        let detail_id = node_id(&view.layout, Component::Detail);
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[device_id, detail_id],
            move |details, id, geometry, scene, _| {
                if let Some((source_id, detail)) = details {
                    if id == device_id {
                        paint_text(scene, geometry.bounds, &source_id, LABEL);
                    } else {
                        paint_text(scene, geometry.bounds, &detail, HELP);
                    }
                }
            },
        )?;
        for (index, (id, _, label)) in BUTTONS.into_iter().enumerate() {
            let pending = view.pending;
            let selected = view.selected;
            let hovered = view.hovered;
            let armed = view.armed;
            let refresh = view.can_refresh;
            let pages = view.pages;
            let memo = scope.create_memo(move |_| {
                let enabled = !pending.get()
                    && match id.0 {
                        20 => selected.get(),
                        22 => refresh.get(),
                        23 => pages.get().0,
                        24 => pages.get().1,
                        _ => true,
                    };
                (
                    enabled,
                    enabled && hovered.get() == Some(id),
                    enabled && armed.get() == Some(id),
                )
            });
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &[node_id(&view.layout, Component::Action(index))],
                move |(enabled, hovered, armed), _, geometry, scene, hits| {
                    let bounds = geometry.bounds;
                    button(scene, bounds, label, hovered, armed);
                    if enabled {
                        hits.push((id, bounds));
                    }
                },
            )?;
        }
        let pending = view.pending;
        let memo = scope.create_memo(move |_| pending.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[node_id(&view.layout, Component::Pending)],
            |pending, _, geometry, scene, _| {
                if pending {
                    paint_text(scene, geometry.bounds, "LOADING DEVICES", NOTICE);
                }
            },
        )?;
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[node_id(&view.layout, Component::Error)],
            |error, _, geometry, scene, _| {
                if let Some(error) = error {
                    paint_text(scene, geometry.bounds, &error, ERROR);
                }
            },
        )?;
        view.nodes.validate()?;
        Ok(view)
    }
    /// Update the actual logical extent while preserving the mounted instances
    /// and metadata. Fixed allocations crop under the same paint/hit clip.
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
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
        Ok(true)
    }
    pub fn hit(&self, point: (f64, f64)) -> Option<ControlId> {
        self.nodes.hit(point)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn update(&self, frame: DevicesFrame<'_>) -> Result<(), String> {
        validate(&frame)?;
        let choices = frame.catalog.choices();
        self.update_projected(
            DeviceStatus {
                count: choices.len(),
                first: frame.first,
                selected: frame.selected,
                pending: frame.pending,
                keyboard: frame.catalog.request().is_keyboard(),
                player: frame.player,
                browser: false,
                can_use: frame.selected.is_some(),
                can_refresh: true,
                pages: (true, true),
                error: frame.error,
                hovered: frame.hovered,
                armed: frame.armed,
            },
            |index| {
                choices.get(index).map(|choice| {
                    (
                        choice.id.as_str(),
                        choice.label.as_str(),
                        choice.detail.as_str(),
                        choice.selectable,
                        None,
                    )
                })
            },
        )
    }
    pub fn update_browser(&self, frame: BrowserDevicesFrame<'_>) -> Result<(), String> {
        validate_browser(&frame)?;
        self.update_projected(
            DeviceStatus {
                count: frame.sources.len(),
                first: frame.first,
                selected: frame.selected,
                pending: frame.pending,
                keyboard: false,
                player: frame.player,
                browser: true,
                can_use: frame.can_assign
                    && frame
                        .selected
                        .is_some_and(|index| frame.sources[index].selectable),
                can_refresh: frame.can_refresh,
                pages: (frame.first > 0, frame.first + 10 < frame.sources.len()),
                error: frame.error,
                hovered: frame.hovered,
                armed: frame.armed,
            },
            |index| {
                frame.sources.get(index).map(|source| {
                    (
                        source.id,
                        source.label,
                        source.detail,
                        source.selectable && frame.can_assign,
                        Some(source.kind),
                    )
                })
            },
        )
    }
    fn update_projected<'a>(
        &self,
        frame: DeviceStatus<'_>,
        choice: impl Fn(usize) -> Option<(&'a str, &'a str, &'a str, bool, Option<BrowserInputKind>)>,
    ) -> Result<(), String> {
        for (slot, signal) in self.rows.iter().enumerate() {
            let index = frame.first + slot;
            if let Some((id, label, _, selectable, kind)) = choice(index) {
                let selected = frame.selected == Some(index);
                if !signal.with_untracked(|old| {
                    old.as_ref().is_some_and(|old| {
                        old.index == index
                            && old.label == label
                            && old.source_id == id
                            && old.kind == kind
                            && old.selected == selected
                            && old.selectable == selectable
                            && old.pending == frame.pending
                    })
                }) {
                    signal.set(Some(Row {
                        index,
                        label: label.to_owned(),
                        source_id: id.to_owned(),
                        kind,
                        selected,
                        selectable,
                        pending: frame.pending,
                    }));
                }
            } else if signal.with_untracked(Option::is_some) {
                signal.set(None);
            }
        }
        let header = (frame.keyboard, frame.player);
        if self.browser.get_untracked() != frame.browser {
            self.browser.set(frame.browser);
        }
        if self.can_refresh.get_untracked() != frame.can_refresh {
            self.can_refresh.set(frame.can_refresh);
        }
        if self.pages.get_untracked() != frame.pages {
            self.pages.set(frame.pages);
        }
        if self.header.get_untracked() != header {
            self.header.set(header);
        }
        if self.count.get_untracked() != frame.count {
            self.count.set(frame.count);
        }
        let detail = frame.selected.and_then(choice);
        if !self.details.with_untracked(|old| match (old, detail) {
            (Some((id, text)), Some((source_id, _, detail, _, _))) => {
                id == source_id && text == detail
            }
            (None, None) => true,
            _ => false,
        }) {
            self.details
                .set(detail.map(|(id, _, detail, _, _)| (id.to_owned(), detail.to_owned())));
        }
        if self.pending.get_untracked() != frame.pending {
            self.pending.set(frame.pending);
        }
        if self.selected.get_untracked() != frame.can_use {
            self.selected.set(frame.can_use);
        }
        if !self
            .error
            .with_untracked(|old| old.as_deref() == frame.error)
        {
            self.error.set(frame.error.map(str::to_owned));
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
}
impl Drop for DevicesView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
#[path = "devices_declarative_fixtures.rs"]
mod declarative_fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_catalog::{DeviceChoice, DeviceRequest};

    fn catalog(count: usize) -> DeviceCatalog {
        DeviceCatalog::new(
            DeviceRequest::LinuxKeyboard,
            (0..count)
                .map(|index| DeviceChoice {
                    id: format!("/dev/input/event{index}"),
                    label: format!("KEYBOARD {index}"),
                    detail: format!("DETAIL {index}"),
                    selectable: index % 2 == 0,
                })
                .collect(),
        )
        .unwrap()
    }
    fn frame(catalog: &DeviceCatalog) -> DevicesFrame<'_> {
        DevicesFrame {
            catalog,
            player: None,
            selected: None,
            first: 0,
            pending: false,
            error: None,
            hovered: None,
            armed: None,
        }
    }
    #[test]
    fn unchanged_and_error_updates_preserve_rows_and_button_hover_is_local() {
        let catalog = catalog(4);
        let view = DevicesView::new(ScreenInstanceId(50), 960, 720).unwrap();
        view.update(frame(&catalog)).unwrap();
        let initial = view.nodes.paints();
        view.update(frame(&catalog)).unwrap();
        assert_eq!(view.nodes.paints(), initial);
        let mut update = frame(&catalog);
        update.error = Some("QUERY FAILED");
        view.update(update).unwrap();
        let status = view.nodes.paints();
        assert_eq!(&status[3..13], &initial[3..13]);
        let mut update = frame(&catalog);
        update.error = Some("QUERY FAILED");
        update.hovered = Some(ControlId(22));
        update.armed = Some(ControlId(22));
        view.update(update).unwrap();
        let hovered = view.nodes.paints();
        for index in 0..hovered.len() {
            if index == 17 {
                assert!(hovered[index] > status[index]);
            } else {
                assert_eq!(hovered[index], status[index]);
            }
        }
        let mut update = frame(&catalog);
        update.selected = Some(0);
        view.update(update).unwrap();
        let selected = view.nodes.paints();
        let mut update = frame(&catalog);
        update.selected = Some(2);
        view.update(update).unwrap();
        let changed = view.nodes.paints();
        assert!(changed[3] > selected[3]);
        assert!(changed[5] > selected[5]);
        assert_eq!(changed[4], selected[4]);
        assert_eq!(&changed[6..13], &selected[6..13]);
    }
    #[test]
    fn disabled_selected_metadata_keeps_use_gate_and_actual_paged_row_ids() {
        let catalog = catalog(21);
        let view = DevicesView::new(ScreenInstanceId(51), 960, 720).unwrap();
        let mut page = frame(&catalog);
        page.first = 10;
        page.selected = Some(11);
        page.player = Some(PlayerId(u32::MAX));
        assert_eq!(hit(&page, Some((30.0, 125.0))), Some(ControlId(10010)));
        assert_eq!(hit(&page, Some((30.0, 164.0))), None);
        assert_eq!(hit(&page, Some((30.0, 625.0))), Some(ControlId(20)));
        view.update(page).unwrap();
        assert_eq!(
            view.details.get_untracked(),
            Some(("/dev/input/event11".into(), "DETAIL 11".into()))
        );
        assert_eq!(
            view.header.get_untracked(),
            (true, Some(PlayerId(u32::MAX)))
        );
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[..5].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![10010, 10012, 10014, 10016, 10018]
        );
        assert_eq!(
            hits[5..].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![20, 21, 22, 23, 24]
        );
        let mut pending = frame(&catalog);
        pending.first = 10;
        pending.selected = Some(11);
        pending.pending = true;
        assert_eq!(hit(&pending, Some((30.0, 625.0))), None);
        view.update(pending).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
    }
    #[test]
    fn empty_catalog_pages_still_admit_legacy_controls_and_rejections_are_atomic() {
        let catalog = catalog(0);
        let view = DevicesView::new(ScreenInstanceId(52), 960, 720).unwrap();
        view.update(frame(&catalog)).unwrap();
        assert_eq!(
            hit(&frame(&catalog), Some((600.0, 625.0))),
            Some(ControlId(23))
        );
        assert_eq!(
            hit(&frame(&catalog), Some((790.0, 625.0))),
            Some(ControlId(24))
        );
        assert_eq!(hit(&frame(&catalog), Some((30.0, 625.0))), None);
        let before = view.nodes.paints();
        let identities = view.nodes.identities();
        let mut invalid = frame(&catalog);
        invalid.selected = Some(0);
        invalid.error = Some("MUST NOT APPLY");
        assert!(view.update(invalid).is_err());
        let mut invalid = frame(&catalog);
        invalid.first = usize::MAX;
        assert!(view.update(invalid).is_err());
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.error.get_untracked(), None);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let count = scene.rectangles().len();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![21, 22, 23, 24]
        );
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.nodes.paints(), before);
        let weak = view.nodes.weak_dirty();
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(DevicesView::new(ScreenInstanceId(52), 800, 600).is_ok());
    }
}
