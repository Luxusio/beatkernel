//! Retained Display draft fields with independent editor and focus dependencies.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    layout::{LayoutChange, LayoutUpdate, MountedLayout, Node, NodeId, TextStyle},
    molecules::{button, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::font_text::FontText;
use crate::scene::Scene;
use crate::screen_lifecycle::ScreenInstanceId;
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};

pub const BUTTONS: [(ControlId, Bounds, &'static str); 2] = [
    (
        ControlId(40),
        Bounds {
            x: 24,
            y: 620,
            width: 170,
            height: 34,
        },
        "DONE",
    ),
    (
        ControlId(41),
        Bounds {
            x: 212,
            y: 620,
            width: 170,
            height: 34,
        },
        "BACK",
    ),
];
// Named visual styles and the complete screen hierarchy live together here.
const TITLE: TextStyle = TextStyle {
    scale: 3,
    color: 0xf0f4ff,
};
const SUBTITLE: TextStyle = TextStyle {
    scale: 2,
    color: 0x9bb1cf,
};
const LABEL: TextStyle = TextStyle {
    scale: 1,
    color: 0xf0f4ff,
};
const HELP: TextStyle = TextStyle {
    scale: 1,
    color: 0x9bb1cf,
};
const ERROR: TextStyle = TextStyle {
    scale: 1,
    color: 0xff8e8e,
};
#[derive(Clone, Copy)]
enum Component {
    Background(u32),
    Header(&'static str, TextStyle),
    FieldLabel(usize, &'static str),
    Editor(usize, ControlId),
    Hint(&'static str),
    Action(ControlId, &'static str),
    Error,
}
type N = Node<'static, Component>;
// Fixed logical coordinates anchor sections; rows/columns express their contents.
const SCREEN: N =
    N::layer(
        [960, 720],
        &[
            N::leaf([960, 720], Component::Background(0x10151e)).at(0, 0),
            N::column(
                [936, 59],
                24,
                &[
                    N::leaf([936, 21], Component::Header("BEATKERNEL BMS PLAYER", TITLE)),
                    N::leaf(
                        [936, 14],
                        Component::Header("DISPLAY - ENTER DONE - ESC BACK", SUBTITLE),
                    ),
                ],
            )
            .at(24, 20),
            N::column(
                [906, 259],
                41,
                &[
                    N::row(
                        [906, 34],
                        24,
                        &[
                            N::layer(
                                [232, 34],
                                &[N::leaf([232, 7], Component::FieldLabel(0, "GPU BACKEND"))
                                    .at(0, 10)],
                            ),
                            N::leaf([650, 34], Component::Editor(0, ControlId(40000))),
                        ],
                    ),
                    N::row(
                        [906, 34],
                        24,
                        &[
                            N::layer(
                                [232, 34],
                                &[N::leaf([232, 7], Component::FieldLabel(1, "PRESENT MODE"))
                                    .at(0, 10)],
                            ),
                            N::leaf([650, 34], Component::Editor(1, ControlId(40001))),
                        ],
                    ),
                    N::row(
                        [906, 34],
                        24,
                        &[
                            N::layer(
                                [232, 34],
                                &[N::leaf([232, 7], Component::FieldLabel(2, "UI FPS")).at(0, 10)],
                            ),
                            N::leaf([650, 34], Component::Editor(2, ControlId(40002))),
                        ],
                    ),
                    N::row(
                        [906, 34],
                        24,
                        &[
                            N::layer(
                                [232, 34],
                                &[N::leaf([232, 7], Component::FieldLabel(3, "LOOKAHEAD MS"))
                                    .at(0, 10)],
                            ),
                            N::leaf([650, 34], Component::Editor(3, ControlId(40003))),
                        ],
                    ),
                ],
            )
            .at(24, 130),
            N::column(
                [936, 77],
                18,
                &[
                    N::column(
                        [936, 37],
                        8,
                        &[
                            N::leaf(
                                [936, 7],
                                Component::Hint("BACKEND: AUTO / VULKAN / DX12 / METAL / GL"),
                            ),
                            N::leaf(
                                [936, 7],
                                Component::Hint("PRESENT: FIFO / IMMEDIATE / MAILBOX"),
                            ),
                            N::leaf(
                                [936, 7],
                                Component::Hint("UI FPS: 30..240   LOOKAHEAD: 100..10000 MS"),
                            ),
                        ],
                    ),
                    N::column(
                        [936, 22],
                        8,
                        &[
                            N::leaf(
                                [936, 7],
                                Component::Hint("SAVE PROFILE + RESTART FOR GPU BACKEND"),
                            ),
                            N::leaf(
                                [936, 7],
                                Component::Hint("DONE UPDATES DRAFT - APPLY IS SEPARATE"),
                            ),
                        ],
                    ),
                ],
            )
            .at(24, 445),
            // The public button definitions are also consumed by desktop hover handling.
            N::row(
                [358, 34],
                BUTTONS[1].1.x - BUTTONS[0].1.x - BUTTONS[0].1.width,
                &[
                    N::leaf(
                        [BUTTONS[0].1.width, BUTTONS[0].1.height],
                        Component::Action(BUTTONS[0].0, BUTTONS[0].2),
                    ),
                    N::leaf(
                        [BUTTONS[1].1.width, BUTTONS[1].1.height],
                        Component::Action(BUTTONS[1].0, BUTTONS[1].2),
                    ),
                ],
            )
            .at(BUTTONS[0].1.x, BUTTONS[0].1.y),
            N::leaf([936, 7], Component::Error).at(24, 690),
        ],
    )
    .clipped();
fn reflow_display(
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
pub struct DisplayFrame<'a> {
    pub editors: &'a [LineEditor; 4],
    pub selected: usize,
    pub error: Option<&'a str>,
    pub pending: bool,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
/// Navigator-instance-owned main-thread bindings; Done/Back remain coordinator intents.
pub struct DisplayView {
    id: ScreenInstanceId,
    scope: Scope,
    editors: [RwSignal<LineEditor>; 4],
    selected: RwSignal<usize>,
    input_font: RwSignal<Option<FontText>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
    layout: MountedLayout<Component>,
    anchors: Vec<(NodeId, [i64; 2])>,
}
impl DisplayView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let nodes = RetainedNodes::new(width, height)?;
        let mut layout = MountedLayout::mount(SCREEN)?;
        let anchors: Vec<_> = layout
            .children(NodeId(0))
            .unwrap()
            .iter()
            .copied()
            .filter(|&id| id != NodeId(1))
            .map(|id| {
                let bounds = layout.geometry(id).unwrap().bounds;
                (id, [bounds.x, bounds.y])
            })
            .collect();
        reflow_display(&mut layout, &anchors, [width, height])?;
        let editors = [
            LineEditor::new("auto", 32)?,
            LineEditor::new("fifo", 32)?,
            LineEditor::new("120", 32)?,
            LineEditor::new("2000", 32)?,
        ];
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            editors: editors.map(|editor| scope.create_rw_signal(editor)),
            selected: scope.create_rw_signal(0),
            input_font: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
            layout,
            anchors,
        };
        let header: Vec<_> = view
            .layout
            .leaves()
            .iter()
            .copied()
            .filter(|leaf| {
                matches!(
                    leaf.component,
                    Component::Background(_) | Component::Header(..)
                )
            })
            .collect();
        let header_ids: Vec<_> = header.iter().map(|leaf| leaf.id).collect();
        view.nodes.static_layout_node(
            &view.layout,
            &header_ids,
            move |id, geometry, scene, _| {
                let leaf = header.iter().find(|leaf| leaf.id == id).unwrap();
                let bounds = geometry.bounds;
                match leaf.component {
                    Component::Background(color) => rect(
                        scene,
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        color,
                    ),
                    Component::Header(value, style) => paint_text(scene, bounds, value, style),
                    _ => unreachable!(),
                }
            },
        )?;
        for index in 0..4 {
            let label = view
                .layout
                .leaves()
                .iter()
                .find_map(|leaf| match leaf.component {
                    Component::FieldLabel(field, value) if field == index => Some((leaf.id, value)),
                    _ => None,
                })
                .ok_or("Display field label missing")?;
            let (editor_id, control) = view
                .layout
                .leaves()
                .iter()
                .find_map(|leaf| match leaf.component {
                    Component::Editor(field, id) if field == index => Some((leaf.id, id)),
                    _ => None,
                })
                .ok_or("Display editor missing")?;
            let editor = view.editors[index];
            let selected = view.selected;
            let pending = view.pending;
            let input_font = view.input_font;
            let focus = scope.create_memo(move |_| selected.get() == index);
            let memo = scope
                .create_memo(move |_| (editor.get(), focus.get(), pending.get(), input_font.get()));
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &[label.0, editor_id],
                move |(editor, focused, pending, font), id, geometry, scene, hits| {
                    if id == label.0 {
                        paint_text(scene, geometry.bounds, label.1, LABEL);
                    } else {
                        text_field_with_font(
                            scene,
                            &editor,
                            geometry.bounds,
                            focused && !pending,
                            font.as_ref(),
                        );
                        if !pending {
                            hits.push((control, geometry.bounds));
                        }
                    }
                },
            )?;
        }
        let hints: Vec<_> = view
            .layout
            .leaves()
            .iter()
            .copied()
            .filter(|leaf| matches!(leaf.component, Component::Hint(_)))
            .collect();
        let hint_ids: Vec<_> = hints.iter().map(|leaf| leaf.id).collect();
        view.nodes
            .static_layout_node(&view.layout, &hint_ids, move |id, geometry, scene, _| {
                if let Component::Hint(value) =
                    hints.iter().find(|leaf| leaf.id == id).unwrap().component
                {
                    paint_text(scene, geometry.bounds, value, HELP);
                }
            })?;
        for leaf in view.layout.leaves() {
            let Component::Action(control, label) = leaf.component else {
                continue;
            };
            let hovered = view.hovered;
            let armed = view.armed;
            let pending = view.pending;
            let memo = scope.create_memo(move |_| {
                if pending.get() {
                    (false, false, true)
                } else {
                    (
                        hovered.get() == Some(control),
                        armed.get() == Some(control),
                        false,
                    )
                }
            });
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &[leaf.id],
                move |(hovered, armed, pending), _, geometry, scene, hits| {
                    button(scene, geometry.bounds, label, hovered, armed);
                    if !pending {
                        hits.push((control, geometry.bounds));
                    }
                },
            )?;
        }
        let error_id = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Error))
            .ok_or("Display error region missing")?
            .id;
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[error_id],
            move |error, _, geometry, scene, _| {
                if let Some(error) = error {
                    paint_text(scene, geometry.bounds, &error, ERROR);
                }
            },
        )?;
        view.nodes.validate()?;
        Ok(view)
    }
    /// Direct extent updates retain the mounted tree and draft/editor lifetime.
    /// Each section keeps its declared allocation and follows its proportional
    /// anchor; overflow is clipped identically for painting and pointer admission.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if self.layout.extent() == [width, height] {
            return Ok(false);
        }
        let mut candidate = self.layout.clone();
        if !reflow_display(&mut candidate, &self.anchors, [width, height])? {
            return Ok(false);
        }
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
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
    /// Invalid selection rejects before any signal write. Borrowed equality
    /// avoids cloning unchanged draft values during pointer or status updates.
    pub fn update(&self, frame: DisplayFrame<'_>) -> Result<(), String> {
        if frame.selected >= 4 {
            return Err("Display requires a selected field in 0..4".into());
        }
        for (signal, editor) in self.editors.iter().zip(frame.editors) {
            if !signal.with_untracked(|old| old == editor) {
                signal.set(editor.clone());
            }
        }
        if self.selected.get_untracked() != frame.selected {
            self.selected.set(frame.selected);
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
impl Drop for DisplayView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn editors() -> [LineEditor; 4] {
        ["auto", "fifo", "120", "2000"].map(|value| LineEditor::new(value, 32).unwrap())
    }
    fn frame(editors: &[LineEditor; 4]) -> DisplayFrame<'_> {
        DisplayFrame {
            editors,
            selected: 0,
            error: None,
            pending: false,
            hovered: None,
            armed: None,
        }
    }
    // Independent coordinates from the renderer before this migration.
    fn legacy_scene(frame: &DisplayFrame<'_>) -> (Scene, Vec<(ControlId, Bounds)>) {
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        rect(&mut scene, 0, 0, 960, 720, 0x10151e);
        text(&mut scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
        text(
            &mut scene,
            24,
            65,
            "DISPLAY - ENTER DONE - ESC BACK",
            2,
            0x9bb1cf,
        );
        for (index, label) in ["GPU BACKEND", "PRESENT MODE", "UI FPS", "LOOKAHEAD MS"]
            .into_iter()
            .enumerate()
        {
            let y = 130 + index as i64 * 75;
            text(&mut scene, 24, (y + 10) as usize, label, 1, 0xf0f4ff);
            let bounds = Bounds {
                x: 280,
                y,
                width: 650,
                height: 34,
            };
            text_field_with_font(
                &mut scene,
                &frame.editors[index],
                bounds,
                frame.selected == index && !frame.pending,
                None,
            );
            if !frame.pending {
                hits.push((ControlId(40000 + index as u64), bounds));
            }
        }
        for (y, value) in [
            (445, "BACKEND: AUTO / VULKAN / DX12 / METAL / GL"),
            (460, "PRESENT: FIFO / IMMEDIATE / MAILBOX"),
            (475, "UI FPS: 30..240   LOOKAHEAD: 100..10000 MS"),
            (500, "SAVE PROFILE + RESTART FOR GPU BACKEND"),
            (515, "DONE UPDATES DRAFT - APPLY IS SEPARATE"),
        ] {
            text(&mut scene, 24, y, value, 1, 0x9bb1cf);
        }
        for (id, x, label) in [(ControlId(40), 24, "DONE"), (ControlId(41), 212, "BACK")] {
            let bounds = Bounds {
                x,
                y: 620,
                width: 170,
                height: 34,
            };
            button(
                &mut scene,
                bounds,
                label,
                !frame.pending && frame.hovered == Some(id),
                !frame.pending && frame.armed == Some(id),
            );
            if !frame.pending {
                hits.push((id, bounds));
            }
        }
        if let Some(error) = frame.error {
            text(&mut scene, 24, 690, error, 1, 0xff8e8e);
        }
        (scene, hits)
    }
    #[test]
    fn declarative_screen_matches_all_legacy_geometry_and_hit_bounds() {
        let view = DisplayView::new(ScreenInstanceId(8), 960, 720).unwrap();
        let mut editors = editors();
        editors[2].left();
        for (selected, pending, hovered, armed, error) in [
            (0, false, None, None, None),
            (2, false, Some(ControlId(40)), None, None),
            (
                3,
                false,
                Some(ControlId(41)),
                Some(ControlId(41)),
                Some("INVALID VALUE"),
            ),
            (
                1,
                true,
                Some(ControlId(40)),
                Some(ControlId(40)),
                Some("SAVING"),
            ),
            (0, false, None, None, None),
        ] {
            let update = DisplayFrame {
                editors: &editors,
                selected,
                pending,
                hovered,
                armed,
                error,
            };
            let (expected, expected_hits) = legacy_scene(&update);
            view.update(update).unwrap();
            let mut scene = Scene::new(960, 720);
            let mut hits = Vec::new();
            view.compose(&mut scene, &mut hits).unwrap();
            let geometry = |scene: &Scene| {
                scene
                    .rectangles()
                    .iter()
                    .map(|r| (r.bounds, r.color, r.uv))
                    .collect::<Vec<_>>()
            };
            assert_eq!(geometry(&scene), geometry(&expected));
            let regions = |hits: &[(ControlId, Bounds)]| {
                hits.iter()
                    .map(|(id, b)| (*id, b.x, b.y, b.width, b.height))
                    .collect::<Vec<_>>()
            };
            assert_eq!(regions(&hits), regions(&expected_hits));
            if !pending {
                assert_eq!(
                    regions(&hits[4..]),
                    regions(&BUTTONS.map(|(id, b, _)| (id, b)))
                );
            }
        }
    }
    #[test]
    fn individual_editor_error_and_focus_repaint_only_dependent_nodes() {
        let view = DisplayView::new(ScreenInstanceId(3), 960, 720).unwrap();
        let mut editors = editors();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = view.nodes.paints();
        view.update(frame(&editors)).unwrap();
        assert_eq!(view.nodes.paints(), before);
        assert!(!view.dirty());
        editors[2].left();
        view.update(frame(&editors)).unwrap();
        let cursor = view.nodes.paints();
        assert_eq!(cursor[3], before[3] + 1);
        assert_eq!(&cursor[..3], &before[..3]);
        assert_eq!(&cursor[4..], &before[4..]);
        let mut update = frame(&editors);
        update.selected = 1;
        view.update(update).unwrap();
        let focused = view.nodes.paints();
        assert_eq!(focused[1], cursor[1] + 1);
        assert_eq!(focused[2], cursor[2] + 1);
        assert_eq!(&focused[3..], &cursor[3..]);
        let mut update = frame(&editors);
        update.selected = 1;
        update.error = Some("INVALID");
        view.update(update).unwrap();
        let error = view.nodes.paints();
        assert_eq!(&error[..8], &focused[..8]);
        assert_eq!(error[8], focused[8] + 1);
    }
    #[test]
    fn pending_hits_and_invalid_admission_are_atomic_and_buttons_are_selective() {
        let view = DisplayView::new(ScreenInstanceId(3), 960, 720).unwrap();
        let editors = editors();
        view.update(frame(&editors)).unwrap();
        let before = view.nodes.paints();
        let mut bad = frame(&editors);
        bad.selected = 4;
        bad.error = Some("MUST NOT CHANGE");
        bad.pending = true;
        assert!(view.update(bad).is_err());
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.error.get_untracked(), None);
        let mut hovered = frame(&editors);
        hovered.hovered = Some(ControlId(40));
        view.update(hovered).unwrap();
        let after = view.nodes.paints();
        assert_eq!(&after[..6], &before[..6]);
        assert_eq!(after[6], before[6] + 1);
        assert_eq!(&after[7..], &before[7..]);
        let mut pending = frame(&editors);
        pending.pending = true;
        view.update(pending).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
        view.update(frame(&editors)).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![40000, 40001, 40002, 40003, 40, 41]
        );
    }
    #[test]
    fn retained_order_restore_and_disposal_match_actual_display_geometry() {
        let view = DisplayView::new(ScreenInstanceId(3), 960, 720).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        for (index, (_, bounds)) in hits[..4].iter().enumerate() {
            assert_eq!(
                (bounds.x, bounds.y, bounds.width, bounds.height),
                (280, 130 + index as i64 * 75, 650, 34)
            );
        }
        let count = scene.rectangles().len();
        let before = view.nodes.paints();
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(view.nodes.paints(), before);
        assert!(!view.dirty());
        let weak = view.nodes.weak_dirty();
        assert!(weak.strong_count() > 1);
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(DisplayView::new(ScreenInstanceId(3), 800, 600).is_ok());
    }
}
