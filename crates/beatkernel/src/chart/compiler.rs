use super::*;
use crate::time::Timestamp;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum combined count of objects and timing/visual markers in one chart.
///
/// This bounds compiler sorting and indexing work. Metadata bytes remain owned
/// by the caller and should be bounded by any untrusted file adapter.
pub const MAX_SOURCE_ITEMS: usize = 1_000_000;

impl SourceChart {
    /// Compiles this chart into absolute song time without modifying the source.
    ///
    /// Compilation allocates and belongs outside real-time callbacks. Objects
    /// and markers at a STOP beat receive its pre-STOP timestamp; only later
    /// beats are delayed. Fractional nanoseconds truncate at genuine timing
    /// boundaries. Redundant tempo changes and zero STOPs do not re-anchor.
    pub fn compile(&self) -> Result<CompiledChart, ChartError> {
        compile(self)
    }
}

/// Compiles a source chart with checked integer timing and deterministic order.
pub fn compile(source: &SourceChart) -> Result<CompiledChart, ChartError> {
    if source.ticks_per_beat == 0 {
        return Err(ChartError::InvalidResolution);
    }
    let count = [
        source.objects.len(),
        source.bpm_changes.len(),
        source.stops.len(),
        source.scroll_changes.len(),
    ]
    .into_iter()
    .try_fold(0usize, |sum, size| sum.checked_add(size))
    .filter(|count| *count <= MAX_SOURCE_ITEMS)
    .ok_or(ChartError::TooManyItems)?;

    let mut bpms = BTreeMap::new();
    let mut stops = BTreeMap::new();
    let mut scrolls = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut beats = Vec::new();
    beats
        .try_reserve_exact(count * 2 + 1)
        .map_err(|_| ChartError::TooManyItems)?;
    beats.push(Beat::default());
    for event in &source.bpm_changes {
        if bpms.insert(event.beat, event.bpm).is_some() {
            return Err(ChartError::DuplicateBpm { beat: event.beat });
        }
        beats.push(event.beat);
    }
    for event in &source.stops {
        if event.duration.as_nanos() < 0 {
            return Err(ChartError::NegativeStop { beat: event.beat });
        }
        if stops.insert(event.beat, event.duration).is_some() {
            return Err(ChartError::DuplicateStop { beat: event.beat });
        }
        beats.push(event.beat);
    }
    for event in &source.scroll_changes {
        if scrolls.insert(event.beat, event.velocity).is_some() {
            return Err(ChartError::DuplicateScroll { beat: event.beat });
        }
        beats.push(event.beat);
    }
    for object in &source.objects {
        if !ids.insert(object.id) {
            return Err(ChartError::DuplicateObjectId { id: object.id });
        }
        if object.end.is_some_and(|end| end < object.start) {
            return Err(ChartError::ReversedRange { id: object.id });
        }
        beats.push(object.start);
        if let Some(end) = object.end {
            beats.push(end);
        }
    }
    beats.sort_unstable();
    beats.dedup();

    let mut times = BTreeMap::new();
    let mut anchor_beat = Beat::default();
    let mut anchor_time = 0i128;
    let mut bpm = source.initial_bpm;
    for beat in beats {
        let elapsed = beat_nanos(
            beat.ticks() - anchor_beat.ticks(),
            bpm,
            source.ticks_per_beat,
        )?;
        let before_stop = anchor_time
            .checked_add(elapsed)
            .ok_or(ChartError::Overflow)?;
        let timestamp = checked_timestamp(before_stop)?;
        times.insert(beat, timestamp);

        if let Some(&next) = bpms.get(&beat) {
            if next != bpm {
                anchor_beat = beat;
                anchor_time = before_stop;
                bpm = next;
            }
        }
        if let Some(duration) = stops.get(&beat).filter(|duration| duration.as_nanos() > 0) {
            anchor_beat = beat;
            anchor_time = before_stop
                .checked_add(i128::from(duration.as_nanos()))
                .ok_or(ChartError::Overflow)?;
            checked_timestamp(anchor_time)?;
        }
    }

    let mut objects: Vec<_> = source
        .objects
        .iter()
        .map(|object| TimedObject {
            id: object.id,
            time: TimeRange {
                start: times[&object.start],
                end: object.end.map(|end| times[&end]),
            },
            interaction: object.interaction,
            visual: object.visual,
            audio: object.audio,
            metadata: object.metadata.clone(),
        })
        .collect();
    objects.sort_unstable_by_key(|object| (object.time.start, object.id));
    Ok(CompiledChart {
        ticks_per_beat: source.ticks_per_beat,
        initial_bpm: source.initial_bpm,
        objects,
        bpm_changes: bpms
            .into_iter()
            .map(|(beat, bpm)| TimedBpmChange {
                time: times[&beat],
                bpm,
            })
            .collect(),
        stops: stops
            .into_iter()
            .map(|(beat, duration)| TimedStop {
                time: times[&beat],
                duration,
            })
            .collect(),
        scroll_changes: scrolls
            .into_iter()
            .map(|(beat, velocity)| TimedScrollChange {
                time: times[&beat],
                velocity,
            })
            .collect(),
    })
}

fn beat_nanos(ticks: i64, bpm: Bpm, resolution: u32) -> Result<i128, ChartError> {
    let numerator = i128::from(ticks)
        .checked_mul(60_000_000_000)
        .and_then(|value| value.checked_mul(i128::from(bpm.denominator())))
        .ok_or(ChartError::Overflow)?;
    let denominator = i128::from(resolution) * i128::from(bpm.numerator());
    Ok(numerator / denominator)
}

fn checked_timestamp(nanos: i128) -> Result<Timestamp, ChartError> {
    i64::try_from(nanos)
        .map(Timestamp::from_nanos)
        .map_err(|_| ChartError::Overflow)
}
