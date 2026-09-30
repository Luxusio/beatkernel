use beatkernel::{
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    time::{Duration, Timestamp},
    visual::{Projection, RenderFrame, RenderObjectState, VisualBinding, VisualProjector},
};
use std::{error::Error, fmt::Write};

fn main() -> Result<(), Box<dyn Error>> {
    let argument = std::env::args().nth(1).unwrap_or_else(|| "--help".into());
    if argument == "--help" {
        println!(
            "BeatKernel external SVG renderer\nUsage: visual --fixture\nWrites a finite SVG for four lanes and a path from logical RenderFrame states.\nNo native rendering, gameplay judging or hardware measurements."
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
