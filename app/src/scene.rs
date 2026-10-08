//! Bounded, ordered geometry shared by native and browser renderers.

pub const MAX_RECTANGLES: usize = 65_536;
use crate::playfield_gpu::{PlayfieldCache, PlayfieldFrame, MAX_PLAYFIELDS};
use crate::texture::TextureId;
use crate::{screen_lifecycle::ScreenInstanceId, ui::layout::NodeId};
use std::ops::Range;
use std::sync::Arc;

/// Immutable per-call clipping rectangle with checked exclusive endpoints.
/// Signed origins are allowed; dimensions must be positive and representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipRect {
    endpoints: [i64; 4],
}
impl ClipRect {
    pub fn new(bounds: [i64; 4]) -> Result<Self, String> {
        let [x, y, width, height] = bounds;
        if width <= 0 || height <= 0 {
            return Err("clip rectangle dimensions must be positive".into());
        }
        let right = x
            .checked_add(width)
            .ok_or("clip rectangle right endpoint overflow")?;
        let bottom = y
            .checked_add(height)
            .ok_or("clip rectangle bottom endpoint overflow")?;
        Ok(Self {
            endpoints: [x, y, right, bottom],
        })
    }
}

#[derive(Clone)]
pub(crate) struct DrawBatch {
    pub texture: TextureId,
    pub first: u32,
    pub count: u32,
    pub playfield: Option<usize>,
    pub component: u32,
}

