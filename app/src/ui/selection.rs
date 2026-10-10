//! Retained Selection nodes with Floem dependency tracking on the UI thread.
use super::{
    atoms::{rect, text, text_clipped},
    interaction::{Bounds, ControlId},
    layout::{LayoutChange, LayoutUpdate, MountedLayout, Node, NodeId, TextStyle},
    molecules::{button, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText,
    scene::{ClipRect, Scene},
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::sync::Arc;

pub const VISIBLE_ROWS: usize = 15;
pub const ROW_HEIGHT: usize = 34;

const TITLE: TextStyle = TextStyle {
    scale: 3,
    color: 0xf0f4ff,
};
const HELP: TextStyle = TextStyle {
    scale: 1,
    color: 0x9bb1cf,
};
const STATUS: TextStyle = TextStyle {
    scale: 1,
    color: 0xd8b36b,
};
const ERROR_TITLE: TextStyle = TextStyle {
    scale: 2,
    color: 0xff8e8e,
};
const ERROR_DETAIL: TextStyle = TextStyle {
    scale: 1,
    color: 0xffaaaa,
};
#[derive(Clone, Copy)]
enum Component {
    Background(u32),
    Text(&'static str, TextStyle),
    Count(TextStyle),
    ChartRow(usize),
    Diagnostic(usize, TextStyle),
    Action(ControlId, &'static str),
    Empty(&'static str, &'static str, TextStyle),
    Search(ControlId),
    Pending(&'static str, TextStyle),
    ErrorTitle(&'static str, TextStyle),
    ErrorDetail(TextStyle),
}
type N = Node<'static, Component>;
const fn chart_rows() -> [N; VISIBLE_ROWS] {
    let mut rows = [N::leaf([924, 30], Component::ChartRow(0)); VISIBLE_ROWS];
    let mut slot = 0;
    while slot < VISIBLE_ROWS {
        rows[slot] = N::leaf([924, 30], Component::ChartRow(slot));
        slot += 1;
    }
    rows
}
// Sections declare allocation, hierarchy, labels and painter order together.
// Catalog data changes existing row properties; it never replaces this tree.
const SCREEN: N = N::layer(
    [960, 720],
    &[
        N::leaf([960, 720], Component::Background(0x10151e)).at(0, 0),
        N::leaf([936, 21], Component::Text("BEATKERNEL BMS PLAYER", TITLE)).at(24, 20),
        N::column(
            [936, 28],
            14,
            &[
                N::leaf(
                    [936, 7],
                    Component::Text(
                        "ARROWS/PAGE SELECT  ENTER PLAY  F2 SETTINGS  F3 SEARCH",
                        HELP,
                    ),
                ),
                N::leaf(
                    [936, 7],
                    Component::Text("HOME/END FIRST/LAST  WHEEL OVER CHARTS", HELP),
                ),
            ],
        )
        .at(24, 65),
        N::leaf([936, 7], Component::Count(STATUS)).at(24, 106),
        N::column([924, 506], 4, &chart_rows()).at(18, 134),
        N::column(
            [936, 29],
            15,
            &[
                N::leaf([936, 7], Component::Diagnostic(0, STATUS)),
                N::leaf([936, 7], Component::Diagnostic(1, STATUS)),
            ],
        )
        .at(24, 654),
        N::leaf([180, 34], Component::Action(ControlId(1), "START")).at(550, 65),
        N::leaf([180, 30], Component::Action(ControlId(5), "SETTINGS")).at(750, 20),
        N::leaf([180, 34], Component::Action(ControlId(4), "EXIT")).at(750, 65),
        N::leaf(
            [936, 14],
            Component::Empty(
                "NO SUPPORTED CHARTS FOUND",
                "NO MATCHING CHARTS",
                ERROR_TITLE,
            ),
        )
        .at(24, 150),
        N::leaf([490, 34], Component::Search(ControlId(80))).at(440, 102),
        N::leaf(
            [936, 7],
            Component::Pending("GPU BACKEND PENDING - SAVE PROFILE AND RESTART", STATUS),
        )
        .at(24, 700),
        N::column(
            [936, 39],
            18,
            &[
                N::leaf(
                    [936, 14],
                    Component::ErrorTitle("ERROR - ENTER RETURNS TO SELECTION", ERROR_TITLE),
                ),
                N::leaf([936, 7], Component::ErrorDetail(ERROR_DETAIL)),
            ],
        )
        .at(24, 650),
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
fn reflow_selection(
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

pub struct SelectionItem {
    pub title: String,
    pub artist: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionFrame {
    pub selected: usize,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
    pub error: Option<String>,
    pub backend_pending: bool,
}

/// Instance-local mounted hierarchy and retained geometry, shared by native and
/// browser menu owners. Effects own no I/O, handles or gameplay clocks.
pub struct SelectionView {
    id: ScreenInstanceId,
    scope: Scope,
    catalog_count: usize,
    projection: RwSignal<Arc<[usize]>>,
    cursor: RwSignal<Option<usize>>,
    search: RwSignal<LineEditor>,
    search_focused: RwSignal<bool>,
    input_font: RwSignal<Option<FontText>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    error: RwSignal<Option<String>>,
    backend_pending: RwSignal<bool>,
    nodes: RetainedNodes,
    layout: MountedLayout<Component>,
    anchors: Vec<(NodeId, [i64; 2])>,
    row_height: u32,
}
impl SelectionView {
    pub fn new(
        id: ScreenInstanceId,
        items: Arc<[SelectionItem]>,
        diagnostics: Arc<[String]>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        Self::new_with_font(id, items, diagnostics, width, height, None)
    }
    pub fn new_with_font(
        id: ScreenInstanceId,
        items: Arc<[SelectionItem]>,
        diagnostics: Arc<[String]>,
        width: u32,
        height: u32,
        font: Option<FontText>,
    ) -> Result<Self, String> {
        if items
            .len()
            .checked_add(100)
            .and_then(|count| u64::try_from(count).ok())
            .is_none()
        {
            return Err("Selection catalog control identity overflow".into());
        }
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
        reflow_selection(&mut layout, &anchors, [width, height])?;
        let nodes = RetainedNodes::new(width, height)?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            catalog_count: items.len(),
            projection: scope.create_rw_signal((0..items.len()).collect::<Vec<_>>().into()),
            cursor: scope.create_rw_signal((!items.is_empty()).then_some(0)),
            search: scope.create_rw_signal(LineEditor::new("", 256)?),
            search_focused: scope.create_rw_signal(false),
            input_font: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            backend_pending: scope.create_rw_signal(false),
            nodes,
            layout,
            anchors,
            row_height: ROW_HEIGHT as u32,
        };
        let leaves = view.layout.leaves().to_vec();
        let background_leaf = leaves
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Background(_)))
            .unwrap();
        let background = background_leaf.id;
        let Component::Background(background_color) = background_leaf.component else {
            unreachable!()
        };
        view.nodes.static_layout_node(
            &view.layout,
            &[background],
            move |_, geometry, scene, _| {
                let b = geometry.bounds;
                rect(scene, b.x, b.y, b.width, b.height, background_color);
            },
        )?;
        let title_leaf = leaves.iter().find(|leaf| matches!(leaf.component, Component::Text(_, style) if style.scale == TITLE.scale)).unwrap();
        let title = title_leaf.id;
        let Component::Text(title_label, title_style) = title_leaf.component else {
            unreachable!()
        };
        view.nodes
            .static_layout_node(&view.layout, &[title], move |_, geometry, scene, _| {
                paint_text(scene, geometry.bounds, title_label, title_style);
            })?;
        let help: Vec<_> = leaves.iter().copied().filter(|leaf| matches!(leaf.component, Component::Text(_, style) if style.scale == HELP.scale)).collect();
        let help_ids: Vec<_> = help.iter().map(|leaf| leaf.id).collect();
        view.nodes
            .static_layout_node(&view.layout, &help_ids, move |id, geometry, scene, _| {
                if let Component::Text(label, style) =
                    help.iter().find(|leaf| leaf.id == id).unwrap().component
                {
                    paint_text(scene, geometry.bounds, label, style);
                }
            })?;
        let count_leaf = leaves
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Count(_)))
            .unwrap();
        let count_id = count_leaf.id;
        let Component::Count(count_style) = count_leaf.component else {
            unreachable!()
        };
        let projection = view.projection;
        let count = items.len();
        let diagnostic_count = diagnostics.len();
        let memo = scope.create_memo(move |_| projection.with(|indices| indices.len()));
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[count_id],
            move |matches, _, geometry, scene, _| {
                paint_text(
                    scene,
                    geometry.bounds,
                    &format!("{matches}/{count} CHARTS  {diagnostic_count} SCAN DIAGNOSTICS"),
                    count_style,
                );
            },
        )?;
        for leaf in &leaves {
            let Component::ChartRow(slot) = leaf.component else {
                continue;
            };
            let cursor = view.cursor;
            let projection = view.projection;
            let memo = scope.create_memo(move |_| {
                let cursor = cursor.get();
                let position = cursor.unwrap_or(0).saturating_sub(8) + slot;
                projection
                    .with(|indices| indices.get(position).copied())
                    .map(|index| (index, cursor == Some(position)))
            });
            let rows = Arc::clone(&items);
            let font = font.clone();
            view.nodes.bind_layout(
                scope,
                memo,
                &view.layout,
                &[leaf.id],
                move |&value, _, geometry, scene, hits| {
                    if let Some((index, selected)) = value {
                        let bounds = geometry.bounds;
                        if selected {
                            rect(
                                scene,
                                bounds.x,
                                bounds.y,
                                bounds.width,
                                bounds.height,
                                0x263d59,
                            );
                        }
                        let item = &rows[index];
                        let text_x = bounds.x + 10;
                        let title_y = bounds.y + if item.artist.is_empty() { 6 } else { 1 };
                        let title_clip = ClipRect::new([
                            text_x,
                            bounds.y,
                            bounds.width - 20,
                            if item.artist.is_empty() {
                                bounds.height
                            } else {
                                15
                            },
                        ])
                        .expect("mounted selection title bounds");
                        let artist_clip =
                            ClipRect::new([text_x, bounds.y + 15, bounds.width - 20, 15])
                                .expect("mounted selection artist bounds");
                        let result = if let Some(font) = &font {
                            font.draw_clipped(
                                scene,
                                text_x,
                                title_y,
                                &item.title,
                                0xf0f4ff,
                                title_clip,
                            )
                            .and_then(|()| {
                                font.draw_clipped(
                                    scene,
                                    text_x,
                                    bounds.y + 15,
                                    &item.artist,
                                    0x9bb1cf,
                                    artist_clip,
                                )
                            })
                        } else {
                            text_clipped(
                                scene,
                                text_x as usize,
                                title_y as usize,
                                &item.title,
                                2,
                                0xf0f4ff,
                                title_clip,
                            )
                            .and_then(|()| {
                                text_clipped(
                                    scene,
                                    text_x as usize,
                                    (bounds.y + 20) as usize,
                                    &item.artist,
                                    1,
                                    0x9bb1cf,
                                    artist_clip,
                                )
                            })
                        };
                        if let Err(error) = result {
                            scene.reject(error);
                        }
                        hits.push((ControlId(100 + index as u64), bounds));
                    }
                },
            )?;
        }
        for leaf in &leaves {
            let Component::Diagnostic(index, style) = leaf.component else {
                continue;
            };
            let Some(diagnostic) = diagnostics.get(index) else {
                continue;
            };
            let diagnostic = diagnostic.clone();
            view.nodes.static_layout_node(
                &view.layout,
                &[leaf.id],
                move |_, geometry, scene, _| {
                    paint_text(scene, geometry.bounds, &diagnostic, style);
                },
            )?;
        }
        for leaf in &leaves {
            let Component::Action(control, label) = leaf.component else {
                continue;
            };
            view.button_node(leaf.id, control, label)?;
        }
        let empty_leaf = leaves
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Empty(..)))
            .unwrap();
        let empty_id = empty_leaf.id;
        let Component::Empty(empty_label, filtered_label, empty_style) = empty_leaf.component
        else {
            unreachable!()
        };
        let projection = view.projection;
        let memo = scope.create_memo(move |_| projection.with(|indices| indices.is_empty()));
        let empty_catalog = items.is_empty();
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[empty_id],
            move |&empty, _, geometry, scene, _| {
                if empty {
                    paint_text(
                        scene,
                        geometry.bounds,
                        if empty_catalog {
                            empty_label
                        } else {
                            filtered_label
                        },
                        empty_style,
                    );
                }
            },
        )?;
        let search_leaf = leaves
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Search(_)))
            .unwrap();
        let search_id = search_leaf.id;
        let Component::Search(search_control) = search_leaf.component else {
            unreachable!()
        };
        let search = view.search;
        let focused = view.search_focused;
        let input_font = view.input_font;
        let memo = scope.create_memo(move |_| (search.get(), focused.get(), input_font.get()));
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[search_id],
            move |(editor, focused, font), _, geometry, scene, hits| {
                text_field_with_font(scene, editor, geometry.bounds, *focused, font.as_ref());
                hits.push((search_control, geometry.bounds));
            },
        )?;
        let pending_leaf = leaves
            .iter()
            .find(|leaf| matches!(leaf.component, Component::Pending(..)))
            .unwrap();
        let pending_id = pending_leaf.id;
        let Component::Pending(pending_label, pending_style) = pending_leaf.component else {
            unreachable!()
        };
        let pending = view.backend_pending;
        let memo = scope.create_memo(move |_| pending.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &[pending_id],
            move |&pending, _, geometry, scene, _| {
                if pending {
                    paint_text(scene, geometry.bounds, pending_label, pending_style);
                }
            },
        )?;
        let error_leaves: Vec<_> = leaves
            .iter()
            .copied()
            .filter(|leaf| {
                matches!(
                    leaf.component,
                    Component::ErrorTitle(..) | Component::ErrorDetail(_)
                )
            })
            .collect();
        let error_ids: Vec<_> = error_leaves.iter().map(|leaf| leaf.id).collect();
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind_layout(
            scope,
            memo,
            &view.layout,
            &error_ids,
            move |error, id, geometry, scene, _| {
                if let Some(error) = error {
                    match error_leaves
                        .iter()
                        .find(|leaf| leaf.id == id)
                        .unwrap()
                        .component
                    {
                        Component::ErrorTitle(label, style) => {
                            paint_text(scene, geometry.bounds, label, style)
                        }
                        Component::ErrorDetail(style) => {
                            paint_text(scene, geometry.bounds, error, style)
                        }
                        _ => unreachable!(),
                    }
                }
            },
        )?;
        // Check immediate effects before handing ownership to the coordinator.
        view.nodes.validate()?;
        Ok(view)
    }
    /// Explicit extent changes move section anchors while retaining catalog,
    /// search, focus, identities and the declared child allocations.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if self.layout.extent() == [width, height] {
            return Ok(false);
        }
        let mut candidate = self.layout.clone();
        reflow_selection(&mut candidate, &self.anchors, [width, height])?;
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
        Ok(true)
    }
    /// Change chart-row allocation directly. The column reflows all following
    /// siblings; the four-pixel separator remains outside each hit region.
    pub fn set_row_height(&mut self, height: u32) -> Result<bool, String> {
        if height <= 4 {
            return Err("Selection row height must exceed its four-pixel separator".into());
        }
        if self.row_height == height {
            return Ok(false);
        }
        let row_ids: Vec<_> = self
            .layout
            .leaves()
            .iter()
            .filter_map(|leaf| matches!(leaf.component, Component::ChartRow(_)).then_some(leaf.id))
            .collect();
        let column = self
            .layout
            .children(NodeId(0))
            .unwrap()
            .iter()
            .copied()
            .find(|&id| {
                self.layout
                    .children(id)
                    .is_some_and(|children| children == row_ids.as_slice())
            })
            .ok_or("Selection chart column missing")?;
        let mut updates = vec![LayoutUpdate {
            id: column,
            change: LayoutChange::Size([924, i64::from(height) * VISIBLE_ROWS as i64 - 4]),
        }];
        updates.extend(row_ids.into_iter().map(|id| LayoutUpdate {
            id,
            change: LayoutChange::Size([924, i64::from(height) - 4]),
        }));
        let mut candidate = self.layout.clone();
        candidate.update(&updates)?;
        self.nodes.relayout(&candidate)?;
        self.layout = candidate;
        self.row_height = height;
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
    /// Equality suppresses unchanged writes. Independent field signals prevent
    /// status changes from subscribing or repainting catalog rows.
    pub fn update(&self, frame: SelectionFrame) {
        let cursor = self
            .projection
            .with_untracked(|indices| indices.binary_search(&frame.selected).ok());
        if (cursor.is_some() || self.projection.with_untracked(|indices| indices.is_empty()))
            && self.cursor.get_untracked() != cursor
        {
            self.cursor.set(cursor);
        }
        if self.hovered.get_untracked() != frame.hovered {
            self.hovered.set(frame.hovered);
        }
        if self.armed.get_untracked() != frame.armed {
            self.armed.set(frame.armed);
        }
        if self.error.get_untracked() != frame.error {
            self.error.set(frame.error);
        }
        if self.backend_pending.get_untracked() != frame.backend_pending {
            self.backend_pending.set(frame.backend_pending);
        }
    }
    /// Sorted original indices are validated before changing any signal. A
    /// shared projection and unchanged cursor skip both scanning and writes.
    pub fn set_projection(
        &self,
        indices: Arc<[usize]>,
        cursor: Option<usize>,
    ) -> Result<(), String> {
        let same = self
            .projection
            .with_untracked(|old| Arc::ptr_eq(old, &indices));
        if same && self.cursor.get_untracked() == cursor {
            return Ok(());
        }
        if cursor.is_some_and(|cursor| cursor >= indices.len())
            || (cursor.is_none() != indices.is_empty())
        {
            return Err("Selection projection cursor is outside its results".into());
        }
        if !same
            && (indices.iter().any(|&index| index >= self.catalog_count)
                || indices.windows(2).any(|pair| pair[0] >= pair[1]))
        {
            return Err("Selection projection must contain sorted unique catalog indices".into());
        }
        if !same {
            self.projection.set(indices);
        }
        if self.cursor.get_untracked() != cursor {
            self.cursor.set(cursor);
        }
        Ok(())
    }
    /// Borrows and compares editor contents/cursor before cloning a changed value.
    pub fn set_search(&self, editor: &LineEditor, focused: bool) -> Result<(), String> {
        if editor.value().len() > 256 {
            return Err("Selection search exceeds 256 bytes".into());
        }
        if !self.search.with_untracked(|old| old == editor) {
            self.search.set(editor.clone());
        }
        if self.search_focused.get_untracked() != focused {
            self.search_focused.set(focused);
        }
        Ok(())
    }
    pub fn dirty(&self) -> bool {
        self.nodes.dirty()
    }
    pub fn contains_chart(&self, point: (f64, f64)) -> bool {
        self.nodes.hit(point).is_some_and(|id| id.0 >= 100)
    }
    /// Reuses retained packets in painter order, including forced scene restore.
    /// Failure leaves the view dirty so the coordinator cannot cache partial output.
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
        let projection = self.projection;
        let memo = self.scope.create_memo(move |_| {
            let enabled = id != ControlId(1) || projection.with(|indices| !indices.is_empty());
            (
                enabled,
                enabled && hovered.get() == Some(id),
                enabled && armed.get() == Some(id),
            )
        });
        self.nodes.bind_layout(
            self.scope,
            memo,
            &self.layout,
            &[node],
            move |&(enabled, hovered, armed), _, geometry, scene, hits| {
                button(scene, geometry.bounds, label, hovered, armed);
                if enabled {
                    hits.push((id, geometry.bounds));
                }
            },
        )
    }
}
impl Drop for SelectionView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
#[cfg(test)]
#[path = "selection_declarative_fixtures.rs"]
mod selection_declarative_fixtures;

