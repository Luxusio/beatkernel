//! Retained local-player draft presentation; attachment and launch validation are external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::button,
    retained::RetainedNodes,
};
use crate::{local_setup::LocalSetup, scene::Scene, screen_lifecycle::ScreenInstanceId};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};

pub const BUTTONS: [(ControlId, Bounds, &'static str); 8] = [
    (
        ControlId(36),
        Bounds {
            x: 620,
            y: 550,
            width: 150,
            height: 34,
        },
        "PREVIOUS",
    ),
    (
        ControlId(37),
        Bounds {
            x: 780,
            y: 550,
            width: 150,
            height: 34,
        },
        "NEXT",
    ),
    (
        ControlId(30),
        Bounds {
            x: 24,
            y: 620,
            width: 140,
            height: 34,
        },
        "DONE",
    ),
    (
        ControlId(31),
        Bounds {
            x: 174,
            y: 620,
            width: 140,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(32),
        Bounds {
            x: 324,
            y: 620,
            width: 140,
            height: 34,
        },
        "REMOVE",
    ),
    (
        ControlId(33),
        Bounds {
            x: 474,
            y: 620,
            width: 140,
            height: 34,
        },
        "ADD",
    ),
    (
        ControlId(34),
        Bounds {
            x: 624,
            y: 620,
            width: 150,
            height: 34,
        },
        "ASSIGN KEYBOARD",
    ),
    (
        ControlId(35),
        Bounds {
            x: 784,
            y: 620,
            width: 150,
            height: 34,
        },
        "CLEAR",
    ),
];
pub struct PlayersFrame<'a> {
    pub model: &'a LocalSetup,
    pub selected: usize,
    pub first: usize,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
    pub hovered: Option<ControlId>,
    pub armed: Option<ControlId>,
}
fn bounds(slot: usize) -> Bounds {
    Bounds {
        x: 24,
        y: 120 + slot as i64 * 39,
        width: 906,
        height: 34,
    }
}
fn validate(frame: &PlayersFrame<'_>) -> Result<(), String> {
    let count = frame.model.players().len();
    if !(1..=64).contains(&count)
        || frame.selected >= count
        || frame.first >= count
        || frame.first.checked_add(10).is_none()
    {
        return Err("Players frame exceeds roster or index bounds".into());
    }
    Ok(())
}
fn gate(frame: &PlayersFrame<'_>, id: ControlId) -> (bool, bool) {
    let count = frame.model.players().len();
    let solo = count == 1;
    let visible = match id.0 {
        36 => frame.first > 0,
        37 => frame.first.saturating_add(10) < count,
        34 | 35 => !solo,
        _ => true,
    };
    (
        visible,
        visible && !frame.pending && !(id.0 == 32 && solo) && !(id.0 == 33 && count == 64),
    )
}
/// Current-frame hit routing in reverse painter order, including armed player rows.
pub fn hit(frame: &PlayersFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    let point = point?;
    if frame.pending || validate(frame).is_err() {
        return None;
    }
    for (id, bounds, _) in BUTTONS.iter().rev() {
        if gate(frame, *id).1 && bounds.contains(point) {
            return Some(*id);
        }
    }
    for slot in (0..10).rev() {
        if frame.first + slot < frame.model.players().len() && bounds(slot).contains(point) {
            return Some(ControlId(20000 + (frame.first + slot) as u64));
        }
    }
    None
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    id: u32,
    input: Option<String>,
    solo: bool,
    selected: bool,
    pending: bool,
}
pub struct PlayersView {
    id: ScreenInstanceId,
    scope: Scope,
    rows: [RwSignal<Option<Row>>; 10],
    summary: RwSignal<(usize, usize)>,
    pending: RwSignal<bool>,
    message: RwSignal<Option<String>>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    gates: [RwSignal<(bool, bool)>; 8],
    nodes: RetainedNodes,
}
impl PlayersView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let nodes = RetainedNodes::new(width, height)?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            rows: std::array::from_fn(|_| scope.create_rw_signal(None)),
            summary: scope.create_rw_signal((0, 0)),
            pending: scope.create_rw_signal(false),
            message: scope.create_rw_signal(None),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            gates: std::array::from_fn(|_| scope.create_rw_signal((false, false))),
            nodes,
        };
        view.nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x10151e);
            text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
            text(
                scene,
                24,
                65,
                "PLAYERS - +/- COUNT - SPACE ASSIGN - ENTER DONE - ESC BACK",
                1,
                0x9bb1cf,
            );
        });
        let summary = view.summary;
        let memo = scope.create_memo(move |_| summary.get());
        view.nodes.bind(scope, memo, |(count, first), scene, _| {
            text(
                scene,
                24,
                91,
                &format!(
                    "{} PLAYERS  ROWS {}-{} OF {}",
                    count,
                    first + 1,
                    (first + 10).min(count),
                    count
                ),
                1,
                0xd8b36b,
            )
        });
        for (slot, row) in view.rows.iter().copied().enumerate() {
            let hovered = view.hovered;
            let armed = view.armed;
            let interaction = scope.create_memo(move |_| {
                row.with(|row| {
                    row.as_ref().map(|row| {
                        if row.pending {
                            (false, false)
                        } else {
                            let id = ControlId(20000 + row.index as u64);
                            (hovered.get() == Some(id), armed.get() == Some(id))
                        }
                    })
                })
            });
            let memo = scope.create_memo(move |_| (row.get(), interaction.get()));
            view.nodes
                .bind(scope, memo, move |(row, interaction), scene, hits| {
                    if let Some(row) = row {
                        let bounds = bounds(slot);
                        let label = if row.solo {
                            "SOLO - INPUT AUTOMATIC".into()
                        } else {
                            format!(
                                "P{}  {}",
                                row.id,
                                row.input.as_deref().unwrap_or("NO KEYBOARD ASSIGNED")
                            )
                        };
                        if row.selected {
                            rect(scene, bounds.x - 4, bounds.y, 4, bounds.height, 0x74e5c5);
                        }
                        let (hovered, armed) = interaction.unwrap_or((false, false));
                        button(scene, bounds, &label, hovered, armed);
                        if !row.pending {
                            hits.push((ControlId(20000 + row.index as u64), bounds));
                        }
                    }
                });
        }
        let summary = view.summary;
        let memo = scope.create_memo(move |_| summary.get().0 == 1);
        view.nodes.bind(scope, memo, |solo, scene, _| {
            text(
                scene,
                24,
                535,
                if solo {
                    "SOLO STARTS WITHOUT DEVICE SELECTION"
                } else {
                    "ASSIGN A DISTINCT KEYBOARD TO EACH PLAYER"
                },
                1,
                0x9bb1cf,
            )
        });
        for index in 0..8 {
            let (id, bounds, label) = BUTTONS[index];
            let gate = view.gates[index];
            let hovered = view.hovered;
            let armed = view.armed;
            let memo = scope.create_memo(move |_| {
                let (visible, enabled) = gate.get();
                (
                    visible,
                    enabled,
                    enabled && hovered.get() == Some(id),
                    enabled && armed.get() == Some(id),
                )
            });
            view.nodes.bind(
                scope,
                memo,
                move |(visible, enabled, hovered, armed), scene, hits| {
                    if visible {
                        button(scene, bounds, label, hovered, armed);
                        if enabled {
                            hits.push((id, bounds));
                        }
                    }
                },
            );
        }
        let pending = view.pending;
        let memo = scope.create_memo(move |_| pending.get());
        view.nodes.bind(scope, memo, |pending, scene, _| {
            if pending {
                text(scene, 24, 590, "LOADING KEYBOARDS", 1, 0xd8b36b);
            }
        });
        let message = view.message;
        let error = view.error;
        let memo = scope.create_memo(move |_| {
            if let Some(error) = error.get() {
                (true, Some(error))
            } else {
                (false, message.get())
            }
        });
        view.nodes.bind(scope, memo, |(error, value), scene, _| {
            if let Some(value) = value {
                text(
                    scene,
                    24,
                    690,
                    &value,
                    1,
                    if error { 0xff8e8e } else { 0x74e5c5 },
                );
            }
        });
        view.nodes.validate()?;
        Ok(view)
    }
    pub const fn id(&self) -> ScreenInstanceId {
        self.id
    }
    pub fn update(&self, frame: PlayersFrame<'_>) -> Result<(), String> {
        validate(&frame)?;
        let players = frame.model.players();
        let solo = players.len() == 1;
        for (slot, signal) in self.rows.iter().enumerate() {
            let index = frame.first + slot;
            if let Some(player) = players.get(index) {
                let selected = frame.selected == index;
                if !signal.with_untracked(|old| {
                    old.as_ref().is_some_and(|old| {
                        old.index == index
                            && old.id == player.id.0
                            && old.input.as_deref() == player.input()
                            && old.solo == solo
                            && old.selected == selected
                            && old.pending == frame.pending
                    })
                }) {
                    signal.set(Some(Row {
                        index,
                        id: player.id.0,
                        input: player.input().map(str::to_owned),
                        solo,
                        selected,
                        pending: frame.pending,
                    }));
                }
            } else if signal.with_untracked(Option::is_some) {
                signal.set(None);
            }
        }
        let summary = (players.len(), frame.first);
        if self.summary.get_untracked() != summary {
            self.summary.set(summary);
        }
        if self.pending.get_untracked() != frame.pending {
            self.pending.set(frame.pending);
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
        for (signal, (id, _, _)) in self.gates.iter().zip(BUTTONS) {
            let gate = gate(&frame, id);
            if signal.get_untracked() != gate {
                signal.set(gate);
            }
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
impl Drop for PlayersView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{NativeSettings, SettingsHost};

    fn model(count: usize) -> LocalSetup {
        let settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
        let mut model = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
        model.resize(count).unwrap();
        model
    }
    fn frame(model: &LocalSetup) -> PlayersFrame<'_> {
        PlayersFrame {
            model,
            selected: 0,
            first: 0,
            pending: false,
            error: None,
            message: None,
            hovered: None,
            armed: None,
        }
    }
    #[test]
    fn unchanged_status_and_row_interactions_keep_unrelated_packets() {
        let model = model(4);
        let view = PlayersView::new(ScreenInstanceId(40), 960, 720).unwrap();
        view.update(frame(&model)).unwrap();
        let initial = view.nodes.paints();
        view.update(frame(&model)).unwrap();
        assert_eq!(view.nodes.paints(), initial);
        let mut update = frame(&model);
        update.error = Some("EDITABLE DRAFT");
        view.update(update).unwrap();
        let status = view.nodes.paints();
        assert_eq!(&status[2..12], &initial[2..12]);
        let mut update = frame(&model);
        update.error = Some("EDITABLE DRAFT");
        update.hovered = Some(ControlId(20002));
        update.armed = Some(ControlId(20002));
        view.update(update).unwrap();
        let hovered = view.nodes.paints();
        for index in 2..12 {
            if index == 4 {
                assert!(hovered[index] > status[index]);
            } else {
                assert_eq!(hovered[index], status[index]);
            }
        }
        let mut update = frame(&model);
        update.selected = 1;
        update.error = Some("EDITABLE DRAFT");
        update.hovered = Some(ControlId(20002));
        update.armed = Some(ControlId(20002));
        view.update(update).unwrap();
        let selected = view.nodes.paints();
        assert!(selected[2] > hovered[2]);
        assert!(selected[3] > hovered[3]);
        assert_eq!(&selected[4..12], &hovered[4..12]);
    }
    #[test]
    fn solo_and_full_roster_paging_preserve_legacy_admission_and_order() {
        let solo = model(1);
        let view = PlayersView::new(ScreenInstanceId(41), 960, 720).unwrap();
        view.update(frame(&solo)).unwrap();
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![20000, 30, 31, 33]
        );
        assert_eq!(hit(&frame(&solo), Some((330.0, 625.0))), None);
        assert_eq!(hit(&frame(&solo), Some((630.0, 625.0))), None);
        let group = model(64);
        let mut page = frame(&group);
        page.first = 50;
        page.selected = 50;
        assert_eq!(hit(&page, Some((30.0, 125.0))), Some(ControlId(20050)));
        view.update(page).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[..10].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            (20050..20060).collect::<Vec<_>>()
        );
        assert!(hits.iter().any(|(id, _)| id.0 == 36));
        assert!(hits.iter().any(|(id, _)| id.0 == 37));
        assert!(!hits.iter().any(|(id, _)| id.0 == 33));
        let mut pending = frame(&group);
        pending.first = 50;
        pending.selected = 50;
        pending.pending = true;
        assert_eq!(hit(&pending, Some((30.0, 125.0))), None);
        view.update(pending).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
    }
    #[test]
    fn restored_max_id_is_data_not_a_row_index_and_failure_is_atomic() {
        let args = vec![
            "--local-player".into(),
            "4294967295:/last".into(),
            "--local-player".into(),
            "7:/other".into(),
        ];
        let settings = NativeSettings::from_args(&args, SettingsHost::Linux).unwrap();
        let model = LocalSetup::from_settings(&settings, SettingsHost::Linux).unwrap();
        let view = PlayersView::new(ScreenInstanceId(42), 960, 720).unwrap();
        view.update(frame(&model)).unwrap();
        assert_eq!(view.rows[0].get_untracked().unwrap().id, u32::MAX);
        let before = view.nodes.paints();
        let identities = view.nodes.identities();
        let mut invalid = frame(&model);
        invalid.selected = 2;
        invalid.error = Some("MUST NOT APPLY");
        assert!(view.update(invalid).is_err());
        let mut invalid = frame(&model);
        invalid.first = usize::MAX;
        assert!(view.update(invalid).is_err());
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.error.get_untracked(), None);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let count = scene.rectangles().len();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.nodes.paints(), before);
        let weak = view.nodes.weak_dirty();
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(PlayersView::new(ScreenInstanceId(42), 800, 600).is_err());
    }
}
