//! Retained precise practice-section draft editing; no native transport or clocks.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    layout::{LayoutChange, LayoutUpdate, MountedLayout, Node, NodeId, TextStyle},
    molecules::{button, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText, practice::PracticeStart, scene::Scene, screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate};

const TITLE: TextStyle = TextStyle {
    scale: 3,
    color: 0xf0f4ff,
};
const SUBTITLE: TextStyle = TextStyle {
    scale: 2,
    color: 0xf0f4ff,
};
const LABEL: TextStyle = TextStyle {
    scale: 1,
    color: 0x9bb1cf,
};
const FORMAT: TextStyle = TextStyle {
    scale: 2,
    color: 0x9bb1cf,
};
const PREVIEW: TextStyle = TextStyle {
    scale: 2,
    color: 0xd8b36b,
};
const ERROR: TextStyle = TextStyle {
    scale: 1,
    color: 0xffaaaa,
};
#[derive(Clone, Copy)]
enum Component {
    Background(u32),
    Text(&'static str, TextStyle),
    StartEditor(ControlId),
    StartPreview(TextStyle),
    EndEditor(ControlId),
    EndPreview(TextStyle),
    Action(ControlId, &'static str),
    Error(TextStyle),
}
type N = Node<'static, Component>;
// Static hints retain their original painter order; each editor/preview pair
// shares a column so a direct editor allocation moves its dependent preview.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([960, 720], Component::Background(0x10151e)).at(0, 0),
        N::column(
            [936, 59],
            24,
            &[
                N::leaf([936, 21], Component::Text("BEATKERNEL BMS PLAYER", TITLE)),
                N::leaf([936, 14], Component::Text("PRACTICE SECTION", SUBTITLE)),
            ],
        )
        .at(24, 20),
        N::leaf([936, 7], Component::Text("START", LABEL)).at(24, 134),
        N::leaf(
            [936, 7],
            Component::Text("END (OPTIONAL; EMPTY PLAYS THROUGH SONG END)", LABEL),
        )
        .at(24, 260),
        N::leaf(
            [936, 14],
            Component::Text("SECONDS / M:SS / H:MM:SS  FRACTION UP TO 9 DIGITS", FORMAT),
        )
        .at(24, 105),
        N::column(
            [936, 73],
            15,
            &[
                N::leaf(
                    [936, 7],
                    Component::Text(
                        "DONE UPDATES SETTINGS DRAFT; APPLY CHANGES THE NEXT SESSION",
                        LABEL,
                    ),
                ),
                N::leaf(
                    [936, 7],
                    Component::Text(
                        "F5 RETRIES THE PINNED SESSION; BACK DISCARDS THESE EDITS",
                        LABEL,
                    ),
                ),
                N::leaf(
                    [936, 7],
                    Component::Text(
                        "FULL SONG RESETS START AND END; THROUGH END CLEARS ONLY END",
                        LABEL,
                    ),
                ),
                N::leaf(
                    [936, 7],
                    Component::Text("TAB SWITCHES START / END; END MUST BE AFTER START", LABEL),
                ),
            ],
        )
        .at(24, 450),
        N::column(
            [906, 94],
            40,
            &[
                N::leaf([906, 40], Component::StartEditor(ControlId(70))),
                N::leaf([906, 14], Component::StartPreview(PREVIEW)),
            ],
        )
        .at(24, 150),
        N::column(
            [906, 69],
            15,
            &[
                N::leaf([906, 40], Component::EndEditor(ControlId(75))),
                N::leaf([906, 14], Component::EndPreview(PREVIEW)),
            ],
        )
        .at(24, 280),
        N::row(
            [768, 34],
            16,
            &[
                N::leaf([180, 34], Component::Action(ControlId(71), "DONE")),
                N::leaf([180, 34], Component::Action(ControlId(72), "BACK")),
                N::leaf([180, 34], Component::Action(ControlId(73), "FULL SONG")),
                N::leaf([180, 34], Component::Action(ControlId(76), "THROUGH END")),
            ],
        )
        .at(24, 380),
        N::leaf([936, 7], Component::Error(ERROR)).at(24, 560),
    ],
)
.clipped();
fn paint_text(scene: &mut Scene, bounds: Bounds, label: &str, style: TextStyle) {
    text(
        scene,
        bounds.x as usize,
        bounds.y as usize,
        label,
        style.scale,
        style.color,
    );
}
fn reflow_practice(
    layout: &mut MountedLayout<Component>,
    anchors: &[(NodeId, [i64; 2])],
    extent: [u32; 2],
) -> Result<bool, String> {
    if extent.contains(&0) {
        return layout.resize(extent);
    }
    let mut updates = vec![LayoutUpdate {
        id: NodeId(1),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PracticeFrame {
    pub editor: LineEditor,
    pub end_editor: LineEditor,
    pub end_focused: bool,
    pub error: Option<String>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}

/// Mounted retained hierarchy owned by its native or browser Navigator instance.
/// Done updates the parent draft; Apply and pinned F5 remain coordinator-owned.
pub struct PracticeView {
    id: ScreenInstanceId,
    scope: Scope,
    editor: RwSignal<LineEditor>,
    end_editor: RwSignal<LineEditor>,
    end_focused: RwSignal<bool>,
    input_font: RwSignal<Option<FontText>>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
    layout: MountedLayout<Component>,
    anchors: Vec<(NodeId, [i64; 2])>,
    editor_height: u32,
}
impl PracticeView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let editor = LineEditor::new("0:00", 64)?;
        let end_editor = LineEditor::new("", 64)?;
        let mut layout = MountedLayout::mount(SCREEN)?;
        let anchors = layout
            .children(NodeId(0))
            .unwrap()
            .iter()
            .copied()
            .filter(|&id| id != NodeId(1))
            .map(|id| {
                let bounds = layout.geometry(id).unwrap().bounds;
                (id, [bounds.x, bounds.y])
            })
            .collect::<Vec<_>>();
        reflow_practice(&mut layout, &anchors, [width, height])?;
        let nodes = RetainedNodes::new(width, height)?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            editor: scope.create_rw_signal(editor),
            end_editor: scope.create_rw_signal(end_editor),
            end_focused: scope.create_rw_signal(false),
            input_font: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
            layout,
            anchors,
            editor_height: 40,
        };
        let leaves = view.layout.leaves().to_vec();
        let static_leaves: Vec<_> = leaves
            .iter()
            .copied()
            .filter(|leaf| {
                matches!(
                    leaf.component,
                    Component::Background(_) | Component::Text(..)
                )
            })
            .collect();
        let static_ids: Vec<_> = static_leaves.iter().map(|leaf| leaf.id).collect();
        view.nodes.static_layout_node(
            &view.layout,
            &static_ids,
            move |id, geometry, scene, _| match static_leaves
                .iter()
                .find(|leaf| leaf.id == id)
                .unwrap()
                .component
            {
                Component::Background(color) => {
                    let b = geometry.bounds;
                    rect(scene, b.x, b.y, b.width, b.height, color);
                }
                Component::Text(label, style) => paint_text(scene, geometry.bounds, label, style),
                _ => unreachable!(),
            },
        )?;
        for end in [false, true] {
            let pair: Vec<_> = leaves
                .iter()
                .copied()
                .filter(|leaf| {
                    if end {
                        matches!(
                            leaf.component,
                            Component::EndEditor(_) | Component::EndPreview(_)
                        )
                    } else {
                        matches!(
                            leaf.component,
                            Component::StartEditor(_) | Component::StartPreview(_)
                        )
                    }
                })
                .collect();
            let pair_ids: Vec<_> = pair.iter().map(|leaf| leaf.id).collect();
            let editor = if end { view.end_editor } else { view.editor };
            let end_focused = view.end_focused;
            let input_font = view.input_font;
            let memo =
                scope.create_memo(move |_| (editor.get(), end_focused.get(), input_font.get()));
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &pair_ids,
                move |(editor, end_focused, font), id, geometry, scene, hits| match pair
                    .iter()
                    .find(|leaf| leaf.id == id)
                    .unwrap()
                    .component
                {
                    Component::StartEditor(control) | Component::EndEditor(control) => {
                        text_field_with_font(
                            scene,
                            &editor,
                            geometry.bounds,
                            end == end_focused,
                            font.as_ref(),
                        );
                        hits.push((control, geometry.bounds));
                    }
                    Component::StartPreview(style) | Component::EndPreview(style) => {
                        let preview = if end && editor.value().is_empty() {
                            "THROUGH SONG END".into()
                        } else {
                            PracticeStart::parse(editor.value())
                                .map(|time| {
                                    format!(
                                        "EXACT {}: {} NS",
                                        if end { "END" } else { "START" },
                                        time.nanoseconds()
                                    )
                                })
                                .unwrap_or_else(|_| {
                                    format!("INVALID {}", if end { "END" } else { "START" })
                                })
                        };
                        paint_text(scene, geometry.bounds, &preview, style);
                    }
                    _ => unreachable!(),
                },
            )?;
        }
        for leaf in &leaves {
            let Component::Action(control, label) = leaf.component else {
                continue;
            };
            view.button_node(leaf.id, control, label)?;
        }
        let error_leaf = leaves
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Error(_)))
            .unwrap();
        let Component::Error(error_style) = error_leaf.component else {
            unreachable!()
        };
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[error_leaf.id],
            move |error, _, geometry, scene, _| {
                if let Some(error) = error {
                    paint_text(scene, geometry.bounds, &error, error_style);
                }
            },
        )?;
        view.nodes.validate()?;
        Ok(view)
    }
    /// Reflows section anchors and inherited clips without replacing editors,
    /// reactive scopes or the caller-owned practice draft.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if self.layout.extent() == [width, height] {
            return Ok(false);
        }
        let mut candidate = self.layout.clone();
        reflow_practice(&mut candidate, &self.anchors, [width, height])?;
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
        Ok(true)
    }
    /// Direct allocation changes move each exact-time preview through its
    /// existing column dependency, keeping the action and hint packets intact.
    pub fn set_editor_height(&mut self, height: u32) -> Result<bool, String> {
        if height == 0 {
            return Err("Practice editor height must be positive".into());
        }
        if self.editor_height == height {
            return Ok(false);
        }
        let mut updates = Vec::new();
        for leaf in self.layout.leaves() {
            let gap = match leaf.component {
                Component::StartEditor(_) => 40,
                Component::EndEditor(_) => 15,
                _ => continue,
            };
            let column = self
                .layout
                .children(NodeId(0))
                .unwrap()
                .iter()
                .copied()
                .find(|&id| {
                    self.layout
                        .children(id)
                        .is_some_and(|children| children.first() == Some(&leaf.id))
                })
                .ok_or("Practice editor column missing")?;
            updates.push(LayoutUpdate {
                id: column,
                change: LayoutChange::Size([906, i64::from(height) + gap + 14]),
            });
            updates.push(LayoutUpdate {
                id: leaf.id,
                change: LayoutChange::Size([906, i64::from(height)]),
            });
        }
        let mut candidate = self.layout.clone();
        candidate.update(&updates)?;
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
        self.editor_height = height;
        Ok(true)
    }
    pub fn hit(&self, point: (f64, f64)) -> Option<ControlId> {
        self.nodes.hit(point)
    }
    /// Resolve a mounted node only from this view's actual published control geometry.
    pub fn node_for_control(&self, control: ControlId) -> Result<Option<NodeId>, String> {
        self.nodes.node_for_control(control)
    }
    pub fn compose_components(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
        screen: ScreenInstanceId,
        animated: &[NodeId],
    ) -> Result<(), String> {
        self.nodes.compose_components(scene, hits, screen, animated)
    }
    pub fn hit_components(
        &self,
        scene: &Scene,
        screen: ScreenInstanceId,
        point: (f64, f64),
    ) -> Option<ControlId> {
        self.nodes.hit_components(scene, screen, point)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn set_input_font(&self, font: Option<FontText>) {
        if self.input_font.get_untracked() != font {
            self.input_font.set(font);
        }
    }
    pub fn update(&self, frame: PracticeFrame) {
        if self.editor.get_untracked() != frame.editor {
            self.editor.set(frame.editor);
        }
        if self.end_editor.get_untracked() != frame.end_editor {
            self.end_editor.set(frame.end_editor);
        }
        if self.end_focused.get_untracked() != frame.end_focused {
            self.end_focused.set(frame.end_focused);
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
    pub fn dirty(&self) -> bool {
        self.nodes.dirty()
    }
    /// Concatenates retained geometry in painter order, also for forced restore.
    /// A partial composition never clears dirty status.
    pub fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        self.nodes.compose(scene, hits)
    }
    fn button_node(
        &mut self,
        node: NodeId,
        id: ControlId,
        label: &'static str,
    ) -> Result<(), String> {
        let hovered = self.hovered;
        let armed = self.armed;
        let memo = self
            .scope
            .create_memo(move |_| (hovered.get() == Some(id), armed.get() == Some(id)));
        self.nodes.bind_layout(
            self.scope,
            memo,
            &self.layout,
            &[node],
            move |(hovered, armed), _, geometry, scene, hits| {
                button(scene, geometry.bounds, label, hovered, armed);
                hits.push((id, geometry.bounds));
            },
        )
    }
}
impl Drop for PracticeView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
#[cfg(test)]
#[path = "practice_declarative_fixtures.rs"]
mod practice_declarative_fixtures;

#[cfg(test)]
mod fixtures {
    use super::*;
    fn frame(value: &str) -> PracticeFrame {
        PracticeFrame {
            editor: LineEditor::new(value, 64).unwrap(),
            end_editor: LineEditor::new("", 64).unwrap(),
            end_focused: false,
            error: None,
            hovered: None,
            armed: None,
        }
    }
    fn paints(view: &PracticeView) -> Vec<usize> {
        view.nodes.paints()
    }
    #[test]
    fn editor_error_and_buttons_repaint_only_their_retained_dependencies() {
        let view = PracticeView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = paints(&view);
        view.update(frame("0:00"));
        assert_eq!(paints(&view), before);
        assert!(!view.dirty());
        let mut update = frame("20:00:00.000000001");
        view.update(update.clone());
        let after = paints(&view);
        assert_eq!(after[1], before[1] + 1);
        assert_eq!(after[0], before[0]);
        assert_eq!(&after[2..], &before[2..]);
        update.error = Some("REJECTED".into());
        view.update(update.clone());
        let error = paints(&view);
        assert_eq!(&error[..7], &after[..7]);
        assert_eq!(error[7], after[7] + 1);
        update.hovered = Some(ControlId(71));
        view.update(update.clone());
        let hovered = paints(&view);
        assert_eq!(hovered[3], error[3] + 1);
        assert_eq!(&hovered[..3], &error[..3]);
        assert_eq!(&hovered[4..], &error[4..]);
        view.compose(&mut scene, &mut hits).unwrap();
        view.update(update);
        assert!(!view.dirty());
        assert_eq!(paints(&view), hovered);
    }
    #[test]
    fn independent_end_edit_cursor_and_focus_leave_status_buttons_and_idle_nodes_unchanged() {
        let view = PracticeView::new(ScreenInstanceId(10), 960, 720).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let identities = view.nodes.identities();
        let before = paints(&view);
        let mut update = frame("0:00");
        update.end_editor = LineEditor::new("168:00:00.000000001", 64).unwrap();
        view.update(update.clone());
        let edited = paints(&view);
        assert_eq!(edited[2], before[2] + 1);
        assert_eq!(&edited[..2], &before[..2]);
        assert_eq!(&edited[3..], &before[3..]);
        update.end_editor.left();
        view.update(update.clone());
        let cursor = paints(&view);
        assert_eq!(cursor[2], edited[2] + 1);
        assert_eq!(&cursor[..2], &edited[..2]);
        assert_eq!(&cursor[3..], &edited[3..]);
        update.end_focused = true;
        view.update(update.clone());
        let focused = paints(&view);
        assert_eq!(focused[1], cursor[1] + 1);
        assert_eq!(focused[2], cursor[2] + 1);
        assert_eq!(focused[0], cursor[0]);
        assert_eq!(&focused[3..], &cursor[3..]);
        update.hovered = Some(ControlId(76));
        update.armed = Some(ControlId(76));
        view.update(update.clone());
        let button = paints(&view);
        assert!(button[6] > focused[6]);
        assert_eq!(&button[..6], &focused[..6]);
        assert_eq!(button[7], focused[7]);
        update.end_editor = LineEditor::new("", 64).unwrap();
        view.update(update.clone());
        let cleared = paints(&view);
        assert_eq!(cleared[2], button[2] + 1);
        assert_eq!(&cleared[..2], &button[..2]);
        assert_eq!(&cleared[3..], &button[3..]);
        view.compose(&mut scene, &mut hits).unwrap();
        view.update(update);
        assert!(!view.dirty());
        assert_eq!(paints(&view), cleared);
        assert_eq!(view.nodes.identities(), identities);
    }
    #[test]
    fn composition_preserves_hit_order_restores_geometry_and_drop_disposes_effects() {
        let view = PracticeView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.update(frame("invalid"));
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![70, 75, 71, 72, 73, 76]
        );
        assert_eq!(
            (hits[0].1.x, hits[0].1.y, hits[0].1.width, hits[0].1.height),
            (24, 150, 906, 40)
        );
        assert_eq!(
            (hits[1].1.x, hits[1].1.y, hits[1].1.width, hits[1].1.height),
            (24, 280, 906, 40)
        );
        for (index, (_, bounds)) in hits.iter().enumerate() {
            assert!(bounds.x >= 0 && bounds.y >= 0);
            assert!(bounds.x + bounds.width <= 960 && bounds.y + bounds.height <= 720);
            for (_, other) in &hits[index + 1..] {
                assert!(
                    bounds.x + bounds.width <= other.x
                        || other.x + other.width <= bounds.x
                        || bounds.y + bounds.height <= other.y
                        || other.y + other.height <= bounds.y
                );
            }
        }
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        let count = scene.rectangles().len();
        let before = paints(&view);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(paints(&view), before);
        let weak = view.nodes.weak_dirty();
        assert!(weak.strong_count() >= 2);
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(PracticeView::new(ScreenInstanceId(9), 800, 600).is_ok());
    }
}
