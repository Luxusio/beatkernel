//! Retained precise practice-start draft editing; no native transport or clocks.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field},
    text_input::LineEditor,
};
use crate::{
    practice::PracticeStart,
    scene::{GeometrySnapshot, Scene},
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PracticeFrame {
    pub editor: LineEditor,
    pub error: Option<String>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
struct Packet {
    geometry: Result<GeometrySnapshot, String>,
    hits: Vec<(ControlId, Bounds)>,
    #[cfg(test)]
    paints: usize,
}
type Node = Rc<RefCell<Packet>>;
/// Fixed main-thread node tree retained for its Navigator instance.
/// Done updates the parent draft; Apply and pinned F5 remain coordinator-owned.
pub struct PracticeView {
    id: ScreenInstanceId,
    scope: Scope,
    editor: RwSignal<LineEditor>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: Vec<Node>,
    dirty: Rc<Cell<bool>>,
}
impl PracticeView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        if (width, height) != (960, 720) {
            return Err("Practice requires the 960x720 logical viewport".into());
        }
        let editor = LineEditor::new("0:00", 64)?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            editor: scope.create_rw_signal(editor),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes: Vec::new(),
            dirty: Rc::new(Cell::new(true)),
        };
        view.nodes.push(Rc::new(RefCell::new(paint_packet(
            width,
            height,
            |scene, _| {
                rect(scene, 0, 0, 960, 720, 0x10151e);
                text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
                text(scene, 24, 65, "PRACTICE START", 2, 0xf0f4ff);
                text(
                    scene,
                    24,
                    105,
                    "SECONDS / M:SS / H:MM:SS  FRACTION UP TO 9 DIGITS",
                    2,
                    0x9bb1cf,
                );
                text(
                    scene,
                    24,
                    270,
                    "DONE UPDATES SETTINGS DRAFT; APPLY CHANGES THE NEXT SESSION",
                    1,
                    0x9bb1cf,
                );
                text(
                    scene,
                    24,
                    292,
                    "F5 RETRIES THE PINNED SESSION; BACK DISCARDS THESE EDITS",
                    1,
                    0x9bb1cf,
                );
                text(
                    scene,
                    24,
                    314,
                    "FULL SONG RESETS THIS EDITOR TO ZERO",
                    1,
                    0x9bb1cf,
                );
            },
        ))));
        let editor = view.editor;
        let memo = scope.create_memo(move |_| editor.get());
        view.reactive_node(memo, width, height, |editor, scene, hits| {
            let bounds = Bounds {
                x: 24,
                y: 150,
                width: 906,
                height: 40,
            };
            text_field(scene, &editor, bounds, true);
            hits.push((ControlId(70), bounds));
            let preview = PracticeStart::parse(editor.value())
                .map(|start| format!("EXACT START: {} NS", start.nanoseconds()))
                .unwrap_or_else(|_| "INVALID START".into());
            text(scene, 24, 230, &preview, 2, 0xd8b36b);
        });
        for (id, x, label) in [(71, 24, "DONE"), (72, 220, "BACK"), (73, 416, "FULL SONG")] {
            view.button_node(
                width,
                height,
                ControlId(id),
                Bounds {
                    x,
                    y: 380,
                    width: 180,
                    height: 34,
                },
                label,
            );
        }
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.reactive_node(memo, width, height, |error, scene, _| {
            if let Some(error) = error {
                text(scene, 24, 450, &error, 1, 0xffaaaa);
            }
        });
        for node in &view.nodes {
            node.borrow().geometry.as_ref().map_err(Clone::clone)?;
        }
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn update(&self, frame: PracticeFrame) {
        if self.editor.get_untracked() != frame.editor {
            self.editor.set(frame.editor);
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
        self.dirty.get()
    }
    /// Concatenates retained geometry in painter order, also for forced restore.
    /// A partial composition never clears dirty status.
    pub fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        self.dirty.set(true);
        scene.clear();
        hits.clear();
        for node in &self.nodes {
            let node = node.borrow();
            scene.append_geometry(node.geometry.as_ref().map_err(Clone::clone)?)?;
            hits.extend_from_slice(&node.hits);
        }
        self.dirty.set(false);
        Ok(())
    }
    fn reactive_node<T: Clone + 'static>(
        &mut self,
        memo: Memo<T>,
        width: u32,
        height: u32,
        paint: impl Fn(T, &mut Scene, &mut Vec<(ControlId, Bounds)>) + 'static,
    ) {
        let node = Rc::new(RefCell::new(paint_packet(width, height, |_, _| {})));
        self.nodes.push(Rc::clone(&node));
        let dirty = Rc::clone(&self.dirty);
        self.scope.create_effect(move |_| {
            let value = memo.get();
            let packet = paint_packet(width, height, |scene, hits| paint(value, scene, hits));
            #[cfg(test)]
            let packet = {
                let mut packet = packet;
                packet.paints = node.borrow().paints + 1;
                packet
            };
            *node.borrow_mut() = packet;
            dirty.set(true);
        });
    }
    fn button_node(
        &mut self,
        width: u32,
        height: u32,
        id: ControlId,
        bounds: Bounds,
        label: &'static str,
    ) {
        let hovered = self.hovered;
        let armed = self.armed;
        let memo = self
            .scope
            .create_memo(move |_| (hovered.get() == Some(id), armed.get() == Some(id)));
        self.reactive_node(memo, width, height, move |(hovered, armed), scene, hits| {
            button(scene, bounds, label, hovered, armed);
            hits.push((id, bounds));
        });
    }
}
impl Drop for PracticeView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
fn paint_packet(
    width: u32,
    height: u32,
    paint: impl FnOnce(&mut Scene, &mut Vec<(ControlId, Bounds)>),
) -> Packet {
    let mut scene = Scene::with_capacity(width, height, 64);
    let mut hits = Vec::new();
    paint(&mut scene, &mut hits);
    Packet {
        geometry: scene.geometry_snapshot(),
        hits,
        #[cfg(test)]
        paints: 1,
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn frame(value: &str) -> PracticeFrame {
        PracticeFrame {
            editor: LineEditor::new(value, 64).unwrap(),
            error: None,
            hovered: None,
            armed: None,
        }
    }
    fn paints(view: &PracticeView) -> Vec<usize> {
        view.nodes.iter().map(|node| node.borrow().paints).collect()
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
        assert_eq!(&error[..5], &after[..5]);
        assert_eq!(error[5], after[5] + 1);
        update.hovered = Some(ControlId(71));
        view.update(update.clone());
        let hovered = paints(&view);
        assert_eq!(hovered[2], error[2] + 1);
        assert_eq!(&hovered[..2], &error[..2]);
        assert_eq!(&hovered[3..], &error[3..]);
        view.compose(&mut scene, &mut hits).unwrap();
        view.update(update);
        assert!(!view.dirty());
        assert_eq!(paints(&view), hovered);
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
            vec![70, 71, 72, 73]
        );
        assert_eq!(
            (hits[0].1.x, hits[0].1.y, hits[0].1.width, hits[0].1.height),
            (24, 150, 906, 40)
        );
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        let count = scene.rectangles().len();
        let before = paints(&view);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(paints(&view), before);
        let weak = Rc::downgrade(&view.nodes[1]);
        assert!(weak.strong_count() >= 2);
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(PracticeView::new(ScreenInstanceId(9), 800, 600).is_err());
    }
}
