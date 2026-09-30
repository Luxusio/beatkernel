use beatkernel::{
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, TimedObject,
        VisualId,
    },
    time::{Duration, Timestamp},
    visual::{
        CustomProjection, CustomProjectionContext, CustomProjectionHandle, CustomRenderState,
        Projection, RenderFrame, RenderObjectState, VisualBinding, VisualError, VisualProjector,
    },
};
use std::{collections::BTreeMap, error::Error, fmt::Write, sync::Arc};

#[derive(Debug)]
struct Scene3D {
    geometry_ref: u64,
    points: Arc<[[f64; 3]]>,
    path: bool,
}
impl CustomProjection for Scene3D {
    fn validate(&self, object: &TimedObject) -> Result<(), VisualError> {
        if self.points.is_empty()
            || self.points.iter().flatten().any(|value| !value.is_finite())
            || self.points.iter().any(|point| point[2] <= -2.0)
            || (self.path
                && (self.points.len() < 2
                    || object.time.end.is_none_or(|end| end <= object.time.start)))
        {
            return Err(VisualError::InvalidGeometry);
        }
        Ok(())
    }
    fn project(
        &self,
        context: CustomProjectionContext<'_>,
    ) -> Result<CustomRenderState, VisualError> {
        let fraction = |time: Timestamp| {
            if let Some(end) = context.object.time.end {
                let elapsed = (i128::from(time.as_nanos())
                    - i128::from(context.object.time.start.as_nanos()))
                    as f64;
                let duration = (i128::from(end.as_nanos())
                    - i128::from(context.object.time.start.as_nanos()))
                    as f64;
                (elapsed / duration).clamp(0.0, 1.0)
            } else {
                let remaining = (i128::from(context.object.time.start.as_nanos())
                    - i128::from(time.as_nanos())) as f64;
                (1.0 - remaining / 500_000_000.0).clamp(0.0, 1.0)
            }
        };
        let progress = fraction(context.song_time);
        let coordinate = progress * (self.points.len() - 1) as f64;
        let index = coordinate.floor() as usize;
        let next = (index + 1).min(self.points.len() - 1);
        let weight = coordinate - index as f64;
        let mut values = [0.0; 16];
        for axis in 0..3 {
            values[axis] =
                self.points[index][axis] * (1.0 - weight) + self.points[next][axis] * weight;
        }
        values[6] = 1.0; // xyz position + xyzw identity target orientation.
        if self.path {
            values[7] = fraction(context.window_start);
            values[8] = fraction(context.window_end);
        }
        Ok(CustomRenderState {
            object: context.object.id,
            visual: context.object.visual,
            type_tag: if self.path { 2 } else { 1 },
            geometry_ref: self.geometry_ref,
            values,
            value_count: if self.path { 9 } else { 7 },
            progress,
        })
    }
}

// Perspective transform belongs entirely to this external SVG renderer.
fn perspective(point: [f64; 3]) -> [f64; 2] {
    let depth = 2.0 + point[2];
    [
        470.0 + point[0] * 100.0 / depth,
        150.0 - point[1] * 100.0 / depth,
    ]
}

