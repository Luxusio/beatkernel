//! Retained Display draft fields with independent editor and focus dependencies.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field},
    retained::RetainedNodes,
    text_input::LineEditor,
};
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
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
}
impl DisplayView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let nodes = RetainedNodes::new(width, height)?;
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
            error: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
        };
        view.nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x10151e);
            text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
            text(
                scene,
                24,
                65,
                "DISPLAY - ENTER DONE - ESC BACK",
                2,
                0x9bb1cf,
            );
        });
        for (index, label) in ["GPU BACKEND", "PRESENT MODE", "UI FPS", "LOOKAHEAD MS"]
            .iter()
            .copied()
            .enumerate()
        {
            let editor = view.editors[index];
            let selected = view.selected;
            let pending = view.pending;
            let focus = scope.create_memo(move |_| selected.get() == index);
            let memo = scope.create_memo(move |_| (editor.get(), focus.get(), pending.get()));
            view.nodes.bind(
                scope,
                memo,
                move |(editor, focused, pending), scene, hits| {
                    let y = 130 + index as i64 * 75;
                    text(scene, 24, (y + 10) as usize, label, 1, 0xf0f4ff);
                    let bounds = Bounds {
                        x: 280,
                        y,
                        width: 650,
                        height: 34,
                    };
                    text_field(scene, &editor, bounds, focused && !pending);
                    if !pending {
                        hits.push((ControlId(40000 + index as u64), bounds));
                    }
                },
            );
        }
        view.nodes.static_node(|scene, _| {
            for (y, hint) in [
                (445, "BACKEND: AUTO / VULKAN / DX12 / METAL / GL"),
                (460, "PRESENT: FIFO / IMMEDIATE / MAILBOX"),
                (475, "UI FPS: 30..240   LOOKAHEAD: 100..10000 MS"),
                (500, "SAVE PROFILE + RESTART FOR GPU BACKEND"),
                (515, "DONE UPDATES DRAFT - APPLY IS SEPARATE"),
            ] {
                text(scene, 24, y, hint, 1, 0x9bb1cf);
            }
        });
        for (id, bounds, label) in BUTTONS {
            let hovered = view.hovered;
            let armed = view.armed;
            let pending = view.pending;
            let memo = scope.create_memo(move |_| {
                if pending.get() {
                    (false, false, true)
                } else {
                    (hovered.get() == Some(id), armed.get() == Some(id), false)
                }
            });
            view.nodes.bind(
                scope,
                memo,
                move |(hovered, armed, pending), scene, hits| {
                    button(scene, bounds, label, hovered, armed);
                    if !pending {
                        hits.push((id, bounds));
                    }
                },
            );
        }
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind(scope, memo, |error, scene, _| {
            if let Some(error) = error {
                text(scene, 24, 690, &error, 1, 0xff8e8e);
            }
        });
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
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
        assert!(DisplayView::new(ScreenInstanceId(3), 800, 600).is_err());
    }
}