pub const MAX_UI_COMPONENTS: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTransform {
    offset: [f32; 2],
    scale: [f32; 2],
    opacity: f32,
}
impl Default for UiTransform {
    fn default() -> Self {
        Self {
            offset: [0.0; 2],
            scale: [1.0; 2],
            opacity: 1.0,
        }
    }
}
impl UiTransform {
    pub fn new(offset: [f32; 2], scale: [f32; 2], opacity: f32) -> Result<Self, String> {
        if offset
            .iter()
            .any(|v| !v.is_finite() || v.abs() > (1 << 24) as f32)
            || scale
                .iter()
                .any(|v| !v.is_finite() || !(1.0 / 16.0..=16.0).contains(v))
            || !opacity.is_finite()
            || !(0.0..=1.0).contains(&opacity)
        {
            return Err("invalid bounded UI component transform".into());
        }
        Ok(Self {
            offset,
            scale,
            opacity,
        })
    }
    pub const fn offset(self) -> [f32; 2] {
        self.offset
    }
    pub const fn scale(self) -> [f32; 2] {
        self.scale
    }
    pub const fn opacity(self) -> f32 {
        self.opacity
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiComponentKey {
    pub screen: ScreenInstanceId,
    pub node: NodeId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiComponentId {
    slot: usize,
    key: UiComponentKey,
    epoch: u64,
}
impl UiComponentId {
    pub const fn key(self) -> UiComponentKey {
        self.key
    }
}
#[derive(Clone)]
struct ComponentBinding {
    key: UiComponentKey,
    transform: UiTransform,
    source: Option<ClipRect>,
    parent: Option<ClipRect>,
    pivot: [i64; 2],
    live: bool,
    epoch: u64,
}

/// Input pose committed by the presentation owner only after a painted frame.
#[derive(Clone)]
pub struct UiPresentedPose {
    width: u32,
    height: u32,
    translation: UiTranslation,
    components: [Option<ComponentBinding>; MAX_UI_COMPONENTS],
}
impl UiPresentedPose {
    pub fn project(&self, key: Option<UiComponentKey>, point: (f64, f64)) -> Option<(f64, f64)> {
        if let Some(binding) = key.and_then(|key| {
            self.components
                .iter()
                .flatten()
                .find(|b| b.live && b.key == key)
        }) {
            return project_component(self.width, self.height, self.translation, binding, point);
        }
        let inside = |(x, y): (f64, f64)| {
            x.is_finite()
                && y.is_finite()
                && x >= 0.0
                && y >= 0.0
                && x < f64::from(self.width)
                && y < f64::from(self.height)
        };
        if !inside(point) {
            return None;
        }
        let local = (
            point.0 - f64::from(self.translation.x),
            point.1 - f64::from(self.translation.y),
        );
        inside(local).then_some(local)
    }
}
fn project_component(
    width: u32,
    height: u32,
    translation: UiTranslation,
    binding: &ComponentBinding,
    point: (f64, f64),
) -> Option<(f64, f64)> {
    if !point.0.is_finite()
        || !point.1.is_finite()
        || point.0 < 0.0
        || point.1 < 0.0
        || point.0 >= f64::from(width)
        || point.1 >= f64::from(height)
    {
        return None;
    }
    if binding.transform.opacity == 0.0 {
        return None;
    }
    let point = (
        point.0 - f64::from(translation.x),
        point.1 - f64::from(translation.y),
    );
    let inside = |p: (f64, f64), clip: [i64; 4]| {
        p.0.is_finite()
            && p.1.is_finite()
            && p.0 >= clip[0] as f64
            && p.0 < clip[2] as f64
            && p.1 >= clip[1] as f64
            && p.1 < clip[3] as f64
    };
    if !inside(point, [0, 0, i64::from(width), i64::from(height)])
        || !inside(point, binding.parent?.endpoints)
    {
        return None;
    }
    let t = binding.transform;
    let source = binding.source?.endpoints;
    let pivot = binding.pivot;
    let inverse = (
        (point.0 - pivot[0] as f64 - f64::from(t.offset[0])) / f64::from(t.scale[0])
            + pivot[0] as f64,
        (point.1 - pivot[1] as f64 - f64::from(t.offset[1])) / f64::from(t.scale[1])
            + pivot[1] as f64,
    );
    inside(inverse, source).then_some(inverse)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Rectangle {
    pub bounds: [f32; 4],
    pub color: [f32; 4],
    pub uv: [f32; 4],
}

/// Immutable ordered UI geometry for one retained component. Timed playfields
/// remain separate; these packets never own transport or native resources.
#[derive(Clone)]
pub struct GeometrySnapshot {
    width: u32,
    height: u32,
    rectangles: Arc<[Rectangle]>,
    batches: Arc<[DrawBatch]>,
}
impl GeometrySnapshot {
    pub(crate) fn rectangle_count(&self) -> usize {
        self.rectangles.len()
    }
}

/// Integer translation of the ordinary UI surface, separate from timed notes.
/// Offsets are bounded so their GPU uniform representation is exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiTranslation {
    x: i32,
    y: i32,
}
impl UiTranslation {
    pub const MAX_OFFSET: i32 = 1 << 24;

    pub fn new(x: i32, y: i32) -> Result<Self, String> {
        if !(-Self::MAX_OFFSET..=Self::MAX_OFFSET).contains(&x)
            || !(-Self::MAX_OFFSET..=Self::MAX_OFFSET).contains(&y)
        {
            return Err("UI translation exceeds exact integer GPU offset range".into());
        }
        Ok(Self { x, y })
    }

    pub fn offset(self) -> [i32; 2] {
        [self.x, self.y]
    }
}

pub struct Scene {
    width: u32,
    height: u32,
    rectangles: Vec<Rectangle>,
    batches: Vec<DrawBatch>,
    overflow: bool,
    error: Option<String>,
    playfields: Vec<PlayfieldFrame>,
    playfield_caches: Vec<PlayfieldCache>,
    // Lazy backing storage: static retained nodes never run a note query.
    visible_note_indices: Vec<usize>,
    visible_mine_indices: Vec<usize>,
    geometry_identity: Arc<()>,
    geometry_epoch: u64,
    ui_translation: UiTranslation,
    components: [Option<ComponentBinding>; MAX_UI_COMPONENTS],
    component_owner: Option<ScreenInstanceId>,
    next_component_epoch: u64,
    source_geometry: bool,
}

impl Scene {
    pub fn logical_extent(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub fn new(width: u32, height: u32) -> Self {
        Self::with_capacity(width, height, MAX_RECTANGLES)
    }

    /// Small retained components need only their own initial allocation. The
    /// same hard geometry limit applies regardless of the initial capacity.
    pub fn with_capacity(width: u32, height: u32, capacity: usize) -> Self {
        Self {
            width,
            height,
            rectangles: Vec::with_capacity(capacity.min(MAX_RECTANGLES)),
            batches: Vec::with_capacity(capacity.min(MAX_RECTANGLES)),
            overflow: false,
            error: None,
            playfields: Vec::with_capacity(MAX_PLAYFIELDS),
            playfield_caches: (0..MAX_PLAYFIELDS)
                .map(|_| PlayfieldCache::default())
                .collect(),
            visible_note_indices: Vec::new(),
            visible_mine_indices: Vec::new(),
            geometry_identity: Arc::new(()),
            geometry_epoch: 0,
            ui_translation: UiTranslation::default(),
            components: std::array::from_fn(|_| None),
            component_owner: None,
            next_component_epoch: 1,
            source_geometry: false,
        }
    }

    pub fn clear(&mut self) {
        self.source_geometry = false;
        self.component_owner = None;
        for component in self.components.iter_mut().flatten() {
            component.live = false;
        }
        self.ui_translation = UiTranslation::default();
        self.geometry_changed();
        self.rectangles.clear();
        self.batches.clear();
        self.overflow = false;
        self.error = None;
        self.playfields.clear();
        self.visible_note_indices.clear();
        self.visible_mine_indices.clear();
    }

    fn geometry_changed(&mut self) {
        if let Some(next) = self.geometry_epoch.checked_add(1) {
            self.geometry_epoch = next;
        } else {
            // Never alias an earlier upload when the counter is exhausted.
            self.geometry_identity = Arc::new(());
            self.geometry_epoch = 0;
        }
    }

    pub fn geometry_stamp(&self) -> (&Arc<()>, u64) {
        (&self.geometry_identity, self.geometry_epoch)
    }

    pub fn ui_translation(&self) -> UiTranslation {
        self.ui_translation
    }

    /// Changes presentation only: retained geometry and its upload identity stay intact.
    pub fn set_ui_translation(&mut self, translation: UiTranslation) {
        self.ui_translation = translation;
    }

    pub fn component_id(&self, key: UiComponentKey) -> Option<UiComponentId> {
        self.components
            .iter()
            .position(|v| v.as_ref().is_some_and(|v| v.key == key))
            .map(|slot| UiComponentId {
                slot,
                key,
                epoch: self.components[slot].as_ref().unwrap().epoch,
            })
    }
    pub fn component_transform(&self, id: UiComponentId) -> Option<UiTransform> {
        self.components
            .get(id.slot)?
            .as_ref()
            .filter(|v| v.key == id.key && v.epoch == id.epoch)
            .map(|v| v.transform)
    }
    pub fn bind_component(
        &mut self,
        key: UiComponentKey,
        ranges: &[Range<u32>],
        source_clip: ClipRect,
        parent_clip: Option<ClipRect>,
    ) -> Result<UiComponentId, String> {
        self.bind_component_clipped(
            key,
            ranges,
            [source_clip.endpoints[0], source_clip.endpoints[1]],
            Some(source_clip),
            parent_clip,
        )
    }
    /// Explicit node-bounds pivot preserves scaling when the node's local clip
    /// begins inside its allocation. Empty source/ancestor clips remain hidden.
    pub fn bind_component_clipped(
        &mut self,
        key: UiComponentKey,
        ranges: &[Range<u32>],
        pivot: [i64; 2],
        source_clip: Option<ClipRect>,
        parent_clip: Option<ClipRect>,
    ) -> Result<UiComponentId, String> {
        self.status()?;
        if key.screen.0 == 0
            || key.node.0 >= 1024
            || ranges.len() > 1024
            || ranges
                .iter()
                .any(|r| r.start > r.end || r.end as usize > self.rectangles.len())
            || ranges.windows(2).any(|pair| pair[0].end > pair[1].start)
        {
            return Err("invalid bounded UI component binding".into());
        }
        let slot = self
            .component_id(key)
            .map(|id| id.slot)
            .or_else(|| self.components.iter().position(Option::is_none))
            .ok_or("UI component capacity exhausted")?;
        let component = (slot + 1) as u32;
        let epoch = self.components[slot]
            .as_ref()
            .map_or(self.next_component_epoch, |v| v.epoch);
        let next_epoch = if self.components[slot].is_none() {
            self.next_component_epoch
                .checked_add(1)
                .ok_or("UI component identity exhausted")?
        } else {
            self.next_component_epoch
        };
        for batch in &self.batches {
            for range in ranges {
                if batch.playfield.is_some() && range.start < batch.first && batch.first < range.end
                {
                    return Err("UI component cannot contain an indexed note layer".into());
                }
                if batch.playfield.is_none()
                    && batch.first < range.end
                    && range.start < batch.first + batch.count
                    && batch.component != 0
                    && batch.component != component
                {
                    return Err("UI component ranges overlap another component".into());
                }
            }
        }
        let mut batches = Vec::new();
        batches
            .try_reserve_exact(self.batches.len() + ranges.len() * 2)
            .map_err(|_| "UI component batch allocation failed")?;
        for batch in &self.batches {
            if batch.playfield.is_some() {
                batches.push(batch.clone());
                continue;
            }
            let end = batch.first + batch.count;
            let mut first = batch.first;
            while first < end {
                let covered = ranges.iter().find(|r| r.start <= first && first < r.end);
                let last = covered
                    .map_or_else(
                        || {
                            ranges
                                .iter()
                                .filter(|r| r.start > first)
                                .map(|r| r.start)
                                .min()
                                .unwrap_or(end)
                        },
                        |r| r.end,
                    )
                    .min(end);
                let mut next = batch.clone();
                next.first = first;
                next.count = last - first;
                if covered.is_some() {
                    next.component = component;
                }
                batches.push(next);
                first = last;
            }
        }
        let transform = self.components[slot]
            .as_ref()
            .map_or(UiTransform::default(), |v| v.transform);
        self.components[slot] = Some(ComponentBinding {
            key,
            transform,
            source: source_clip,
            parent: parent_clip,
            pivot,
            live: true,
            epoch,
        });
        self.next_component_epoch = next_epoch;
        self.batches = batches;
        Ok(UiComponentId { slot, key, epoch })
    }
    pub fn set_component_transforms(
        &mut self,
        updates: &[(UiComponentId, UiTransform)],
    ) -> Result<bool, String> {
        self.status()?;
        if updates.len() > MAX_UI_COMPONENTS
            || updates.iter().enumerate().any(|(i, (id, _))| {
                self.component_transform(*id).is_none()
                    || updates[..i].iter().any(|(other, _)| other == id)
            })
        {
            return Err("invalid or stale UI component update batch".into());
        }
        let changed = updates
            .iter()
            .any(|(id, value)| self.component_transform(*id) != Some(*value));
        for (id, value) in updates {
            self.components[id.slot].as_mut().unwrap().transform = *value;
        }
        Ok(changed)
    }
    pub fn project_component_point(
        &self,
        id: UiComponentId,
        point: (f64, f64),
    ) -> Option<(f64, f64)> {
        let binding = self
            .components
            .get(id.slot)?
            .as_ref()
            .filter(|v| v.key == id.key && v.epoch == id.epoch && v.live)?;
        project_component(self.width, self.height, self.ui_translation, binding, point)
    }
    /// Only bounded paint/input metadata is copied; geometry and note caches stay local.
    pub fn presented_pose(&self) -> UiPresentedPose {
        UiPresentedPose {
            width: self.width,
            height: self.height,
            translation: self.ui_translation,
            components: self.components.clone(),
        }
    }
    pub fn dispose_components(&mut self, screen: ScreenInstanceId) -> usize {
        let mut removed = 0;
        for (slot, value) in self.components.iter_mut().enumerate() {
            if value.as_ref().is_some_and(|v| v.key.screen == screen) {
                *value = None;
                removed += 1;
                for batch in &mut self.batches {
                    if batch.component == (slot + 1) as u32 {
                        batch.component = 0;
                    }
                }
            }
        }
        if self.component_owner == Some(screen) {
            self.component_owner = None;
        }
        removed
    }
    pub(crate) fn component_gpu_uniform(&self, slot: usize) -> [f32; 16] {
        let Some(binding) = slot
            .checked_sub(1)
            .and_then(|slot| self.components.get(slot))
            .and_then(Option::as_ref)
        else {
            return [
                0.0,
                0.0,
                1.0,
                1.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                self.width as f32,
                self.height as f32,
                0.0,
                0.0,
                self.width as f32,
                self.height as f32,
            ];
        };
        let t = binding.transform;
        let parent = binding.parent.map_or([0; 4], |clip| clip.endpoints);
        let source = binding.source.map_or([0; 4], |clip| clip.endpoints);
        [
            t.offset[0],
            t.offset[1],
            t.scale[0],
            t.scale[1],
            binding.pivot[0] as f32,
            binding.pivot[1] as f32,
            t.opacity,
            0.0,
            parent[0] as f32,
            parent[1] as f32,
            parent[2] as f32,
            parent[3] as f32,
            source[0] as f32,
            source[1] as f32,
            source[2] as f32,
            source[3] as f32,
        ]
    }
    pub(crate) fn component_uniform_live(&self, slot: usize) -> bool {
        slot == 0
            || self
                .components
                .get(slot - 1)
                .and_then(Option::as_ref)
                .is_some_and(|v| v.live)
    }
    pub(crate) fn component_owner(&self) -> Option<ScreenInstanceId> {
        self.component_owner
    }
    pub(crate) fn component_live(&self, key: UiComponentKey) -> Option<UiComponentId> {
        let id = self.component_id(key)?;
        self.components[id.slot].as_ref()?.live.then_some(id)
    }
    pub(crate) fn component_candidate(&self) -> Self {
        let mut next = Self::with_capacity(self.width, self.height, self.rectangles.len());
        next.components = self.components.clone();
        next.next_component_epoch = self.next_component_epoch;
        for value in next.components.iter_mut().flatten() {
            value.live = false;
        }
        next
    }
    pub fn retain_component_keys(&mut self, screen: ScreenInstanceId, nodes: &[NodeId]) {
        for component in &mut self.components {
            if component
                .as_ref()
                .is_some_and(|b| b.key.screen != screen || !nodes.contains(&b.key.node))
            {
                *component = None;
            }
        }
    }
    /// Cold component source capture defers viewport cropping until the GPU
    /// applies the node transform and fixed ancestor clip. Ordinary scenes keep
    /// their existing CPU clipping behavior.
    pub(crate) fn component_source(width: u32, height: u32, capacity: usize) -> Self {
        let mut scene = Self::with_capacity(width, height, capacity);
        scene.source_geometry = true;
        scene
    }
    pub(crate) fn publish_component_scene(&mut self, next: Self, owner: ScreenInstanceId) {
        self.rectangles = next.rectangles;
        self.batches = next.batches;
        self.components = next.components;
        self.next_component_epoch = next.next_component_epoch;
        self.ui_translation = next.ui_translation;
        self.playfields.clear();
        self.error = next.error;
        self.overflow = next.overflow;
        self.component_owner = Some(owner);
        self.geometry_changed();
    }

    /// Inverse presentation projection for ordinary retained UI hit bounds.
    /// Both the visible viewport and the original clipped surface exclude far edges.
    pub fn project_ui_point(&self, point: (f64, f64)) -> Option<(f64, f64)> {
        let inside = |(x, y): (f64, f64)| {
            x.is_finite()
                && y.is_finite()
                && x >= 0.0
                && y >= 0.0
                && x < f64::from(self.width)
                && y < f64::from(self.height)
        };
        if !inside(point) {
            return None;
        }
        let source = (
            point.0 - f64::from(self.ui_translation.x),
            point.1 - f64::from(self.ui_translation.y),
        );
        inside(source).then_some(source)
    }

    /// Freeze component geometry without cloning its rectangles or batches.
    pub fn geometry_snapshot(self) -> Result<GeometrySnapshot, String> {
        self.status()?;
        if self.ui_translation != UiTranslation::default() {
            return Err("translated UI surface cannot become a static geometry packet".into());
        }
        if self.batches.iter().any(|batch| batch.component != 0) {
            return Err("animated components cannot become static geometry packets".into());
        }
        if !self.playfields.is_empty() {
            return Err("timed playfields cannot become static UI geometry".into());
        }
        Ok(GeometrySnapshot {
            width: self.width,
            height: self.height,
            rectangles: self.rectangles.into(),
            batches: self.batches.into(),
        })
    }

    /// Concatenate an existing retained packet with checked extent/capacity and
    /// original painter order. Rejection leaves this scene unchanged.
    pub fn append_geometry(&mut self, geometry: &GeometrySnapshot) -> Result<(), String> {
        self.status()?;
        if (self.width, self.height) != (geometry.width, geometry.height) {
            return Err("retained geometry logical viewport mismatch".into());
        }
        if geometry.rectangles.len() > MAX_RECTANGLES - self.rectangles.len() {
            return Err(format!("scene exceeds {MAX_RECTANGLES} rectangles"));
        }
        let offset = self.rectangles.len() as u32;
        self.rectangles.extend_from_slice(&geometry.rectangles);
        for source in geometry.batches.iter() {
            if let Some(last) = self.batches.last_mut().filter(|last| {
                last.playfield.is_none() && last.component == 0 && last.texture == source.texture
            }) {
                last.count += source.count;
            } else {
                self.batches.push(DrawBatch {
                    texture: source.texture,
                    first: source.first + offset,
                    count: source.count,
                    playfield: None,
                    component: 0,
                });
            }
        }
        if !geometry.rectangles.is_empty() {
            self.geometry_changed();
        }
        Ok(())
    }

    /// Insert a retained GPU note layer at this exact position in painter order.
    /// Cache lifetime crosses `clear`; membership/geometry/seek invalidate it.
    #[cfg(test)]
    pub(crate) fn playfield(
        &mut self,
        chart: &crate::player_chart::PlayerChart,
        now: beatkernel::time::Timestamp,
        lookahead: i64,
        bounds: crate::ui::interaction::Bounds,
    ) -> Result<(), String> {
        self.playfield_with_progress(chart, now, lookahead, bounds, None)
    }

    pub(crate) fn playfield_with_progress(
        &mut self,
        chart: &crate::player_chart::PlayerChart,
        now: beatkernel::time::Timestamp,
        lookahead: i64,
        bounds: crate::ui::interaction::Bounds,
        progress: Option<&crate::note_progress::NoteProgress>,
    ) -> Result<(), String> {
        if lookahead <= 0 {
            return Err("playfield lookahead must be positive".into());
        }
        let slot = self.playfields.len();
        if slot == MAX_PLAYFIELDS {
            return Err(format!(
                "scene exceeds {MAX_PLAYFIELDS} displayed playfields"
            ));
        }
        chart.visible_note_indices_with_progress_checked(
            now,
            lookahead,
            150_000_000,
            progress,
            &mut self.visible_note_indices,
        )?;
        chart.visible_mine_indices_checked(
            now,
            lookahead,
            150_000_000,
            &mut self.visible_mine_indices,
        )?;
        if self
            .visible_note_indices
            .iter()
            .any(|&index| chart.notes[index].lane_index >= chart.lanes.len())
        {
            return Err("playfield note references an unavailable lane".into());
        }
        if self
            .visible_mine_indices
            .iter()
            .any(|&index| chart.mines()[index].lane_index >= chart.lanes.len())
        {
            return Err("playfield mine references an unavailable lane".into());
        }
        let frame = self.playfield_caches[slot].frame_indexed_with_mines_and_progress(
            &chart.notes,
            &self.visible_note_indices,
            chart.mines(),
            &self.visible_mine_indices,
            chart.lanes.len(),
            bounds,
            now,
            lookahead,
            progress,
        );
        self.playfields.push(frame);
        self.batches.push(DrawBatch {
            texture: TextureId::WHITE,
            first: self.rectangles.len() as u32,
            count: 0,
            playfield: Some(slot),
            component: 0,
        });
        Ok(())
    }

    /// Clips to the logical viewport. Capacity exhaustion is sticky until clear,
    /// and must be checked by the renderer before submitting this scene.
    pub fn rect(&mut self, x: i64, y: i64, width: i64, height: i64, color: u32) {
        self.push(
            TextureId::WHITE,
            [x, y, width, height],
            [0.0, 0.0, 1.0, 1.0],
            color,
        );
    }

    /// UV is normalized [left, top, width, height]. Clipping crops rather than stretches it.
    pub fn sprite(
        &mut self,
        texture: TextureId,
        bounds: [i64; 4],
        uv: [f32; 4],
        tint: u32,
    ) -> Result<(), String> {
        Self::validate_uv(uv)?;
        self.push(texture, bounds, uv, tint);
        self.status()
    }

    /// Crops to the viewport and this call's immutable clip in original sprite
    /// coordinates. It never changes clipping for later sprites or siblings.
    pub fn sprite_clipped(
        &mut self,
        texture: TextureId,
        bounds: [i64; 4],
        uv: [f32; 4],
        tint: u32,
        clip: ClipRect,
    ) -> Result<(), String> {
        self.sprite_clipped_alpha(texture, bounds, uv, tint, 255, clip)
    }

    /// Multiplies sampled straight alpha by this byte without changing pixels
    /// or texture identity. Clipping and RGB tint retain their usual semantics.
    pub fn sprite_clipped_alpha(
        &mut self,
        texture: TextureId,
        bounds: [i64; 4],
        uv: [f32; 4],
        tint: u32,
        alpha: u8,
        clip: ClipRect,
    ) -> Result<(), String> {
        Self::validate_uv(uv)?;
        if let Some(endpoints) = self.clip_bounds(clip) {
            self.push_in_alpha(texture, bounds, uv, tint, alpha, endpoints);
        }
        self.status()
    }

    pub(crate) fn clip_bounds(&self, clip: ClipRect) -> Option<[i64; 4]> {
        if self.source_geometry {
            return Some(clip.endpoints);
        }
        let [left, top, right, bottom] = clip.endpoints;
        let endpoints = [
            left.max(0),
            top.max(0),
            right.min(i64::from(self.width)),
            bottom.min(i64::from(self.height)),
        ];
        (endpoints[0] < endpoints[2] && endpoints[1] < endpoints[3]).then_some(endpoints)
    }

    fn validate_uv(uv: [f32; 4]) -> Result<(), String> {
        if uv.iter().any(|value| !value.is_finite() || *value < 0.0)
            || uv[2] <= 0.0
            || uv[3] <= 0.0
            || uv[0] + uv[2] > 1.0
            || uv[1] + uv[3] > 1.0
        {
            return Err("sprite UV rectangle must lie inside normalized texture bounds".into());
        }
        Ok(())
    }

    pub(crate) fn glyph(&mut self, bounds: [i64; 4], uv: [f32; 4], color: u32) {
        self.push(TextureId::FONT, bounds, uv, color);
    }

    fn push(&mut self, texture: TextureId, bounds: [i64; 4], uv: [f32; 4], color: u32) {
        self.push_in(
            texture,
            bounds,
            uv,
            color,
            if self.source_geometry {
                [i64::MIN, i64::MIN, i64::MAX, i64::MAX]
            } else {
                [0, 0, i64::from(self.width), i64::from(self.height)]
            },
        );
    }

    fn push_in(
        &mut self,
        texture: TextureId,
        bounds: [i64; 4],
        uv: [f32; 4],
        color: u32,
        clip: [i64; 4],
    ) {
        self.push_in_alpha(texture, bounds, uv, color, 255, clip);
    }

    fn push_in_alpha(
        &mut self,
        texture: TextureId,
        bounds: [i64; 4],
        uv: [f32; 4],
        color: u32,
        alpha: u8,
        clip: [i64; 4],
    ) {
        if alpha == 0 {
            return;
        }
        let [x, y, width, height] = bounds;
        if width <= 0 || height <= 0 {
            return;
        }
        let left = x.clamp(clip[0], clip[2]);
        let top = y.clamp(clip[1], clip[3]);
        let right = x.saturating_add(width).clamp(clip[0], clip[2]);
        let bottom = y.saturating_add(height).clamp(clip[1], clip[3]);
        if right <= left || bottom <= top {
            return;
        }
        if self.rectangles.len() == MAX_RECTANGLES {
            self.overflow = true;
            return;
        }
        self.geometry_changed();
        let first = self.rectangles.len() as u32;
        if let Some(batch) = self.batches.last_mut().filter(|batch| {
            batch.playfield.is_none() && batch.component == 0 && batch.texture == texture
        }) {
            batch.count += 1;
        } else {
            self.batches.push(DrawBatch {
                texture,
                first,
                count: 1,
                playfield: None,
                component: 0,
            });
        }
        let horizontal = (i128::from(left) - i128::from(x)) as f64 / width as f64;
        let vertical = (i128::from(top) - i128::from(y)) as f64 / height as f64;
        self.rectangles.push(Rectangle {
            bounds: [
                left as f32,
                top as f32,
                (right - left) as f32,
                (bottom - top) as f32,
            ],
            color: [
                ((color >> 16) & 255) as f32 / 255.0,
                ((color >> 8) & 255) as f32 / 255.0,
                (color & 255) as f32 / 255.0,
                f32::from(alpha) / 255.0,
            ],
            uv: [
                uv[0] + uv[2] * horizontal as f32,
                uv[1] + uv[3] * vertical as f32,
                uv[2] * ((right - left) as f64 / width as f64) as f32,
                uv[3] * ((bottom - top) as f64 / height as f64) as f32,
            ],
        });
    }

    pub(crate) fn reject(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    pub fn status(&self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.width == 0 || self.height == 0 {
            Err("scene logical extent must be nonzero".into())
        } else if self.overflow {
            Err(format!("scene exceeds {MAX_RECTANGLES} rectangles"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn dimensions(&self) -> [f32; 2] {
        [self.width as f32, self.height as f32]
    }

    pub(crate) fn rectangles(&self) -> &[Rectangle] {
        &self.rectangles
    }
    pub(crate) fn batches(&self) -> &[DrawBatch] {
        &self.batches
    }
    pub(crate) fn playfields(&self) -> &[PlayfieldFrame] {
        &self.playfields
    }
}

#[cfg(test)]
#[path = "component_motion_scene_fixtures.rs"]
mod component_motion_fixtures;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_alpha_preserves_uv_tint_and_rejects_invalid_geometry_atomically() {
        let mut scene = Scene::new(100, 100);
        let clip = ClipRect::new([25, 25, 50, 50]).unwrap();
        scene
            .sprite_clipped_alpha(
                TextureId::FONT,
                [0, 0, 100, 100],
                [0.0, 0.0, 1.0, 1.0],
                0x406080,
                128,
                clip,
            )
            .unwrap();
        let rectangle = scene.rectangles()[0];
        assert_eq!(rectangle.bounds, [25.0, 25.0, 50.0, 50.0]);
        assert_eq!(rectangle.uv, [0.25, 0.25, 0.5, 0.5]);
        assert_eq!(
            rectangle.color,
            [64.0 / 255.0, 96.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0]
        );
        let epoch = scene.geometry_epoch;
        scene
            .sprite_clipped_alpha(
                TextureId::FONT,
                [0, 0, 100, 100],
                [0.0, 0.0, 1.0, 1.0],
                0,
                0,
                clip,
            )
            .unwrap();
        assert!(scene
            .sprite_clipped_alpha(
                TextureId::FONT,
                [0, 0, 100, 100],
                [f32::NAN, 0.0, 1.0, 1.0],
                0,
                0,
                clip
            )
            .is_err());
        assert_eq!(scene.geometry_epoch, epoch);
        assert_eq!(scene.rectangles().len(), 1);
        scene
            .sprite_clipped(
                TextureId::FONT,
                [0, 0, 100, 100],
                [0.0, 0.0, 1.0, 1.0],
                0x406080,
                clip,
            )
            .unwrap();
        assert_eq!(scene.rectangles()[1].color[3], 1.0);
        assert_eq!(scene.rectangles()[1].uv, rectangle.uv);
    }

    #[test]
    fn clip_rect_checks_endpoints_and_intersects_signed_origins() {
        for bounds in [
            [0, 0, 0, 1],
            [0, 0, 1, 0],
            [0, 0, -1, 1],
            [0, 0, 1, -1],
            [i64::MAX, 0, 1, 1],
            [0, i64::MAX, 1, 1],
        ] {
            assert!(ClipRect::new(bounds).is_err());
        }
        let scene = Scene::new(100, 100);
        assert_eq!(
            scene.clip_bounds(ClipRect::new([-20, -30, 80, 90]).unwrap()),
            Some([0, 0, 60, 60])
        );
        assert_eq!(
            scene.clip_bounds(ClipRect::new([80, 90, 100, 100]).unwrap()),
            Some([80, 90, 100, 100])
        );
        assert_eq!(
            scene.clip_bounds(ClipRect::new([100, 0, 1, 1]).unwrap()),
            None
        );
        assert_eq!(
            scene.clip_bounds(ClipRect::new([i64::MIN, 0, i64::MAX, 1]).unwrap()),
            None
        );
        assert_eq!(
            Scene::new(0, 100).clip_bounds(ClipRect::new([0, 0, 1, 1]).unwrap()),
            None
        );
    }

    #[test]
    fn sprite_clips_each_edge_and_all_edges_in_original_uv_coordinates() {
        for (clip, bounds, uv) in [
            (
                [25, 0, 75, 100],
                [25.0, 0.0, 75.0, 100.0],
                [0.25, 0.0, 0.75, 1.0],
            ),
            (
                [0, 25, 100, 75],
                [0.0, 25.0, 100.0, 75.0],
                [0.0, 0.25, 1.0, 0.75],
            ),
            (
                [0, 0, 75, 100],
                [0.0, 0.0, 75.0, 100.0],
                [0.0, 0.0, 0.75, 1.0],
            ),
            (
                [0, 0, 100, 75],
                [0.0, 0.0, 100.0, 75.0],
                [0.0, 0.0, 1.0, 0.75],
            ),
            (
                [25, 25, 50, 50],
                [25.0, 25.0, 50.0, 50.0],
                [0.25, 0.25, 0.5, 0.5],
            ),
        ] {
            let mut scene = Scene::new(100, 100);
            scene
                .sprite_clipped(
                    TextureId::FONT,
                    [0, 0, 100, 100],
                    [0.0, 0.0, 1.0, 1.0],
                    0xffffff,
                    ClipRect::new(clip).unwrap(),
                )
                .unwrap();
            assert_eq!(scene.rectangles()[0].bounds, bounds);
            assert_eq!(scene.rectangles()[0].uv, uv);
        }
        let mut scene = Scene::new(100, 100);
        scene
            .sprite_clipped(
                TextureId::FONT,
                [-20, -20, 80, 80],
                [0.25, 0.25, 0.5, 0.5],
                0xffffff,
                ClipRect::new([-20, -30, 80, 90]).unwrap(),
            )
            .unwrap();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 60.0, 60.0]);
        assert_eq!(scene.rectangles()[0].uv, [0.375, 0.375, 0.375, 0.375]);
    }

    #[test]
    fn clipped_invalid_uv_and_invisible_bounds_preserve_geometry() {
        let mut scene = Scene::new(100, 100);
        scene.rect(0, 0, 1, 1, 0);
        let epoch = scene.geometry_epoch;
        let clip = ClipRect::new([100, 100, 1, 1]).unwrap();
        for uv in [
            [f32::NAN, 0.0, 1.0, 1.0],
            [0.0, f32::INFINITY, 1.0, 1.0],
            [-0.1, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.5, 0.0, 0.75, 1.0],
        ] {
            assert!(scene
                .sprite_clipped(TextureId::FONT, [0, 0, 1, 1], uv, 0, clip)
                .is_err());
            assert_eq!(scene.geometry_epoch, epoch);
            assert_eq!(scene.rectangles.len(), 1);
            assert_eq!(scene.batches.len(), 1);
            assert!(scene.status().is_ok());
        }
        scene
            .sprite_clipped(TextureId::FONT, [0, 0, 1, 1], [0.0, 0.0, 1.0, 1.0], 0, clip)
            .unwrap();
        let clip = ClipRect::new([0, 0, 100, 100]).unwrap();
        for bounds in [
            [i64::MIN, 0, i64::MAX, 1],
            [i64::MAX, 0, i64::MAX, 1],
            [0, 0, -1, 1],
        ] {
            scene
                .sprite_clipped(TextureId::FONT, bounds, [0.0, 0.0, 1.0, 1.0], 0, clip)
                .unwrap();
            assert_eq!(scene.geometry_epoch, epoch);
        }
        scene
            .sprite_clipped(
                TextureId::FONT,
                [-1, -1, i64::MAX, i64::MAX],
                [0.0, 0.0, 1.0, 1.0],
                0,
                clip,
            )
            .unwrap();
        assert_eq!(scene.rectangles()[1].bounds, [0.0, 0.0, 100.0, 100.0]);
        assert!(scene.rectangles()[1]
            .uv
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0));
    }

    #[test]
    fn clipped_packets_append_in_order_without_leaking_clip_to_siblings() {
        let mut packet = Scene::with_capacity(100, 100, 4);
        packet
            .sprite_clipped(
                TextureId::FONT,
                [0, 0, 100, 100],
                [0.0, 0.0, 1.0, 1.0],
                0xff0000,
                ClipRect::new([25, 25, 50, 50]).unwrap(),
            )
            .unwrap();
        packet.rect(0, 0, 100, 100, 0x00ff00);
        packet
            .sprite(
                TextureId::FONT,
                [0, 0, 100, 100],
                [0.0, 0.0, 1.0, 1.0],
                0x0000ff,
            )
            .unwrap();
        let packet = packet.geometry_snapshot().unwrap();
        let mut scene = Scene::new(100, 100);
        scene.append_geometry(&packet).unwrap();
        scene.glyph([0, 0, 100, 100], [0.0, 0.0, 1.0, 1.0], 0xffffff);
        assert_eq!(scene.rectangles()[0].bounds, [25.0, 25.0, 50.0, 50.0]);
        for rectangle in &scene.rectangles()[1..] {
            assert_eq!(rectangle.bounds, [0.0, 0.0, 100.0, 100.0]);
            assert_eq!(rectangle.uv, [0.0, 0.0, 1.0, 1.0]);
        }
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|batch| (batch.texture, batch.first, batch.count))
                .collect::<Vec<_>>(),
            [
                (TextureId::FONT, 0, 1),
                (TextureId::WHITE, 1, 1),
                (TextureId::FONT, 2, 2)
            ]
        );
    }

    #[test]
    fn clips_signed_extremes_and_preserves_painter_order() {
        let mut scene = Scene::new(960, 720);
        scene.rect(-5, -7, 10, 12, 0xff0000);
        scene.rect(959, 719, i64::MAX, i64::MAX, 0x00ff00);
        scene.rect(i64::MIN, 0, i64::MAX, 10, 0);
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 5.0, 5.0]);
        assert_eq!(scene.rectangles()[1].bounds, [959.0, 719.0, 1.0, 1.0]);
        assert_eq!(scene.rectangles()[0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn capacity_failure_is_reported_until_clear() {
        let mut scene = Scene::new(1, 1);
        for _ in 0..=MAX_RECTANGLES {
            scene.rect(0, 0, 1, 1, 0);
        }
        assert_eq!(scene.rectangles().len(), MAX_RECTANGLES);
        assert!(scene.status().is_err());
        assert!(scene
            .sprite_clipped(
                TextureId::FONT,
                [0, 0, 1, 1],
                [0.0, 0.0, 1.0, 1.0],
                0,
                ClipRect::new([0, 0, 1, 1]).unwrap()
            )
            .is_err());
        assert_eq!(scene.rectangles().len(), MAX_RECTANGLES);
        scene.clear();
        assert!(scene.status().is_ok());
        assert!(scene.rectangles().is_empty());
    }
    #[test]
    fn sprites_crop_uvs_and_batches_keep_painter_order() {
        let mut scene = Scene::new(960, 720);
        scene
            .sprite(
                TextureId::FONT,
                [-10, 0, 20, 20],
                [0.0, 0.0, 1.0, 1.0],
                0xffffff,
            )
            .unwrap();
        assert_eq!(scene.rectangles()[0].bounds, [0.0, 0.0, 10.0, 20.0]);
        assert_eq!(scene.rectangles()[0].uv, [0.5, 0.0, 0.5, 1.0]);
        scene.rect(0, 0, 1, 1, 0);
        scene
            .sprite(
                TextureId::FONT,
                [0, 0, 1, 1],
                [0.0, 0.0, 1.0, 1.0],
                0xffffff,
            )
            .unwrap();
        scene
            .sprite(
                TextureId::FONT,
                [1, 0, 1, 1],
                [0.0, 0.0, 1.0, 1.0],
                0xffffff,
            )
            .unwrap();
        assert_eq!(
            scene
                .batches()
                .iter()
                .map(|batch| (batch.texture, batch.first, batch.count))
                .collect::<Vec<_>>(),
            vec![
                (TextureId::FONT, 0, 1),
                (TextureId::WHITE, 1, 1),
                (TextureId::FONT, 2, 2)
            ]
        );
        assert!(scene
            .sprite(TextureId::FONT, [0, 0, 1, 1], [f32::NAN, 0.0, 1.0, 1.0], 0)
            .is_err());
        assert_eq!(scene.rectangles().len(), 4);
        scene
            .sprite(TextureId::FONT, [0, 0, 1, 1], [0.2, 0.2, 0.8, 0.8], 0)
            .unwrap();
    }

    #[test]
    fn retained_packets_preserve_order_and_reject_without_mutation() {
        let mut first = Scene::with_capacity(10, 10, 1);
        first.rect(0, 0, 1, 1, 0xff0000);
        first.glyph([1, 0, 1, 1], [0.0, 0.0, 1.0, 1.0], 0x00ff00);
        let first = first.geometry_snapshot().unwrap();
        let mut second = Scene::with_capacity(10, 10, 1);
        second.glyph([2, 0, 1, 1], [0.0, 0.0, 1.0, 1.0], 0x0000ff);
        second.rect(3, 0, 1, 1, 0xffffff);
        let second = second.geometry_snapshot().unwrap();
        let mut output = Scene::new(10, 10);
        output.append_geometry(&first).unwrap();
        output.append_geometry(&second).unwrap();
        assert_eq!(
            output
                .rectangles
                .iter()
                .map(|r| r.bounds[0])
                .collect::<Vec<_>>(),
            [0.0, 1.0, 2.0, 3.0]
        );
        assert_eq!(
            output
                .batches
                .iter()
                .map(|b| (b.texture, b.first, b.count))
                .collect::<Vec<_>>(),
            [
                (TextureId::WHITE, 0, 1),
                (TextureId::FONT, 1, 2),
                (TextureId::WHITE, 3, 1)
            ]
        );
        let epoch = output.geometry_epoch;
        let wrong_extent = Scene::new(11, 10).geometry_snapshot().unwrap();
        assert!(output.append_geometry(&wrong_extent).is_err());
        assert_eq!(output.geometry_epoch, epoch);
        assert_eq!(output.rectangles.len(), 4);
        for _ in 4..MAX_RECTANGLES {
            output.rect(0, 0, 1, 1, 0);
        }
        let epoch = output.geometry_epoch;
        assert!(output.append_geometry(&first).is_err());
        assert_eq!(output.geometry_epoch, epoch);
        assert_eq!(output.rectangles.len(), MAX_RECTANGLES);
        assert!(output.status().is_ok());
    }

    #[test]
    fn ui_translation_preserves_geometry_storage_order_and_upload_identity() {
        let mut scene = Scene::new(960, 720);
        scene.rect(10, 20, 30, 40, 0xffaabb);
        scene.rect(50, 60, 70, 80, 0x112233);
        let identity = Arc::clone(scene.geometry_stamp().0);
        let epoch = scene.geometry_stamp().1;
        let rectangles = scene.rectangles.as_ptr();
        let batches = scene.batches.as_ptr();
        let data = bytemuck::cast_slice::<Rectangle, u8>(&scene.rectangles).to_vec();
        for offset in [[50, -30], [-20, 40], [0, 0]] {
            scene.set_ui_translation(UiTranslation::new(offset[0], offset[1]).unwrap());
            assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
            assert_eq!(scene.geometry_stamp().1, epoch);
            assert_eq!(scene.rectangles.as_ptr(), rectangles);
            assert_eq!(scene.batches.as_ptr(), batches);
            assert_eq!(
                bytemuck::cast_slice::<Rectangle, u8>(&scene.rectangles),
                data
            );
            assert_eq!(scene.batches[0].first, 0);
            assert_eq!(scene.batches[0].count, 2);
        }
        let before = scene.ui_translation();
        assert!(UiTranslation::new(i32::MAX, 0).is_err());
        assert_eq!(scene.ui_translation(), before);
        scene.set_ui_translation(UiTranslation::new(1, 0).unwrap());
        assert!(scene.geometry_snapshot().is_err());
        let mut unshifted = Scene::new(960, 720);
        unshifted.rect(0, 0, 1, 1, 0);
        assert!(unshifted.geometry_snapshot().is_ok());
    }

    #[test]
    fn geometry_stamp_tracks_mutation_and_cannot_alias_other_scenes_or_wrap() {
        let mut first = Scene::new(10, 10);
        let second = Scene::new(10, 10);
        let identity = Arc::clone(first.geometry_stamp().0);
        assert!(!Arc::ptr_eq(&identity, second.geometry_stamp().0));
        first.rect(20, 20, 1, 1, 0);
        assert_eq!(first.geometry_stamp().1, 0);
        first.rect(0, 0, 1, 1, 0);
        assert_eq!(first.geometry_stamp().1, 1);
        assert!(Arc::ptr_eq(&identity, first.geometry_stamp().0));
        first.geometry_epoch = u64::MAX;
        first.clear();
        assert_eq!(first.geometry_stamp().1, 0);
        assert!(!Arc::ptr_eq(&identity, first.geometry_stamp().0));
        assert!(first.rectangles.is_empty());
    }

    #[test]
    fn retained_note_layer_splits_rectangles_and_survives_clear() {
        let source = beatkernel_bms::parse(
            "#BPM 60\n#WAV01 head.wav\n#00011:01\n",
            beatkernel_bms::ParseOptions::default(),
        )
        .unwrap();
        let chart = crate::player_chart::PlayerChart::from_compiled(
            &source,
            &source.compile().unwrap().chart,
        )
        .unwrap();
        let bounds = crate::ui::interaction::Bounds {
            x: 80,
            y: 106,
            width: 640,
            height: 528,
        };
        let mut scene = Scene::new(960, 720);
        assert_eq!(scene.visible_note_indices.capacity(), 0);
        scene.rect(0, 0, 1, 1, 0);
        scene
            .playfield(
                &chart,
                beatkernel::time::Timestamp::ZERO,
                1_000_000_000,
                bounds,
            )
            .unwrap();
        scene.rect(0, 0, 1, 1, 0);
        assert_eq!(scene.batches().len(), 3);
        assert_eq!(scene.batches()[0].first, 0);
        assert_eq!(scene.batches()[1].playfield, Some(0));
        assert_eq!(scene.batches()[2].first, 1);
        let cached = std::sync::Arc::clone(&scene.playfields()[0].instances);
        let scratch = scene.visible_note_indices.as_ptr();
        let capacity = scene.visible_note_indices.capacity();
        scene.clear();
        assert!(scene.playfields().is_empty());
        assert!(scene.visible_note_indices.is_empty());
        assert_eq!(scene.visible_note_indices.as_ptr(), scratch);
        scene
            .playfield(
                &chart,
                beatkernel::time::Timestamp::ZERO,
                1_000_000_000,
                bounds,
            )
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &cached,
            &scene.playfields()[0].instances
        ));
        for _ in 1..MAX_PLAYFIELDS {
            scene
                .playfield(
                    &chart,
                    beatkernel::time::Timestamp::ZERO,
                    1_000_000_000,
                    bounds,
                )
                .unwrap();
        }
        assert!(scene
            .playfield(
                &chart,
                beatkernel::time::Timestamp::ZERO,
                1_000_000_000,
                bounds
            )
            .is_err());
        assert_eq!(scene.playfields().len(), MAX_PLAYFIELDS);
        assert_eq!(scene.visible_note_indices.as_ptr(), scratch);
        assert_eq!(scene.visible_note_indices.capacity(), capacity);
    }

    #[test]
    fn one_lazy_scratch_serves_growing_membership_four_slots_seek_and_rejection() {
        use crate::player_chart::{PlayerChart, MAX_VISIBLE_NOTES};
        use beatkernel::time::Timestamp;
        use std::sync::Arc;
        let model = |text: &str| {
            let source =
                beatkernel_bms::parse(text, beatkernel_bms::ParseOptions::default()).unwrap();
            PlayerChart::from_compiled(&source, &source.compile().unwrap().chart).unwrap()
        };
        let chart = model("#BPM 120\n#WAV01 head.wav\n#LNTYPE 1\n#00051:0101\n#00012:0001");
        let bounds = crate::ui::interaction::Bounds {
            x: 80,
            y: 106,
            width: 640,
            height: 528,
        };
        let mut scene = Scene::with_capacity(960, 720, 64);
        assert_eq!(scene.visible_note_indices.capacity(), 0);
        scene
            .playfield(&chart, Timestamp::ZERO, 750_000_000, bounds)
            .unwrap();
        assert_eq!(scene.visible_note_indices, vec![0]);
        let first = Arc::clone(&scene.playfields[0].instances);
        let pointer = scene.visible_note_indices.as_ptr();
        let capacity = scene.visible_note_indices.capacity();
        assert!(capacity >= MAX_VISIBLE_NOTES + 1);
        scene.clear();
        scene
            .playfield(
                &chart,
                Timestamp::from_nanos(250_000_000),
                750_000_000,
                bounds,
            )
            .unwrap();
        assert_eq!(scene.visible_note_indices, vec![0, 1]);
        assert_eq!(scene.visible_note_indices.as_ptr(), pointer);
        assert_eq!(scene.visible_note_indices.capacity(), capacity);
        assert!(!Arc::ptr_eq(&first, &scene.playfields[0].instances));
        for _ in 1..MAX_PLAYFIELDS {
            scene
                .playfield(
                    &chart,
                    Timestamp::from_nanos(250_000_000),
                    750_000_000,
                    bounds,
                )
                .unwrap();
        }
        let frames: Vec<_> = scene
            .playfields
            .iter()
            .map(|frame| Arc::clone(&frame.instances))
            .collect();
        scene.clear();
        for (slot, previous) in frames.iter().enumerate() {
            scene
                .playfield(
                    &chart,
                    Timestamp::from_nanos(260_000_000),
                    750_000_000,
                    bounds,
                )
                .unwrap();
            assert!(Arc::ptr_eq(previous, &scene.playfields[slot].instances));
            assert_eq!(scene.visible_note_indices.as_ptr(), pointer);
        }
        scene.clear();
        scene
            .playfield(
                &chart,
                Timestamp::from_nanos(250_000_000),
                750_000_000,
                bounds,
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&frames[0], &scene.playfields[0].instances));
        let mut replacement = chart.clone();
        replacement.notes[0].object = beatkernel::chart::ObjectId(99);
        let before = Arc::clone(&scene.playfields[0].instances);
        scene.clear();
        scene
            .playfield(
                &replacement,
                Timestamp::from_nanos(250_000_000),
                750_000_000,
                bounds,
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&before, &scene.playfields[0].instances));
        let admitted = scene.playfields.len();
        let batches = scene.batches.len();
        let mut malformed = chart.clone();
        malformed.notes[0].lane_index = usize::MAX;
        assert!(scene
            .playfield(&malformed, Timestamp::ZERO, 750_000_000, bounds)
            .is_err());
        assert_eq!(scene.playfields.len(), admitted);
        assert_eq!(scene.batches.len(), batches);
        let dense = model(&format!(
            "#BPM 60\n#WAV01 head.wav\n#00011:{}",
            "01".repeat(MAX_VISIBLE_NOTES + 1)
        ));
        assert!(scene
            .playfield(&dense, Timestamp::ZERO, i64::MAX, bounds)
            .is_err());
        assert!(scene.visible_note_indices.is_empty());
        assert_eq!(scene.playfields.len(), admitted);
        assert_eq!(scene.batches.len(), batches);
        assert_eq!(scene.visible_note_indices.as_ptr(), pointer);
        assert_eq!(scene.visible_note_indices.capacity(), capacity);
    }
}
