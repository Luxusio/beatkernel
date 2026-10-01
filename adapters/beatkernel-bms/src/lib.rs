//! Bounded deterministic BMS text/profile adapter with no native dependencies.
//! Supports documented timing, lane, keysound, paired LNTYPE1 and LNOBJ features.
//! Long-note tail tokens are metadata only and never automatic sounds.
//! Asset paths are opaque references; loading/decoding belongs to the application.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
mod parser;
mod rational;
use beatkernel::{
    audio::SampleId,
    chart::*,
    input::GameControlId,
    interaction::{HoldEvaluator, InstantEvaluator},
    judge::Rule,
    time::Timestamp,
};
pub use parser::parse;
use std::collections::{BTreeMap, BTreeSet};

/// Policy for overlapping nonzero positions/definitions; zeros never delete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DuplicatePolicy {
    /// Reject repeated definitions or same-channel/same-position objects.
    #[default]
    Reject,
    /// Keep the later definition/nonzero same-channel position explicitly.
    /// BGM always layers, and conflicting timing channels still reject.
    LastWins,
}
/// Explicit parser resource limits and compatibility policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseOptions {
    /// Maximum UTF-8 text bytes before any parsing work.
    pub max_bytes: usize,
    /// Maximum input line count, including comments/empty lines.
    pub max_lines: usize,
    /// Maximum bytes in one physical line.
    pub max_line_bytes: usize,
    /// Maximum nonzero tokens and final source objects/markers/BGM combined.
    pub max_objects: usize,
    /// Maximum exact quarter-beat tick resolution (LCM); never silently rounded.
    pub max_resolution: u32,
    /// Policy for conflicting non-BGM channel positions and definitions.
    pub duplicates: DuplicatePolicy,
}
impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_lines: 100_000,
            max_line_bytes: 64 * 1024,
            max_objects: 100_000,
            max_resolution: 1_000_000_000,
            duplicates: DuplicatePolicy::Reject,
        }
    }
}
/// Canonical BMS visible lane identity, private to this adapter's conventions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BmsLane(pub(crate) u8);
impl BmsLane {
    /// Original visible channel code (0x11..0x19 or 0x21..0x29).
    pub const fn channel(self) -> u8 {
        self.0
    }
    /// Player side 1 or 2.
    pub const fn player(self) -> u8 {
        self.0 >> 4
    }
    /// Original channel column 1 through 9, retaining reserved/native identities.
    pub const fn column(self) -> u8 {
        self.0 & 15
    }
    /// Scratch channels are 0x16 and 0x26.
    pub const fn is_scratch(self) -> bool {
        self.column() == 6
    }
    /// Adapter-owned logical control, independent of native keys/devices.
    pub const fn control(self) -> GameControlId {
        GameControlId(self.0 as u32)
    }
}
/// Gameplay object mapping retained outside the generic core chart model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsNote {
    /// Unique chart-local compiled/source object identity.
    pub object: ObjectId,
    /// Original player/lane identity.
    pub lane: BmsLane,
    /// Head/instant keysound identity, equal to the base36 WAV index.
    pub sample: SampleId,
    /// Unsounded LNTYPE1 or LNOBJ endpoint token, even if WAV is undefined.
    pub tail_sample: Option<SampleId>,
    /// Source line of the head/instant token, for diagnostics.
    pub line: usize,
}
/// Exact source measure boundaries on the selected integer beat grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmsMeasure {
    /// Original 000..999 measure number.
    pub number: u16,
    /// Exact start position in quarter-beat ticks.
    pub start: Beat,
    /// Exact following measure boundary.
    pub end: Beat,
}
/// Layered BGM token before compilation into absolute song time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgmEvent {
    /// Exact source quarter-beat position.
    pub beat: Beat,
    /// WAV index mapped to a core sample identity.
    pub sample: SampleId,
    /// Acquisition ordinal, preserving simultaneous layered order.
    pub ordinal: u64,
}
/// BGM event scheduled at core-compiled pre-STOP song time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledBgm {
    /// Absolute song timestamp, requiring explicit audio-clock mapping by app.
    pub at: Timestamp,
    /// Referenced sample identity.
    pub sample: SampleId,
    /// Original source acquisition ordinal for equal-time ordering.
    pub ordinal: u64,
}
/// Explicitly ignored descriptive/visual feature, never silent timing fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsWarning {
    /// Source line with the ignored feature.
    pub line: usize,
    /// Human-readable unsupported visual feature description.
    pub message: String,
}
/// Parsed chart plus adapter-owned samples, lanes, BGM and metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsChart {
    /// Actual gameplay SourceChart; no BGM-only fake gameplay notes.
    pub source: SourceChart,
    /// Exact WAV paths by case-insensitive base36 index; assets are not opened.
    pub samples: BTreeMap<u16, String>,
    /// Lane/keysound mapping by chart-local object identity.
    pub notes: Vec<BmsNote>,
    /// Layered source BGM events in beat/ordinal order.
    pub bgm: Vec<BgmEvent>,
    /// Listed descriptive headers, preserved without invented grading/gauge rules.
    pub metadata: BTreeMap<String, String>,
    /// Explicit ignored visual features.
    pub warnings: Vec<BmsWarning>,
    /// Exact original measure boundaries.
    pub measures: Vec<BmsMeasure>,
}
/// Real compiled gameplay timeline and separately scheduled BGM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledBms {
    /// Actual chart consumed by the same live/replay JudgeEngine.
    pub chart: CompiledChart,
    /// Separately timed automatic keysounds, not judged gameplay objects.
    pub bgm: Vec<ScheduledBgm>,
}
impl BmsChart {
    /// Compiles gameplay and BGM with the same checked core BPM/STOP semantics.
    pub fn compile(&self) -> Result<CompiledBms, BmsError> {
        let chart = self
            .source
            .compile()
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
        // A separate timing-only chart projects BGM positions through the core
        // compiler without exposing automatic sounds as judged gameplay notes.
        let mut timing = self.source.clone();
        timing.objects.clear();
        timing.objects.extend(
            self.bgm
                .iter()
                .enumerate()
                .map(|(index, event)| SourceObject {
                    id: ObjectId(index as u64),
                    start: event.beat,
                    end: None,
                    interaction: InteractionId(0),
                    visual: VisualId(0),
                    audio: None,
                    metadata: ObjectMetadata::default(),
                }),
        );
        let timed = timing
            .compile()
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
        let mut bgm = Vec::with_capacity(self.bgm.len());
        for object in timed.objects() {
            let event = self.bgm[object.id.0 as usize];
            bgm.push(ScheduledBgm {
                at: object.time.start,
                sample: event.sample,
                ordinal: event.ordinal,
            });
        }
        bgm.sort_by_key(|event| (event.at, event.ordinal));
        Ok(CompiledBms { chart, bgm })
    }
    /// Creates lane-specific registrations using existing builtin evaluators.
    /// Timing windows/offsets stay caller-controlled; RANK is not guessed.
    pub fn rules(&self) -> Vec<Rule> {
        let controls: BTreeMap<_, _> = self
            .notes
            .iter()
            .map(|note| (note.object, note.lane.control()))
            .collect();
        let mut registered = BTreeSet::new();
        let mut rules = Vec::new();
        for object in &self.source.objects {
            let Some(&control) = controls.get(&object.id) else {
                continue;
            };
            if registered.insert(object.interaction) {
                let hold = object.end.is_some();
                rules.push(Rule {
                    interaction: object.interaction,
                    control,
                    evaluator: if hold {
                        Box::new(HoldEvaluator)
                    } else {
                        Box::new(InstantEvaluator)
                    },
                });
            }
        }
        rules
    }
}
/// Structured failure classification with an original source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BmsErrorKind {
    /// Input/options exceed explicit resource limits.
    Limit(&'static str),
    /// Invalid ASCII directive, token, decimal or channel syntax.
    Syntax(&'static str),
    /// Unsupported gameplay/timing/conditional feature; no partial chart returned.
    Unsupported(String),
    /// Conflicting non-BGM position or repeated definition.
    Duplicate(&'static str),
    /// Referenced WAV/BPM/STOP index has no definition.
    MissingDefinition {
        /// Definition kind (WAV, BPM or STOP).
        kind: &'static str,
        /// Original numeric base36 index.
        index: u16,
    },
    /// Exact arithmetic or target representation overflow.
    Overflow,
    /// Exact LCM resolution exceeds caller cap.
    Resolution,
    /// Invalid/dangling/overlapping long-note range.
    LongNote(&'static str),
    /// Core chart validation/compilation error.
    Compile(ChartError),
}
/// Parse/compile failure; line 0 identifies document-wide configuration errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsError {
    /// Original 1-based source line, or 0 for document-wide failures.
    pub line: usize,
    /// Precise structured diagnostic category.
    pub kind: BmsErrorKind,
}
impl BmsError {
    pub(crate) fn new(line: usize, kind: BmsErrorKind) -> Self {
        Self { line, kind }
    }
}
impl std::fmt::Display for BmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS line {}: {:?}", self.line, self.kind)
    }
}
impl std::error::Error for BmsError {}
