//! Ordered retained geometry nodes; reactive scope lifetime stays with each view.
use super::interaction::{Bounds, ControlId};
use crate::scene::{GeometrySnapshot, Scene};
use floem_reactive::{Memo, Scope, SignalGet};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Packet {
    geometry: Result<GeometrySnapshot, String>,
    hits: Vec<(ControlId, Bounds)>,
    #[cfg(test)]
    paints: usize,
}
/// Small geometry storage shared by retained views, with no signal scheduler or I/O.
/// Rc fields ensure main-thread use; the caller disposes its own Floem scope.
pub(crate) struct RetainedNodes {
    width: u32,
    height: u32,
    packets: Vec<Rc<RefCell<Packet>>>,
    dirty: Rc<Cell<bool>>,
}
impl RetainedNodes {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self, String> {
        if (width, height) != (960, 720) {
            return Err("retained views require the 960x720 logical viewport".into());
        }
        Ok(Self {
            width,
            height,
            packets: Vec::new(),
            dirty: Rc::new(Cell::new(true)),
        })
    }
    pub(crate) fn static_node(
        &mut self,
        paint: impl FnOnce(&mut Scene, &mut Vec<(ControlId, Bounds)>),
    ) {
        self.packets.push(Rc::new(RefCell::new(paint_packet(
            self.width,
            self.height,
            paint,
        ))));
        self.dirty.set(true);
    }
    pub(crate) fn bind<T: Clone + 'static>(
        &mut self,
        scope: Scope,
        memo: Memo<T>,
        paint: impl Fn(T, &mut Scene, &mut Vec<(ControlId, Bounds)>) + 'static,
    ) {
        let packet = Rc::new(RefCell::new(paint_packet(
            self.width,
            self.height,
            |_, _| {},
        )));
        self.packets.push(Rc::clone(&packet));
        let width = self.width;
        let height = self.height;
        let dirty = Rc::clone(&self.dirty);
        scope.create_effect(move |_| {
            let value = memo.get();
            let next = paint_packet(width, height, |scene, hits| paint(value, scene, hits));
            #[cfg(test)]
            let next = {
                let mut next = next;
                next.paints = packet.borrow().paints + 1;
                next
            };
            *packet.borrow_mut() = next;
            dirty.set(true);
        });
    }
    pub(crate) fn dirty(&self) -> bool {
        self.dirty.get()
    }
    /// Reuses existing packets for normal composition or forced scene restoration.
    /// Only successful complete composition clears dirty; errors remain explicit.
    pub(crate) fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        self.dirty.set(true);
        scene.clear();
        hits.clear();
        for packet in &self.packets {
            let packet = packet.borrow();
            scene.append_geometry(packet.geometry.as_ref().map_err(Clone::clone)?)?;
            hits.extend_from_slice(&packet.hits);
        }
        self.dirty.set(false);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn paints(&self) -> Vec<usize> {
        self.packets
            .iter()
            .map(|packet| packet.borrow().paints)
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn weak_dirty(&self) -> std::rc::Weak<Cell<bool>> {
        Rc::downgrade(&self.dirty)
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
