use crate::{BmsChart, BmsError, BmsErrorKind, BmsLane};
use beatkernel::{chart::*, time::Timestamp};
use std::collections::BTreeSet;

/// Exact nonzero base36 mine damage, with ZZ reserved for instant death.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MineDamage(u16);

impl MineDamage {
    /// Accepts 1..=1295; zero is a rest, not a mine damage value.
    pub fn from_raw(raw: u16) -> Result<Self, BmsErrorKind> {
        if !(1..=1295).contains(&raw) {
            return Err(BmsErrorKind::Syntax(
                "mine damage requires nonzero base36 value",
            ));
        }
        Ok(Self(raw))
    }

    /// Original numeric base36 token, without conversion to a WAV identity.
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Whether this is the distinct ZZ instant-death token.
    pub const fn is_fatal(self) -> bool {
        self.0 == 1295
    }

    /// Exact half-percent units for nonfatal values, including values above 100%.
    /// Fatal damage has no numeric percentage representation.
    pub const fn half_percent_units(self) -> Option<u16> {
        if self.is_fatal() { None } else { Some(self.0) }
    }
}

/// Original mine selection on the independent exact mine grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MineEvent {
    /// Nonnegative position in `BmsChart::mine_ticks_per_beat` ticks.
    pub beat: Beat,
    /// Original visible lane identity, mapped from a D/E mine channel.
    pub lane: BmsLane,
    /// Validated damage token, independent of resource radix and WAV definitions.
    pub damage: MineDamage,
    /// Independent mine-token ordinal for simultaneous ordering.
    pub ordinal: u64,
    /// Original one-based physical source line.
    pub line: usize,
}

/// A separately timed hazard selection; not a judged object or automatic sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledMine {
    /// Exact core-compiled song time, before a STOP at the same beat.
    pub at: Timestamp,
    /// Original visible lane identity.
    pub lane: BmsLane,
    /// Original validated damage token.
    pub damage: MineDamage,
    /// Original mine-token ordinal.
    pub ordinal: u64,
    /// Original physical source line.
    pub line: usize,
}

impl BmsChart {
    /// Compiles mines through checked core timing without modifying the ordinary
    /// chart. Fabricated grids, lane positions, ordinals and total source counts
    /// are validated before constructing the temporary timing-only chart.
    pub fn compile_mines(&self) -> Result<Vec<ScheduledMine>, BmsError> {
        [
            self.source.objects.len(),
            self.source.bpm_changes.len(),
            self.source.stops.len(),
            self.source.scroll_changes.len(),
            self.bgm.len(),
            self.bga.len(),
            self.bga_opacity.len(),
            self.invisible.len(),
            self.mines.len(),
        ]
        .into_iter()
        .try_fold(0usize, |count, len| count.checked_add(len))
        .filter(|count| *count <= MAX_SOURCE_ITEMS)
        .ok_or_else(|| BmsError::new(0, BmsErrorKind::Limit("source items")))?;

        let original = self.source.ticks_per_beat;
        let resolution = self.mine_ticks_per_beat;
        if original == 0 || resolution == 0 || resolution % original != 0 {
            return Err(BmsError::new(0, BmsErrorKind::Resolution));
        }
        let mut positions = BTreeSet::new();
        let mut ordinals = BTreeSet::new();
        for event in &self.mines {
            if !matches!(event.lane.channel(), 0x11..=0x19 | 0x21..=0x29) {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::Syntax("invalid mine lane"),
                ));
            }
            if !positions.insert((event.beat, event.lane)) {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::Duplicate("mine lane position"),
                ));
            }
            if !ordinals.insert(event.ordinal) {
                return Err(BmsError::new(
                    event.line,
                    BmsErrorKind::Duplicate("mine ordinal"),
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
        let allocation = |_| BmsError::new(0, BmsErrorKind::Limit("mine timing allocation"));
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
            .try_reserve_exact(self.mines.len())
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
        for (index, event) in self.mines.iter().enumerate() {
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
            .try_reserve_exact(self.mines.len())
            .map_err(allocation)?;
        for object in compiled.objects() {
            let event = self.mines[object.id.0 as usize];
            scheduled.push(ScheduledMine {
                at: object.time.start,
                lane: event.lane,
                damage: event.damage,
                ordinal: event.ordinal,
                line: event.line,
            });
        }
        scheduled.sort_by_key(|event| (event.at, event.ordinal));
        Ok(scheduled)
    }
}
