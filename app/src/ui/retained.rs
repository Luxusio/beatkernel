//! Ordered retained geometry nodes; reactive scope lifetime stays with each view.
use super::{
    interaction::{Bounds, ControlId},
    layout::{ComponentClips, LayoutGeometry, MountedLayout, NodeId},
};
use crate::{
    scene::{ClipRect, GeometrySnapshot, Scene, UiComponentKey, MAX_UI_COMPONENTS},
    screen_lifecycle::ScreenInstanceId,
};
use floem_reactive::{Memo, Scope, SignalGet};
use std::ops::Range;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct LayoutBinding {
    ids: Vec<NodeId>,
    geometries: RefCell<Vec<LayoutDependency>>,
    paint: Box<dyn Fn(&[LayoutDependency], [u32; 2], &[NodeId]) -> Packet>,
}
#[derive(Clone, Copy)]
struct LayoutDependency {
    geometry: LayoutGeometry,
    clips: ComponentClips,
}
#[derive(Clone)]
struct Packet {
    geometry: Result<GeometrySnapshot, String>,
    hits: Vec<(ControlId, Bounds)>,
    layout: Option<Rc<LayoutBinding>>,
    parts: Vec<ComponentPart>,
    #[cfg(test)]
    paints: usize,
}
#[derive(Clone)]
struct ComponentPart {
    node: NodeId,
    rectangles: Range<u32>,
    hits: Range<usize>,
    geometry: LayoutGeometry,
    clips: ComponentClips,
}
/// Small geometry storage shared by retained views, with no signal scheduler or I/O.
/// Rc fields ensure main-thread use; the caller disposes its own Floem scope.
pub(crate) struct RetainedNodes {
    width: u32,
    height: u32,
    packets: Vec<Rc<RefCell<Packet>>>,
    dirty: Rc<Cell<bool>>,
    extent: Rc<Cell<[u32; 2]>>,
    animated: RefCell<Vec<NodeId>>,
    presented: RefCell<Vec<Packet>>,
}
impl RetainedNodes {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self, String> {
        Ok(Self {
            width,
            height,
            packets: Vec::new(),
            dirty: Rc::new(Cell::new(true)),
            extent: Rc::new(Cell::new([width, height])),
            animated: RefCell::new(Vec::new()),
            presented: RefCell::new(Vec::new()),
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
    pub(crate) fn static_layout_node<C: Copy>(
        &mut self,
        layout: &MountedLayout<C>,
        ids: &[NodeId],
        paint: impl Fn(NodeId, LayoutGeometry, &mut Scene, &mut Vec<(ControlId, Bounds)>) + 'static,
    ) -> Result<(), String> {
        if ids.len() > 1024 || self.packets.len() >= 1024 {
            return Err("retained layout exceeds node/dependency capacity".into());
        }
        let ids = ids.to_vec();
        let geometries = layout_geometries(layout, &ids)?;
        let painter_ids = ids.clone();
        let binding = Rc::new(LayoutBinding {
            ids,
            geometries: RefCell::new(geometries.clone()),
            paint: Box::new(move |geometry, extent, animated| {
                paint_layout_packet(extent, &painter_ids, geometry, animated, &paint)
            }),
        });
        let mut packet = (binding.paint)(&geometries, self.extent.get(), &[]);
        packet.geometry.as_ref().map_err(Clone::clone)?;
        packet.layout = Some(binding);
        self.packets.push(Rc::new(RefCell::new(packet)));
        self.dirty.set(true);
        Ok(())
    }
    pub(crate) fn bind_layout<T: Clone + 'static, C: Copy>(
        &mut self,
        scope: Scope,
        memo: Memo<T>,
        layout: &MountedLayout<C>,
        ids: &[NodeId],
        paint: impl Fn(&T, NodeId, LayoutGeometry, &mut Scene, &mut Vec<(ControlId, Bounds)>) + 'static,
    ) -> Result<(), String> {
        if ids.len() > 1024 || self.packets.len() >= 1024 {
            return Err("retained layout exceeds node/dependency capacity".into());
        }
        let ids = ids.to_vec();
        let geometries = layout_geometries(layout, &ids)?;
        let value = Rc::new(RefCell::new(Rc::new(memo.get_untracked())));
        let paint_value = Rc::clone(&value);
        let painter_ids = ids.clone();
        let binding = Rc::new(LayoutBinding {
            ids,
            geometries: RefCell::new(geometries),
            paint: Box::new(move |geometry, extent, animated| {
                // Keep one immutable snapshot for the whole packet and release
                // the publication borrow before invoking any painter callback.
                let value = Rc::clone(&paint_value.borrow());
                paint_layout_packet(
                    extent,
                    &painter_ids,
                    geometry,
                    animated,
                    &|id, geometry, scene, hits| {
                        paint(value.as_ref(), id, geometry, scene, hits);
                    },
                )
            }),
        });
        let mut initial = (binding.paint)(&binding.geometries.borrow(), self.extent.get(), &[]);
        initial.geometry.as_ref().map_err(Clone::clone)?;
        initial.layout = Some(Rc::clone(&binding));
        let packet = Rc::new(RefCell::new(initial));
        self.packets.push(Rc::clone(&packet));
        let extent = Rc::clone(&self.extent);
        let dirty = Rc::clone(&self.dirty);
        scope.create_effect(move |_| {
            *value.borrow_mut() = Rc::new(memo.get());
            let mut next = (binding.paint)(&binding.geometries.borrow(), extent.get(), &[]);
            next.layout = Some(Rc::clone(&binding));
            #[cfg(test)]
            {
                next.paints = packet.borrow().paints + 1;
            }
            *packet.borrow_mut() = next;
            dirty.set(true);
        });
        self.dirty.set(true);
        Ok(())
    }
    /// Stage all dependent packets first. Refusal leaves bounds, clips, packets
    /// and admission unchanged. Suspension preserves packets for later restoration.
    pub(crate) fn relayout<C: Copy>(&self, layout: &MountedLayout<C>) -> Result<bool, String> {
        let extent = layout.extent();
        let previous_extent = self.extent.get();
        if layout.suspended() {
            if previous_extent == extent {
                return Ok(false);
            }
            self.extent.set(extent);
            self.dirty.set(true);
            return Ok(true);
        }
        let mut candidates = Vec::new();
        candidates
            .try_reserve_exact(self.packets.len())
            .map_err(|_| "retained layout allocation failed")?;
        for (index, packet) in self.packets.iter().enumerate() {
            let packet = packet.borrow();
            let Some(binding) = packet.layout.as_ref() else {
                if extent != previous_extent {
                    return Err("retained extent changes require layout-bound packets".into());
                }
                continue;
            };
            let geometries = layout_geometries(layout, &binding.ids)?;
            let changed = extent != previous_extent
                || binding
                    .geometries
                    .borrow()
                    .iter()
                    .zip(&geometries)
                    .any(|(old, next)| !dependency_equal(*old, *next));
            if !changed {
                continue;
            }
            let mut next = (binding.paint)(&geometries, extent, &[]);
            next.geometry.as_ref().map_err(Clone::clone)?;
            next.layout = Some(Rc::clone(binding));
            #[cfg(test)]
            {
                next.paints = packet.paints + 1;
            }
            candidates.push((index, geometries, next));
        }
        if candidates.is_empty() && extent == previous_extent {
            return Ok(false);
        }
        let mut validation = Scene::with_capacity(extent[0], extent[1], 64);
        for (index, packet) in self.packets.iter().enumerate() {
            if let Some((_, _, next)) = candidates
                .iter()
                .find(|(candidate, _, _)| *candidate == index)
            {
                validation.append_geometry(next.geometry.as_ref().map_err(Clone::clone)?)?;
            } else {
                validation
                    .append_geometry(packet.borrow().geometry.as_ref().map_err(Clone::clone)?)?;
            }
        }
        for (index, geometries, next) in candidates {
            *next.layout.as_ref().unwrap().geometries.borrow_mut() = geometries;
            *self.packets[index].borrow_mut() = next;
        }
        self.extent.set(extent);
        self.dirty.set(true);
        Ok(true)
    }
    /// Checks immediate packet errors before a constructed view is published.
    pub(crate) fn validate(&self) -> Result<(), String> {
        for packet in &self.packets {
            packet.borrow().geometry.as_ref().map_err(Clone::clone)?;
        }
        Ok(())
    }
    pub(crate) fn dirty(&self) -> bool {
        self.dirty.get()
    }
    /// Retained packets remain valid when the coordinator clears click hits
    /// pending redraw. Respect the same painter order as composition.
    pub(crate) fn hit(&self, point: (f64, f64)) -> Option<ControlId> {
        if self.extent.get().contains(&0) {
            return None;
        }
        self.packets.iter().rev().find_map(|packet| {
            packet
                .borrow()
                .hits
                .iter()
                .rev()
                .find(|(_, bounds)| bounds.contains(point))
                .map(|(id, _)| *id)
        })
    }
    /// Reuses existing packets for normal composition or forced scene restoration.
    /// Only successful complete composition clears dirty; errors remain explicit.
    pub(crate) fn compose(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
    ) -> Result<(), String> {
        self.dirty.set(true);
        if !self.extent.get().contains(&0) && scene.logical_extent() != self.extent.get() {
            return Err("retained composition extent mismatch".into());
        }
        self.validate()?;
        scene.clear();
        hits.clear();
        self.presented.borrow_mut().clear();
        if self.extent.get().contains(&0) {
            self.dirty.set(false);
            return Ok(());
        }
        for packet in &self.packets {
            let packet = packet.borrow();
            scene.append_geometry(packet.geometry.as_ref().map_err(Clone::clone)?)?;
            hits.extend_from_slice(&packet.hits);
        }
        self.dirty.set(false);
        Ok(())
    }
    /// Associates only explicitly animated mounted nodes with their actual
    /// packet parts. Multiple packets for one node share one binding; differing
    /// geometry is refused. All candidates stage before the published scene changes.
    pub(crate) fn node_for_control(&self, control: ControlId) -> Result<Option<NodeId>, String> {
        let mut node = None;
        let mut inspect = |packet: &Packet| -> Result<(), String> {
            for part in &packet.parts {
                if packet.hits[part.hits.clone()]
                    .iter()
                    .any(|(id, _)| *id == control)
                {
                    if node.is_some_and(|old| old != part.node) {
                        return Err("control has ambiguous mounted node association".into());
                    }
                    node = Some(part.node);
                }
            }
            Ok(())
        };
        // Component source capture can retain a hit that ordinary ancestor
        // clipping hid. Its association still comes from the mounted node.
        let presented = self.presented.borrow();
        if presented.is_empty() {
            for packet in &self.packets {
                inspect(&packet.borrow())?;
            }
        } else {
            for packet in presented.iter() {
                inspect(packet)?;
            }
        }
        Ok(node)
    }
    pub(crate) fn compose_components(
        &self,
        scene: &mut Scene,
        hits: &mut Vec<(ControlId, Bounds)>,
        screen: ScreenInstanceId,
        animated: &[NodeId],
    ) -> Result<(), String> {
        if screen.0 == 0
            || animated.len() > MAX_UI_COMPONENTS
            || animated
                .iter()
                .enumerate()
                .any(|(i, node)| animated[..i].contains(node))
        {
            return Err("invalid animated mounted node set".into());
        }
        self.validate()?;
        if !self.extent.get().contains(&0) && scene.logical_extent() != self.extent.get() {
            return Err("retained composition extent mismatch".into());
        }
        if !self.dirty()
            && scene.component_owner() == Some(screen)
            && self.animated.borrow().as_slice() == animated
        {
            hits.clear();
            if !self.extent.get().contains(&0) {
                for packet in self.presented.borrow().iter() {
                    hits.extend_from_slice(&packet.hits);
                }
            }
            return Ok(());
        }
        let mut presented = Vec::new();
        presented
            .try_reserve_exact(self.packets.len())
            .map_err(|_| "component source packet allocation failed")?;
        for packet in &self.packets {
            let packet = packet.borrow();
            let next = if !self.extent.get().contains(&0) {
                if let Some(binding) = packet
                    .layout
                    .as_ref()
                    .filter(|binding| binding.ids.iter().any(|id| animated.contains(id)))
                {
                    (binding.paint)(&binding.geometries.borrow(), self.extent.get(), animated)
                } else {
                    packet.clone()
                }
            } else {
                packet.clone()
            };
            next.geometry.as_ref().map_err(Clone::clone)?;
            presented.push(next);
        }
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(animated.len())
            .map_err(|_| "component association allocation failed")?;
        for &node in animated {
            let mut offset = 0u32;
            let mut ranges = Vec::new();
            let mut geometry = None;
            for packet in &presented {
                for part in packet.parts.iter().filter(|p| p.node == node) {
                    let dependency = LayoutDependency {
                        geometry: part.geometry,
                        clips: part.clips,
                    };
                    if geometry.is_some_and(|old| !dependency_equal(old, dependency)) {
                        return Err("ambiguous mounted component geometry".into());
                    }
                    geometry = Some(dependency);
                    if ranges.len() == 1024 {
                        return Err("mounted component part capacity exceeded".into());
                    }
                    ranges.push(offset + part.rectangles.start..offset + part.rectangles.end);
                }
                offset += packet
                    .geometry
                    .as_ref()
                    .map_err(Clone::clone)?
                    .rectangle_count() as u32;
            }
            let geometry = geometry.ok_or("animated node has no mounted packet association")?;
            let pivot = [geometry.geometry.bounds.x, geometry.geometry.bounds.y];
            let b = geometry.clips.source;
            let source = if b.width == 0 || b.height == 0 {
                None
            } else {
                Some(ClipRect::new([b.x, b.y, b.width, b.height])?)
            };
            let b = geometry.clips.inherited;
            let parent = if b.width == 0 || b.height == 0 {
                None
            } else {
                Some(ClipRect::new([b.x, b.y, b.width, b.height])?)
            };
            bindings.push((node, ranges, pivot, source, parent));
        }
        let mut next = scene.component_candidate();
        next.retain_component_keys(screen, animated);
        if !self.extent.get().contains(&0) {
            for packet in &presented {
                next.append_geometry(packet.geometry.as_ref().map_err(Clone::clone)?)?;
            }
            for (node, ranges, pivot, source, parent) in &bindings {
                next.bind_component_clipped(
                    UiComponentKey {
                        screen,
                        node: *node,
                    },
                    ranges,
                    *pivot,
                    *source,
                    *parent,
                )?;
            }
        }
        // Reserve remembered targets and hit storage before scene publication.
        self.animated
            .borrow_mut()
            .try_reserve(animated.len())
            .map_err(|_| "component target allocation failed")?;
        let count: usize = presented.iter().map(|p| p.hits.len()).sum();
        hits.try_reserve(count)
            .map_err(|_| "component hit allocation failed")?;
        scene.publish_component_scene(next, screen);
        hits.clear();
        if !self.extent.get().contains(&0) {
            for packet in &presented {
                hits.extend_from_slice(&packet.hits);
            }
        }
        let mut targets = self.animated.borrow_mut();
        targets.clear();
        targets.extend_from_slice(animated);
        *self.presented.borrow_mut() = presented;
        self.dirty.set(false);
        Ok(())
    }
    pub(crate) fn hit_components(
        &self,
        scene: &Scene,
        screen: ScreenInstanceId,
        point: (f64, f64),
    ) -> Option<ControlId> {
        if self.extent.get().contains(&0) || scene.component_owner() != Some(screen) {
            return None;
        }
        for packet in self.presented.borrow().iter().rev() {
            if packet.parts.is_empty() {
                let point = scene.project_ui_point(point)?;
                if let Some((id, _)) = packet.hits.iter().rev().find(|(_, b)| b.contains(point)) {
                    return Some(*id);
                }
            } else {
                for part in packet.parts.iter().rev() {
                    let key = UiComponentKey {
                        screen,
                        node: part.node,
                    };
                    let projected = if let Some(id) = scene.component_live(key) {
                        scene.project_component_point(id, point)
                    } else {
                        scene.project_ui_point(point)
                    };
                    if let Some(point) = projected {
                        if let Some((id, _)) = packet.hits[part.hits.clone()]
                            .iter()
                            .rev()
                            .find(|(_, b)| b.contains(point))
                        {
                            return Some(*id);
                        }
                    }
                }
            }
        }
        None
    }
    #[cfg(test)]
    pub(crate) fn identities(&self) -> Vec<usize> {
        self.packets
            .iter()
            .map(|packet| Rc::as_ptr(packet) as usize)
            .collect()
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
        layout: None,
        parts: Vec::new(),
        #[cfg(test)]
        paints: 1,
    }
}

fn geometry_equal(a: LayoutGeometry, b: LayoutGeometry) -> bool {
    let tuple = |b: Bounds| (b.x, b.y, b.width, b.height);
    tuple(a.bounds) == tuple(b.bounds) && tuple(a.clip) == tuple(b.clip)
}
fn dependency_equal(a: LayoutDependency, b: LayoutDependency) -> bool {
    let tuple = |b: Bounds| (b.x, b.y, b.width, b.height);
    geometry_equal(a.geometry, b.geometry)
        && tuple(a.clips.source) == tuple(b.clips.source)
        && tuple(a.clips.inherited) == tuple(b.clips.inherited)
}
fn layout_geometries<C: Copy>(
    layout: &MountedLayout<C>,
    ids: &[NodeId],
) -> Result<Vec<LayoutDependency>, String> {
    if ids.len() > 1024 {
        return Err("retained dependency count exceeds layout capacity".into());
    }
    ids.iter()
        .map(|&id| {
            Ok(LayoutDependency {
                geometry: layout
                    .geometry(id)
                    .ok_or("retained node is outside layout")?,
                clips: layout
                    .component_clips(id)
                    .ok_or("retained component clips unavailable")?,
            })
        })
        .collect()
}
fn paint_layout_packet(
    extent: [u32; 2],
    ids: &[NodeId],
    geometries: &[LayoutDependency],
    animated: &[NodeId],
    paint: &impl Fn(NodeId, LayoutGeometry, &mut Scene, &mut Vec<(ControlId, Bounds)>),
) -> Packet {
    let mut scene = if animated.is_empty() {
        Scene::with_capacity(extent[0], extent[1], 64)
    } else {
        Scene::component_source(extent[0], extent[1], 64)
    };
    let mut hits = Vec::new();
    let mut parts = Vec::new();
    let result = (|| -> Result<(), String> {
        for (&id, &dependency) in ids.iter().zip(geometries) {
            let mut geometry = dependency.geometry;
            let source = animated.contains(&id);
            if source {
                geometry.clip = dependency.clips.source;
            }
            let first = scene.rectangles().len() as u32;
            let first_hit = hits.len();
            if geometry.clip.width == 0 || geometry.clip.height == 0 {
                parts.push(ComponentPart {
                    node: id,
                    rectangles: first..first,
                    hits: first_hit..first_hit,
                    geometry,
                    clips: dependency.clips,
                });
                continue;
            }
            let mut part = if source {
                Scene::component_source(extent[0], extent[1], 64)
            } else {
                Scene::with_capacity(extent[0], extent[1], 64)
            };
            let mut part_hits = Vec::new();
            paint(id, geometry, &mut part, &mut part_hits);
            part.status()?;
            let clip = ClipRect::new([
                geometry.clip.x,
                geometry.clip.y,
                geometry.clip.width,
                geometry.clip.height,
            ])?;
            for batch in part.batches() {
                for rectangle in
                    &part.rectangles()[batch.first as usize..(batch.first + batch.count) as usize]
                {
                    let color = rectangle
                        .color
                        .map(|channel| (channel * 255.0).round() as u8);
                    let tint = (u32::from(color[0]) << 16)
                        | (u32::from(color[1]) << 8)
                        | u32::from(color[2]);
                    scene.sprite_clipped_alpha(
                        batch.texture,
                        rectangle.bounds.map(|n| n as i64),
                        rectangle.uv,
                        tint,
                        color[3],
                        clip,
                    )?;
                }
            }
            for (control, bounds) in part_hits {
                let x = bounds.x.max(geometry.clip.x);
                let y = bounds.y.max(geometry.clip.y);
                let right = bounds
                    .x
                    .checked_add(bounds.width)
                    .ok_or("retained hit overflow")?
                    .min(geometry.clip.x + geometry.clip.width);
                let bottom = bounds
                    .y
                    .checked_add(bounds.height)
                    .ok_or("retained hit overflow")?
                    .min(geometry.clip.y + geometry.clip.height);
                if right > x && bottom > y {
                    hits.push((
                        control,
                        Bounds {
                            x,
                            y,
                            width: right - x,
                            height: bottom - y,
                        },
                    ));
                }
            }
            parts.push(ComponentPart {
                node: id,
                rectangles: first..scene.rectangles().len() as u32,
                hits: first_hit..hits.len(),
                geometry: dependency.geometry,
                clips: dependency.clips,
            });
        }
        Ok(())
    })();
    Packet {
        geometry: result.and_then(|_| scene.geometry_snapshot()),
        hits,
        layout: None,
        parts,
        #[cfg(test)]
        paints: 1,
    }
}

#[cfg(test)]
#[path = "retained_layout_fixtures.rs"]
mod retained_layout_fixtures;

#[cfg(test)]
mod fixtures {
    use super::*;
    use floem_reactive::SignalUpdate;
    #[test]
    fn initial_and_reactive_packet_errors_remain_dirty_until_recovered_composition() {
        let scope = Scope::new();
        let overflow = scope.create_rw_signal(true);
        let memo = scope.create_memo(move |_| overflow.get());
        let mut nodes = RetainedNodes::new(960, 720).unwrap();
        nodes.static_node(|scene, hits| {
            scene.rect(0, 0, 10, 10, 0);
            hits.push((
                ControlId(1),
                Bounds {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            ));
        });
        nodes.bind(scope, memo, |overflow, scene, hits| {
            let count = if overflow {
                crate::scene::MAX_RECTANGLES + 1
            } else {
                1
            };
            for _ in 0..count {
                scene.rect(10, 0, 10, 10, 0xffffff);
            }
            hits.push((
                ControlId(2),
                Bounds {
                    x: 10,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            ));
        });
        assert!(nodes.validate().is_err());
        assert!(nodes.dirty());
        let mut scene = Scene::with_capacity(960, 720, 64);
        let mut hits = Vec::new();
        assert!(nodes.compose(&mut scene, &mut hits).is_err());
        assert!(nodes.dirty());
        let identities = nodes.identities();
        overflow.set(false);
        assert!(nodes.validate().is_ok());
        nodes.compose(&mut scene, &mut hits).unwrap();
        assert!(!nodes.dirty());
        assert_eq!(nodes.identities(), identities);
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(scene.rectangles()[1].bounds, [10.0, 0.0, 10.0, 10.0]);
        assert_eq!(
            hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            vec![1, 2]
        );
        scope.dispose();
    }
}
