//! Retained output-only child panel; native command ownership remains external.
use super::{
    atoms::{rect, text},
    molecules::{button, text_field_with_font, text_field_value_with_font},
    interaction::{Bounds, ControlId},
    text_input::LineEditor,
    retained::RetainedNodes,
};
use crate::{
    scene::Scene, settings::SettingsField, screen_lifecycle::ScreenInstanceId, font_text::FontText,
};
use floem_reactive::{Scope, RwSignal, SignalGet, SignalWith, SignalUpdate};
pub const BUTTONS: [(ControlId, Bounds, &str); 2] = [
    (
        ControlId(90),
        Bounds {
            x: 24,
            y: 620,
            width: 190,
            height: 34,
        },
        "APPLY OUTPUT",
    ),
    (
        ControlId(91),
        Bounds {
            x: 230,
            y: 620,
            width: 170,
            height: 34,
        },
        "BACK TO PLAY",
    ),
];
#[derive(Clone, PartialEq)]
struct State {
    fields: Vec<SettingsField>,
    selected: usize,
    editor: LineEditor,
    message: Option<String>,
    pending: bool,
    hovered: Option<ControlId>,
    armed: Option<ControlId>,
}
pub struct LiveAudioFrame<'a> {
    pub fields: &'a [SettingsField],
    pub selected: usize,
    pub editor: &'a LineEditor,
    pub message: Option<&'a str>,
    pub pending: bool,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
pub struct LiveAudioView {
    id: ScreenInstanceId,
    scope: Scope,
    state: RwSignal<Option<State>>,
    font: RwSignal<Option<FontText>>,
    nodes: RetainedNodes,
}
impl LiveAudioView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let scope = Scope::new();
        let state = scope.create_rw_signal(None::<State>);
        let font = scope.create_rw_signal(None::<FontText>);
        let mut nodes = RetainedNodes::new(width, height)?;
        nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x101822);
            text(scene, 24, 24, "LIVE AUDIO OUTPUT", 3, 0xf0f4fa);
            text(
                scene,
                24,
                74,
                "PLAYBACK STAYS PAUSED. EMPTY FIELDS KEEP THE CURRENT SETTING.",
                1,
                0xaab8ca,
            );
        });
        let memo = scope.create_memo(move |_| (state.get(), font.get()));
        nodes.bind(scope, memo, |(state, font), scene, hits| {
            let Some(state) = state else {
                return;
            };
            for (index, field) in state.fields.iter().enumerate() {
                let spacing = if state.fields.len() == 4 { 100 } else { 130 };
                let y = 120 + index * spacing;
                text(scene, 24, y, field.label, 2, 0xe0e8f0);
                let bounds = Bounds {
                    x: 24,
                    y: (y + 30) as i64,
                    width: 900,
                    height: 44,
                };
                if index == state.selected {
                    text_field_with_font(
                        scene,
                        &state.editor,
                        bounds,
                        !state.pending,
                        font.as_ref(),
                    );
                } else {
                    text_field_value_with_font(scene, &field.value, bounds, font.as_ref());
                }
                if !state.pending {
                    hits.push((ControlId(1000 + index as u64), bounds));
                }
                text(scene, 24, y + 82, field.hint, 1, 0xaab8ca);
            }
            if let Some(message) = state.message {
                text(scene, 24, 540, &message, 1, 0xffd080);
            }
            if state.pending {
                text(
                    scene,
                    24,
                    574,
                    "APPLYING AUDIO OUTPUT SETTINGS...",
                    1,
                    0xffd080,
                );
            }
            for (id, bounds, label) in BUTTONS {
                button(
                    scene,
                    bounds,
                    label,
                    state.hovered == Some(id),
                    state.armed == Some(id),
                );
                if id == ControlId(91) || !state.pending {
                    hits.push((id, bounds));
                }
            }
        });
        nodes.validate()?;
        Ok(Self {
            id,
            scope,
            state,
            font,
            nodes,
        })
    }
    pub fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn set_input_font(&self, font: Option<FontText>) {
        if self.font.get_untracked() != font {
            self.font.set(font);
        }
    }
    pub fn update(&self, frame: LiveAudioFrame<'_>) -> Result<(), String> {
        if frame.fields.is_empty() || frame.fields.len() > 4 || frame.selected >= frame.fields.len()
        {
            return Err("live audio draft exceeds capability fields".into());
        }
        let unchanged = self.state.with_untracked(|state| {
            state.as_ref().is_some_and(|s| {
                s.fields == frame.fields
                    && s.selected == frame.selected
                    && s.editor == *frame.editor
                    && s.message.as_deref() == frame.message
                    && s.pending == frame.pending
                    && s.hovered == frame.hovered
                    && s.armed == frame.armed
            })
        });
        if !unchanged {
            self.state.set(Some(State {
                fields: frame.fields.to_vec(),
                selected: frame.selected,
                editor: frame.editor.clone(),
                message: frame.message.map(str::to_owned),
                pending: frame.pending,
                hovered: frame.hovered,
                armed: frame.armed,
            }));
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
impl Drop for LiveAudioView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}
