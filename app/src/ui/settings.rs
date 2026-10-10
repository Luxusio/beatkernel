//! Retained fine-grained Settings geometry; drafts and metadata ownership are external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    layout::{LayoutChange, LayoutUpdate, MountedLayout, Node, NodeId, TextStyle},
    molecules::{button, text_field_value_with_font, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText,
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
    settings::{SettingsField, MAX_FIELDS},
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::rc::Rc;

/// The shared authoritative button layout for painting and hover projection.
pub const BUTTONS: [(ControlId, Bounds, &'static str); 10] = [
    (
        ControlId(74),
        Bounds {
            x: 265,
            y: 60,
            width: 125,
            height: 34,
        },
        "PRACTICE",
    ),
    (
        ControlId(19),
        Bounds {
            x: 401,
            y: 60,
            width: 125,
            height: 34,
        },
        "RECORDS",
    ),
    (
        ControlId(18),
        Bounds {
            x: 537,
            y: 60,
            width: 125,
            height: 34,
        },
        "DISPLAY",
    ),
    (
        ControlId(17),
        Bounds {
            x: 673,
            y: 60,
            width: 125,
            height: 34,
        },
        "PLAYERS",
    ),
    (
        ControlId(16),
        Bounds {
            x: 809,
            y: 60,
            width: 125,
            height: 34,
        },
        "AUDIO",
    ),
    (
        ControlId(10),
        Bounds {
            x: 24,
            y: 620,
            width: 170,
            height: 34,
        },
        "APPLY",
    ),
    (
        ControlId(11),
        Bounds {
            x: 212,
            y: 620,
            width: 170,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(12),
        Bounds {
            x: 400,
            y: 620,
            width: 170,
            height: 34,
        },
        "ADD BINDING",
    ),
    (
        ControlId(13),
        Bounds {
            x: 588,
            y: 620,
            width: 170,
            height: 34,
        },
        "LOAD",
    ),
    (
        ControlId(14),
        Bounds {
            x: 776,
            y: 620,
            width: 170,
            height: 34,
        },
        "SAVE",
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
const MESSAGE: TextStyle = TextStyle {
    scale: 1,
    color: 0x74e5c5,
};
const ERROR: TextStyle = TextStyle {
    scale: 1,
    color: 0xff8e8e,
};

#[derive(Clone, Copy)]
enum Component {
    Background(u32),
    Header(&'static str, TextStyle),
    Action(ControlId, &'static str),
    Paging,
    FieldLabel(usize),
    Editor(usize),
    Hint,
    ProfileLabel,
    ProfileEditor,
    Status,
    Error,
}
type N = Node<'static, Component>;
const fn action(index: usize) -> N {
    N::leaf(
        [BUTTONS[index].1.width, BUTTONS[index].1.height],
        Component::Action(BUTTONS[index].0, BUTTONS[index].2),
    )
}
macro_rules! field_row {
    ($slot:expr) => {
        N::row(
            [906, 32],
            24,
            &[
                N::layer(
                    [232, 32],
                    &[N::leaf([232, 7], Component::FieldLabel($slot)).at(0, 10)],
                ),
                N::leaf([650, 32], Component::Editor($slot)),
            ],
        )
    };
}
// Mounted once. The native/browser owners continue to supply actual drafts and
// navigation capabilities; this declaration owns only view structure and style.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([960, 720], Component::Background(0x10151e)).at(0, 0),
        N::column(
            [936, 52],
            24,
            &[
                N::leaf([936, 21], Component::Header("BEATKERNEL BMS PLAYER", TITLE)),
                N::leaf(
                    [936, 7],
                    Component::Header("F4 RECORDS / F6 PRACTICE", HELP),
                ),
            ],
        )
        .at(24, 20),
        N::row(
            [669, 34],
            11,
            &[action(0), action(1), action(2), action(3), action(4)],
        )
        .at(265, 60),
        N::leaf([906, 7], Component::Paging).at(24, 102),
        N::column(
            [906, 383],
            7,
            &[
                field_row!(0),
                field_row!(1),
                field_row!(2),
                field_row!(3),
                field_row!(4),
                field_row!(5),
                field_row!(6),
                field_row!(7),
                field_row!(8),
                field_row!(9),
            ],
        )
        .at(24, 120),
        N::leaf([906, 7], Component::Hint).at(24, 525),
        N::row(
            [906, 34],
            24,
            &[
                N::layer(
                    [112, 34],
                    &[N::leaf([112, 7], Component::ProfileLabel).at(0, 12)],
                ),
                N::leaf([770, 34], Component::ProfileEditor),
            ],
        )
        .at(24, 558),
        N::row(
            [922, 34],
            18,
            &[action(5), action(6), action(7), action(8), action(9)],
        )
        .at(24, 620),
        N::leaf([912, 7], Component::Status).at(24, 665),
        N::leaf([912, 7], Component::Error).at(24, 690),
    ],
)
.clipped();

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
fn reflow_settings(
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

pub struct SettingsFrame<'a> {
    pub fields: &'a [SettingsField],
    pub selected: usize,
    pub editor: &'a LineEditor,
    pub profile: &'a LineEditor,
    pub profile_focused: bool,
    pub message: Option<&'a str>,
    pub error: Option<&'a str>,
    pub pending: bool,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    field: SettingsField,
    editor: Option<LineEditor>,
    focused: bool,
    pending: bool,
}

/// Stable bindings per Settings instance, retained while its children are open.
/// Rc geometry storage keeps this scope on the UI thread; no I/O occurs in effects.
pub struct SettingsView {
    id: ScreenInstanceId,
    scope: Scope,
    fields: Rc<[RwSignal<Option<SettingsField>>]>,
    count: RwSignal<usize>,
    selected: RwSignal<usize>,
    editor: RwSignal<LineEditor>,
    profile: RwSignal<LineEditor>,
    profile_focused: RwSignal<bool>,
    input_font: RwSignal<Option<FontText>>,
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
    layout: MountedLayout<Component>,
    anchors: Vec<(NodeId, [i64; 2])>,
}
impl SettingsView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let empty = LineEditor::new("", 4096)?;
        let nodes = RetainedNodes::new(width, height)?;
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
        reflow_settings(&mut layout, &anchors, [width, height])?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            fields: (0..MAX_FIELDS)
                .map(|_| scope.create_rw_signal(None))
                .collect::<Vec<_>>()
                .into(),
            count: scope.create_rw_signal(0),
            selected: scope.create_rw_signal(0),
            editor: scope.create_rw_signal(empty.clone()),
            profile: scope.create_rw_signal(empty),
            profile_focused: scope.create_rw_signal(false),
            input_font: scope.create_rw_signal(None),
            message: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
            layout,
            anchors,
        };
        let header = view
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
            .collect::<Vec<_>>();
        let header_ids = header.iter().map(|leaf| leaf.id).collect::<Vec<_>>();
        view.nodes.static_layout_node(
            &view.layout,
            &header_ids,
            move |id, geometry, scene, _| match header
                .iter()
                .find(|leaf| leaf.id == id)
                .unwrap()
                .component
            {
                Component::Background(color) => {
                    let b = geometry.bounds;
                    rect(scene, b.x, b.y, b.width, b.height, color);
                }
                Component::Header(value, style) => paint_text(scene, geometry.bounds, value, style),
                _ => unreachable!(),
            },
        )?;
        for (id, _, _) in BUTTONS.iter().take(5).copied() {
            view.button_node(id)?;
        }
        let paging = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Paging))
            .unwrap()
            .id;
        let count = view.count;
        let selected = view.selected;
        let memo = scope.create_memo(move |_| (selected.get() / 10 * 10, count.get()));
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[paging],
            |(first, count), _, geometry, scene, _| {
                if count > 0 {
                    paint_text(
                        scene,
                        geometry.bounds,
                        &format!(
                            "FIELDS {}-{} OF {}   UP/DOWN OR TAB SELECT",
                            first + 1,
                            (first + 10).min(count),
                            count
                        ),
                        NOTICE,
                    );
                }
            },
        )?;
        for slot in 0..10 {
            let label_id = view
                .layout
                .leaves()
                .iter()
                .find(
                    |leaf| matches!(leaf.component, Component::FieldLabel(index) if index == slot),
                )
                .unwrap()
                .id;
            let editor_id = view
                .layout
                .leaves()
                .iter()
                .find(|leaf| matches!(leaf.component, Component::Editor(index) if index == slot))
                .unwrap()
                .id;
            let fields = Rc::clone(&view.fields);
            let selected = view.selected;
            let editor = view.editor;
            let focused = view.profile_focused;
            let pending = view.pending;
            let input_font = view.input_font;
            let memo = scope.create_memo(move |_| {
                let selected = selected.get();
                let index = selected / 10 * 10 + slot;
                (
                    fields
                        .get(index)
                        .and_then(|signal| signal.get())
                        .map(|field| {
                            let (editor, focused) = if index == selected {
                                (Some(editor.get()), !focused.get())
                            } else {
                                (None, false)
                            };
                            Row {
                                index,
                                field,
                                editor,
                                focused,
                                pending: pending.get(),
                            }
                        }),
                    input_font.get(),
                )
            });
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &[label_id, editor_id],
                move |(row, font), id, geometry, scene, hits| {
                    if let Some(row) = row {
                        if id == label_id {
                            paint_text(scene, geometry.bounds, row.field.label, LABEL);
                            return;
                        }
                        let bounds = geometry.bounds;
                        if let Some(editor) = row.editor {
                            text_field_with_font(
                                scene,
                                &editor,
                                bounds,
                                row.focused,
                                font.as_ref(),
                            );
                        } else {
                            text_field_value_with_font(
                                scene,
                                &row.field.value,
                                bounds,
                                font.as_ref(),
                            );
                        }
                        if !row.pending {
                            hits.push((ControlId(1000 + row.index as u64), bounds));
                        }
                    }
                },
            )?;
        }
        let selected = view.selected;
        let fields = Rc::clone(&view.fields);
        let memo = scope.create_memo(move |_| {
            fields
                .get(selected.get())
                .and_then(|signal| signal.with(|field| field.as_ref().map(|field| field.hint)))
        });
        let hint_id = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Hint))
            .unwrap()
            .id;
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[hint_id],
            |hint, _, geometry, scene, _| {
                if let Some(hint) = hint {
                    paint_text(scene, geometry.bounds, hint, HELP);
                }
            },
        )?;
        let profile = view.profile;
        let focused = view.profile_focused;
        let pending = view.pending;
        let input_font = view.input_font;
        let memo = scope.create_memo(move |_| {
            (
                profile.get(),
                focused.get(),
                pending.get(),
                input_font.get(),
            )
        });
        let profile_label = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::ProfileLabel))
            .unwrap()
            .id;
        let profile_editor = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::ProfileEditor))
            .unwrap()
            .id;
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[profile_label, profile_editor],
            move |(profile, focused, pending, font), id, geometry, scene, hits| {
                if id == profile_label {
                    paint_text(scene, geometry.bounds, "PROFILE PATH", LABEL);
                    return;
                }
                let bounds = geometry.bounds;
                text_field_with_font(scene, &profile, bounds, focused, font.as_ref());
                if !pending {
                    hits.push((ControlId(15), bounds));
                }
            },
        )?;
        for (id, _, _) in BUTTONS.iter().skip(5).copied() {
            view.button_node(id)?;
        }
        let pending = view.pending;
        let message = view.message;
        let memo = scope.create_memo(move |_| {
            if pending.get() {
                (true, None)
            } else {
                (false, message.get())
            }
        });
        let status_id = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Status))
            .unwrap()
            .id;
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[status_id],
            |(pending, message), _, geometry, scene, _| {
                if pending {
                    paint_text(scene, geometry.bounds, "LOADING DEVICES", NOTICE);
                } else if let Some(message) = message {
                    paint_text(scene, geometry.bounds, &message, MESSAGE);
                }
            },
        )?;
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        let error_id = view
            .layout
            .leaves()
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Error))
            .unwrap()
            .id;
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[error_id],
            |error, _, geometry, scene, _| {
                if let Some(error) = error {
                    paint_text(scene, geometry.bounds, &error, ERROR);
                }
            },
        )?;
        view.nodes.validate()?;
        Ok(view)
    }
    /// Direct extent updates retain mounted identities, drafts and editor focus.
    /// Section allocations follow their declared anchors; overflow shares one
    /// clip for paint and pointer admission, including zero-extent suspension.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if self.layout.extent() == [width, height] {
            return Ok(false);
        }
        let mut candidate = self.layout.clone();
        if !reflow_settings(&mut candidate, &self.anchors, [width, height])? {
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
    /// Preflight shape validation precedes every signal write. Borrowed equality
    /// avoids cloning entire field vectors or unchanged editor/status values.
    pub fn update(&self, frame: SettingsFrame<'_>) -> Result<(), String> {
        if frame.fields.is_empty()
            || frame.fields.len() > MAX_FIELDS
            || frame.selected >= frame.fields.len()
        {
            return Err("Settings requires 1..128 fields and an in-range selection".into());
        }
        for (index, signal) in self.fields.iter().enumerate() {
            let value = frame.fields.get(index);
            if !signal.with_untracked(|old| old.as_ref() == value) {
                signal.set(value.cloned());
            }
        }
        if self.count.get_untracked() != frame.fields.len() {
            self.count.set(frame.fields.len());
        }
        if self.selected.get_untracked() != frame.selected {
            self.selected.set(frame.selected);
        }
        if !self.editor.with_untracked(|old| old == frame.editor) {
            self.editor.set(frame.editor.clone());
        }
        if !self.profile.with_untracked(|old| old == frame.profile) {
            self.profile.set(frame.profile.clone());
        }
        if self.profile_focused.get_untracked() != frame.profile_focused {
            self.profile_focused.set(frame.profile_focused);
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
    fn button_node(&mut self, id: ControlId) -> Result<(), String> {
        let (node_id, label) = self
            .layout
            .leaves()
            .iter()
            .find_map(|leaf| match leaf.component {
                Component::Action(control, label) if control == id => Some((leaf.id, label)),
                _ => None,
            })
            .unwrap();
        let hovered = self.hovered;
        let armed = self.armed;
        let pending = self.pending;
        let memo = self.scope.create_memo(move |_| {
            if pending.get() {
                (false, false, true)
            } else {
                (hovered.get() == Some(id), armed.get() == Some(id), false)
            }
        });
        self.nodes.bind_layout(
            self.scope,
            memo,
            &self.layout,
            &[node_id],
            move |(hovered, armed, pending), _, geometry, scene, hits| {
                let bounds = geometry.bounds;
                button(scene, bounds, label, hovered, armed);
                if !pending {
                    hits.push((id, bounds));
                }
            },
        )
    }
}
impl Drop for SettingsView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
#[cfg(test)]
#[path = "settings_declarative_fixtures.rs"]
mod declarative_fixtures;
#[cfg(test)]
mod fixtures {
    use super::*;
    fn fields(count: usize) -> Vec<SettingsField> {
        (0..count)
            .map(|index| SettingsField {
                flag: "--bind",
                label: "KEY BINDING",
                hint: "BINDING HINT",
                value: format!("{index}:04"),
            })
            .collect()
    }
    fn frame<'a>(
        fields: &'a [SettingsField],
        editor: &'a LineEditor,
        profile: &'a LineEditor,
    ) -> SettingsFrame<'a> {
        SettingsFrame {
            fields,
            selected: 0,
            editor,
            profile,
            profile_focused: false,
            message: None,
            error: None,
            pending: false,
            hovered: None,
            armed: None,
        }
    }
    fn paints(view: &SettingsView) -> Vec<usize> {
        view.nodes.paints()
    }
    #[test]
    fn prepared_font_generation_updates_value_nodes_and_profile_without_other_controls() {
        use crate::{font_atlas::FontAtlas, texture::TextureId};
        use std::sync::Arc;
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let fields = fields(30);
        let editor = LineEditor::new("가", 4096).unwrap();
        let profile = LineEditor::new("A", 4096).unwrap();
        let values: Vec<_> = fields[..10]
            .iter()
            .map(|field| field.value.as_str())
            .chain([editor.value(), profile.value()])
            .collect();
        let base = Arc::new(
            FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 64).unwrap(),
        );
        let atlas = FontAtlas::extend_texts(&base, &values).unwrap();
        let texture = TextureId::allocate().unwrap();
        let font = FontText::new(Arc::clone(&atlas), texture).unwrap();
        view.update(frame(&fields, &editor, &profile)).unwrap();
        view.set_input_font(Some(font));
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = paints(&view);
        let atlas = FontAtlas::extend_texts(&atlas, &["別"]).unwrap();
        let font = FontText::new(atlas, texture).unwrap();
        view.set_input_font(Some(font.clone()));
        let after = paints(&view);
        assert_eq!(
            before.iter().zip(&after).filter(|(a, b)| a != b).count(),
            11
        );
        assert_eq!(&before[..7], &after[..7]);
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(scene.batches().iter().any(|batch| batch.texture == texture));
        view.set_input_font(Some(font));
        assert_eq!(paints(&view), after);
        assert!(!view.dirty());
        view.set_input_font(None);
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(!scene.batches().iter().any(|batch| batch.texture == texture));
    }
    #[test]
    fn committed_selection_changes_only_the_focused_setting_or_profile_node() {
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let fields = fields(30);
        let base = LineEditor::new("別é", 4096).unwrap();
        let mut selected = base.clone();
        selected.select_all();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        for profile_focused in [false, true] {
            let update = |editor| {
                let mut frame = if profile_focused {
                    frame(&fields, &base, editor)
                } else {
                    frame(&fields, editor, &base)
                };
                frame.profile_focused = profile_focused;
                frame
            };
            view.update(update(&base)).unwrap();
            view.compose(&mut scene, &mut hits).unwrap();
            let before = paints(&view);
            view.update(update(&selected)).unwrap();
            let after = paints(&view);
            assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
            view.compose(&mut scene, &mut hits).unwrap();
            let expected = if profile_focused {
                [168.0, 566.0, 24.0, 16.0]
            } else {
                [288.0, 128.0, 24.0, 16.0]
            };
            assert!(scene.rectangles().iter().any(|r| r.bounds == expected));
            view.update(update(&selected)).unwrap();
            assert_eq!(paints(&view), after);
            assert!(!view.dirty());
        }
    }
    #[test]
    fn composition_range_only_updates_selected_editor_or_profile_without_sibling_repaint() {
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let fields = fields(30);
        let base = LineEditor::new("", 4096).unwrap();
        let first = base.preedit("별é", Some((0, 3))).unwrap();
        let second = base.preedit("별é", Some((0, 5))).unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        for profile_focused in [false, true] {
            let preview = |editor| {
                let mut update = if profile_focused {
                    frame(&fields, &base, editor)
                } else {
                    frame(&fields, editor, &base)
                };
                update.profile_focused = profile_focused;
                update
            };
            view.update(preview(&first)).unwrap();
            view.compose(&mut scene, &mut hits).unwrap();
            let before = paints(&view);
            view.update(preview(&second)).unwrap();
            let after = paints(&view);
            assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
            view.compose(&mut scene, &mut hits).unwrap();
            let expected = if profile_focused {
                [168.0, 582.0, 24.0, 2.0]
            } else {
                [288.0, 144.0, 24.0, 2.0]
            };
            assert!(scene.rectangles().iter().any(|r| r.bounds == expected));
            view.update(preview(&second)).unwrap();
            assert_eq!(paints(&view), after);
            assert!(!view.dirty());
        }
        assert_eq!(fields[0].value, "0:04");
    }
    #[test]
    fn editor_cursor_status_and_profile_focus_invalidate_only_their_dependencies() {
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let fields = fields(30);
        let mut editor = LineEditor::new("0:04", 4096).unwrap();
        let profile = LineEditor::new("profile.bkp", 4096).unwrap();
        view.update(frame(&fields, &editor, &profile)).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = paints(&view);
        view.update(frame(&fields, &editor, &profile)).unwrap();
        assert_eq!(paints(&view), before);
        assert!(!view.dirty());
        editor.left();
        view.update(frame(&fields, &editor, &profile)).unwrap();
        let cursor = paints(&view);
        assert_eq!(cursor[7], before[7] + 1);
        assert_eq!(&cursor[..7], &before[..7]);
        assert_eq!(&cursor[8..], &before[8..]);
        let mut update = frame(&fields, &editor, &profile);
        update.message = Some("SAVED");
        update.error = Some("ERROR");
        view.update(update).unwrap();
        let status = paints(&view);
        assert_eq!(&status[..24], &cursor[..24]);
        assert_eq!(status[24], cursor[24] + 1);
        assert_eq!(status[25], cursor[25] + 1);
        let mut update = frame(&fields, &editor, &profile);
        update.profile_focused = true;
        view.update(update).unwrap();
        let focused = paints(&view);
        assert_eq!(focused[7], status[7] + 1);
        assert_eq!(&focused[8..17], &status[8..17]);
        assert_eq!(focused[18], status[18] + 1);
    }
    #[test]
    fn pages_reordered_and_added_fields_update_existing_nodes_and_actual_hits() {
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let mut fields = fields(25);
        let editor = LineEditor::new("12:04", 4096).unwrap();
        let profile = LineEditor::new("", 4096).unwrap();
        let mut update = frame(&fields, &editor, &profile);
        update.selected = 12;
        view.update(update).unwrap();
        let identities = view.nodes.identities();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[5..15].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            (1010..1020).collect::<Vec<_>>()
        );
        assert_eq!((hits[5].1.x, hits[5].1.y), (280, 120));
        fields.swap(10, 11);
        fields.push(SettingsField {
            flag: "--bind",
            label: "NEW ROW",
            hint: "NEW HINT",
            value: "26:04".into(),
        });
        let mut update = frame(&fields, &editor, &profile);
        update.selected = 25;
        view.update(update).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[5..11].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            (1020..1026).collect::<Vec<_>>()
        );
        assert_eq!(view.nodes.identities(), identities);
        let short = fields[..2].to_vec();
        view.update(frame(&short, &editor, &profile)).unwrap();
        assert!(view.fields[25].with_untracked(|field| field.is_none()));
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[5..7].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![1000, 1001]
        );
    }
    #[test]
    fn pending_disables_every_hit_and_invalid_frames_leave_all_signals_unchanged() {
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let fields = fields(12);
        let editor = LineEditor::new("0:04", 4096).unwrap();
        let profile = LineEditor::new("p", 4096).unwrap();
        view.update(frame(&fields, &editor, &profile)).unwrap();
        let before = paints(&view);
        let mut bad = frame(&fields, &editor, &profile);
        bad.selected = 12;
        bad.error = Some("MUST NOT MUTATE");
        assert!(view.update(bad).is_err());
        assert!(view.update(frame(&[], &editor, &profile)).is_err());
        let too_many = self::fields(MAX_FIELDS + 1);
        assert!(view.update(frame(&too_many, &editor, &profile)).is_err());
        assert_eq!(paints(&view), before);
        assert_eq!(view.count.get_untracked(), 12);
        assert_eq!(view.error.get_untracked(), None);
        let mut pending = frame(&fields, &editor, &profile);
        pending.pending = true;
        pending.hovered = Some(ControlId(74));
        pending.armed = Some(ControlId(74));
        view.update(pending).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
        view.update(frame(&fields, &editor, &profile)).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(hits.len(), 21);
        assert_eq!(
            hits[..5].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![74, 19, 18, 17, 16]
        );
        assert_eq!(hits[15].0, ControlId(15));
        assert_eq!(
            hits[16..].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![10, 11, 12, 13, 14]
        );
    }
    #[test]
    fn compose_restores_geometry_without_effects_and_drop_releases_subscriptions() {
        let view = SettingsView::new(ScreenInstanceId(9), 960, 720).unwrap();
        let fields = fields(2);
        let editor = LineEditor::new("0:04", 4096).unwrap();
        let profile = LineEditor::new("p", 4096).unwrap();
        view.update(frame(&fields, &editor, &profile)).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let count = scene.rectangles().len();
        let before = paints(&view);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(paints(&view), before);
        assert!(!view.dirty());
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        let weak = view.nodes.weak_dirty();
        assert!(weak.strong_count() >= 2);
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(SettingsView::new(ScreenInstanceId(9), 800, 600).is_ok());
    }
}
