use crate::{BmsChart, BmsError, BmsErrorKind, BmsLane};
use beatkernel::{audio::SampleId, chart::*, time::Timestamp};
use std::collections::BTreeSet;

/// An unjudged keysound selection on the chart's independent invisible grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvisibleEvent {
    /// Nonnegative position in `BmsChart::invisible_ticks_per_beat` ticks.
    pub beat: Beat,
    /// Original visible lane identity, mapped from an invisible channel.
    pub lane: BmsLane,
    /// Defined, nonzero WAV resource identity.
    pub sample: SampleId,
    /// Independent invisible-token ordinal for simultaneous ordering.
    pub ordinal: u64,
    /// Original one-based physical source line.
    pub line: usize,
}

/// A timed keysound selection; neither automatic BGM nor a judged object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledInvisible {
    /// Exact core-compiled song timestamp, before any STOP at the same beat.
    pub at: Timestamp,
    /// Original visible lane identity.
    pub lane: BmsLane,
    /// Original WAV resource identity.
    pub sample: SampleId,
    /// Original invisible-token ordinal.
    pub ordinal: u64,
    /// Original physical source line.
    pub line: usize,
}

impl BmsChart {
    /// Compiles invisible selections separately through checked core timing.
    /// Fabricated grids, identities, duplicate positions and combined source
    /// counts are validated before building the timing-only chart. The parsed
    /// gameplay, BGM and visual grids are never changed by this operation.
    pub fn compile_invisible(&self) -> Result<Vec<ScheduledInvisible>, BmsError> {
        [
            self.source.objects.len(),
            self.source.bpm_changes.len(),
            self.source.stops.len(),
            self.source.scroll_changes.len(),
            self.bgm.len(),
            self.bga.len(),
            self.bga_opacity.len(),
            self.invisible.len(),
        ]
        .into_iter()
        .try_fold(0usize, |count, len| count.checked_add(len))
        .filter(|count| *count <= MAX_SOURCE_ITEMS)
        .ok_or_else(|| BmsError::new(0, BmsErrorKind::Limit("source items")))?;

        let original = self.source.ticks_per_beat;
        let resolution = self.invisible_ticks_per_beat;
        if original == 0 || resolution == 0 || resolution % original != 0 {
            return Err(BmsError::new(0, BmsErrorKind::Resolution));
        }
        let mut positions = BTreeSet::new();
        let mut ordinals = BTreeSet::new();
        for event in &self.invisible {
            if !matches!(event.lane.channel(), 0x11..=0x19 | 0x21..=0x29)
                || !(1..=3843).contains(&event.sample.0)
            {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::Syntax("invalid invisible lane or sample identity"),
                ));
            }
            let index = event.sample.0 as u16;
            if !self.samples.contains_key(&index) {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::MissingDefinition { kind: "WAV", index },
                ));
            }
            if !positions.insert((event.beat, event.lane)) {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::Duplicate("invisible lane position"),
                ));
            }
            if !ordinals.insert(event.ordinal) {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::Duplicate("invisible ordinal"),
                ));
            }
        }

        let factor = i64::from(resolution / original);
        let rescale = |beat: Beat| -> Result<Beat, BmsError> {
            let tick = beat
                .ticks()
                .checked_mul(factor)
                .ok_or_else(|| BmsError::new(0, BmsErrorKind::Overflow))?;
            Beat::new(tick).map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))
        };
        let allocation = |_| BmsError::new(0, BmsErrorKind::Limit("invisible timing allocation"));
        let mut timing = SourceChart::new(resolution, self.source.initial_bpm)
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
        timing
            .bpm_changes
            .try_reserve_exact(self.source.bpm_changes.len())
            .map_err(allocation)?;
        timing
            .stops
            .try_reserve_exact(self.source.stops.len())
            .map_err(allocation)?;
        timing
            .objects
            .try_reserve_exact(self.invisible.len())
            .map_err(allocation)?;
        for marker in &self.source.bpm_changes {
            timing.bpm_changes.push(BpmChange {
                beat: rescale(marker.beat)?,
                bpm: marker.bpm,
            });
        }
        for marker in &self.source.stops {
            timing.stops.push(Stop {
                beat: rescale(marker.beat)?,
                duration: marker.duration,
            });
        }
        for (index, event) in self.invisible.iter().enumerate() {
            timing.objects.push(SourceObject {
                id: ObjectId(index as u64),
                start: event.beat,
                end: None,
                interaction: InteractionId(0),
                visual: VisualId(0),
                audio: None,
                metadata: ObjectMetadata::default(),
            });
        }
        let compiled = timing
            .compile()
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
        let mut scheduled = Vec::new();
        scheduled
            .try_reserve_exact(self.invisible.len())
            .map_err(allocation)?;
        for object in compiled.objects() {
            let event = self.invisible[object.id.0 as usize];
            scheduled.push(ScheduledInvisible {
                at: object.time.start,
                lane: event.lane,
                sample: event.sample,
                ordinal: event.ordinal,
                line: event.line,
            });
        }
        scheduled.sort_by_key(|event| (event.at, event.ordinal));
        Ok(scheduled)
    }
}
