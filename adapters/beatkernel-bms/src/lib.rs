//! Bounded deterministic BMS text/profile adapter with no native dependencies.
//! Supports documented timing, lane, keysound, paired LNTYPE1 and LNOBJ features.
//! Seeded RANDOM/SETRANDOM and SWITCH flow resolve before payload interpretation.
//! Long-note tail tokens are metadata only and never automatic sounds.
//! BMP image selections compile separately without changing the gameplay grid.
//! Asset paths are opaque references; loading/decoding belongs to the application.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
mod conditional;
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
pub use parser::{parse, parse_seeded};
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
    /// Maximum nonzero tokens and final gameplay/timing/BGM/BGA items combined.
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
/// Opaque base36 BMP resource identity, including special initial Poor image 00.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageId(
    /// Original numeric base36 image index.
    pub u16,
);
/// Prepared 256×256 canvas definition using half-open source corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BgaCrop {
    /// Original BMP identity; crop definitions are not recursively resolved.
    pub source: ImageId,
    /// Signed source corners `[x1, y1, x2, y2]`.
    pub source_rect: [i32; 4],
    /// Signed placement `[dx, dy]` on the transparent canvas.
    pub destination: [i32; 2],
}
impl BgaCrop {
    /// Checks the base36 source range and strictly positive source extents.
    pub fn validate(&self) -> Result<(), String> {
        if self.source.0 > 1295 {
            return Err("BGA source identity exceeds two base36 digits".into());
        }
        let [x1, y1, x2, y2] = self.source_rect.map(i64::from);
        if x2 - x1 <= 0 || y2 - y1 <= 0 {
            return Err("BGA source rectangle must have positive extents".into());
        }
        Ok(())
    }
}
/// Static Poor-image activation mode; gameplay and timing are unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PoorBgaMode {
    /// Header 0 (also absent): replace normal Base/Layer with raw Poor.
    #[default]
    Replace,
    /// Header 1: retain normal Base/Layer and overlay raw Poor last.
    Overlay,
    /// Header 2: disable miss-driven Poor activation.
    Off,
}
impl PoorBgaMode {
    /// Parses an exact single POORBGA digit; no signs or guessed values.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "0" => Ok(Self::Replace),
            "1" => Ok(Self::Overlay),
            "2" => Ok(Self::Off),
            _ => Err("POORBGA requires exactly 0, 1 or 2".into()),
        }
    }
}
/// Independently selected BMS image layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BgaChannel {
    /// Channel 04 base image.
    Base,
    /// Channel 06 poor-image selection; activation policy belongs to the app.
    Poor,
    /// Channel 07 overlay image.
    Layer,
    /// Channel 0A second overlay image, composed above Layer.
    Layer2,
}
/// Visual selection on its independent exact quarter-beat grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgaEvent {
    /// Source position in BmsChart::bga_ticks_per_beat units.
    pub beat: Beat,
    /// Selected independent layer.
    pub channel: BgaChannel,
    /// Resource selection, even when its BMP definition is absent.
    pub image: ImageId,
    /// Visual-only acquisition ordinal for simultaneous ordering.
    pub ordinal: u64,
}
/// Image selection scheduled through the checked core pre-STOP timing compiler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledBga {
    /// Original absolute song time; no display clock is inferred.
    pub at: Timestamp,
    /// Selected independent layer.
    pub channel: BgaChannel,
    /// Opaque image resource reference.
    pub image: ImageId,
    /// Visual-only source acquisition ordinal.
    pub ordinal: u64,
}
/// Per-role alpha marker on the independent visual quarter-beat grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BgaOpacityEvent {
    /// Source position in BmsChart::bga_ticks_per_beat units.
    pub beat: Beat,
    /// Base/Layer/Layer2/Poor role selected by channels0B..0E.
    pub channel: BgaChannel,
    /// Exact nonzero hexadecimal byte, normalized only during drawing.
    pub alpha: u8,
    /// Shared visual-only acquisition order, including image selections.
    pub ordinal: u64,
}
/// Per-role opacity scheduled with checked core pre-STOP timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduledBgaOpacity {
    /// Original absolute song timestamp.
    pub at: Timestamp,
    /// Independent visual role.
    pub channel: BgaChannel,
    /// Original raw alpha byte.
    pub alpha: u8,
    /// Shared visual source ordinal for simultaneous ordering.
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
/// Parsed gameplay plus opaque audio/image resources and independent timelines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsChart {
    /// Actual gameplay SourceChart; no BGM-only fake gameplay notes.
    pub source: SourceChart,
    /// Exact WAV paths by case-insensitive base36 index; assets are not opened.
    pub samples: BTreeMap<u16, String>,
    /// Exact opaque BMP paths, including optional initial Poor resource 00.
    pub images: BTreeMap<ImageId, String>,
    /// Crop canvases by destination identity, separate from BMP definitions.
    pub bga_crops: BTreeMap<ImageId, BgaCrop>,
    /// Visual selections, kept outside the gameplay and audio grids.
    pub bga: Vec<BgaEvent>,
    /// Independent per-role opacity markers on the shared visual grid.
    pub bga_opacity: Vec<BgaOpacityEvent>,
    /// Independent visual tick grid encompassing the gameplay resolution.
    pub bga_ticks_per_beat: u32,
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
/// Real compiled gameplay and separately scheduled audio/image selections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledBms {
    /// Actual chart consumed by the same live/replay JudgeEngine.
    pub chart: CompiledChart,
    /// Separately timed automatic keysounds, not judged gameplay objects.
    pub bgm: Vec<ScheduledBgm>,
    /// Separately compiled visual image selections, never judged objects.
    pub bga: Vec<ScheduledBga>,
    /// Independently scheduled per-role opacity markers.
    pub bga_opacity: Vec<ScheduledBgaOpacity>,
}
impl BmsChart {
    /// Reads the preserved POORBGA header, rejecting fabricated invalid metadata.
    pub fn poor_bga_mode(&self) -> Result<PoorBgaMode, String> {
        self.metadata
            .get("POORBGA")
            .map_or(Ok(PoorBgaMode::default()), |value| {
                PoorBgaMode::parse(value)
            })
    }
    /// Compiles gameplay, BGM and visual selections with checked core timing.
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
        Ok(CompiledBms {
            chart,
            bgm,
            bga: self.compile_bga()?,
            bga_opacity: self.compile_bga_opacity()?,
        })
    }
    /// Compiles the visual grid separately, preserving gameplay/BGM rounding.
    /// Tempo/STOP beats are checked-rescaled; simultaneous events use pre-STOP time.
    pub fn compile_bga(&self) -> Result<Vec<ScheduledBga>, BmsError> {
        let mut timing = self.visual_timing()?;
        timing.objects.extend(
            self.bga
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
        let compiled = timing
            .compile()
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
        let mut scheduled = Vec::with_capacity(self.bga.len());
        for object in compiled.objects() {
            let event = self.bga[object.id.0 as usize];
            scheduled.push(ScheduledBga {
                at: object.time.start,
                channel: event.channel,
                image: event.image,
                ordinal: event.ordinal,
            });
        }
        scheduled.sort_by_key(|event| (event.at, event.ordinal));
        Ok(scheduled)
    }
    /// Schedules alpha markers separately without changing gameplay or image events.
    pub fn compile_bga_opacity(&self) -> Result<Vec<ScheduledBgaOpacity>, BmsError> {
        let mut timing = self.visual_timing()?;
        timing
            .objects
            .extend(
                self.bga_opacity
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
        let compiled = timing
            .compile()
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
        let mut scheduled = Vec::with_capacity(self.bga_opacity.len());
        for object in compiled.objects() {
            let event = self.bga_opacity[object.id.0 as usize];
            scheduled.push(ScheduledBgaOpacity {
                at: object.time.start,
                channel: event.channel,
                alpha: event.alpha,
                ordinal: event.ordinal,
            });
        }
        scheduled.sort_by_key(|event| (event.at, event.ordinal));
        Ok(scheduled)
    }
    fn visual_timing(&self) -> Result<SourceChart, BmsError> {
        self.bga
            .len()
            .checked_add(self.bga_opacity.len())
            .filter(|count| *count <= MAX_SOURCE_ITEMS)
            .ok_or_else(|| BmsError::new(0, BmsErrorKind::Limit("visual source items")))?;
        let original = self.source.ticks_per_beat;
        if original == 0 || self.bga_ticks_per_beat == 0 || self.bga_ticks_per_beat % original != 0
        {
            return Err(BmsError::new(0, BmsErrorKind::Resolution));
        }
        let factor = i64::from(self.bga_ticks_per_beat / original);
        let rescale = |beat: Beat| -> Result<Beat, BmsError> {
            let tick = beat
                .ticks()
                .checked_mul(factor)
                .ok_or_else(|| BmsError::new(0, BmsErrorKind::Overflow))?;
            Beat::new(tick).map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))
        };
        let mut timing = SourceChart::new(self.bga_ticks_per_beat, self.source.initial_bpm)
            .map_err(|error| BmsError::new(0, BmsErrorKind::Compile(error)))?;
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
        Ok(timing)
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

#[cfg(test)]
mod poor_mode_fixtures {
    use super::*;
    #[test]
    fn strict_header_defaults_duplicates_conditionals_and_gameplay_identity() {
        let source = "#BPM 60\n#WAV01 head.wav\n#00011:01\n";
        let default = parse(source, ParseOptions::default()).unwrap();
        assert_eq!(default.poor_bga_mode().unwrap(), PoorBgaMode::Replace);
        for (value, mode) in [
            ("0", PoorBgaMode::Replace),
            ("1", PoorBgaMode::Overlay),
            ("2", PoorBgaMode::Off),
        ] {
            let parsed = parse(
                &format!("{source}#pOoRbGa {value}"),
                ParseOptions::default(),
            )
            .unwrap();
            assert_eq!(parsed.metadata["POORBGA"], value);
            assert_eq!(parsed.poor_bga_mode().unwrap(), mode);
            assert_eq!(parsed.source, default.source);
            assert_eq!(
                parsed.compile().unwrap().chart,
                default.compile().unwrap().chart
            );
        }
        for value in ["", "00", "01", "3", "+1", "-1", "1 2", "1.0", "１"] {
            assert!(parse(&format!("#POORBGA {value}"), ParseOptions::default()).is_err());
        }
        let repeated = "#POORBGA 0\n#poorbga 1";
        assert_eq!(
            parse(repeated, ParseOptions::default()).unwrap_err().line,
            2
        );
        let last = parse(
            repeated,
            ParseOptions {
                duplicates: DuplicatePolicy::LastWins,
                ..ParseOptions::default()
            },
        )
        .unwrap();
        assert_eq!(last.poor_bga_mode().unwrap(), PoorBgaMode::Overlay);
        let conditional = "#RANDOM 2\n#IF 1\n#POORBGA 2\n#ELSE\n#POORBGA 1\n#ENDIF";
        assert_eq!(
            parse_seeded(conditional, ParseOptions::default(), 3)
                .unwrap()
                .poor_bga_mode()
                .unwrap(),
            PoorBgaMode::Off
        );
        assert_eq!(
            parse_seeded(conditional, ParseOptions::default(), 0)
                .unwrap()
                .poor_bga_mode()
                .unwrap(),
            PoorBgaMode::Overlay
        );
        let inactive = "#SETRANDOM 1\n#IF 2\n#POORBGA malformed\n#ENDIF";
        assert_eq!(
            parse(inactive, ParseOptions::default())
                .unwrap()
                .poor_bga_mode()
                .unwrap(),
            PoorBgaMode::Replace
        );
        let mut fabricated = default;
        fabricated.metadata.insert("POORBGA".into(), "01".into());
        assert!(fabricated.poor_bga_mode().is_err());
    }
    #[test]
    fn second_layer_visual_grid_preserves_gameplay_and_checked_stop_timing() {
        let prefix = "#BPM 120\n#STOP01 48\n#WAV01 head.wav\n#00011:0101\n#00009:0001\n";
        let normal = parse(
            &format!("{prefix}; unchanged physical line\n#00001:0101"),
            ParseOptions::default(),
        )
        .unwrap();
        let layer2 = parse(
            &format!("{prefix}#0000a:00010000000000\n#00001:0101"),
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(layer2.source, normal.source);
        assert_eq!(layer2.notes, normal.notes);
        assert_eq!(layer2.bgm, normal.bgm);
        assert_eq!(layer2.source.ticks_per_beat, 1);
        assert_eq!(layer2.bga_ticks_per_beat, 7);
        let compiled = layer2.compile().unwrap();
        assert_eq!(compiled.chart, normal.compile().unwrap().chart);
        assert_eq!(compiled.bgm, normal.compile().unwrap().bgm);
        assert_eq!(compiled.bga[0].channel, BgaChannel::Layer2);
        assert_eq!(compiled.bga[0].at.as_nanos(), 285_714_285);
        let at_stop = parse(
            "#BPM 120\n#STOP01 48\n#00009:0001\n#0000A:0001",
            ParseOptions::default(),
        )
        .unwrap()
        .compile_bga()
        .unwrap();
        assert_eq!(at_stop[0].at.as_nanos(), 1_000_000_000);
        let later = parse(
            "#BPM 120\n#STOP01 48\n#00009:00010000\n#0000A:00000100",
            ParseOptions::default(),
        )
        .unwrap()
        .compile_bga()
        .unwrap();
        assert_eq!(later[0].at.as_nanos(), 1_500_000_000);
        let cap = ParseOptions {
            max_resolution: 6,
            ..ParseOptions::default()
        };
        assert!(matches!(
            parse("#0000A:00010000000000", cap).unwrap_err().kind,
            BmsErrorKind::Resolution
        ));
    }
    #[test]
    fn layer2_zero_undefined_duplicates_seed_and_source_caps_match_other_visual_rows() {
        let chart = parse("#00007:01\n#0000A:00ZZ", ParseOptions::default()).unwrap();
        assert_eq!(chart.bga.len(), 2);
        assert_eq!(chart.bga[1].image, ImageId(1295));
        assert_eq!(chart.bga[1].channel, BgaChannel::Layer2);
        assert!(parse("#0000A:01\n#0000a:02", ParseOptions::default()).is_err());
        let last = parse(
            "#0000A:01\n#0000a:02\n#0000A:00",
            ParseOptions {
                duplicates: DuplicatePolicy::LastWins,
                ..ParseOptions::default()
            },
        )
        .unwrap();
        assert_eq!(last.bga.len(), 1);
        assert_eq!(last.bga[0].image, ImageId(2));
        assert!(
            parse(
                "#0000A:ZZZZ",
                ParseOptions {
                    max_objects: 1,
                    ..ParseOptions::default()
                }
            )
            .is_err()
        );
        let seeded = "#RANDOM 2\n#IF 1\n#0000A:01\n#ELSE\n#0000a:02\n#ENDIF";
        assert_eq!(
            parse_seeded(seeded, ParseOptions::default(), 3)
                .unwrap()
                .bga[0]
                .image,
            ImageId(1)
        );
        assert_eq!(
            parse_seeded(seeded, ParseOptions::default(), 0)
                .unwrap()
                .bga[0]
                .image,
            ImageId(2)
        );
        assert!(
            parse(
                "#0000A:00",
                ParseOptions {
                    max_objects: 1,
                    ..ParseOptions::default()
                }
            )
            .unwrap()
            .bga
            .is_empty()
        );
    }
    #[test]
    fn opacity_hex_bytes_zero_rests_namespaces_duplicates_and_shared_visual_ordinals() {
        let chart = parse(
            "#BPM 60\n#00004:01\n#0000B:01\n#0000C:ff\n#0000D:80\n#0000E:7F\n#0000B:00",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(chart.bga.len(), 1);
        assert_eq!(chart.bga_opacity.len(), 4);
        assert_eq!(
            chart
                .bga_opacity
                .iter()
                .map(|event| (event.channel, event.alpha, event.ordinal))
                .collect::<Vec<_>>(),
            vec![
                (BgaChannel::Base, 1, 1),
                (BgaChannel::Layer, 255, 2),
                (BgaChannel::Layer2, 128, 3),
                (BgaChannel::Poor, 127, 4)
            ]
        );
        assert!(chart.source.objects.is_empty());
        assert!(chart.images.is_empty());
        let compiled = chart.compile().unwrap();
        assert_eq!(compiled.bga_opacity.len(), 4);
        assert!(
            compiled
                .bga_opacity
                .iter()
                .all(|event| event.at.as_nanos() == 0)
        );
        for value in ["GG", "G1", "ZZ", "0", ""] {
            assert!(parse(&format!("#0000B:{value}"), ParseOptions::default()).is_err());
        }
        let duplicate = "#0000C:01\n#0000c:FF";
        assert_eq!(
            parse(duplicate, ParseOptions::default()).unwrap_err().line,
            2
        );
        let last = parse(
            duplicate,
            ParseOptions {
                duplicates: DuplicatePolicy::LastWins,
                ..ParseOptions::default()
            },
        )
        .unwrap();
        assert_eq!(last.bga_opacity[0].alpha, 255);
        assert_eq!(last.bga_opacity[0].ordinal, 1);
        assert!(
            parse(
                "#00004:01\n#0000B:01",
                ParseOptions {
                    max_objects: 1,
                    ..ParseOptions::default()
                }
            )
            .is_err()
        );
        assert!(
            parse(
                "#0000B:00",
                ParseOptions {
                    max_objects: 1,
                    ..ParseOptions::default()
                }
            )
            .unwrap()
            .bga_opacity
            .is_empty()
        );
    }
    #[test]
    fn opacity_subdivision_and_checked_bpm_stop_preserve_gameplay_and_images() {
        let prefix =
            "#BPM 120\n#STOP01 48\n#WAV01 head.wav\n#00011:0101\n#00009:0001\n#00004:0101\n";
        let plain = parse(
            &format!("{prefix}; ignored physical line\n#00001:0101"),
            ParseOptions::default(),
        )
        .unwrap();
        let alpha = parse(
            &format!("{prefix}#0000D:00010000000000\n#00001:0101"),
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(plain.source, alpha.source);
        assert_eq!(plain.notes, alpha.notes);
        assert_eq!(plain.bgm, alpha.bgm);
        assert_eq!(
            plain
                .bga
                .iter()
                .map(|event| (event.channel, event.image, event.ordinal))
                .collect::<Vec<_>>(),
            alpha
                .bga
                .iter()
                .map(|event| (event.channel, event.image, event.ordinal))
                .collect::<Vec<_>>()
        );
        assert_eq!(alpha.source.ticks_per_beat, 1);
        assert_eq!(alpha.bga_ticks_per_beat, 7);
        assert_eq!(
            plain.compile().unwrap().chart,
            alpha.compile().unwrap().chart
        );
        assert_eq!(plain.compile().unwrap().bgm, alpha.compile().unwrap().bgm);
        assert_eq!(plain.compile_bga().unwrap(), alpha.compile_bga().unwrap());
        assert_eq!(
            alpha.compile_bga_opacity().unwrap()[0].at.as_nanos(),
            285_714_285
        );
        let stops = parse(
            "#BPM 120\n#BPM01 240\n#STOP01 48\n#00008:00010000\n#00009:00010000\n#0000B:00017F00",
            ParseOptions::default(),
        )
        .unwrap()
        .compile_bga_opacity()
        .unwrap();
        assert_eq!(
            stops
                .iter()
                .map(|event| event.at.as_nanos())
                .collect::<Vec<_>>(),
            vec![500_000_000, 1_000_000_000]
        );
        assert!(
            parse(
                "#0000B:00010000000000",
                ParseOptions {
                    max_resolution: 6,
                    ..ParseOptions::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn seeded_opacity_and_combined_fabricated_visual_capacity_are_checked() {
        let text = "#RANDOM 2\n#IF 1\n#0000B:01\n#ELSE\n#0000B:FF\n#ENDIF";
        assert_eq!(
            parse_seeded(text, ParseOptions::default(), 3)
                .unwrap()
                .bga_opacity[0]
                .alpha,
            1
        );
        assert_eq!(
            parse_seeded(text, ParseOptions::default(), 0)
                .unwrap()
                .bga_opacity[0]
                .alpha,
            255
        );
        assert!(
            parse(
                "#SETRANDOM 1\n#IF 2\n#0000E:GG\n#ENDIF",
                ParseOptions::default()
            )
            .unwrap()
            .bga_opacity
            .is_empty()
        );
        let mut oversized = parse("#00004:01\n#0000B:01", ParseOptions::default()).unwrap();
        oversized.bga.resize(MAX_SOURCE_ITEMS, oversized.bga[0]);
        assert!(matches!(
            oversized.compile_bga().unwrap_err().kind,
            BmsErrorKind::Limit("visual source items")
        ));
        assert!(matches!(
            oversized.compile_bga_opacity().unwrap_err().kind,
            BmsErrorKind::Limit("visual source items")
        ));
    }
}

#[cfg(test)]
mod crop_fixtures {
    use super::*;
    #[test]
    fn base36_sugar_namespace_and_original_source_definitions() {
        let chart = parse("#BMP00 初期.bmp\n#BMP01 first.bmp\n#BMP02 second.bmp\n#BGA01 2 -1 -2 3 4 5 6\n#@bga02 1 -1 -2 4 6 5 6\n#BGA00 0 0 0 1 1 0 0", ParseOptions::default()).unwrap();
        assert_eq!(
            chart.bga_crops[&ImageId(1)].source_rect,
            chart.bga_crops[&ImageId(2)].source_rect
        );
        assert_eq!(
            chart.bga_crops[&ImageId(1)].destination,
            chart.bga_crops[&ImageId(2)].destination
        );
        assert_eq!(chart.bga_crops[&ImageId(1)].source_rect, [-1, -2, 3, 4]);
        assert_eq!(chart.bga_crops[&ImageId(0)].source, ImageId(0));
        assert_eq!(chart.images[&ImageId(1)], "first.bmp");
        assert_eq!(chart.bga_crops[&ImageId(2)].source, ImageId(1));
        assert_eq!(
            parse("#BGAzz Z 0 0 1 1 0 0", ParseOptions::default())
                .unwrap()
                .bga_crops[&ImageId(1295)]
                .source,
            ImageId(35)
        );
    }
    #[test]
    fn strict_fields_coordinates_ids_and_duplicate_family() {
        for line in [
            "#BGA01",
            "#BGA01 001 0 0 1 1 0 0",
            "#BGA01 +A 0 0 1 1 0 0",
            "#BGA01 あ 0 0 1 1 0 0",
            "#BGA01 1 0 0 0 1 0 0",
            "#BGA01 1 0 0 1 1 0 0 extra",
            "#BGA01 1 0 0 1.5 1 0 0",
            "#BGA01 1 2147483648 0 1 1 0 0",
            "#@BGA01 1 2147483647 0 1 1 0 0",
            "#@BGA01 1 0 0 -1 1 0 0",
            "#BGA!1 1 0 0 1 1 0 0",
        ] {
            assert!(parse(line, ParseOptions::default()).is_err(), "{line}");
        }
        let duplicate = "#BGA01 1 0 0 1 1 0 0\n#@BGA01 2 0 0 2 2 3 4";
        assert_eq!(
            parse(duplicate, ParseOptions::default()).unwrap_err().line,
            2
        );
        let options = ParseOptions {
            duplicates: DuplicatePolicy::LastWins,
            ..ParseOptions::default()
        };
        assert_eq!(
            parse(duplicate, options).unwrap().bga_crops[&ImageId(1)].source_rect,
            [0, 0, 2, 2]
        );
        assert_eq!(
            parse("#BGALEGACY opaque", ParseOptions::default())
                .unwrap()
                .warnings
                .len(),
            1
        );
        assert!(
            BgaCrop {
                source: ImageId(1296),
                source_rect: [0, 0, 1, 1],
                destination: [0, 0]
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn selected_headers_and_no_gameplay_or_scheduled_changes() {
        let base = "#BPM 120\n#WAV01 note.wav\n#00011:01\n#00001:01\n#00004:0100\n";
        let original = parse(base, ParseOptions::default()).unwrap();
        let with_crops = parse(
            &format!("{base}#BGA01 1 0 0 1 1 0 0\n#@BGA02 1 0 0 1 1 0 0"),
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(original.source, with_crops.source);
        assert_eq!(original.notes, with_crops.notes);
        assert_eq!(original.compile().unwrap(), with_crops.compile().unwrap());
        let conditional =
            "#RANDOM 2\n#IF 1\n#BGA01 malformed\n#ELSE\n#@BGA01 1 0 0 1 1 0 0\n#ENDIF\n#ENDRANDOM";
        assert_eq!(
            parse_seeded(conditional, ParseOptions::default(), 0)
                .unwrap()
                .bga_crops
                .len(),
            1
        );
        assert!(parse_seeded(conditional, ParseOptions::default(), 3).is_err());
        let options = ParseOptions {
            max_lines: 1,
            ..ParseOptions::default()
        };
        assert!(parse("#BGA01 1 0 0 1 1 0 0\n#BGA02 1 0 0 1 1 0 0", options).is_err());
    }
}
