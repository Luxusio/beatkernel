use crate::time::{Duration, Timestamp};
use std::fmt;

/// A nonnegative source position measured in chart ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Beat(i64);

impl Beat {
    /// Constructs a position, rejecting negative ticks.
    pub fn new(ticks: i64) -> Result<Self, ChartError> {
        if ticks < 0 {
            return Err(ChartError::InvalidBeat);
        }
        Ok(Self(ticks))
    }

    /// Returns the source tick count.
    pub const fn ticks(self) -> i64 {
        self.0
    }
}

/// Positive, normalized rational beats per minute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bpm {
    numerator: u32,
    denominator: u32,
}

impl Bpm {
    /// Constructs a tempo, rejecting zero numerator or denominator.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, ChartError> {
        if numerator == 0 || denominator == 0 {
            return Err(ChartError::InvalidBpm);
        }
        let divisor = gcd(u64::from(numerator), u64::from(denominator)) as u32;
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    /// Returns the normalized numerator.
    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    /// Returns the normalized denominator.
    pub const fn denominator(self) -> u32 {
        self.denominator
    }
}

/// A normalized signed rational visual speed, including zero and reverse.
///
/// This value never changes an object's judge target time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollVelocity {
    numerator: i64,
    denominator: u32,
}

impl ScrollVelocity {
    /// Constructs a visual speed, rejecting a zero denominator.
    pub fn new(numerator: i64, denominator: u32) -> Result<Self, ChartError> {
        if denominator == 0 {
            return Err(ChartError::InvalidScrollVelocity);
        }
        let divisor = gcd(numerator.unsigned_abs(), u64::from(denominator));
        Ok(Self {
            numerator: numerator / divisor as i64,
            denominator: denominator / divisor as u32,
        })
    }

    /// Returns the normalized signed numerator.
    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    /// Returns the normalized denominator.
    pub const fn denominator(self) -> u32 {
        self.denominator
    }
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

/// A unique object identifier within a chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u64);

/// An opaque game-defined interaction identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionId(pub u32);

/// An opaque game-defined visual identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VisualId(pub u32);

/// An opaque audio asset or action binding interpreted by a caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AudioBinding(pub u32);

/// Owned opaque game-specific object metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectMetadata(pub Vec<u8>);

/// A source tempo marker applying to intervals after its beat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BpmChange {
    /// Source position.
    pub beat: Beat,
    /// Tempo following this position.
    pub bpm: Bpm,
}

/// A pause that shifts positions strictly after its beat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    /// Source position.
    pub beat: Beat,
    /// Nonnegative pause duration, validated during compilation.
    pub duration: Duration,
}

/// A source visual speed marker independent of judge timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollChange {
    /// Source position.
    pub beat: Beat,
    /// Visual speed following this position.
    pub velocity: ScrollVelocity,
}

/// A game-defined object positioned on the source beat timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceObject {
    /// Unique chart-local identity.
    pub id: ObjectId,
    /// Object start position.
    pub start: Beat,
    /// Optional inclusive endpoint, which must not precede the start.
    pub end: Option<Beat>,
    /// Game-defined interaction binding.
    pub interaction: InteractionId,
    /// Game-defined visual binding.
    pub visual: VisualId,
    /// Optional audio binding.
    pub audio: Option<AudioBinding>,
    /// Opaque game-defined metadata.
    pub metadata: ObjectMetadata,
}

/// An editable source chart compiled outside real-time callbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceChart {
    /// Nonzero number of source ticks per beat, validated at compilation too.
    pub ticks_per_beat: u32,
    /// Tempo at the beginning of the chart.
    pub initial_bpm: Bpm,
    /// Tempo markers; source order is unrestricted.
    pub bpm_changes: Vec<BpmChange>,
    /// Pause markers; source order is unrestricted.
    pub stops: Vec<Stop>,
    /// Visual speed markers; source order is unrestricted.
    pub scroll_changes: Vec<ScrollChange>,
    /// Objects; source order is unrestricted.
    pub objects: Vec<SourceObject>,
}

impl SourceChart {
    /// Constructs an empty chart, rejecting a zero tick resolution.
    pub fn new(ticks_per_beat: u32, initial_bpm: Bpm) -> Result<Self, ChartError> {
        if ticks_per_beat == 0 {
            return Err(ChartError::InvalidResolution);
        }
        Ok(Self {
            ticks_per_beat,
            initial_bpm,
            bpm_changes: Vec::new(),
            stops: Vec::new(),
            scroll_changes: Vec::new(),
            objects: Vec::new(),
        })
    }
}

/// Absolute song timestamps for a point or ranged object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    /// Start timestamp.
    pub start: Timestamp,
    /// Optional endpoint timestamp.
    pub end: Option<Timestamp>,
}

/// An object with compiled absolute song time and preserved game bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedObject {
    /// Unique chart-local identity.
    pub id: ObjectId,
    /// Compiled start and optional end timestamps.
    pub time: TimeRange,
    /// Game-defined interaction binding.
    pub interaction: InteractionId,
    /// Game-defined visual binding.
    pub visual: VisualId,
    /// Optional audio binding.
    pub audio: Option<AudioBinding>,
    /// Opaque game-defined metadata.
    pub metadata: ObjectMetadata,
}

