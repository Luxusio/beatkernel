//! Retained bounded device metadata presentation; selection admission stays external.
use super::{
    atoms::{rect, text},
    interaction::{Bounds, ControlId},
    molecules::button,
    retained::RetainedNodes,
};
use crate::{
    device_catalog::{DeviceCatalog, MAX_DEVICES},
    local_players::PlayerId,
    scene::Scene,
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
pub const BUTTONS: [(ControlId, Bounds, &'static str); 5] = [
    (
        ControlId(20),
        Bounds {
            x: 24,
            y: 620,
            width: 170,
            height: 34,
        },
        "USE DEVICE",
    ),
    (
        ControlId(21),
        Bounds {
            x: 212,
            y: 620,
            width: 170,
            height: 34,
        },
        "BACK",
    ),
    (
        ControlId(22),
        Bounds {
            x: 400,
            y: 620,
            width: 170,
            height: 34,
        },
        "REFRESH",
    ),
    (
        ControlId(23),
        Bounds {
            x: 588,
            y: 620,
            width: 170,
            height: 34,
        },
        "PREV",
    ),
    (
        ControlId(24),
        Bounds {
            x: 776,
            y: 620,
            width: 170,
            height: 34,
        },
        "NEXT",
    ),
];
pub struct DevicesFrame<'a> {
    pub catalog: &'a DeviceCatalog,
    pub player: Option<PlayerId>,
    pub selected: Option<usize>,
    pub first: usize,
    pub pending: bool,
    pub error: Option<&'a str>,
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
fn validate(frame: &DevicesFrame<'_>) -> Result<(), String> {
    let count = frame.catalog.choices().len();
    if count > MAX_DEVICES
        || frame.first.checked_add(10).is_none()
        || (count == 0 && frame.first != 0)
        || (count > 0 && frame.first >= count)
        || frame.selected.is_some_and(|index| index >= count)
    {
        return Err("Devices frame exceeds catalog or index bounds".into());
    }
    Ok(())
}
fn enabled(frame: &DevicesFrame<'_>, id: ControlId) -> bool {
    !frame.pending && (id.0 != 20 || frame.selected.is_some())
}
/// Current-state hit admission; nonselectable rows stay visible but have no hit.
pub fn hit(frame: &DevicesFrame<'_>, point: Option<(f64, f64)>) -> Option<ControlId> {
    let point = point?;
    if frame.pending || validate(frame).is_err() {
        return None;
    }
    for (id, bounds, _) in BUTTONS.iter().rev() {
        if enabled(frame, *id) && bounds.contains(point) {
            return Some(*id);
        }
    }
    for slot in (0..10).rev() {
        let index = frame.first + slot;
        if frame
            .catalog
            .choices()
            .get(index)
            .is_some_and(|choice| choice.selectable)
            && bounds(slot).contains(point)
        {
            return Some(ControlId(10000 + index as u64));
        }
    }
    None
}
#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    label: String,
    selected: bool,
    selectable: bool,
    pending: bool,
}
pub struct DevicesView {
    id: ScreenInstanceId,
    scope: Scope,
    rows: [RwSignal<Option<Row>>; 10],
    header: RwSignal<(bool, Option<PlayerId>)>,
    count: RwSignal<usize>,
    details: RwSignal<Option<(String, String)>>,
    pending: RwSignal<bool>,
    selected: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    hovered: RwSignal<Option<ControlId>>,
    armed: RwSignal<Option<ControlId>>,
    nodes: RetainedNodes,
}
impl DevicesView {
    pub fn new(id: ScreenInstanceId, width: u32, height: u32) -> Result<Self, String> {
        let nodes = RetainedNodes::new(width, height)?;
        let scope = Scope::new();
        let mut view = Self {
            id,
            scope,
            rows: std::array::from_fn(|_| scope.create_rw_signal(None)),
            header: scope.create_rw_signal((false, None)),
            count: scope.create_rw_signal(0),
            details: scope.create_rw_signal(None),
            pending: scope.create_rw_signal(false),
            selected: scope.create_rw_signal(false),
            error: scope.create_rw_signal(None),
            hovered: scope.create_rw_signal(None),
            armed: scope.create_rw_signal(None),
            nodes,
        };
        view.nodes.static_node(|scene, _| {
            rect(scene, 0, 0, 960, 720, 0x10151e);
            text(scene, 24, 20, "BEATKERNEL BMS PLAYER", 3, 0xf0f4ff);
        });
        let header = view.header;
        let memo = scope.create_memo(move |_| header.get());
        view.nodes
            .bind(scope, memo, |(keyboard, player), scene, _| {
                text(
                    scene,
                    24,
                    65,
                    if keyboard {
                        "KEYBOARD DEVICES - UP/DOWN SELECT - ENTER USE - ESC BACK"
                    } else {
                        "AUDIO OUTPUT DEVICES - UP/DOWN SELECT - ENTER USE - ESC BACK"
                    },
                    1,
                    0x9bb1cf,
                );
                if let Some(player) = player {
                    text(scene, 690, 65, &format!("FOR P{}", player.0), 1, 0x74e5c5);
                }
            });
        let count = view.count;
        let memo = scope.create_memo(move |_| count.get());
        view.nodes.bind(scope, memo, |count, scene, _| {
            text(
                scene,
                24,
                91,
                &format!("{} DEVICE ENTRIES - NO AUTOMATIC SELECTION", count),
                1,
                0xd8b36b,
            )
        });
        for (slot, row) in view.rows.iter().copied().enumerate() {
            let memo = scope.create_memo(move |_| row.get());
            view.nodes.bind(scope, memo, move |row, scene, hits| {
                if let Some(row) = row {
                    let bounds = bounds(slot);
                    rect(
                        scene,
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        if row.selected { 0x29475e } else { 0x1d2734 },
                    );
                    text(
                        scene,
                        32,
                        bounds.y as usize + 9,
                        &row.label,
                        2,
                        if row.selectable { 0xf0f4ff } else { 0x687485 },
                    );
                    if !row.pending && row.selectable {
                        hits.push((ControlId(10000 + row.index as u64), bounds));
                    }
                }
            });
        }
        let count = view.count;
        let memo = scope.create_memo(move |_| count.get() == 0);
        view.nodes.bind(scope, memo, |empty, scene, _| {
            if empty {
                text(scene, 24, 145, "NO DEVICES REPORTED", 2, 0x9bb1cf);
            }
        });
        let details = view.details;
        let memo = scope.create_memo(move |_| details.get());
        view.nodes.bind(scope, memo, |details, scene, _| {
            if let Some((id, detail)) = details {
                text(scene, 24, 558, &id, 1, 0xf0f4ff);
                text(scene, 24, 585, &detail, 1, 0x9bb1cf);
            }
        });
        for (id, bounds, label) in BUTTONS {
            let pending = view.pending;
            let selected = view.selected;
            let hovered = view.hovered;
            let armed = view.armed;
            let memo = scope.create_memo(move |_| {
                let enabled = !pending.get() && (id.0 != 20 || selected.get());
                (
                    enabled,
                    enabled && hovered.get() == Some(id),
                    enabled && armed.get() == Some(id),
                )
            });
            view.nodes.bind(
                scope,
                memo,
                move |(enabled, hovered, armed), scene, hits| {
                    button(scene, bounds, label, hovered, armed);
                    if enabled {
                        hits.push((id, bounds));
                    }
                },
            );
        }
        let pending = view.pending;
        let memo = scope.create_memo(move |_| pending.get());
        view.nodes.bind(scope, memo, |pending, scene, _| {
            if pending {
                text(scene, 24, 665, "LOADING DEVICES", 1, 0xd8b36b);
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
    pub fn update(&self, frame: DevicesFrame<'_>) -> Result<(), String> {
        validate(&frame)?;
        let choices = frame.catalog.choices();
        for (slot, signal) in self.rows.iter().enumerate() {
            let index = frame.first + slot;
            if let Some(choice) = choices.get(index) {
                let selected = frame.selected == Some(index);
                if !signal.with_untracked(|old| {
                    old.as_ref().is_some_and(|old| {
                        old.index == index
                            && old.label == choice.label
                            && old.selected == selected
                            && old.selectable == choice.selectable
                            && old.pending == frame.pending
                    })
                }) {
                    signal.set(Some(Row {
                        index,
                        label: choice.label.clone(),
                        selected,
                        selectable: choice.selectable,
                        pending: frame.pending,
                    }));
                }
            } else if signal.with_untracked(Option::is_some) {
                signal.set(None);
            }
        }
        let header = (frame.catalog.request().is_keyboard(), frame.player);
        if self.header.get_untracked() != header {
            self.header.set(header);
        }
        if self.count.get_untracked() != choices.len() {
            self.count.set(choices.len());
        }
        let detail = frame.selected.and_then(|index| choices.get(index));
        if !self.details.with_untracked(|old| match (old, detail) {
            (Some((id, text)), Some(choice)) => id == &choice.id && text == &choice.detail,
            (None, None) => true,
            _ => false,
        }) {
            self.details
                .set(detail.map(|choice| (choice.id.clone(), choice.detail.clone())));
        }
        if self.pending.get_untracked() != frame.pending {
            self.pending.set(frame.pending);
        }
        if self.selected.get_untracked() != frame.selected.is_some() {
            self.selected.set(frame.selected.is_some());
        }
        if !self
            .error
            .with_untracked(|old| old.as_deref() == frame.error)
        {
            self.error.set(frame.error.map(str::to_owned));
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
impl Drop for DevicesView {
    fn drop(&mut self) {
        self.scope.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_catalog::{DeviceChoice, DeviceRequest};

    fn catalog(count: usize) -> DeviceCatalog {
        DeviceCatalog::new(
            DeviceRequest::LinuxKeyboard,
            (0..count)
                .map(|index| DeviceChoice {
                    id: format!("/dev/input/event{index}"),
                    label: format!("KEYBOARD {index}"),
                    detail: format!("DETAIL {index}"),
                    selectable: index % 2 == 0,
                })
                .collect(),
        )
        .unwrap()
    }
    fn frame(catalog: &DeviceCatalog) -> DevicesFrame<'_> {
        DevicesFrame {
            catalog,
            player: None,
            selected: None,
            first: 0,
            pending: false,
            error: None,
            hovered: None,
            armed: None,
        }
    }
    #[test]
    fn unchanged_and_error_updates_preserve_rows_and_button_hover_is_local() {
        let catalog = catalog(4);
        let view = DevicesView::new(ScreenInstanceId(50), 960, 720).unwrap();
        view.update(frame(&catalog)).unwrap();
        let initial = view.nodes.paints();
        view.update(frame(&catalog)).unwrap();
        assert_eq!(view.nodes.paints(), initial);
        let mut update = frame(&catalog);
        update.error = Some("QUERY FAILED");
        view.update(update).unwrap();
        let status = view.nodes.paints();
        assert_eq!(&status[3..13], &initial[3..13]);
        let mut update = frame(&catalog);
        update.error = Some("QUERY FAILED");
        update.hovered = Some(ControlId(22));
        update.armed = Some(ControlId(22));
        view.update(update).unwrap();
        let hovered = view.nodes.paints();
        for index in 0..hovered.len() {
            if index == 17 {
                assert!(hovered[index] > status[index]);
            } else {
                assert_eq!(hovered[index], status[index]);
            }
        }
        let mut update = frame(&catalog);
        update.selected = Some(0);
        view.update(update).unwrap();
        let selected = view.nodes.paints();
        let mut update = frame(&catalog);
        update.selected = Some(2);
        view.update(update).unwrap();
        let changed = view.nodes.paints();
        assert!(changed[3] > selected[3]);
        assert!(changed[5] > selected[5]);
        assert_eq!(changed[4], selected[4]);
        assert_eq!(&changed[6..13], &selected[6..13]);
    }
    #[test]
    fn disabled_selected_metadata_keeps_use_gate_and_actual_paged_row_ids() {
        let catalog = catalog(21);
        let view = DevicesView::new(ScreenInstanceId(51), 960, 720).unwrap();
        let mut page = frame(&catalog);
        page.first = 10;
        page.selected = Some(11);
        page.player = Some(PlayerId(u32::MAX));
        assert_eq!(hit(&page, Some((30.0, 125.0))), Some(ControlId(10010)));
        assert_eq!(hit(&page, Some((30.0, 164.0))), None);
        assert_eq!(hit(&page, Some((30.0, 625.0))), Some(ControlId(20)));
        view.update(page).unwrap();
        assert_eq!(
            view.details.get_untracked(),
            Some(("/dev/input/event11".into(), "DETAIL 11".into()))
        );
        assert_eq!(
            view.header.get_untracked(),
            (true, Some(PlayerId(u32::MAX)))
        );
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits[..5].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![10010, 10012, 10014, 10016, 10018]
        );
        assert_eq!(
            hits[5..].iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![20, 21, 22, 23, 24]
        );
        let mut pending = frame(&catalog);
        pending.first = 10;
        pending.selected = Some(11);
        pending.pending = true;
        assert_eq!(hit(&pending, Some((30.0, 625.0))), None);
        view.update(pending).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert!(hits.is_empty());
    }
    #[test]
    fn empty_catalog_pages_still_admit_legacy_controls_and_rejections_are_atomic() {
        let catalog = catalog(0);
        let view = DevicesView::new(ScreenInstanceId(52), 960, 720).unwrap();
        view.update(frame(&catalog)).unwrap();
        assert_eq!(
            hit(&frame(&catalog), Some((600.0, 625.0))),
            Some(ControlId(23))
        );
        assert_eq!(
            hit(&frame(&catalog), Some((790.0, 625.0))),
            Some(ControlId(24))
        );
        assert_eq!(hit(&frame(&catalog), Some((30.0, 625.0))), None);
        let before = view.nodes.paints();
        let identities = view.nodes.identities();
        let mut invalid = frame(&catalog);
        invalid.selected = Some(0);
        invalid.error = Some("MUST NOT APPLY");
        assert!(view.update(invalid).is_err());
        let mut invalid = frame(&catalog);
        invalid.first = usize::MAX;
        assert!(view.update(invalid).is_err());
        assert_eq!(view.nodes.paints(), before);
        assert_eq!(view.error.get_untracked(), None);
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        let count = scene.rectangles().len();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 960.0, 720.0]);
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![21, 22, 23, 24]
        );
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(scene.rectangles().len(), count);
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.nodes.paints(), before);
        let weak = view.nodes.weak_dirty();
        drop(view);
        assert!(weak.upgrade().is_none());
        assert!(DevicesView::new(ScreenInstanceId(52), 800, 600).is_err());
    }
}
