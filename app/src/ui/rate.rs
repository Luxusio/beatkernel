//! Retained runtime-rate editor owned by an existing Play screen.
use super::{
    atoms::text,
    interaction::{Bounds, ControlId},
    layout::{LayoutChange, LayoutUpdate, MountedLayout, Node, NodeId},
    molecules::{button, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText, practice_control::PracticeRateState, scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate};

pub const NUMERATOR_CONTROL: ControlId = ControlId(60_000);
pub const DENOMINATOR_CONTROL: ControlId = ControlId(60_001);
pub const APPLY_CONTROL: ControlId = ControlId(60_002);

#[derive(Clone, Copy)]
enum Component {
    Label(&'static str),
    Editor(ControlId),
    Apply,
    Status,
}
type N = Node<'static, Component>;
const PANEL: N = N::layer(
    [960, 720],
    &[
        N::leaf([105, 7], Component::Label("RATE NUM")).at(560, 5),
        N::leaf([105, 7], Component::Label("DEN")).at(680, 5),
        N::leaf([105, 30], Component::Editor(NUMERATOR_CONTROL)).at(560, 18),
        N::leaf([105, 30], Component::Editor(DENOMINATOR_CONTROL)).at(680, 18),
        N::leaf([136, 30], Component::Apply).at(800, 18),
        N::leaf([376, 7], Component::Status).at(560, 53),
    ],
)
.clipped();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateFrame {
    pub numerator: LineEditor,
    pub denominator: LineEditor,
    pub focused: Option<ControlId>,
    pub pending: bool,
    pub applied: Option<PracticeRateState>,
    pub error: Option<String>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}

pub struct RateView {
    id: ScreenInstanceId,
    scope: Scope,
    numerator: RwSignal<LineEditor>,
    denominator: RwSignal<LineEditor>,
    focused: RwSignal<Option<ControlId>>,
    pending: RwSignal<bool>,
    applied: RwSignal<Option<PracticeRateState>>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    input_font: RwSignal<Option<FontText>>,
    nodes: RetainedNodes,
    layout: MountedLayout<Component>,
    anchors: Vec<(NodeId, [i64; 2])>,
}
impl RateView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let layout = MountedLayout::mount(PANEL)?;
        let anchors = layout
            .leaves()
            .iter()
            .map(|leaf| (leaf.id, [leaf.geometry.bounds.x, leaf.geometry.bounds.y]))
            .collect();
        let numerator = LineEditor::new("1", 32)?;
        let denominator = LineEditor::new("1", 32)?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            numerator: scope.create_rw_signal(numerator),
            denominator: scope.create_rw_signal(denominator),
            focused: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            applied: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            input_font: scope.create_rw_signal(None),
            nodes: RetainedNodes::new(width, height)?,
            layout,
            anchors,
        };
        // Resolve declaration once. Relayout and signals retain these identities.
        reflow(&mut view.layout, &view.anchors, [width, height])?;
        let leaves = view.layout.leaves().to_vec();
        let labels: Vec<_> = leaves
            .iter()
            .copied()
            .filter(|leaf| matches!(leaf.component, Component::Label(_)))
            .collect();
        let ids: Vec<_> = labels.iter().map(|leaf| leaf.id).collect();
        view.nodes
            .static_layout_node(&view.layout, &ids, move |id, geometry, scene, _| {
                let Component::Label(label) =
                    labels.iter().find(|leaf| leaf.id == id).unwrap().component
                else {
                    unreachable!()
                };
                text(
                    scene,
                    geometry.bounds.x as usize,
                    geometry.bounds.y as usize,
                    label,
                    1,
                    0x9bb1cf,
                );
            })?;
        for leaf in leaves {
            match leaf.component {
                Component::Editor(control) => {
                    let denominator = control == DENOMINATOR_CONTROL;
                    let editor = if denominator {
                        view.denominator
                    } else {
                        view.numerator
                    };
                    let focused = view.focused;
                    let font = view.input_font;
                    let memo = scope.create_memo(move |_| {
                        (editor.get(), focused.get() == Some(control), font.get())
                    });
                    view.nodes.bind_layout(
                        scope,
                        memo,
                        &view.layout,
                        &[leaf.id],
                        move |(editor, focused, font), _, geometry, scene, hits| {
                            text_field_with_font(
                                scene,
                                editor,
                                geometry.bounds,
                                *focused,
                                font.as_ref(),
                            );
                            hits.push((control, geometry.bounds));
                        },
                    )?;
                }
                Component::Apply => {
                    let pending = view.pending;
                    let applied = view.applied;
                    let hovered = view.hovered;
                    let armed = view.armed;
                    let memo = scope.create_memo(move |_| {
                        (
                            pending.get(),
                            applied.get().is_some(),
                            hovered.get() == Some(APPLY_CONTROL),
                            armed.get() == Some(APPLY_CONTROL),
                        )
                    });
                    view.nodes.bind_layout(
                        scope,
                        memo,
                        &view.layout,
                        &[leaf.id],
                        move |&(pending, available, hovered, armed), _, geometry, scene, hits| {
                            button(
                                scene,
                                geometry.bounds,
                                if pending { "PENDING" } else { "APPLY RATE" },
                                available && !pending && hovered,
                                available && !pending && armed,
                            );
                            if available && !pending {
                                hits.push((APPLY_CONTROL, geometry.bounds));
                            }
                        },
                    )?;
                }
                Component::Status => {
                    let applied = view.applied;
                    let pending = view.pending;
                    let error = view.error;
                    let memo =
                        scope.create_memo(move |_| (applied.get(), pending.get(), error.get()));
                    view.nodes.bind_layout(
                        scope,
                        memo,
                        &view.layout,
                        &[leaf.id],
                        move |(applied, pending, error), _, geometry, scene, _| {
                            let mut status = if let Some(applied) = applied {
                                format!(
                                    "APPLIED {}/{}  REV {}{}",
                                    applied.rate.numerator(),
                                    applied.rate.denominator(),
                                    applied.revision,
                                    if *pending { "  PENDING AUDIO" } else { "" }
                                )
                            } else {
                                "RUNTIME RATE UNAVAILABLE".into()
                            };
                            if let Some(error) = error {
                                status.push_str(" | ");
                                status.push_str(error);
                            }
                            text(
                                scene,
                                geometry.bounds.x as usize,
                                geometry.bounds.y as usize,
                                &status,
                                1,
                                if error.is_some() { 0xffaaaa } else { 0xd8b36b },
                            );
                        },
                    )?;
                }
                Component::Label(_) => {}
            }
        }
        view.nodes.validate()?;
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn update(&self, frame: RateFrame) {
        if self.numerator.get_untracked() != frame.numerator {
            self.numerator.set(frame.numerator);
        }
        if self.denominator.get_untracked() != frame.denominator {
            self.denominator.set(frame.denominator);
        }
        if self.focused.get_untracked() != frame.focused {
            self.focused.set(frame.focused);
        }
        if self.pending.get_untracked() != frame.pending {
            self.pending.set(frame.pending);
        }
        if self.applied.get_untracked() != frame.applied {
            self.applied.set(frame.applied);
        }
        if self.error.get_untracked() != frame.error {
            self.error.set(frame.error);
        }
        if self.hovered.get_untracked() != frame.hovered {
            self.hovered.set(frame.hovered);
        }
        if self.armed.get_untracked() != frame.armed {
            self.armed.set(frame.armed);
        }
    }
    pub fn set_input_font(&self, font: Option<FontText>) {
        if self.input_font.get_untracked() != font {
            self.input_font.set(font);
        }
    }
    pub fn dirty(&self) -> bool {
        self.nodes.dirty()
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if self.layout.extent() == [width, height] {
            return Ok(false);
        }
        let mut candidate = self.layout.clone();
        reflow(&mut candidate, &self.anchors, [width, height])?;
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
        Ok(true)
    }
    pub fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        self.nodes.compose(scene, hits)
    }
    pub fn hit(&self, point: (f64, f64)) -> Option<ControlId> {
        self.nodes.hit(point)
    }
    pub fn node_for_control(&self, control: ControlId) -> Result<Option<NodeId>, String> {
        self.nodes.node_for_control(control)
    }
}
fn reflow(
    layout: &mut MountedLayout<Component>,
    anchors: &[(NodeId, [i64; 2])],
    extent: [u32; 2],
) -> Result<bool, String> {
    if extent.contains(&0) {
        return layout.resize(extent);
    }
    let mut updates = vec![LayoutUpdate {
        id: NodeId(0),
        change: LayoutChange::Size(extent.map(i64::from)),
    }];
    updates.extend(anchors.iter().map(|&(id, origin)| LayoutUpdate {
        id,
        change: LayoutChange::Origin([
            origin[0] * i64::from(extent[0]) / 960,
            origin[1] * i64::from(extent[1]) / 720,
        ]),
    }));
    layout.update_on_extent(extent, &updates)
}
impl Drop for RateView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
#[cfg(test)]
#[path = "rate_declarative_fixtures.rs"]
mod rate_declarative_fixtures;
