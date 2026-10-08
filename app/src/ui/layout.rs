//! Mount-time, statically typed layout. Resolved leaves supply both paint and hit bounds.
use super::interaction::Bounds;

/// Bitmap text style; components decide which state supplies the text value.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub scale: usize,
    pub color: u32,
}

#[derive(Clone, Copy)]
pub struct Node<'a, T> {
    pub size: [i64; 2],
    pub content: Content<'a, T>,
    fill: [bool; 2],
    clipped: bool,
}
#[derive(Clone, Copy)]
pub enum Content<'a, T> {
    Leaf(T),
    Row {
        gap: i64,
        children: &'a [Node<'a, T>],
    },
    Column {
        gap: i64,
        children: &'a [Node<'a, T>],
    },
    Layer(&'a [Placed<'a, T>]),
}
#[derive(Clone, Copy)]
pub struct Placed<'a, T> {
    pub origin: [i64; 2],
    pub node: Node<'a, T>,
}
#[derive(Clone, Copy)]
pub struct Resolved<T> {
    pub component: T,
    pub bounds: Bounds,
}
impl<'a, T> Node<'a, T> {
    pub const fn leaf(size: [i64; 2], component: T) -> Self {
        Self {
            size,
            content: Content::Leaf(component),
            fill: [false; 2],
            clipped: false,
        }
    }
    pub const fn row(size: [i64; 2], gap: i64, children: &'a [Self]) -> Self {
        Self {
            size,
            content: Content::Row { gap, children },
            fill: [false; 2],
            clipped: false,
        }
    }
    pub const fn column(size: [i64; 2], gap: i64, children: &'a [Self]) -> Self {
        Self {
            size,
            content: Content::Column { gap, children },
            fill: [false; 2],
            clipped: false,
        }
    }
    pub const fn layer(size: [i64; 2], children: &'a [Placed<'a, T>]) -> Self {
        Self {
            size,
            content: Content::Layer(children),
            fill: [false; 2],
            clipped: false,
        }
    }
    pub const fn at(self, x: i64, y: i64) -> Placed<'a, T> {
        Placed {
            origin: [x, y],
            node: self,
        }
    }
    /// Permit overflowing child allocations, retaining the inherited paint/hit clip.
    pub const fn clipped(mut self) -> Self {
        self.clipped = true;
        self
    }
    /// Stretch selected axes into the parent's remaining allocation.
    pub const fn fill(mut self, axes: [bool; 2]) -> Self {
        self.fill = axes;
        self
    }
}
/// Stable mount-local identity, including container nodes in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(pub usize);
#[derive(Clone, Copy, Debug)]
pub struct LayoutGeometry {
    pub bounds: Bounds,
    pub clip: Bounds,
}
/// Component-local content moves with the node. Ancestor clipping remains in
/// the destination coordinate space; neither changes the legacy published clip.
#[derive(Clone, Copy, Debug)]
pub struct ComponentClips {
    pub source: Bounds,
    pub inherited: Bounds,
}
#[derive(Clone, Copy)]
pub struct MountedLeaf<T> {
    pub id: NodeId,
    pub component: T,
    pub geometry: LayoutGeometry,
}
#[derive(Clone, Copy)]
pub enum LayoutChange {
    Size([i64; 2]),
    Origin([i64; 2]),
    Gap(i64),
    /// A local clip; descendants inherit its intersection with the parent clip.
    Clip(Option<Bounds>),
}
#[derive(Clone, Copy)]
pub struct LayoutUpdate {
    pub id: NodeId,
    pub change: LayoutChange,
}
#[derive(Clone)]
struct MountedNode<T> {
    component: Option<T>,
    size: [i64; 2],
    fill: [bool; 2],
    clipped: bool,
    origin: [i64; 2],
    axis: Option<usize>,
    gap: i64,
    local_clip: Option<Bounds>,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    geometry: LayoutGeometry,
}
/// The declaration is consumed once. Explicit edits stage properties and reflow
/// dependent branches; normal frames borrow the already published leaves.
#[derive(Clone)]
pub struct MountedLayout<T> {
    nodes: Vec<MountedNode<T>>,
    leaves: Vec<MountedLeaf<T>>,
    extent: [u32; 2],
    revision: u64,
    changed: Vec<NodeId>,
}
fn same_bounds(a: Bounds, b: Bounds) -> bool {
    (a.x, a.y, a.width, a.height) == (b.x, b.y, b.width, b.height)
}
fn same_geometry(a: LayoutGeometry, b: LayoutGeometry) -> bool {
    same_bounds(a.bounds, b.bounds) && same_bounds(a.clip, b.clip)
}
fn intersection(a: Bounds, b: Bounds) -> Bounds {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Bounds {
        x,
        y,
        width: (a.x + a.width).min(b.x + b.width).saturating_sub(x).max(0),
        height: (a.y + a.height)
            .min(b.y + b.height)
            .saturating_sub(y)
            .max(0),
    }
}
fn checked_bounds(origin: [i64; 2], size: [i64; 2]) -> Result<Bounds, String> {
    if origin.iter().any(|&n| n < 0) || size.iter().any(|&n| n <= 0) {
        return Err("layout requires positive extents and nonnegative origins".into());
    }
    origin[0]
        .checked_add(size[0])
        .ok_or("layout right overflow")?;
    origin[1]
        .checked_add(size[1])
        .ok_or("layout bottom overflow")?;
    Ok(Bounds {
        x: origin[0],
        y: origin[1],
        width: size[0],
        height: size[1],
    })
}
impl<T: Copy> MountedLayout<T> {
    pub fn mount(root: Node<'_, T>) -> Result<Self, String> {
        fn collect<T: Copy>(
            node: Node<'_, T>,
            origin: [i64; 2],
            parent: Option<NodeId>,
            depth: usize,
            nodes: &mut Vec<MountedNode<T>>,
        ) -> Result<NodeId, String> {
            if depth > 32 || nodes.len() >= 1024 {
                return Err("layout exceeds depth/node limit".into());
            }
            let id = NodeId(nodes.len());
            let (component, axis, gap) = match node.content {
                Content::Leaf(component) => (Some(component), None, 0),
                Content::Row { gap, .. } => (None, Some(0), gap),
                Content::Column { gap, .. } => (None, Some(1), gap),
                Content::Layer(_) => (None, None, 0),
            };
            if gap < 0 {
                return Err("layout gap must be nonnegative".into());
            }
            checked_bounds(origin, node.size)?;
            nodes
                .try_reserve(1)
                .map_err(|_| "layout allocation failed")?;
            nodes.push(MountedNode {
                component,
                size: node.size,
                fill: node.fill,
                clipped: node.clipped,
                origin,
                axis,
                gap,
                local_clip: None,
                parent,
                children: Vec::new(),
                geometry: LayoutGeometry {
                    bounds: checked_bounds([0, 0], node.size)?,
                    clip: checked_bounds([0, 0], node.size)?,
                },
            });
            match node.content {
                Content::Leaf(_) => {}
                Content::Layer(children) => {
                    for child in children {
                        let child = collect(child.node, child.origin, Some(id), depth + 1, nodes)?;
                        nodes[id.0]
                            .children
                            .try_reserve(1)
                            .map_err(|_| "layout allocation failed")?;
                        nodes[id.0].children.push(child);
                    }
                }
                Content::Row { children, .. } | Content::Column { children, .. } => {
                    for child in children {
                        let child = collect(*child, [0, 0], Some(id), depth + 1, nodes)?;
                        nodes[id.0]
                            .children
                            .try_reserve(1)
                            .map_err(|_| "layout allocation failed")?;
                        nodes[id.0].children.push(child);
                    }
                }
            }
            Ok(id)
        }
        let extent = [
            u32::try_from(root.size[0]).map_err(|_| "layout width exceeds viewport")?,
            u32::try_from(root.size[1]).map_err(|_| "layout height exceeds viewport")?,
        ];
        let mut nodes = Vec::new();
        collect(root, [0, 0], None, 0, &mut nodes)?;
        let mut mounted = Self {
            nodes,
            leaves: Vec::new(),
            extent,
            revision: 0,
            changed: Vec::new(),
        };
        mounted.reflow(&vec![true; mounted.nodes.len()])?;
        mounted.publish_leaves();
        mounted.changed.clear();
        Ok(mounted)
    }
    pub fn leaves(&self) -> &[MountedLeaf<T>] {
        &self.leaves
    }
    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.nodes.get(id.0).map(|node| node.children.as_slice())
    }
    pub fn geometry(&self, id: NodeId) -> Option<LayoutGeometry> {
        self.nodes.get(id.0).map(|node| node.geometry)
    }
    pub fn component_clips(&self, id: NodeId) -> Option<ComponentClips> {
        let node = self.nodes.get(id.0)?;
        let bounds = node.geometry.bounds;
        let source = if let Some(local) = node.local_clip {
            intersection(
                bounds,
                Bounds {
                    x: bounds.x.checked_add(local.x)?,
                    y: bounds.y.checked_add(local.y)?,
                    width: local.width,
                    height: local.height,
                },
            )
        } else {
            bounds
        };
        let inherited = node.parent.map_or(node.geometry.bounds, |parent| {
            self.nodes[parent.0].geometry.clip
        });
        Some(ComponentClips { source, inherited })
    }
    pub const fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn suspended(&self) -> bool {
        self.extent.contains(&0)
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    pub fn changed_nodes(&self) -> &[NodeId] {
        &self.changed
    }
    pub fn resize(&mut self, extent: [u32; 2]) -> Result<bool, String> {
        self.edit(&[], Some(extent))
    }
    pub fn update(&mut self, updates: &[LayoutUpdate]) -> Result<bool, String> {
        self.edit(updates, None)
    }
    /// Stages an extent and property batch together, so shrink/reallocation cannot
    /// temporarily publish incompatible child dimensions.
    pub fn update_on_extent(
        &mut self,
        extent: [u32; 2],
        updates: &[LayoutUpdate],
    ) -> Result<bool, String> {
        self.edit(updates, Some(extent))
    }
    fn edit(&mut self, updates: &[LayoutUpdate], extent: Option<[u32; 2]>) -> Result<bool, String> {
        if updates.len() > self.nodes.len() * 4 {
            return Err("layout update exceeds property capacity".into());
        }
        if updates.is_empty() && extent.is_none_or(|extent| extent == self.extent) {
            return Ok(false);
        }
        let mut next = self.clone();
        let mut dirty = vec![false; self.nodes.len()];
        next.changed.clear();
        if let Some(extent) = extent {
            next.extent = extent;
        }
        if next.extent != self.extent && !next.suspended() {
            next.nodes[0].size = next.extent.map(i64::from);
            dirty[0] = true;
        }
        let mut properties_changed = next.extent != self.extent;
        for update in updates {
            let node = next
                .nodes
                .get_mut(update.id.0)
                .ok_or("layout node is outside mount")?;
            let changed = match update.change {
                LayoutChange::Size(size) => {
                    checked_bounds([0, 0], size)?;
                    let changed = node.size != size;
                    node.size = size;
                    changed
                }
                LayoutChange::Origin(origin) => {
                    if update.id.0 == 0
                        || node
                            .parent
                            .is_some_and(|id| self.nodes[id.0].axis.is_some())
                    {
                        return Err("layout origin requires a positioned child".into());
                    }
                    checked_bounds(origin, node.size)?;
                    let changed = node.origin != origin;
                    node.origin = origin;
                    changed
                }
                LayoutChange::Gap(gap) => {
                    if node.axis.is_none() || gap < 0 {
                        return Err("layout gap requires a nonnegative flow gap".into());
                    }
                    let changed = node.gap != gap;
                    node.gap = gap;
                    changed
                }
                LayoutChange::Clip(clip) => {
                    if let Some(clip) = clip {
                        checked_bounds([clip.x, clip.y], [clip.width, clip.height])?;
                    }
                    let changed = match (node.local_clip, clip) {
                        (Some(a), Some(b)) => !same_bounds(a, b),
                        (None, None) => false,
                        _ => true,
                    };
                    node.local_clip = clip;
                    changed
                }
            };
            if changed {
                properties_changed = true;
                let mut id = Some(update.id);
                while let Some(current) = id {
                    dirty[current.0] = true;
                    id = next.nodes[current.0].parent;
                }
            }
        }
        if !properties_changed {
            return Ok(false);
        }
        if !next.suspended() {
            next.reflow(&dirty)?;
            next.publish_leaves();
        } else if dirty.iter().any(|&changed| changed) {
            // Hidden edits still validate against the retained allocation.
            let suspended_extent = next.extent;
            next.extent = [
                u32::try_from(next.nodes[0].size[0])
                    .map_err(|_| "layout width exceeds viewport")?,
                u32::try_from(next.nodes[0].size[1])
                    .map_err(|_| "layout height exceeds viewport")?,
            ];
            next.reflow(&dirty)?;
            next.publish_leaves();
            next.extent = suspended_extent;
        }
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or("layout revision exhausted")?;
        *self = next;
        Ok(true)
    }
    fn reflow(&mut self, dirty: &[bool]) -> Result<(), String> {
        fn visit<T: Copy>(
            layout: &mut MountedLayout<T>,
            id: NodeId,
            origin: [i64; 2],
            parent: LayoutGeometry,
            dirty: &[bool],
            force: bool,
        ) -> Result<(), String> {
            let node = &layout.nodes[id.0];
            let mut size = node.size;
            for axis in 0..2 {
                if node.fill[axis] {
                    size[axis] = [parent.bounds.width, parent.bounds.height][axis]
                        .checked_sub(origin[axis])
                        .ok_or("layout fill overflow")?;
                }
            }
            let bounds = checked_bounds(
                [
                    parent
                        .bounds
                        .x
                        .checked_add(origin[0])
                        .ok_or("layout x overflow")?,
                    parent
                        .bounds
                        .y
                        .checked_add(origin[1])
                        .ok_or("layout y overflow")?,
                ],
                size,
            )?;
            if bounds.x + bounds.width > parent.bounds.x + parent.bounds.width
                || bounds.y + bounds.height > parent.bounds.y + parent.bounds.height
            {
                if !node.parent.is_some_and(|id| layout.nodes[id.0].clipped) {
                    return Err("layout child exceeds its region".into());
                }
            }
            let mut clip = intersection(parent.clip, bounds);
            if let Some(local) = node.local_clip {
                let local = checked_bounds(
                    [
                        bounds
                            .x
                            .checked_add(local.x)
                            .ok_or("layout clip overflow")?,
                        bounds
                            .y
                            .checked_add(local.y)
                            .ok_or("layout clip overflow")?,
                    ],
                    [local.width, local.height],
                )?;
                clip = intersection(clip, local);
            }
            let geometry = LayoutGeometry { bounds, clip };
            let changed = !same_geometry(node.geometry, geometry);
            if !force && !dirty[id.0] && !changed {
                return Ok(());
            }
            if changed {
                layout.changed.push(id);
            }
            layout.nodes[id.0].geometry = geometry;
            let axis = layout.nodes[id.0].axis;
            let gap = layout.nodes[id.0].gap;
            let mut offset = [0i64; 2];
            // Borrow children by index: identities and hierarchy never change.
            for index in 0..layout.nodes[id.0].children.len() {
                let child = layout.nodes[id.0].children[index];
                let origin = if let Some(axis) = axis {
                    if index != 0 {
                        offset[axis] =
                            offset[axis].checked_add(gap).ok_or("layout gap overflow")?;
                    }
                    offset
                } else {
                    layout.nodes[child.0].origin
                };
                visit(layout, child, origin, geometry, dirty, changed || force)?;
                if let Some(axis) = axis {
                    let child_bounds = layout.nodes[child.0].geometry.bounds;
                    offset[axis] = offset[axis]
                        .checked_add([child_bounds.width, child_bounds.height][axis])
                        .ok_or("layout extent overflow")?;
                }
            }
            Ok(())
        }
        let bounds = checked_bounds([0, 0], self.extent.map(i64::from))?;
        visit(
            self,
            NodeId(0),
            [0, 0],
            LayoutGeometry {
                bounds,
                clip: bounds,
            },
            dirty,
            false,
        )
    }
    fn publish_leaves(&mut self) {
        self.leaves.clear();
        self.leaves
            .extend(self.nodes.iter().enumerate().filter_map(|(index, node)| {
                node.component.map(|component| MountedLeaf {
                    id: NodeId(index),
                    component,
                    geometry: node.geometry,
                })
            }));
    }
}
/// Compatibility mount-time leaf resolution.
pub fn resolve<T: Copy>(root: Node<'_, T>) -> Result<Vec<Resolved<T>>, String> {
    Ok(MountedLayout::mount(root)?
        .leaves
        .into_iter()
        .map(|leaf| Resolved {
            component: leaf.component,
            bounds: leaf.geometry.bounds,
        })
        .collect())
}

#[cfg(test)]
#[path = "layout_fixtures.rs"]
mod fixtures;
