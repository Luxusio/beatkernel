//! Retained fine-grained Settings geometry; drafts and metadata ownership are external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::{button, text_field, text_field_value},
    retained::RetainedNodes,
    text_input::LineEditor,
};
use crate::{
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
    settings::{MAX_FIELDS, SettingsField},
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
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
}
impl SettingsView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        if (width, height) != (960, 720) {
            return Err("Settings requires the 960x720 logical viewport".into());
        }
        let empty = LineEditor::new("", 4096)?;
        let nodes = RetainedNodes::new(width, height)?;
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
            message: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
        };
        view.nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x10151e);
            text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
            text(scene, 24, 65, "F4 RECORDS / F6 PRACTICE", 1, 0x9bb1cf);
        });
        for (id, bounds, label) in BUTTONS.iter().take(5).copied() {
            view.button_node(id, bounds, label);
        }
        let count = view.count;
        let selected = view.selected;
        let memo = scope.create_memo(move |_| (selected.get() / 10 * 10, count.get()));
        view.nodes.bind(scope, memo, |(first, count), scene, _| {
            if count > 0 {
                text(
                    scene,
                    24,
                    102,
                    &format!(
                        "FIELDS {}-{} OF {}   UP/DOWN OR TAB SELECT",
                        first + 1,
                        (first + 10).min(count),
                        count
                    ),
                    1,
                    0xd8b36b,
                );
            }
        });
        for slot in 0..10 {
            let fields = Rc::clone(&view.fields);
            let selected = view.selected;
            let editor = view.editor;
            let focused = view.profile_focused;
            let pending = view.pending;
            let memo = scope.create_memo(move |_| {
                let selected = selected.get();
                let index = selected / 10 * 10 + slot;
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
                    })
            });
            view.nodes.bind(scope, memo, move |row, scene, hits| {
                if let Some(row) = row {
                    let y = 120 + slot as i64 * 39;
                    text(scene, 24, (y + 10) as usize, row.field.label, 1, 0xf0f4ff);
                    let bounds = Bounds {
                        x: 280,
                        y,
                        width: 650,
                        height: 32,
                    };
                    if let Some(editor) = row.editor {
                        text_field(scene, &editor, bounds, row.focused);
                    } else {
                        text_field_value(scene, &row.field.value, bounds);
                    }
                    if !row.pending {
                        hits.push((ControlId(1000 + row.index as u64), bounds));
                    }
                }
            });
        }
        let selected = view.selected;
        let fields = Rc::clone(&view.fields);
        let memo = scope.create_memo(move |_| {
            fields
                .get(selected.get())
                .and_then(|signal| signal.with(|field| field.as_ref().map(|field| field.hint)))
        });
        view.nodes.bind(scope, memo, |hint, scene, _| {
            if let Some(hint) = hint {
                text(scene, 24, 525, hint, 1, 0x9bb1cf);
            }
        });
        let profile = view.profile;
        let focused = view.profile_focused;
        let pending = view.pending;
        let memo = scope.create_memo(move |_| (profile.get(), focused.get(), pending.get()));
        view.nodes
            .bind(scope, memo, |(profile, focused, pending), scene, hits| {
                let bounds = Bounds {
                    x: 160,
                    y: 558,
                    width: 770,
                    height: 34,
                };
                text(scene, 24, 570, "PROFILE PATH", 1, 0xf0f4ff);
                text_field(scene, &profile, bounds, focused);
                if !pending {
                    hits.push((ControlId(15), bounds));
                }
            });
        for (id, bounds, label) in BUTTONS.iter().skip(5).copied() {
            view.button_node(id, bounds, label);
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
        view.nodes
            .bind(scope, memo, |(pending, message), scene, _| {
                if pending {
                    text(scene, 24, 665, "LOADING DEVICES", 1, 0xd8b36b);
                } else if let Some(message) = message {
                    text(scene, 24, 665, &message, 1, 0x74e5c5);
                }
            });
        let error = view.error;
        let memo = scope.create_memo(move |_| error.get());
        view.nodes.bind(scope, memo, |error, scene, _| {
            if let Some(error) = error {
                text(scene, 24, 690, &error, 1, 0xff8e8e);
            }
        });
        view.nodes.validate()?;
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
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
    fn button_node(&mut self, id: ControlId, bounds: Bounds, label: &'static str) {
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
        self.nodes.bind(
            self.scope,
            memo,
            move |(hovered, armed, pending), scene, hits| {
                button(scene, bounds, label, hovered, armed);
                if !pending {
                    hits.push((id, bounds));
                }
            },
        );
    }
}
impl Drop for SettingsView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
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
        assert!(SettingsView::new(ScreenInstanceId(9), 800, 600).is_err());
    }
}
