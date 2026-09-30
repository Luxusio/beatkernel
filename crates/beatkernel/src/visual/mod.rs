//! Indexed renderer-independent projection with a separate scroll timeline.

use crate::{
    chart::{CompiledChart, ObjectId, TimedObject, VisualId},
    judge::JudgeEvent,
    time::{Duration, Timestamp},
};
use std::collections::BTreeMap;

/// A caller-supplied visual shape; geometry uses logical units.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// Lane position with a positive song-time distance scale.
    Lane {
        /// Caller-defined lane.
        lane: u32,
        /// Nanoseconds corresponding to one logical distance unit.
        unit: Duration,
    },
    /// Fixed point with an approach interval.
    Point {
        /// Logical two-dimensional position.
        position: [f64; 2],
        /// Positive approach duration.
        approach: Duration,
    },
    /// Piecewise linear path, uniformly parameterized over object duration.
    Path {
        /// At least two finite logical points.
        points: Vec<[f64; 2]>,
    },
}

/// Immutable geometry associated with a chart visual identity.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualBinding {
    /// Opaque chart binding.
    pub id: VisualId,
    /// Shape interpreted by the external renderer.
    pub projection: Projection,
}

/// One calculated logical shape, without drawing commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderObjectState {
    /// Lane distances integrated on the independent scroll timeline.
    Lane {
        /// Chart object.
        object: ObjectId,
        /// Caller lane.
        lane: u32,
        /// Start distance from judge line.
        distance: f64,
        /// Optional tail distance.
        tail_distance: Option<f64>,
    },
    /// Point geometry and normalized approach progress.
    Point {
        /// Chart object.
        object: ObjectId,
        /// Fixed logical point.
        position: [f64; 2],
        /// Zero before approach, one at target.
        approach_progress: f64,
    },
    /// Path reference and calculated head/window progress.
    Path {
        /// Chart object.
        object: ObjectId,
        /// Binding containing immutable geometry.
        visual: VisualId,
        /// Interpolated logical tracking head.
        head: [f64; 2],
        /// Clamped progress over object duration.
        progress: f64,
        /// Visible window's lower path fraction.
        visible_start: f64,
        /// Visible window's upper path fraction.
        visible_end: f64,
    },
}

/// Reusable frame storage owned by the caller.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderFrame {
    /// Song time used for projection.
    pub song_time: Timestamp,
    /// Logical states in chart start-time then object-ID order.
    pub objects: Vec<RenderObjectState>,
    /// Caller-provided judge events, copied in supplied order.
    pub transient_events: Vec<JudgeEvent>,
}

/// Invalid projection configuration or frame request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualError {
    /// Geometry is nonfinite, durations nonpositive, or a path too short.
    InvalidGeometry,
    /// Duplicate visual identity.
    DuplicateBinding(VisualId),
    /// A compiled object lacks caller geometry.
    MissingBinding(VisualId),
    /// A path object lacks a positive ranged duration.
    InvalidPath(ObjectId),
    /// Window end precedes its start.
    ReversedWindow,
    /// A calculated coordinate is nonfinite.
    Overflow,
}
impl std::fmt::Display for VisualError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "visual projection: {self:?}")
    }
}
impl std::error::Error for VisualError {}

#[derive(Clone, Debug)]
struct IntervalNode {
    index: usize,
    left: Option<usize>,
    right: Option<usize>,
    min_start: i64,
    max_end: i64,
}
#[derive(Clone, Copy, Debug)]
struct ScrollSegment {
    at: i64,
    position: f64,
    velocity: f64,
}

/// Immutable compiled geometry and overlap index constructed off-frame.
#[derive(Clone, Debug)]
pub struct VisualProjector {
    chart: CompiledChart,
    bindings: BTreeMap<VisualId, Projection>,
    nodes: Vec<IntervalNode>,
    root: Option<usize>,
    scroll: Vec<ScrollSegment>,
}
impl VisualProjector {
    /// Validates bindings and builds an interval tree in linear space.
    pub fn new(chart: CompiledChart, bindings: Vec<VisualBinding>) -> Result<Self, VisualError> {
        let mut map = BTreeMap::new();
        for binding in bindings {
            match &binding.projection {
                Projection::Lane { unit, .. } if unit.as_nanos() > 0 => {}
                Projection::Point { position, approach }
                    if approach.as_nanos() > 0 && position.iter().all(|v| v.is_finite()) => {}
                Projection::Path { points }
                    if points.len() >= 2 && points.iter().flatten().all(|v| v.is_finite()) => {}
                _ => return Err(VisualError::InvalidGeometry),
            }
            if map.insert(binding.id, binding.projection).is_some() {
                return Err(VisualError::DuplicateBinding(binding.id));
            }
        }
        for object in chart.objects() {
            let projection = map
                .get(&object.visual)
                .ok_or(VisualError::MissingBinding(object.visual))?;
            if matches!(projection, Projection::Path { .. })
                && object.time.end.is_none_or(|end| end <= object.time.start)
            {
                return Err(VisualError::InvalidPath(object.id));
            }
        }
        let mut nodes = Vec::with_capacity(chart.objects().len());
        let root = build_index(chart.objects(), &mut nodes, 0, chart.objects().len());
        let mut scroll = vec![ScrollSegment {
            at: i64::MIN,
            position: 0.0,
            velocity: 1.0,
        }];
        // A zero origin avoids subtracting huge signed endpoints in i64.
        for marker in chart.scroll_changes() {
            let previous = *scroll.last().expect("initial scroll segment");
            let at = marker.time.as_nanos();
            let position = if scroll.len() == 1 {
                at as f64
            } else {
                previous.position
                    + (i128::from(at) - i128::from(previous.at)) as f64 * previous.velocity
            };
            let velocity =
                marker.velocity.numerator() as f64 / f64::from(marker.velocity.denominator());
            if !position.is_finite() {
                return Err(VisualError::Overflow);
            }
            scroll.push(ScrollSegment {
                at,
                position,
                velocity,
            });
        }
        Ok(Self {
            chart,
            bindings: map,
            nodes,
            root,
            scroll,
        })
    }

