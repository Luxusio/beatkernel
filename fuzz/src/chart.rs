//! Bounded structured chart inputs. All integers use `Unstructured::arbitrary`
//! (little endian); tags and counts use the indicated remainder.
//!
//! Schema: mode u8 % 8, domain u8 % 2, resolution u32, initial BPM u32/u32,
//! then four u8 counts: objects % 65, BPM/STOP/scroll markers each % 17.
//! A beat is u32 in domain 0 and i64 in domain 1. Objects contain id u64,
//! start beat, end tag u8 % 2 (1 adds a u32 delta in domain 0, or an absolute
//! i64 endpoint in domain 1), interaction u32, visual u32, audio tag u8 % 2
//! (1 adds u32), metadata length u8 % 65 and that many exact bytes.
//! BPM records contain beat and u32/u32; STOP records contain beat and u32
//! nanoseconds in domain 0 or i64 in domain 1; scroll records contain beat,
//! i64/u32 velocity. Records follow in objects, BPM, STOP, scroll order.
//! The fixed 18-byte header must be present. Remaining integer fields follow
//! arbitrary's zero-padding semantics when bytes run out; metadata requires its
//! exact declared bytes. Trailing bytes are ignored. Invalid constructors and
//! missing metadata reject normally. Domain 0 provides frequent valid charts;
//! domain 1 retains the full
//! signed source range, arbitrary identifiers and opaque metadata.
//!
//! Modes mutate the constructed source: 0 unchanged; 1 unit tempo/resolution,
//! no BPM/STOP and a maximum-tick point; 2 negative STOP at zero; 3 duplicate
//! maximum IDs; 4 maximum-ID reversed hold (1 to 0); 5 duplicate BPM at zero;
//! 6 duplicate zero-duration STOP at zero; 7 zero resolution. Insertions truncate
//! only enough entries to retain the campaign count budgets.

use arbitrary::Unstructured;
use beatkernel::chart::{
    AudioBinding, Beat, Bpm, BpmChange, CompiledChart, InteractionId, ObjectId, ObjectMetadata,
    ScrollChange, ScrollVelocity, SourceChart, SourceObject, Stop, VisualId,
};
use beatkernel::time::Duration;

pub const CHART_MAX_BYTES: usize = 4096;

/// Constructs the campaign source without suppressing compiler failures.
pub fn chart_from_bytes(data: &[u8]) -> Option<SourceChart> {
    if data.len() < 18 || data.len() > CHART_MAX_BYTES {
        return None;
    }
    let mut bytes = Unstructured::new(data);
    let mode = bytes.arbitrary::<u8>().ok()? % 8;
    let raw = bytes.arbitrary::<u8>().ok()? % 2 == 1;
    let resolution = bytes.arbitrary::<u32>().ok()?;
    let initial_bpm = read_bpm(&mut bytes)?;
    let mut source = SourceChart::new(resolution, initial_bpm).ok()?;
    let objects = usize::from(bytes.arbitrary::<u8>().ok()? % 65);
    let bpms = usize::from(bytes.arbitrary::<u8>().ok()? % 17);
    let stops = usize::from(bytes.arbitrary::<u8>().ok()? % 17);
    let scrolls = usize::from(bytes.arbitrary::<u8>().ok()? % 17);
    for _ in 0..objects {
        let id = ObjectId(bytes.arbitrary().ok()?);
        let start = read_beat(&mut bytes, raw)?;
        let end = if bytes.arbitrary::<u8>().ok()? % 2 == 1 {
            let ticks = if raw {
                bytes.arbitrary::<i64>().ok()?
            } else {
                start
                    .ticks()
                    .checked_add(i64::from(bytes.arbitrary::<u32>().ok()?))?
            };
            Some(Beat::new(ticks).ok()?)
        } else {
            None
        };
        let interaction = InteractionId(bytes.arbitrary().ok()?);
        let visual = VisualId(bytes.arbitrary().ok()?);
        let audio = if bytes.arbitrary::<u8>().ok()? % 2 == 1 {
            Some(AudioBinding(bytes.arbitrary().ok()?))
        } else {
            None
        };
        let metadata_len = usize::from(bytes.arbitrary::<u8>().ok()? % 65);
        let metadata = ObjectMetadata(bytes.bytes(metadata_len).ok()?.to_vec());
        source.objects.push(SourceObject {
            id,
            start,
            end,
            interaction,
            visual,
            audio,
            metadata,
        });
    }
    for _ in 0..bpms {
        source.bpm_changes.push(BpmChange {
            beat: read_beat(&mut bytes, raw)?,
            bpm: read_bpm(&mut bytes)?,
        });
    }
    for _ in 0..stops {
        let beat = read_beat(&mut bytes, raw)?;
        let nanos = if raw {
            bytes.arbitrary::<i64>().ok()?
        } else {
            i64::from(bytes.arbitrary::<u32>().ok()?)
        };
        source.stops.push(Stop {
            beat,
            duration: Duration::from_nanos(nanos),
        });
    }
    for _ in 0..scrolls {
        let beat = read_beat(&mut bytes, raw)?;
        let numerator = bytes.arbitrary::<i64>().ok()?;
        let denominator = bytes.arbitrary::<u32>().ok()?;
        source.scroll_changes.push(ScrollChange {
            beat,
            velocity: ScrollVelocity::new(numerator, denominator).ok()?,
        });
    }
    match mode {
        1 => {
            source.ticks_per_beat = 1;
            source.initial_bpm = Bpm::new(1, 1).ok()?;
            source.bpm_changes.clear();
            source.stops.clear();
            source.objects.truncate(63);
            source.objects.push(point(i64::MAX)?);
        }
        2 => {
            source.stops.truncate(15);
            source.stops.push(Stop {
                beat: Beat::default(),
                duration: Duration::from_nanos(-1),
            });
        }
        3 => {
            source.objects.truncate(62);
            source.objects.push(point(0)?);
            source.objects.push(point(0)?);
        }
        4 => {
            source.objects.truncate(63);
            let mut object = point(1)?;
            object.end = Some(Beat::default());
            source.objects.push(object);
        }
        5 => {
            source.bpm_changes.truncate(14);
            let marker = BpmChange {
                beat: Beat::default(),
                bpm: source.initial_bpm,
            };
            source.bpm_changes.extend([marker, marker]);
        }
        6 => {
            source.stops.truncate(14);
            let marker = Stop {
                beat: Beat::default(),
                duration: Duration::from_nanos(0),
            };
            source.stops.extend([marker, marker]);
        }
        7 => source.ticks_per_beat = 0,
        _ => {}
    }
    Some(source)
}