#[cfg(test)]
mod fixtures {
    use super::*;
    fn frame(selected: usize) -> SelectionFrame {
        SelectionFrame {
            selected,
            hovered: None,
            armed: None,
            error: None,
            backend_pending: false,
        }
    }
    fn view(count: usize) -> SelectionView {
        SelectionView::new(
            ScreenInstanceId(7),
            (0..count)
                .map(|index| SelectionItem {
                    title: format!("CHART {index}"),
                    artist: "ARTIST".into(),
                })
                .collect::<Vec<_>>()
                .into(),
            Arc::from([]),
            960,
            720,
        )
        .unwrap()
    }
    fn paints(view: &SelectionView) -> Vec<usize> {
        view.nodes.paints()
    }
    #[test]
    fn input_font_generation_repaints_search_without_rebuilding_catalog_packets() {
        use crate::{font_atlas::FontAtlas, texture::TextureId};
        let atlas = Arc::new(
            FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap(),
        );
        let atlas = FontAtlas::extend_texts(&atlas, &["A"]).unwrap();
        let texture = TextureId::allocate().unwrap();
        let old = FontText::new(Arc::clone(&atlas), texture).unwrap();
        let items: Arc<[SelectionItem]> = vec![SelectionItem {
            title: "A".into(),
            artist: String::new(),
        }]
        .into();
        let view = SelectionView::new_with_font(
            ScreenInstanceId(7),
            items,
            Arc::from([]),
            960,
            720,
            Some(old.clone()),
        )
        .unwrap();
        view.set_search(&LineEditor::new("A", 256).unwrap(), true)
            .unwrap();
        view.set_input_font(Some(old));
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = paints(&view);
        let next = FontAtlas::extend_texts(&atlas, &["가"]).unwrap();
        let font = FontText::new(next, texture).unwrap();
        view.set_input_font(Some(font.clone()));
        let after = paints(&view);
        assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
        assert_eq!(&before[4..19], &after[4..19]);
        view.compose(&mut scene, &mut hits).unwrap();
        view.set_input_font(Some(font));
        assert_eq!(paints(&view), after);
        assert!(!view.dirty());
        view.set_search(&LineEditor::new("가", 256).unwrap(), true)
            .unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            scene
                .batches()
                .iter()
                .filter(|b| b.texture == texture)
                .map(|b| b.count)
                .sum::<u32>(),
            2
        );
    }
    #[test]
    fn committed_selection_only_repaints_search_and_identical_selection_stays_idle() {
        let view = view(30);
        let mut editor = LineEditor::new("音é", 256).unwrap();
        view.set_search(&editor, true).unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = paints(&view);
        editor.select_all(); // Same text and caret; only the anchor changes.
        view.set_search(&editor, true).unwrap();
        let after = paints(&view);
        assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
        assert_eq!(&before[4..19], &after[4..19]);
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [448.0, 110.0, 24.0, 16.0]));
        view.set_search(&editor, true).unwrap();
        assert_eq!(paints(&view), after);
        assert!(!view.dirty());
        view.set_search(&editor, false).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(!scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [448.0, 110.0, 24.0, 16.0]));
        assert_eq!(editor.selection(), Some((0, 5)));
    }
    #[test]
    fn ime_selection_end_only_repaints_search_and_keeps_catalog_nodes_retained() {
        let view = view(30);
        let base = LineEditor::new("", 256).unwrap();
        let first = base.preedit("音é", Some((0, 3))).unwrap();
        let second = base.preedit("音é", Some((0, 5))).unwrap();
        assert_eq!(
            (first.value(), first.cursor()),
            (second.value(), second.cursor())
        );
        view.set_search(&first, true).unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let before = paints(&view);
        view.set_search(&first, true).unwrap();
        assert_eq!(paints(&view), before);
        assert!(!view.dirty());
        view.set_search(&second, true).unwrap();
        let after = paints(&view);
        assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
        assert_eq!(&before[4..19], &after[4..19]);
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [448.0, 110.0, 24.0, 16.0]));
        assert!(scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [448.0, 126.0, 24.0, 2.0]));
        view.set_search(&second, true).unwrap();
        assert_eq!(paints(&view), after);
        assert!(!view.dirty());
        view.set_search(&base, true).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(!scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [448.0, 126.0, 24.0, 2.0]));
    }
    #[test]
    fn artist_lines_keep_bitmap_row_bounds_and_empty_artist_placement() {
        let view = SelectionView::new(
            ScreenInstanceId(7),
            vec![
                SelectionItem {
                    title: "A".into(),
                    artist: "B".into(),
                },
                SelectionItem {
                    title: "A".into(),
                    artist: String::new(),
                },
            ]
            .into(),
            Arc::from([]),
            960,
            720,
        )
        .unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        for (bounds, uv) in [
            ([28.0, 135.0, 10.0, 14.0], crate::font::glyph_uv('A')),
            ([28.0, 154.0, 5.0, 7.0], crate::font::glyph_uv('B')),
            ([28.0, 174.0, 10.0, 14.0], crate::font::glyph_uv('A')),
        ] {
            assert!(scene
                .rectangles()
                .iter()
                .any(|r| r.bounds == bounds && r.uv == uv));
        }
        assert_eq!(hits[0].0, ControlId(100));
        assert_eq!((hits[0].1.y, hits[0].1.height), (134, 30));
        let before = paints(&view);
        view.update(frame(0));
        assert_eq!(paints(&view), before);
        view.set_projection(Arc::from([1]), Some(0)).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(hits[0].0, ControlId(101));
        assert!(scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [28.0, 140.0, 10.0, 14.0]));
        assert!(!scene
            .rectangles()
            .iter()
            .any(|r| r.bounds == [28.0, 154.0, 5.0, 7.0]));
    }
    #[test]
    fn actual_selection_clips_long_prepared_text_and_overhang_to_each_line_band() {
        use crate::{font_atlas::FontAtlas, texture::TextureId};
        let mut atlas =
            FontAtlas::new(crate::font_fixture::font_bytes(), 64.0, 128, 128, 16).unwrap();
        atlas.prepare('A').unwrap();
        atlas.prepare('가').unwrap();
        let texture = TextureId::allocate().unwrap();
        let font = FontText::new(Arc::new(atlas), texture).unwrap();
        let mut uncropped = Scene::new(960, 720);
        font.draw(&mut uncropped, 28, 135, "A", 0xf0f4ff).unwrap();
        assert!(uncropped.rectangles()[0].bounds[1] + uncropped.rectangles()[0].bounds[3] > 149.0);
        let view = SelectionView::new_with_font(
            ScreenInstanceId(7),
            vec![
                SelectionItem {
                    title: "A".repeat(256),
                    artist: "가".repeat(256),
                },
                SelectionItem {
                    title: "A".repeat(256),
                    artist: String::new(),
                },
            ]
            .into(),
            Arc::from([]),
            960,
            720,
            Some(font),
        )
        .unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let glyphs = scene
            .batches()
            .iter()
            .filter(|b| b.texture == texture)
            .flat_map(|b| &scene.rectangles()[b.first as usize..(b.first + b.count) as usize])
            .collect::<Vec<_>>();
        assert!(!glyphs.is_empty());
        let mut bands = [false; 3];
        for glyph in glyphs {
            let [x, y, width, height] = glyph.bounds;
            assert!(x >= 28.0 && x + width <= 932.0);
            let (index, bottom) = if y < 149.0 {
                (0, 149.0)
            } else if y < 164.0 {
                (1, 164.0)
            } else {
                assert!(y >= 168.0);
                (2, 198.0)
            };
            assert!(y >= 134.0 && y + height <= bottom);
            bands[index] = true;
        }
        assert_eq!(bands, [true; 3]);
        assert_eq!(
            (hits[0].0, hits[0].1.y, hits[0].1.height),
            (ControlId(100), 134, 30)
        );
        let before = paints(&view);
        view.update(frame(0));
        assert_eq!(paints(&view), before);
        view.set_projection(Arc::from([1]), Some(0)).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(hits[0].0, ControlId(101));
        for glyph in scene
            .batches()
            .iter()
            .filter(|b| b.texture == texture)
            .flat_map(|b| &scene.rectangles()[b.first as usize..(b.first + b.count) as usize])
        {
            assert!(glyph.bounds[1] >= 134.0 && glyph.bounds[1] + glyph.bounds[3] <= 164.0);
        }
    }
    #[test]
    fn prepared_artist_lines_follow_projection_and_renderer_texture_identity() {
        use crate::{font_atlas::FontAtlas, font_text::FontText, texture::TextureId};
        let mut atlas =
            FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap();
        atlas.prepare('A').unwrap();
        atlas.prepare('가').unwrap();
        let atlas = Arc::new(atlas);
        let items: Arc<[SelectionItem]> = vec![
            SelectionItem {
                title: "A".into(),
                artist: "가".into(),
            },
            SelectionItem {
                title: "가".into(),
                artist: String::new(),
            },
        ]
        .into();
        let old = TextureId::allocate().unwrap();
        let new = TextureId::allocate().unwrap();
        for texture in [old, new] {
            let view = SelectionView::new_with_font(
                ScreenInstanceId(7),
                Arc::clone(&items),
                Arc::from([]),
                960,
                720,
                Some(FontText::new(Arc::clone(&atlas), texture).unwrap()),
            )
            .unwrap();
            let mut scene = Scene::new(960, 720);
            let mut hits = Vec::new();
            view.compose(&mut scene, &mut hits).unwrap();
            assert_eq!(
                scene
                    .batches()
                    .iter()
                    .filter(|b| b.texture == texture)
                    .map(|b| b.count)
                    .sum::<u32>(),
                3
            );
            let before = paints(&view);
            view.update(frame(0));
            assert_eq!(paints(&view), before);
            view.set_projection(Arc::from([0]), Some(0)).unwrap();
            view.compose(&mut scene, &mut hits).unwrap();
            let glyphs = scene
                .batches()
                .iter()
                .filter(|b| b.texture == texture)
                .flat_map(|b| &scene.rectangles()[b.first as usize..(b.first + b.count) as usize])
                .collect::<Vec<_>>();
            assert_eq!(glyphs.len(), 2);
            assert_eq!(glyphs[1].bounds[1] - glyphs[0].bounds[1], 14.0);
            assert!(glyphs
                .iter()
                .all(|r| r.bounds[1] >= 134.0 && r.bounds[1] + r.bounds[3] <= 164.0));
            assert_eq!(hits[0].0, ControlId(100));
            if texture == new {
                assert!(scene.batches().iter().all(|b| b.texture != old));
            }
        }
        let mut incomplete =
            FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap();
        incomplete.prepare('A').unwrap();
        assert!(SelectionView::new_with_font(
            ScreenInstanceId(7),
            items,
            Arc::from([]),
            960,
            720,
            Some(FontText::new(Arc::new(incomplete), new).unwrap())
        )
        .is_err());
    }
    #[test]
    fn retained_chart_admission_matches_painter_order_before_recomposition() {
        let view = view(30);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        for point in [
            (28.0, 140.0),
            (500.0, 134.0),
            (28.0, 166.0),
            (28.0, 700.0),
            (960.0, 140.0),
            (f64::NAN, 140.0),
        ] {
            let hit = hits
                .iter()
                .rev()
                .find(|(_, bounds)| bounds.contains(point))
                .map(|(id, _)| *id);
            assert_eq!(
                view.contains_chart(point),
                hit.is_some_and(|id| id.0 >= 100)
            );
        }
        assert!(!view.contains_chart((500.0, 134.0))); // Search paints above the first row.
        hits.clear();
        assert!(view.contains_chart((28.0, 140.0)));
        view.set_projection(vec![2, 8].into(), Some(0)).unwrap();
        assert!(view.contains_chart((28.0, 174.0)));
        assert!(!view.contains_chart((28.0, 208.0)));
        view.set_projection(Arc::from([]), None).unwrap();
        assert!(!view.contains_chart((28.0, 140.0)));
    }
    #[test]
    fn unchanged_state_and_unrelated_status_do_not_repaint_rows_or_buttons() {
        let view = view(30);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(!view.dirty());
        let before = paints(&view);
        view.update(frame(0));
        assert_eq!(paints(&view), before);
        assert!(!view.dirty());
        let mut update = frame(0);
        update.error = Some("ERROR".into());
        view.update(update.clone());
        let after = paints(&view);
        assert_eq!(&after[..after.len() - 1], &before[..before.len() - 1]);
        assert_eq!(after.last().unwrap(), &(before.last().unwrap() + 1));
        view.compose(&mut scene, &mut hits).unwrap();
        view.update(update);
        assert_eq!(paints(&view), after);
        assert!(!view.dirty());
    }
    #[test]
    fn same_visible_page_repaints_only_old_and_new_highlight_rows_and_relevant_button() {
        let view = view(30);
        let before = paints(&view);
        view.update(frame(1));
        let after = paints(&view);
        let changed = after
            .iter()
            .zip(&before)
            .enumerate()
            .filter_map(|(index, (after, before))| (after != before).then_some(index))
            .collect::<Vec<_>>();
        assert_eq!(changed, vec![4, 5]);
        let mut update = frame(1);
        update.hovered = Some(ControlId(1));
        view.update(update.clone());
        let hovered = paints(&view);
        assert_eq!(&hovered[..19], &after[..19]);
        assert_eq!(hovered[19], after[19] + 1);
        assert_eq!(&hovered[20..], &after[20..]);
        update.hovered = Some(ControlId(101)); // Rows have no new hover behavior.
        view.update(update);
        assert_eq!(&paints(&view)[4..19], &after[4..19]);
    }
    #[test]
    fn page_hits_use_actual_catalog_indices_and_compose_restores_stable_geometry() {
        let view = view(30);
        view.update(frame(20));
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[..15].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            (112..127).collect::<Vec<_>>()
        );
        assert_eq!(hits[0].1.y, 134);
        assert_eq!(hits[14].1.y, 610);
        assert_eq!(
            hits[15..].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![1, 5, 4, 80]
        );
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        let count = scene.rectangles().len();
        let before = paints(&view);
        scene.clear();
        scene.rect(0, 0, 1, 1, 0);
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(paints(&view), before);
        assert!(!view.dirty());
        let empty = SelectionView::new(ScreenInstanceId(8), Arc::from([]), Arc::from([]), 960, 720)
            .unwrap();
        empty.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![5, 4, 80]
        );
    }
    #[test]
    fn drop_disposes_subscription_captures_and_bad_viewport_rejects() {
        let view = view(3);
        let weak = view.nodes.weak_dirty();
        assert!(weak.strong_count() >= 2); // View and retained effect closures share the dirty token.
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(
            SelectionView::new(ScreenInstanceId(1), Arc::from([]), Arc::from([]), 800, 600).is_ok()
        );
    }
    #[test]
    fn filtered_rows_keep_original_identity_and_empty_results_disable_start() {
        let view = view(30);
        let indices: Arc<[usize]> = vec![2, 8, 20, 25].into();
        view.set_projection(indices.clone(), Some(2)).unwrap();
        let before = paints(&view);
        view.set_projection(indices, Some(2)).unwrap();
        assert_eq!(paints(&view), before);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[..4].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![102, 108, 120, 125]
        );
        assert_eq!(
            hits[4..].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![1, 5, 4, 80]
        );
        let settings = hits.iter().find(|(id, _)| id.0 == 5).unwrap().1;
        let search = hits.iter().find(|(id, _)| id.0 == 80).unwrap().1;
        assert_eq!(
            (settings.x, settings.y, settings.width, settings.height),
            (750, 20, 180, 30)
        );
        assert_eq!(
            (search.x, search.y, search.width, search.height),
            (440, 102, 490, 34)
        );
        view.set_projection(Arc::from([]), None).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![5, 4, 80]
        );
        assert_eq!(view.cursor.get_untracked(), None);
    }
    #[test]
    fn projection_preflight_and_search_updates_preserve_unrelated_nodes() {
        let view = view(30);
        let indices: Arc<[usize]> = vec![2, 8, 20].into();
        view.set_projection(indices.clone(), Some(0)).unwrap();
        let before = paints(&view);
        let identities = view.nodes.identities();
        for (indices, cursor) in [
            (vec![8, 2], Some(0)),
            (vec![2, 2], Some(0)),
            (vec![30], Some(0)),
            (vec![2, 8], Some(2)),
            (vec![2, 8], None),
            (vec![], Some(0)),
        ] {
            assert!(view.set_projection(indices.into(), cursor).is_err());
            assert_eq!(paints(&view), before);
        }
        assert!(view
            .projection
            .with_untracked(|old| Arc::ptr_eq(old, &indices)));
        let mut editor = LineEditor::new("blue", 256).unwrap();
        view.set_search(&editor, true).unwrap();
        let changed = paints(&view);
        let changed_nodes = changed
            .iter()
            .zip(&before)
            .enumerate()
            .filter_map(|(index, (after, before))| (after != before).then_some(index))
            .collect::<Vec<_>>();
        assert_eq!(changed_nodes, vec![23]);
        view.set_search(&editor, true).unwrap();
        assert_eq!(paints(&view), changed);
        editor.left();
        view.set_search(&editor, true).unwrap();
        assert_eq!(&paints(&view)[4..19], &before[4..19]);
        let before = paints(&view);
        assert!(view
            .set_search(&LineEditor::new(&"x".repeat(257), 4096).unwrap(), false)
            .is_err());
        assert_eq!(paints(&view), before);
        assert_eq!(view.nodes.identities(), identities);
    }
}
