//! Retained Selection nodes with Floem dependency tracking on the UI thread.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::button,
    retained::RetainedNodes,
};
use crate::{scene::Scene, screen_lifecycle::ScreenInstanceId};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use std::sync::Arc;

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
    selected: RwSignal<usize>,
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
            selected: scope.create_rw_signal(0),
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
                "UP/DOWN SELECT  ENTER PLAY  F2 SETTINGS",
                2,
                0x9bb1cf,
            )
        });
        let count = format!(
            "{} CHARTS   {} SCAN DIAGNOSTICS",
            items.len(),
            diagnostics.len()
        );
        view.nodes
            .static_node(move |scene, _| text(scene, 24, 96, &count, 2, 0xd8b36b));
        for slot in 0..15 {
            let selected = view.selected;
            let rows = Arc::clone(&items);
            let memo = scope.create_memo(move |_| {
                let selected = selected.get();
                selected
                    .saturating_sub(8)
                    .checked_add(slot)
                    .filter(|&index| index < rows.len())
                    .map(|index| (index, index == selected))
            });
            let rows = Arc::clone(&items);
            view.nodes.bind(scope, memo, move |value, scene, hits| {
                if let Some((index, selected)) = value {
                    let y = 140 + slot * 34;
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
                    text(scene, 28, y, &rows[index].title, 2, 0xf0f4ff);
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
        if items.is_empty() {
            view.nodes.static_node(|scene, _| {
                text(scene, 24, 150, "NO SUPPORTED CHARTS FOUND", 2, 0xff8e8e)
            });
        } else {
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
        }
        view.button_node(
            ControlId(5),
            Bounds {
                x: 750,
                y: 102,
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
        if self.selected.get_untracked() != frame.selected {
            self.selected.set(frame.selected);
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
    pub fn dirty(&self) -> bool {
        self.nodes.dirty()
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
        let memo = self
            .scope
            .create_memo(move |_| (hovered.get() == Some(id), armed.get() == Some(id)));
        self.nodes
            .bind(self.scope, memo, move |(hovered, armed), scene, hits| {
                button(scene, bounds, label, hovered, armed);
                hits.push((id, bounds));
            });
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
            vec![1, 5, 4]
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
            vec![5, 4]
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
}
