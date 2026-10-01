//! Retained Selection nodes with Floem dependency tracking on the UI thread.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{font_text::FontText, scene::Scene, screen_lifecycle::ScreenInstanceId};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::sync::Arc;

pub const VISIBLE_ROWS: usize = 15;
pub const ROW_HEIGHT: usize = 34;

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

/// One fixed node tree per retained screen instance; never moved to a worker.
/// Effects produce only geometry, with no native I/O, handles or gameplay clocks.
pub struct SelectionView {
    id: ScreenInstanceId,
    scope: Scope,
    catalog_count: usize,
    projection: RwSignal<Arc<[usize]>>,
    cursor: RwSignal<Option<usize>>,
    search: RwSignal<LineEditor>,
    search_focused: RwSignal<bool>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    error: RwSignal<Option<String>>,
    backend_pending: RwSignal<bool>,
    nodes: RetainedNodes,
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
        if (width, height) != (960, 720) {
            return Err("Selection requires the 960x720 logical viewport".into());
        }
        if items
            .len()
            .checked_add(100)
            .and_then(|count| u64::try_from(count).ok())
            .is_none()
        {
            return Err("Selection catalog control identity overflow".into());
        }
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
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            backend_pending: scope.create_rw_signal(false),
            nodes,
        };
        view.nodes
            .static_node(|scene, _| rect(scene, 0, 0, 960, 720, 0x10151e));
        view.nodes
            .static_node(|scene, _| text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff));
        view.nodes.static_node(|scene, _| {
            text(
                scene,
                24,
                65,
                "ARROWS/PAGE SELECT  ENTER PLAY  F2 SETTINGS  F3 SEARCH",
                1,
                0x9bb1cf,
            );
            text(
                scene,
                24,
                86,
                "HOME/END FIRST/LAST  WHEEL OVER CHARTS",
                1,
                0x9bb1cf,
            )
        });
        let projection = view.projection;
        let count = items.len();
        let diagnostic_count = diagnostics.len();
        let memo = scope.create_memo(move |_| projection.with(|indices| indices.len()));
        view.nodes.bind(scope, memo, move |matches, scene, _| {
            text(
                scene,
                24,
                106,
                &format!("{matches}/{count} CHARTS  {diagnostic_count} SCAN DIAGNOSTICS"),
                1,
                0xd8b36b,
            );
        });
        for slot in 0..VISIBLE_ROWS {
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
            view.nodes.bind(scope, memo, move |value, scene, hits| {
                if let Some((index, selected)) = value {
                    let y = 140 + slot * ROW_HEIGHT;
                    let bounds = Bounds {
                        x: 18,
                        y: y as i64 - 6,
                        width: 924,
                        height: 30,
                    };
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
                    let title_y = if item.artist.is_empty() { y } else { y - 5 };
                    if let Some(font) = &font {
                        if let Err(error) = font
                            .draw(scene, 28, title_y as i64, &item.title, 0xf0f4ff)
                            .and_then(|()| {
                                font.draw(scene, 28, (y + 9) as i64, &item.artist, 0x9bb1cf)
                            })
                        {
                            scene.reject(error);
                        }
                    } else {
                        text(scene, 28, title_y, &item.title, 2, 0xf0f4ff);
                        text(scene, 28, y + 14, &item.artist, 1, 0x9bb1cf);
                    }
                    hits.push((ControlId(100 + index as u64), bounds));
                }
            });
        }
        for (index, diagnostic) in diagnostics.iter().take(2).enumerate() {
            let diagnostic = diagnostic.clone();
            view.nodes.static_node(move |scene, _| {
                text(scene, 24, 654 + index * 22, &diagnostic, 1, 0xd8b36b)
            });
        }
        view.button_node(
            ControlId(1),
            Bounds {
                x: 550,
                y: 65,
                width: 180,
                height: 34,
            },
            "START",
        );
        view.button_node(
            ControlId(5),
            Bounds {
                x: 750,
                y: 20,
                width: 180,
                height: 30,
            },
            "SETTINGS",
        );
        view.button_node(
            ControlId(4),
            Bounds {
                x: 750,
                y: 65,
                width: 180,
                height: 34,
            },
            "EXIT",
        );
        let projection = view.projection;
        let memo = scope.create_memo(move |_| projection.with(|indices| indices.is_empty()));
        let empty_catalog = items.is_empty();
        view.nodes.bind(scope, memo, move |empty, scene, _| {
            if empty {
                text(
                    scene,
                    24,
                    150,
                    if empty_catalog {
                        "NO SUPPORTED CHARTS FOUND"
                    } else {
                        "NO MATCHING CHARTS"
                    },
                    2,
                    0xff8e8e,
                );
            }
        });
        let search = view.search;
        let focused = view.search_focused;
        let memo = scope.create_memo(move |_| (search.get(), focused.get()));
        view.nodes
            .bind(scope, memo, |(editor, focused), scene, hits| {
                let bounds = Bounds {
                    x: 440,
                    y: 102,
                    width: 490,
                    height: 34,
                };
                text_field(scene, &editor, bounds, focused);
                hits.push((ControlId(80), bounds));
            });
        let pending = view.backend_pending;
        let memo = scope.create_memo(move |_| pending.get());
        view.nodes.bind(scope, memo, |pending, scene, _| {
            if pending {
                text(
                    scene,
                    24,
                    700,
                    "GPU BACKEND PENDING - SAVE PROFILE AND RESTART",
                    1,
                    0xd8b36b,
                );
            }
        });
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind(scope, memo, |error, scene, _| {
            if let Some(error) = error {
                text(
                    scene,
                    24,
                    650,
                    "ERROR - ENTER RETURNS TO SELECTION",
                    2,
                    0xff8e8e,
                );
                text(scene, 24, 682, &error, 1, 0xffaaaa);
            }
        });
        // Check immediate effects before handing ownership to the coordinator.
        view.nodes.validate()?;
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
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
    fn button_node(&mut self, id: ControlId, bounds: Bounds, label: &'static str) {
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
        self.nodes.bind(
            self.scope,
            memo,
            move |(enabled, hovered, armed), scene, hits| {
                button(scene, bounds, label, hovered, armed);
                if enabled {
                    hits.push((id, bounds));
                }
            },
        );
    }
}
impl Drop for SelectionView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
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
            assert!(
                scene
                    .rectangles()
                    .iter()
                    .any(|r| r.bounds == bounds && r.uv == uv)
            );
        }
        assert_eq!(hits[0].0, ControlId(100));
        assert_eq!((hits[0].1.y, hits[0].1.height), (134, 30));
        let before = paints(&view);
        view.update(frame(0));
        assert_eq!(paints(&view), before);
        view.set_projection(Arc::from([1]), Some(0)).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(hits[0].0, ControlId(101));
        assert!(
            scene
                .rectangles()
                .iter()
                .any(|r| r.bounds == [28.0, 140.0, 10.0, 14.0])
        );
        assert!(
            !scene
                .rectangles()
                .iter()
                .any(|r| r.bounds == [28.0, 154.0, 5.0, 7.0])
        );
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
            assert!(
                glyphs
                    .iter()
                    .all(|r| r.bounds[1] >= 134.0 && r.bounds[1] + r.bounds[3] <= 164.0)
            );
            assert_eq!(hits[0].0, ControlId(100));
            if texture == new {
                assert!(scene.batches().iter().all(|b| b.texture != old));
            }
        }
        let mut incomplete =
            FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap();
        incomplete.prepare('A').unwrap();
        assert!(
            SelectionView::new_with_font(
                ScreenInstanceId(7),
                items,
                Arc::from([]),
                960,
                720,
                Some(FontText::new(Arc::new(incomplete), new).unwrap())
            )
            .is_err()
        );
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
            SelectionView::new(ScreenInstanceId(1), Arc::from([]), Arc::from([]), 800, 600)
                .is_err()
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
        assert!(
            view.projection
                .with_untracked(|old| Arc::ptr_eq(old, &indices))
        );
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
        assert!(
            view.set_search(&LineEditor::new(&"x".repeat(257), 4096).unwrap(), false)
                .is_err()
        );
        assert_eq!(paints(&view), before);
        assert_eq!(view.nodes.identities(), identities);
    }
}