fn read_beat(bytes: &mut Unstructured<'_>, raw: bool) -> Option<Beat> {
    let ticks = if raw {
        bytes.arbitrary::<i64>().ok()?
    } else {
        i64::from(bytes.arbitrary::<u32>().ok()?)
    };
    Beat::new(ticks).ok()
}

fn read_bpm(bytes: &mut Unstructured<'_>) -> Option<Bpm> {
    Bpm::new(bytes.arbitrary().ok()?, bytes.arbitrary().ok()?).ok()
}

fn point(ticks: i64) -> Option<SourceObject> {
    Some(SourceObject {
        id: ObjectId(u64::MAX),
        start: Beat::new(ticks).ok()?,
        end: None,
        interaction: InteractionId(0),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    })
}

/// Returns true precisely when the primary production compilation succeeds.
pub fn check_chart(data: &[u8]) -> bool {
    let Some(source) = chart_from_bytes(data) else {
        return false;
    };
    let primary = source.compile();
    assert_eq!(primary, source.compile(), "compiler determinism");
    let Ok(compiled) = primary else {
        return false;
    };
    check_preservation(&source, &compiled);

    // Invalid charts can report a different first error after permutation. Only
    // successfully validated sources have the semantic permutation property.
    let mut reversed = source.clone();
    reversed.objects.reverse();
    reversed.bpm_changes.reverse();
    reversed.stops.reverse();
    reversed.scroll_changes.reverse();
    assert_eq!(Ok(compiled.clone()), reversed.compile());

    if source.bpm_changes.is_empty() && source.stops.is_empty() {
        check_constant_tempo(&source, &compiled);
    }
    true
}

