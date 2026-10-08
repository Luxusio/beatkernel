//! Retained local-player draft presentation; attachment and launch validation are external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    layout::{LayoutUpdate, MountedLayout, Node, NodeId, TextStyle},
    molecules::button,
    retained::RetainedNodes,
};
use crate::{
    local_setup::{BrowserInputKind, BrowserLocalProjection, LocalSetup},
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::cell::RefCell;

pub const BUTTONS: [(ControlId, Bounds, &'static str); 8] = [
    (
        ControlId(36),
        Bounds {
            x: 620,
            y: 550,
            width: 150,
            height: 34,
        },
        "PREVIOUS",
    ),
    (
        ControlId(37),
        Bounds {
            x: 780,
            y: 550,
            width: 150,
            height: 34,
        },
        "NEXT",
    ),
    (
        ControlId(30),
        Bounds {
            x: 24,
            y: 620,
            width: 140,
            height: 34,
        },
        "DONE",
    ),
    (
        ControlId(31),
        Bounds {
            x: 174,
            y: 620,
            width: 140,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(32),
        Bounds {
            x: 324,
            y: 620,
            width: 140,
            height: 34,
        },
        "REMOVE",
    ),
    (
        ControlId(33),
        Bounds {
            x: 474,
            y: 620,
            width: 140,
            height: 34,
        },
        "ADD",
    ),
    (
        ControlId(34),
        Bounds {
            x: 624,
            y: 620,
            width: 150,
            height: 34,
        },
        "ASSIGN KEYBOARD",
    ),
    (
        ControlId(35),
        Bounds {
            x: 784,
            y: 620,
            width: 150,
            height: 34,
        },
        "CLEAR",
    ),
];
const TITLE: TextStyle = TextStyle {
    scale: 3,
    color: 0xf0f4ff,
};
const LABEL: TextStyle = TextStyle {
    scale: 1,
    color: 0x9bb1cf,
};
const SUMMARY: TextStyle = TextStyle {
    scale: 1,
    color: 0xd8b36b,
};
#[derive(Clone, Copy)]
enum Component {
    Background(u32),
    Text(&'static str, TextStyle),
    Summary,
    Row(usize),
    Hint,
    Action(usize),
    Pending,
    Status,
}
type N = Node<'static, Component>;
/// The viewport crops this original logical arrangement; row updates retain identity.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([960, 720], Component::Background(0x10151e))
            .fill([true, true])
            .at(0, 0),
        N::column(
            [936, 52],
            24,
            &[
                N::leaf([936, 21], Component::Text("BEATKERNEL BMS PLAYER", TITLE)),
                N::leaf(
                    [936, 7],
                    Component::Text(
                        "PLAYERS - +/- COUNT - SPACE ASSIGN - ENTER DONE - ESC BACK",
                        LABEL,
                    ),
                ),
            ],
        )
        .at(24, 20),
        N::leaf([906, 7], Component::Summary).at(24, 91),
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
        .at(24, 120),
        N::leaf([906, 7], Component::Hint).at(24, 535),
        N::row(
            [310, 34],
            10,
            &[
                N::leaf([150, 34], Component::Action(0)),
                N::leaf([150, 34], Component::Action(1)),
            ],
        )
        .at(620, 550),
        N::leaf([906, 7], Component::Pending).at(24, 590),
        N::row(
            [910, 34],
            10,
            &[
                N::leaf([140, 34], Component::Action(2)),
                N::leaf([140, 34], Component::Action(3)),
                N::leaf([140, 34], Component::Action(4)),
                N::leaf([140, 34], Component::Action(5)),
                N::leaf([150, 34], Component::Action(6)),
                N::leaf([150, 34], Component::Action(7)),
            ],
        )
        .at(24, 620),
        N::leaf([906, 7], Component::Status).at(24, 690),
    ],
)
.clipped();
fn node_id(layout: &MountedLayout<Component>, matches: impl Fn(Component) -> bool) -> NodeId {
    layout
        .leaves()
        .iter()
        .find(|leaf| matches(leaf.component))
        .expect("mounted Players component")
        .id
}
/// Conversion failure enters the retained packet's checked refusal path.
fn paint_text(scene: &mut Scene, bounds: Bounds, value: &str, style: TextStyle) {
    match usize::try_from(bounds.x)
        .ok()
        .zip(usize::try_from(bounds.y).ok())
    {
        Some((x, y)) => text(scene, x, y, value, style.scale, style.color),
        None => scene.reject("Players text origin exceeds the supported coordinate range".into()),
    }
}
pub struct PlayersFrame<'a> {
    pub model: &'a LocalSetup,
    pub selected: usize,
    pub first: usize,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
pub struct BrowserPlayersFrame<'a> {
    pub model: &'a BrowserLocalProjection<'a>,
    pub selected: usize,
    pub first: usize,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
fn validate_browser(frame: &BrowserPlayersFrame<'_>) -> Result<(), String> {
    frame.model.validate()?;
    if frame.selected >= frame.model.players.len()
        || frame.first >= frame.model.players.len()
        || frame.first.checked_add(10).is_none()
    {
        return Err("browser Players frame exceeds index bounds".into());
    }
    Ok(())
}
pub fn hit_browser(
    frame: &BrowserPlayersFrame<'_>,
    point: Option<(f64, f64)>,
) -> Option<ControlId> {
    validate_browser(frame).ok()?;
    hit_projected(
        frame.model.players.len(),
        frame.first,
        frame.pending,
        frame.model.can_assign,
        point,
    )
}
fn bounds(slot: usize) -> Bounds {
    Bounds {
        x: 24,
        y: 120 + slot as i64 * 39,
        width: 906,
        height: 34,
    }
}
fn validate(frame: &PlayersFrame<'_>) -> Result<(), String> {
    let count = frame.model.players().len();
    if !(1..=64).contains(&count)
        || frame.selected >= count
        || frame.first >= count
        || frame.first.checked_add(10).is_none()
    {
        return Err("Players frame exceeds roster or index bounds".into());
    }
    Ok(())
}
fn gate_projected(
    count: usize,
    first: usize,
    pending: bool,
    can_assign: bool,
    id: ControlId,
) -> (bool, bool) {
    let solo = count == 1;
    let visible = match id.0 {
        36 => first > 0,
        37 => first.saturating_add(10) < count,
        34 | 35 => !solo,
        _ => true,
    };
    (
        visible,
        visible
            && !pending
            && !(id.0 == 32 && solo)
            && !(id.0 == 33 && count == 64)
            && (!matches!(id.0, 34 | 35) || can_assign),
    )
}
/// Current-frame hit routing in reverse painter order, including armed player rows.
pub fn hit(frame: &PlayersFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    validate(frame).ok()?;
    hit_projected(
        frame.model.players().len(),
        frame.first,
        frame.pending,
        true,
        point,
    )
}
fn hit_projected(
    count: usize,
    first: usize,
    pending: bool,
    can_assign: bool,
    point: Option<(f64, f64)>,
) -> Option<ControlId> {
    let point = point?;
    if pending {
        return None;
    }
    for (id, bounds, _) in BUTTONS.iter().rev() {
        if gate_projected(count, first, pending, can_assign, *id).1 && bounds.contains(point) {
            return Some(*id);
        }
    }
    for slot in (0..10).rev() {
        if first + slot < count && bounds(slot).contains(point) {
            return Some(ControlId(20000 + (first + slot) as u64));
        }
    }
    None
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    id: u32,
    input: Option<String>,
    input_label: Option<String>,
    input_kind: Option<BrowserInputKind>,
    browser: bool,
    solo: bool,
    selected: bool,
    pending: bool,
}
struct PlayerStatus<'a> {
    count: usize,
    first: usize,
    selected: usize,
    pending: bool,
    message: Option<&'a str>,
    error: Option<&'a str>,
    hovered: Option<ControlId>,
    armed: Option<ControlId>,
    browser: bool,
    can_assign: bool,
}
pub struct PlayersView {
    id: ScreenInstanceId,
    scope: Scope,
    rows: [RwSignal<Option<Row>>; 10],
    summary: RwSignal<(usize, usize)>,
    browser: RwSignal<bool>,
    pending: RwSignal<bool>,
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    gates: [RwSignal<(bool, bool)>; 8],
    nodes: RetainedNodes,
    layout: RefCell<MountedLayout<Component>>,
}
impl PlayersView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let mut layout = MountedLayout::mount(SCREEN)?;
        let initial_extent = if width == 0 || height == 0 {
            [960, 720]
        } else {
            [width, height]
        };
        layout.resize(initial_extent)?;
        let nodes = RetainedNodes::new(initial_extent[0], initial_extent[1])?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            rows: std::array::from_fn(|_| scope.create_rw_signal(None)),
            summary: scope.create_rw_signal((0, 0)),
            browser: scope.create_rw_signal(false),
            pending: scope.create_rw_signal(false),
            message: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            gates: std::array::from_fn(|_| scope.create_rw_signal((false, false))),
            nodes,
            layout: RefCell::new(layout),
        };
        let layout = view.layout.borrow().clone();
        let headers = layout
            .leaves()
            .iter()
            .filter(|leaf| {
                matches!(
                    leaf.component,
                    Component::Background(_) | Component::Text(..)
                )
            })
            .map(|leaf| (leaf.id, leaf.component))
            .collect::<Vec<_>>();
        let ids = headers.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        view.nodes.static_layout_node(
            &layout,
            &ids,
            move |id, geometry, scene, _| match headers
                .iter()
                .find(|(node, _)| *node == id)
                .unwrap()
                .1
            {
                Component::Background(color) => {
                    let bounds = geometry.bounds;
                    rect(
                        scene,
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        color,
                    );
                }
                Component::Text(value, style) => paint_text(scene, geometry.bounds, value, style),
                _ => unreachable!("mounted Players header"),
            },
        )?;
        let summary = view.summary;
        let memo = scope.create_memo(move |_| summary.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, |component| {
                matches!(component, Component::Summary)
            })],
            |(count, first), _, geometry, scene, _| {
                paint_text(
                    scene,
                    geometry.bounds,
                    &format!(
                        "{} PLAYERS  ROWS {}-{} OF {}",
                        count,
                        first + 1,
                        (first + 10).min(count),
                        count
                    ),
                    SUMMARY,
                )
            },
        )?;
        for (slot, row) in view.rows.iter().copied().enumerate() {
            let hovered = view.hovered;
            let armed = view.armed;
            let interaction = scope.create_memo(move |_| {
                row.with(|row| {
                    row.as_ref().map(|row| {
                        if row.pending {
                            (false, false)
                        } else {
                            let id = ControlId(20000 + row.index as u64);
                            (hovered.get() == Some(id), armed.get() == Some(id))
                        }
                    })
                })
            });
            let memo = scope.create_memo(move |_| (row.get(), interaction.get()));
            view.nodes.bind_layout(
                scope,
                memo,
                &layout,
                &[node_id(
                    &layout,
                    |component| matches!(component, Component::Row(index) if index == slot),
                )],
                move |(row, interaction), _, geometry, scene, hits| {
                    if let Some(row) = row {
                        let bounds = geometry.bounds;
                        let label = if row.solo {
                            "SOLO - INPUT AUTOMATIC".into()
                        } else {
                            if let Some(kind) = row.input_kind {
                                format!(
                                    "P{}  {} {} ({})",
                                    row.id,
                                    kind.label(),
                                    row.input_label.as_deref().unwrap_or(""),
                                    row.input.as_deref().unwrap_or("")
                                )
                            } else {
                                format!(
                                    "P{}  {}",
                                    row.id,
                                    row.input.as_deref().unwrap_or(if row.browser {
                                        "NO INPUT SOURCE ASSIGNED"
                                    } else {
                                        "NO KEYBOARD ASSIGNED"
                                    })
                                )
                            }
                        };
                        if row.selected {
                            rect(scene, bounds.x - 4, bounds.y, 4, bounds.height, 0x74e5c5);
                        }
                        let (hovered, armed) = interaction.unwrap_or((false, false));
                        button(scene, bounds, &label, hovered, armed);
                        if !row.pending {
                            hits.push((ControlId(20000 + row.index as u64), bounds));
                        }
                    }
                },
            )?;
        }
        let summary = view.summary;
        let browser = view.browser;
        let memo = scope.create_memo(move |_| (summary.get().0 == 1, browser.get()));
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, |component| {
                matches!(component, Component::Hint)
            })],
            |(solo, browser), _, geometry, scene, _| {
                paint_text(
                    scene,
                    geometry.bounds,
                    if solo {
                        "SOLO STARTS WITHOUT DEVICE SELECTION"
                    } else if browser {
                        "ASSIGN A DISTINCT INPUT SOURCE TO EACH PLAYER"
                    } else {
                        "ASSIGN A DISTINCT KEYBOARD TO EACH PLAYER"
                    },
                    LABEL,
                )
            },
        )?;
        for index in 0..8 {
            let (id, _, label) = BUTTONS[index];
            let gate = view.gates[index];
            let hovered = view.hovered;
            let armed = view.armed;
            let browser = view.browser;
            let memo = scope.create_memo(move |_| {
                let (visible, enabled) = gate.get();
                (
                    visible,
                    enabled,
                    enabled && hovered.get() == Some(id),
                    enabled && armed.get() == Some(id),
                    browser.get(),
                )
            });
            view.nodes.bind_layout(
                scope,
                memo,
                &layout,
                &[node_id(
                    &layout,
                    |component| matches!(component, Component::Action(action) if action == index),
                )],
                move |(visible, enabled, hovered, armed, browser), _, geometry, scene, hits| {
                    let bounds = geometry.bounds;
                    if visible {
                        button(
                            scene,
                            bounds,
                            if browser && id == ControlId(34) {
                                "ASSIGN INPUT"
                            } else {
                                label
                            },
                            hovered,
                            armed,
                        );
                        if enabled {
                            hits.push((id, bounds));
                        }
                    }
                },
            )?;
        }
        let pending = view.pending;
        let memo = scope.create_memo(move |_| pending.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, |component| {
                matches!(component, Component::Pending)
            })],
            |pending, _, geometry, scene, _| {
                if pending {
                    paint_text(scene, geometry.bounds, "LOADING KEYBOARDS", SUMMARY);
                }
            },
        )?;
        let message = view.message;
        let error = view.error;
        let memo = scope.create_memo(move |_| {
            if let Some(error) = error.get() {
                (true, Some(error))
            } else {
                (false, message.get())
            }
        });
        view.nodes.bind_layout(
            scope,
            memo,
            &layout,
            &[node_id(&layout, |component| {
                matches!(component, Component::Status)
            })],
            |(error, value), _, geometry, scene, _| {
                if let Some(value) = value {
                    paint_text(
                        scene,
                        geometry.bounds,
                        &value,
                        TextStyle {
                            scale: 1,
                            color: if error { 0xff8e8e } else { 0x74e5c5 },
                        },
                    );
                }
            },
        )?;
        view.nodes.validate()?;
        view.resize(width, height)?;
        Ok(view)
    }
    /// Update only the viewport clip while preserving the published logical arrangement.
    pub fn resize(&self, width: u32, height: u32) -> Result<bool, String> {
        let mut candidate = self.layout.borrow().clone();
        if !candidate.resize([width, height])? {
            return Ok(false);
        }
        self.nodes.relayout(&candidate)?;
        *self.layout.borrow_mut() = candidate;
        Ok(true)
    }
    /// Stage dependent row/action geometry before committing any retained packet.
    pub fn update_layout(&self, updates: &[LayoutUpdate]) -> Result<bool, String> {
        let mut candidate = self.layout.borrow().clone();
        if !candidate.update(updates)? {
            return Ok(false);
        }
        self.nodes.relayout(&candidate)?;
        *self.layout.borrow_mut() = candidate;
        Ok(true)
    }
    pub fn hit(&self, point: (f64, f64)) -> Option<ControlId> {
        self.nodes.hit(point)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn update(&self, frame: PlayersFrame<'_>) -> Result<(), String> {
        validate(&frame)?;
        let players = frame.model.players();
        self.update_projected(
            PlayerStatus {
                count: players.len(),
                first: frame.first,
                selected: frame.selected,
                pending: frame.pending,
                message: frame.message,
                error: frame.error,
                hovered: frame.hovered,
                armed: frame.armed,
                browser: false,
                can_assign: true,
            },
            |index| {
                players
                    .get(index)
                    .map(|player| (player.id.0, player.input(), None, None))
            },
        )
    }
    pub fn update_browser(&self, frame: BrowserPlayersFrame<'_>) -> Result<(), String> {
        validate_browser(&frame)?;
        self.update_projected(
            PlayerStatus {
                count: frame.model.players.len(),
                first: frame.first,
                selected: frame.selected,
                pending: frame.pending,
                message: frame.message,
                error: frame.error,
                hovered: frame.hovered,
                armed: frame.armed,
                browser: true,
                can_assign: frame.model.can_assign,
            },
            |index| {
                frame.model.players.get(index).map(|player| {
                    let source = player
                        .source
                        .and_then(|id| frame.model.sources.iter().find(|source| source.id == id));
                    (
                        player.id.0,
                        player.source,
                        source.map(|source| source.label),
                        source.map(|source| source.kind),
                    )
                })
            },
        )
    }
    fn update_projected<'a>(
        &self,
        frame: PlayerStatus<'_>,
        player: impl Fn(
            usize,
        ) -> Option<(
            u32,
            Option<&'a str>,
            Option<&'a str>,
            Option<BrowserInputKind>,
        )>,
    ) -> Result<(), String> {
        let solo = frame.count == 1;
        for (slot, signal) in self.rows.iter().enumerate() {
            let index = frame.first + slot;
            if let Some((id, input, input_label, input_kind)) = player(index) {
                let selected = frame.selected == index;
                if !signal.with_untracked(|old| {
                    old.as_ref().is_some_and(|old| {
                        old.index == index
                            && old.id == id
                            && old.input.as_deref() == input
                            && old.input_label.as_deref() == input_label
                            && old.input_kind == input_kind
                            && old.browser == frame.browser
                            && old.solo == solo
                            && old.selected == selected
                            && old.pending == frame.pending
                    })
                }) {
                    signal.set(Some(Row {
                        index,
                        id,
                        input: input.map(str::to_owned),
                        input_label: input_label.map(str::to_owned),
                        input_kind,
                        browser: frame.browser,
                        solo,
                        selected,
                        pending: frame.pending,
                    }));
                }
            } else if signal.with_untracked(Option::is_some) {
                signal.set(None);
            }
        }
        let summary = (frame.count, frame.first);
        if self.browser.get_untracked() != frame.browser {
            self.browser.set(frame.browser);
        }
        if self.summary.get_untracked() != summary {
            self.summary.set(summary);
        }
        if self.pending.get_untracked() != frame.pending {
            self.pending.set(frame.pending);
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
        for (signal, (id, _, _)) in self.gates.iter().zip(BUTTONS) {
            let gate = gate_projected(
                frame.count,
                frame.first,
                frame.pending,
                frame.can_assign,
                id,
            );
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
        !self.layout.borrow().suspended() && self.nodes.dirty()
    }
    pub fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        if self.layout.borrow().suspended() {
            scene.clear();
            hits.clear();
            return Ok(());
        }
        self.nodes.compose(scene, hits)
    }
}
impl Drop for PlayersView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{NativeSettings, SettingsHost};

    fn model(count: usize) -> LocalSetup {
        let settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        let mut model = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
        model.resize(count).unwrap();
        model
    }
    fn frame(model: &LocalSetup) -> PlayersFrame<'_> {
        PlayersFrame {
            model,
            selected: 0,
            first: 0,
            pending: false,
            error: None,
            message: None,
            hovered: None,
            armed: None,
        }
    }
    #[test]
    fn unchanged_status_and_row_interactions_keep_unrelated_packets() {
        let model = model(4);
        let view = PlayersView::new(ScreenInstanceId(40), 960, 720).unwrap();
        view.update(frame(&model)).unwrap();
        let initial = view.nodes.paints();
        view.update(frame(&model)).unwrap();
        assert_eq!(view.nodes.paints(), initial);
        let mut update = frame(&model);
        update.error = Some("EDITABLE DRAFT");
        view.update(update).unwrap();
        let status = view.nodes.paints();
        assert_eq!(&status[2..12], &initial[2..12]);
        let mut update = frame(&model);
        update.error = Some("EDITABLE DRAFT");
        update.hovered = Some(ControlId(20002));
        update.armed = Some(ControlId(20002));
        view.update(update).unwrap();
        let hovered = view.nodes.paints();
        for index in 2..12 {
            if index == 4 {
                assert!(hovered[index] > status[index]);
            } else {
                assert_eq!(hovered[index], status[index]);
            }
        }
        let mut update = frame(&model);
        update.selected = 1;
        update.error = Some("EDITABLE DRAFT");
        update.hovered = Some(ControlId(20002));
        update.armed = Some(ControlId(20002));
        view.update(update).unwrap();
        let selected = view.nodes.paints();
        assert!(selected[2] > hovered[2]);
        assert!(selected[3] > hovered[3]);
        assert_eq!(&selected[4..12], &hovered[4..12]);
    }
    #[test]
    fn solo_and_full_roster_paging_preserve_legacy_admission_and_order() {
        let solo = model(1);
        let view = PlayersView::new(ScreenInstanceId(41), 960, 720).unwrap();
        view.update(frame(&solo)).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![20000, 30, 31, 33]
        );
        assert_eq!(hit(&frame(&solo), Some((330.0, 625.0))), None);
        assert_eq!(hit(&frame(&solo), Some((630.0, 625.0))), None);
        let group = model(64);
        let mut page = frame(&group);
        page.first = 50;
        page.selected = 50;
        assert_eq!(hit(&page, Some((30.0, 125.0))), Some(ControlId(20050)));
        view.update(page).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[..10].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            (20050..20060).collect::<Vec<_>>()
        );
        assert!(hits.iter().any(|(id, _)| id.0 == 36));
        assert!(hits.iter().any(|(id, _)| id.0 == 37));
        assert!(!hits.iter().any(|(id, _)| id.0 == 33));
        let mut pending = frame(&group);
        pending.first = 50;
        pending.selected = 50;
        pending.pending = true;
        assert_eq!(hit(&pending, Some((30.0, 125.0))), None);
        view.update(pending).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
    }
    #[test]
    fn restored_max_id_is_data_not_a_row_index_and_failure_is_atomic() {
        let args = vec![
            "--local-player".into(),
            "4294967295:/last".into(),
            "--local-player".into(),
            "7:/other".into(),
        ];
        let settings = NativeSettings::from_args(&args, SettingsHost::Linux).unwrap();
        let model = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
        let view = PlayersView::new(ScreenInstanceId(42), 960, 720).unwrap();
        view.update(frame(&model)).unwrap();
        assert_eq!(view.rows[0].get_untracked().unwrap().id, u32::MAX);
        let before = view.nodes.paints();
        let identities = view.nodes.identities();
        let mut invalid = frame(&model);
        invalid.selected = 2;
        invalid.error = Some("MUST NOT APPLY");
        assert!(view.update(invalid).is_err());
        let mut invalid = frame(&model);
        invalid.first = usize::MAX;
        assert!(view.update(invalid).is_err());
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.error.get_untracked(), None);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let count = scene.rectangles().len();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.nodes.paints(), before);
        let weak = view.nodes.weak_dirty();
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(PlayersView::new(ScreenInstanceId(42), 800, 600).is_ok());
    }
}

#[cfg(test)]
#[path = "players_declarative_fixtures.rs"]
mod players_declarative_fixtures;