/// A tempo marker at its absolute pre-STOP song time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedBpmChange {
    /// Absolute timestamp before any same-beat STOP.
    pub time: Timestamp,
    /// Tempo following the marker.
    pub bpm: Bpm,
}

/// A pause marker at its absolute pre-STOP song time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedStop {
    /// Absolute timestamp before this STOP.
    pub time: Timestamp,
    /// Pause duration.
    pub duration: Duration,
}

/// A visual speed marker at its absolute pre-STOP song time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedScrollChange {
    /// Absolute timestamp before any same-beat STOP.
    pub time: Timestamp,
    /// Visual speed following the marker.
    pub velocity: ScrollVelocity,
}

/// An immutable owned chart with objects sorted by start time, then ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledChart {
    pub(crate) ticks_per_beat: u32,
    pub(crate) initial_bpm: Bpm,
    pub(crate) objects: Vec<TimedObject>,
    pub(crate) bpm_changes: Vec<TimedBpmChange>,
    pub(crate) stops: Vec<TimedStop>,
    pub(crate) scroll_changes: Vec<TimedScrollChange>,
}

impl CompiledChart {
    /// Returns the source tick resolution.
    pub const fn ticks_per_beat(&self) -> u32 {
        self.ticks_per_beat
    }

    /// Returns the initial source tempo.
    pub const fn initial_bpm(&self) -> Bpm {
        self.initial_bpm
    }

    /// Borrows all objects, sorted by start timestamp and then object ID.
    pub fn objects(&self) -> &[TimedObject] {
        &self.objects
    }

    /// Borrows tempo markers in source-beat order.
    pub fn bpm_changes(&self) -> &[TimedBpmChange] {
        &self.bpm_changes
    }

    /// Borrows STOP markers in source-beat order.
    pub fn stops(&self) -> &[TimedStop] {
        &self.stops
    }

    /// Borrows visual markers separately from object judge targets.
    pub fn scroll_changes(&self) -> &[TimedScrollChange] {
        &self.scroll_changes
    }

    /// Borrows objects whose start timestamp is in `[start, end)`.
    ///
    /// Empty or reversed windows return an empty slice. This lookup does not
    /// allocate and does not include objects merely overlapping the window.
    pub fn objects_in_window(&self, start: Timestamp, end: Timestamp) -> &[TimedObject] {
        let first = self
            .objects
            .partition_point(|object| object.time.start < start);
        if end <= start {
            return &self.objects[first..first];
        }
        let last = self
            .objects
            .partition_point(|object| object.time.start < end);
        &self.objects[first..last]
    }
}

/// A validation, arithmetic, or capacity error without a partial compiled chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartError {
    /// Source ticks must be nonnegative.
    InvalidBeat,
    /// Tick resolution must be nonzero.
    InvalidResolution,
    /// Tempo numerator and denominator must be nonzero.
    InvalidBpm,
    /// Visual speed denominator must be nonzero.
    InvalidScrollVelocity,
    /// A STOP duration is negative.
    NegativeStop {
        /// Position of the invalid STOP.
        beat: Beat,
    },
    /// More than one BPM marker occupies a beat.
    DuplicateBpm {
        /// Duplicate source position.
        beat: Beat,
    },
    /// More than one STOP occupies a beat.
    DuplicateStop {
        /// Duplicate source position.
        beat: Beat,
    },
    /// More than one visual speed marker occupies a beat.
    DuplicateScroll {
        /// Duplicate source position.
        beat: Beat,
    },
    /// More than one object has the same chart-local ID.
    DuplicateObjectId {
        /// Duplicate object identity.
        id: ObjectId,
    },
    /// An object's endpoint precedes its start.
    ReversedRange {
        /// Identity of the invalid object.
        id: ObjectId,
    },
    /// An intermediate or final song time cannot be represented.
    Overflow,
    /// Item count or required allocation exceeds supported capacity.
    TooManyItems,
}

impl fmt::Display for ChartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBeat => formatter.write_str("source beat ticks must be nonnegative"),
            Self::InvalidResolution => formatter.write_str("ticks per beat must be nonzero"),
            Self::InvalidBpm => {
                formatter.write_str("BPM numerator and denominator must be positive")
            }
            Self::InvalidScrollVelocity => {
                formatter.write_str("scroll denominator must be positive")
            }
            Self::NegativeStop { beat } => {
                write!(formatter, "negative STOP at tick {}", beat.ticks())
            }
            Self::DuplicateBpm { beat } => {
                write!(formatter, "duplicate BPM at tick {}", beat.ticks())
            }
            Self::DuplicateStop { beat } => {
                write!(formatter, "duplicate STOP at tick {}", beat.ticks())
            }
            Self::DuplicateScroll { beat } => {
                write!(formatter, "duplicate scroll at tick {}", beat.ticks())
            }
            Self::DuplicateObjectId { id } => write!(formatter, "duplicate object ID {}", id.0),
            Self::ReversedRange { id } => {
                write!(formatter, "reversed range for object ID {}", id.0)
            }
            Self::Overflow => formatter.write_str("chart song time overflow"),
            Self::TooManyItems => formatter.write_str("chart exceeds supported item capacity"),
        }
    }
}

impl std::error::Error for ChartError {}
