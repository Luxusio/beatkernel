use beatkernel::chart::{
    Beat, Bpm, BpmChange, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, Stop,
    VisualId,
};
use beatkernel::time::{Duration, Timestamp};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = SourceChart::new(480, Bpm::new(120, 1)?)?;
    source.bpm_changes.push(BpmChange {
        beat: Beat::new(480)?,
        bpm: Bpm::new(60, 1)?,
    });
    source.stops.push(Stop {
        beat: Beat::new(480)?,
        duration: Duration::from_nanos(250_000_000),
    });
    for (id, start, end) in [(1, 480, None), (2, 960, Some(1440))] {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(start)?,
            end: end.map(Beat::new).transpose()?,
            interaction: InteractionId(0),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let compiled = source.compile()?;
    for object in compiled.objects() {
        println!(
            "object={} start_ns={} end_ns={:?}",
            object.id.0,
            object.time.start.as_nanos(),
            object.time.end.map(Timestamp::as_nanos)
        );
    }
    let first_second =
        compiled.objects_in_window(Timestamp::ZERO, Timestamp::from_nanos(1_000_000_000));
    println!("objects starting in first second={}", first_second.len());
    Ok(())
}