fn main() -> Result<(), Box<dyn Error>> {
    let argument = std::env::args().nth(1).unwrap_or_else(|| "--help".into());
    if argument == "--help" {
        println!(
            "BeatKernel external SVG renderer\nUsage: visual --fixture\nWrites a finite SVG for lanes, path, radial approach and caller-defined 3D target/path states.\nNo native rendering, gameplay judging or hardware measurements."
        );
        return Ok(());
    }
    if argument != "--fixture" {
        return Err("expected --help or --fixture".into());
    }
    let mut chart = SourceChart::new(1000, Bpm::new(60, 1)?)?;
    let mut bindings = Vec::new();
    for lane in 0..4 {
        chart.objects.push(SourceObject {
            id: ObjectId(lane + 1),
            start: Beat::new(1000 + lane as i64 * 100)?,
            end: None,
            interaction: InteractionId(1),
            visual: VisualId(lane as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
        bindings.push(VisualBinding {
            id: VisualId(lane as u32),
            projection: Projection::Lane {
                lane: lane as u32,
                unit: Duration::from_nanos(1_000_000_000),
            },
        });
    }
    chart.objects.push(SourceObject {
        id: ObjectId(5),
        start: Beat::new(0)?,
        end: Some(Beat::new(2000)?),
        interaction: InteractionId(2),
        visual: VisualId(4),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    bindings.push(VisualBinding {
        id: VisualId(4),
        projection: Projection::Path {
            points: vec![[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]],
        },
    });
    chart.objects.push(SourceObject {
        id: ObjectId(6),
        start: Beat::new(1000)?,
        end: None,
        interaction: InteractionId(3),
        visual: VisualId(5),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    bindings.push(VisualBinding {
        id: VisualId(5),
        projection: Projection::Polar {
            center: [150.0, 170.0],
            angle: -std::f64::consts::FRAC_PI_4,
            radius: 35.0,
            approach_radius: 100.0,
            approach: Duration::from_nanos(1_000_000_000),
        },
    });
    let target: Arc<[[f64; 3]]> = Arc::from([[0.8, 0.5, 0.2]]);
    let path: Arc<[[f64; 3]]> = Arc::from([[-1.0, 0.0, 0.0], [0.0, 0.8, 1.0], [1.0, 0.0, 0.2]]);
    let geometry = BTreeMap::from([(601u64, Arc::clone(&target)), (701, Arc::clone(&path))]);
    for (id, visual, geometry_ref, points, ranged) in
        [(7, 6, 601, target, false), (8, 7, 701, path, true)]
    {
        chart.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(if ranged { 0 } else { 1000 })?,
            end: if ranged { Some(Beat::new(2000)?) } else { None },
            interaction: InteractionId(4),
            visual: VisualId(visual),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
        bindings.push(VisualBinding {
            id: VisualId(visual),
            projection: Projection::Custom {
                projection: CustomProjectionHandle::new(Scene3D {
                    geometry_ref,
                    points,
                    path: ranged,
                }),
            },
        });
    }
    let projector = VisualProjector::new(chart.compile()?, bindings)?;
    let mut frame = RenderFrame::default();
    projector.project(
        Timestamp::from_nanos(750_000_000),
        Timestamp::from_nanos(500_000_000),
        Timestamp::from_nanos(1_500_000_000),
        &[],
        &mut frame,
    )?;
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"480\" viewBox=\"0 0 640 480\">\n<rect width=\"640\" height=\"480\" fill=\"#101820\"/>\n<path d=\"M20 420H300\" stroke=\"white\"/>\n",
    );
    for object in &frame.objects {
        match object {
            RenderObjectState::Lane { lane, distance, .. } => {
                writeln!(
                    svg,
                    "<rect x=\"{}\" y=\"{}\" width=\"40\" height=\"12\" fill=\"#4dd0e1\"/>",
                    30 + lane * 65,
                    420.0 - distance * 250.0
                )?;
            }
            RenderObjectState::Point { position, .. } => {
                writeln!(
                    svg,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"10\" fill=\"orange\"/>",
                    position[0], position[1]
                )?;
            }
            RenderObjectState::Polar {
                center,
                angle,
                radius,
                approach_progress,
                ..
            } => {
                let position = [
                    center[0] + angle.cos() * radius,
                    center[1] + angle.sin() * radius,
                ];
                writeln!(svg, "<circle cx=\"{}\" cy=\"{}\" r=\"35\" stroke=\"#ffcc80\" fill=\"none\"/><circle cx=\"{}\" cy=\"{}\" r=\"8\" fill=\"#ffcc80\"/><text x=\"70\" y=\"70\" fill=\"white\">radial progress {:.2}</text>", center[0], center[1], position[0], position[1], approach_progress)?;
            }
            RenderObjectState::Custom(state) => {
                let points = geometry
                    .get(&state.geometry_ref)
                    .ok_or("renderer geometry reference missing")?;
                if state.type_tag == 2 {
                    let mut coordinates = String::new();
                    for point in points.iter() {
                        let [x, y] = perspective(*point);
                        write!(coordinates, "{x},{y} ")?;
                    }
                    writeln!(svg, "<polyline points=\"{coordinates}\" stroke=\"#a5d6a7\" stroke-width=\"3\" fill=\"none\"/>")?;
                }
                let [x, y] = perspective([state.values[0], state.values[1], state.values[2]]);
                writeln!(svg, "<circle cx=\"{x}\" cy=\"{y}\" r=\"{}\" stroke=\"#a5d6a7\" fill=\"none\"/><text x=\"350\" y=\"{}\" fill=\"white\">3D {} xyz {:.2},{:.2},{:.2}</text>", 6.0 + (1.0 - state.progress) * 12.0, if state.type_tag == 1 { 70 } else { 95 }, if state.type_tag == 1 { "target" } else { "path" }, state.values[0], state.values[1], state.values[2])?;
            }
            RenderObjectState::Path { visual, head, .. } => {
                if let Some(Projection::Path { points }) = projector.projection(*visual) {
                    let mut coordinates = String::new();
                    for point in points {
                        write!(
                            coordinates,
                            "{},{} ",
                            350.0 + point[0] * 230.0,
                            400.0 - point[1] * 250.0
                        )?;
                    }
                    writeln!(
                        svg,
                        "<polyline points=\"{coordinates}\" stroke=\"#ce93d8\" fill=\"none\" stroke-width=\"4\"/>"
                    )?;
                    writeln!(
                        svg,
                        "<circle cx=\"{}\" cy=\"{}\" r=\"8\" fill=\"white\"/>",
                        350.0 + head[0] * 230.0,
                        400.0 - head[1] * 250.0
                    )?;
                }
            }
        }
    }
    svg.push_str("</svg>");
    println!("{svg}");
    Ok(())
}