fn check_preservation(source: &SourceChart, compiled: &CompiledChart) {
    assert_eq!(compiled.ticks_per_beat(), source.ticks_per_beat);
    assert_eq!(compiled.initial_bpm(), source.initial_bpm);
    assert_eq!(compiled.objects().len(), source.objects.len());
    assert_eq!(compiled.bpm_changes().len(), source.bpm_changes.len());
    assert_eq!(compiled.stops().len(), source.stops.len());
    assert_eq!(compiled.scroll_changes().len(), source.scroll_changes.len());
    let mut source_ids: Vec<_> = source.objects.iter().map(|object| object.id).collect();
    let mut compiled_ids: Vec<_> = compiled.objects().iter().map(|object| object.id).collect();
    source_ids.sort_unstable();
    compiled_ids.sort_unstable();
    assert_eq!(source_ids, compiled_ids);
    assert!(compiled
        .objects()
        .windows(2)
        .all(|pair| (pair[0].time.start, pair[0].id) <= (pair[1].time.start, pair[1].id)));
    for object in compiled.objects() {
        let original = source
            .objects
            .iter()
            .find(|candidate| candidate.id == object.id)
            .expect("compiled object must retain a source identity");
        assert_eq!(object.interaction, original.interaction);
        assert_eq!(object.visual, original.visual);
        assert_eq!(object.audio, original.audio);
        assert_eq!(object.metadata, original.metadata);
        assert_eq!(object.time.end.is_some(), original.end.is_some());
        assert!(object.time.start.as_nanos() >= 0);
        if let Some(end) = object.time.end {
            assert!(end >= object.time.start);
        }
    }
    let mut bpms = source.bpm_changes.clone();
    bpms.sort_unstable_by_key(|marker| marker.beat);
    for (original, timed) in bpms.iter().zip(compiled.bpm_changes()) {
        assert_eq!(original.bpm, timed.bpm);
    }
    let mut stops = source.stops.clone();
    stops.sort_unstable_by_key(|marker| marker.beat);
    for (original, timed) in stops.iter().zip(compiled.stops()) {
        assert_eq!(original.duration, timed.duration);
    }
    let mut scrolls = source.scroll_changes.clone();
    scrolls.sort_unstable_by_key(|marker| marker.beat);
    for (original, timed) in scrolls.iter().zip(compiled.scroll_changes()) {
        assert_eq!(original.velocity, timed.velocity);
    }
    assert!(compiled
        .bpm_changes()
        .windows(2)
        .all(|p| p[0].time <= p[1].time));
    assert!(compiled.stops().windows(2).all(|p| p[0].time <= p[1].time));
    assert!(compiled
        .scroll_changes()
        .windows(2)
        .all(|p| p[0].time <= p[1].time));
}

// This independent oracle applies only with one tempo and no STOP: time is
// floor(ticks * sixty-billion * BPM-denominator / (resolution * BPM-numerator)).
fn expected_nanos(source: &SourceChart, beat: Beat) -> Option<i64> {
    let nanos_per_minute = 60_000_000_000i128;
    let numerator = nanos_per_minute
        .checked_mul(i128::from(source.initial_bpm.denominator()))?
        .checked_mul(i128::from(beat.ticks()))?;
    let denominator = i128::from(source.ticks_per_beat)
        .checked_mul(i128::from(source.initial_bpm.numerator()))?;
    i64::try_from(numerator.checked_div(denominator)?).ok()
}

fn check_constant_tempo(source: &SourceChart, compiled: &CompiledChart) {
    for original in &source.objects {
        let object = compiled
            .objects()
            .iter()
            .find(|o| o.id == original.id)
            .unwrap();
        assert_eq!(
            Some(object.time.start.as_nanos()),
            expected_nanos(source, original.start)
        );
        assert_eq!(
            object.time.end.map(|end| end.as_nanos()),
            original.end.and_then(|beat| expected_nanos(source, beat))
        );
    }
    let mut scrolls = source.scroll_changes.clone();
    scrolls.sort_unstable_by_key(|marker| marker.beat);
    for (original, timed) in scrolls.iter().zip(compiled.scroll_changes()) {
        assert_eq!(
            Some(timed.time.as_nanos()),
            expected_nanos(source, original.beat)
        );
    }

    // Prove the scaled coordinates, resolution and intermediate integer product
    // fit before requiring grid invariance. This never treats a legitimate
    // overflow outside the proven domain as a fuzz failure.
    let Some(resolution) = source.ticks_per_beat.checked_mul(2) else {
        return;
    };
    let mut scaled = source.clone();
    scaled.ticks_per_beat = resolution;
    for object in &mut scaled.objects {
        let Some(start) = double_beat(object.start) else {
            return;
        };
        object.start = start;
        if let Some(end) = object.end {
            let Some(end) = double_beat(end) else {
                return;
            };
            object.end = Some(end);
        }
    }
    for marker in &mut scaled.scroll_changes {
        let Some(beat) = double_beat(marker.beat) else {
            return;
        };
        marker.beat = beat;
    }
    if scaled.objects.iter().any(|o| {
        expected_nanos(&scaled, o.start).is_none()
            || o.end
                .is_some_and(|end| expected_nanos(&scaled, end).is_none())
    }) || scaled
        .scroll_changes
        .iter()
        .any(|m| expected_nanos(&scaled, m.beat).is_none())
    {
        return;
    }
    let scaled_compiled = scaled
        .compile()
        .expect("proven exact grid scaling must compile");
    assert_eq!(compiled.objects(), scaled_compiled.objects());
    assert_eq!(compiled.scroll_changes(), scaled_compiled.scroll_changes());
    assert_eq!(compiled.initial_bpm(), scaled_compiled.initial_bpm());
}

fn double_beat(beat: Beat) -> Option<Beat> {
    Beat::new(beat.ticks().checked_mul(2)?).ok()
}