    /// Borrows immutable path or point/lane geometry for a renderer.
    pub fn projection(&self, visual: VisualId) -> Option<&Projection> {
        self.bindings.get(&visual)
    }

    /// Projects objects overlapping the inclusive window into reusable storage.
    ///
    /// Reversed windows fail before output changes. Coordinate failures clear
    /// partial objects and report an error; no successful partial frame is returned.
    pub fn project(
        &self,
        song_time: Timestamp,
        start: Timestamp,
        end: Timestamp,
        events: &[JudgeEvent],
        frame: &mut RenderFrame,
    ) -> Result<(), VisualError> {
        if end < start {
            return Err(VisualError::ReversedWindow);
        }
        frame.objects.clear();
        if let Some(root) = self.root {
            if let Err(error) = self.visit(root, song_time, start, end, &mut frame.objects) {
                frame.objects.clear();
                frame.transient_events.clear();
                return Err(error);
            }
        }
        frame.song_time = song_time;
        frame.transient_events.clear();
        frame.transient_events.extend_from_slice(events);
        Ok(())
    }

    fn scroll_position(&self, time: Timestamp) -> f64 {
        let count = self
            .scroll
            .partition_point(|segment| segment.at <= time.as_nanos());
        let segment = self.scroll[count.saturating_sub(1)];
        if count <= 1 {
            time.as_nanos() as f64
        } else {
            segment.position
                + (i128::from(time.as_nanos()) - i128::from(segment.at)) as f64 * segment.velocity
        }
    }

    fn visit(
        &self,
        node: usize,
        song: Timestamp,
        start: Timestamp,
        end: Timestamp,
        out: &mut Vec<RenderObjectState>,
    ) -> Result<(), VisualError> {
        let node = &self.nodes[node];
        if node.max_end < start.as_nanos() || node.min_start > end.as_nanos() {
            return Ok(());
        }
        if let Some(left) = node.left {
            self.visit(left, song, start, end, out)?;
        }
        let object = &self.chart.objects()[node.index];
        if object.time.start <= end && object.time.end.unwrap_or(object.time.start) >= start {
            out.push(self.state(object, song, start, end)?);
        }
        if let Some(right) = node.right {
            self.visit(right, song, start, end, out)?;
        }
        Ok(())
    }

    fn state(
        &self,
        object: &TimedObject,
        song: Timestamp,
        window_start: Timestamp,
        window_end: Timestamp,
    ) -> Result<RenderObjectState, VisualError> {
        let result = match &self.bindings[&object.visual] {
            Projection::Lane { lane, unit } => {
                let now = self.scroll_position(song);
                let distance =
                    (self.scroll_position(object.time.start) - now) / unit.as_nanos() as f64;
                let tail_distance = object
                    .time
                    .end
                    .map(|end| (self.scroll_position(end) - now) / unit.as_nanos() as f64);
                if !distance.is_finite() || tail_distance.is_some_and(|v| !v.is_finite()) {
                    return Err(VisualError::Overflow);
                }
                RenderObjectState::Lane {
                    object: object.id,
                    lane: *lane,
                    distance,
                    tail_distance,
                }
            }
            Projection::Point { position, approach } => {
                let remaining =
                    (i128::from(object.time.start.as_nanos()) - i128::from(song.as_nanos())) as f64;
                RenderObjectState::Point {
                    object: object.id,
                    position: *position,
                    approach_progress: (1.0 - remaining / approach.as_nanos() as f64)
                        .clamp(0.0, 1.0),
                }
            }
            Projection::Path { points } => {
                let end = object.time.end.expect("validated ranged path");
                let duration =
                    (i128::from(end.as_nanos()) - i128::from(object.time.start.as_nanos())) as f64;
                let fraction = |at: Timestamp| {
                    ((i128::from(at.as_nanos()) - i128::from(object.time.start.as_nanos())) as f64
                        / duration)
                        .clamp(0.0, 1.0)
                };
                let progress = fraction(song);
                let coordinate = progress * (points.len() - 1) as f64;
                let index = (coordinate.floor() as usize).min(points.len() - 2);
                let weight = coordinate - index as f64;
                let head = std::array::from_fn(|axis| {
                    points[index][axis] * (1.0 - weight) + points[index + 1][axis] * weight
                });
                if !head.iter().all(|v: &f64| v.is_finite()) {
                    return Err(VisualError::Overflow);
                }
                RenderObjectState::Path {
                    object: object.id,
                    visual: object.visual,
                    head,
                    progress,
                    visible_start: fraction(window_start),
                    visible_end: fraction(window_end),
                }
            }
        };
        Ok(result)
    }
}

fn build_index(
    objects: &[TimedObject],
    nodes: &mut Vec<IntervalNode>,
    first: usize,
    end: usize,
) -> Option<usize> {
    if first == end {
        return None;
    }
    let middle = first + (end - first) / 2;
    let left = build_index(objects, nodes, first, middle);
    let right = build_index(objects, nodes, middle + 1, end);
    let mut max_end = objects[middle]
        .time
        .end
        .unwrap_or(objects[middle].time.start)
        .as_nanos();
    for child in [left, right].into_iter().flatten() {
        max_end = max_end.max(nodes[child].max_end);
    }
    let index = nodes.len();
    nodes.push(IntervalNode {
        index: middle,
        left,
        right,
        min_start: objects[first].time.start.as_nanos(),
        max_end,
    });
    Some(index)
}
