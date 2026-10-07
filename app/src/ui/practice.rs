//! Retained precise practice-section draft editing; no native transport or clocks.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field_with_font},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    font_text::FontText, practice::PracticeStart, scene::Scene, screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PracticeFrame {
    pub editor: LineEditor,
    pub end_editor: LineEditor,
    pub end_focused: bool,
    pub error: Option<String>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}

/// Fixed main-thread node tree retained for its Navigator instance.
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
}
impl PracticeView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        if (width, height) != (960, 720) {
            return Err("Practice requires the 960x720 logical viewport".into());
        }
        let editor = LineEditor::new("0:00", 64)?;
        let end_editor = LineEditor::new("", 64)?;
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
        };
        view.nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x10151e);
            text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
            text(scene, 24, 65, "PRACTICE SECTION", 2, 0xf0f4ff);
            text(scene, 24, 134, "START", 1, 0x9bb1cf);
            text(
                scene,
                24,
                260,
                "END (OPTIONAL; EMPTY PLAYS THROUGH SONG END)",
                1,
                0x9bb1cf,
            );
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
                450,
                "DONE UPDATES SETTINGS DRAFT; APPLY CHANGES THE NEXT SESSION",
                1,
                0x9bb1cf,
            );
            text(
                scene,
                24,
                472,
                "F5 RETRIES THE PINNED SESSION; BACK DISCARDS THESE EDITS",
                1,
                0x9bb1cf,
            );
            text(
                scene,
                24,
                494,
                "FULL SONG RESETS START AND END; THROUGH END CLEARS ONLY END",
                1,
                0x9bb1cf,
            );
            text(
                scene,
                24,
                516,
                "TAB SWITCHES START / END; END MUST BE AFTER START",
                1,
                0x9bb1cf,
            );
        });
        let editor = view.editor;
        let end_focused = view.end_focused;
        let input_font = view.input_font;
        let memo = scope.create_memo(move |_| (editor.get(), end_focused.get(), input_font.get()));
        view.nodes
            .bind(scope, memo, |(editor, end_focused, font), scene, hits| {
                let bounds = Bounds {
                    x: 24,
                    y: 150,
                    width: 906,
                    height: 40,
                };
                text_field_with_font(scene, &editor, bounds, !end_focused, font.as_ref());
                hits.push((ControlId(70), bounds));
                let preview = PracticeStart::parse(editor.value())
                    .map(|start| format!("EXACT START: {} NS", start.nanoseconds()))
                    .unwrap_or_else(|_| "INVALID START".into());
                text(scene, 24, 230, &preview, 2, 0xd8b36b);
            });
        let editor = view.end_editor;
        let memo = scope.create_memo(move |_| (editor.get(), end_focused.get(), input_font.get()));
        view.nodes
            .bind(scope, memo, |(editor, end_focused, font), scene, hits| {
                let bounds = Bounds {
                    x: 24,
                    y: 280,
                    width: 906,
                    height: 40,
                };
                text_field_with_font(scene, &editor, bounds, end_focused, font.as_ref());
                hits.push((ControlId(75), bounds));
                let preview = if editor.value().is_empty() {
                    "THROUGH SONG END".into()
                } else {
                    PracticeStart::parse(editor.value())
                        .map(|end| format!("EXACT END: {} NS", end.nanoseconds()))
                        .unwrap_or_else(|_| "INVALID END".into())
                };
                text(scene, 24, 335, &preview, 2, 0xd8b36b);
            });
        for (id, x, label) in [
            (71, 24, "DONE"),
            (72, 220, "BACK"),
            (73, 416, "FULL SONG"),
            (76, 612, "THROUGH END"),
        ] {
            view.button_node(
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
        view.nodes.bind(scope, memo, |error, scene, _| {
            if let Some(error) = error {
                text(scene, 24, 560, &error, 1, 0xffaaaa);
            }
        });
        view.nodes.validate()?;
        Ok(view)
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
impl Drop for PracticeView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
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
        assert!(PracticeView::new(ScreenInstanceId(9), 800, 600).is_err());
    }
}
