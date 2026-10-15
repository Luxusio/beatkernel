//! Cold original-asset programs and copied, bounded retained-output controls.
use super::{AudioError, AudioLimits, SampleBank, SampleId, VoiceId};
use crate::time::{ClockDomainId, ClockPoint, Duration, Timestamp};
use crate::transport::{PositiveRatePhase, PositiveRatePhaseError, Rate};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq)]
/// One original-song BGM cue referencing immutable decoded PCM.
pub struct PracticeCue {
    /// Voice.
    pub voice: VoiceId,
    /// Sample.
    pub sample: SampleId,
    /// At.
    pub at: Timestamp,
    /// Gain.
    pub gain: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Exact original-song half-open practice interval.
pub struct PracticeRegion {
    /// Start.
    pub start: Timestamp,
    /// End.
    pub end: Timestamp,
    /// Repeat.
    pub repeat: bool,
}
impl PracticeRegion {
    /// Validates the supplied original values before changing any owner.
    pub fn new(start: Timestamp, end: Timestamp, repeat: bool) -> Result<Self, PracticeError> {
        let region = Self { start, end, repeat };
        if start >= end {
            return Err(PracticeError::InvalidRegion);
        }
        Ok(region)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Explicit cold storage, overlap and callback work bounds.
pub struct PracticeLimits {
    /// Max cues.
    pub max_cues: usize,
    /// Max overlap.
    pub max_overlap: usize,
    /// Max events per render.
    pub max_events_per_render: usize,
    /// Control capacity.
    pub control_capacity: usize,
    /// Receipt capacity.
    pub receipt_capacity: usize,
}
impl PracticeLimits {
    /// Validates the supplied original values before changing any owner.
    pub fn new(
        max_cues: usize,
        max_overlap: usize,
        max_events_per_render: usize,
        control_capacity: usize,
        receipt_capacity: usize,
    ) -> Result<Self, PracticeError> {
        if max_cues == 0
            || max_overlap > AudioLimits::MAX_VOICES
            || max_events_per_render == 0
            || control_capacity == 0
            || receipt_capacity == 0
            || control_capacity > AudioLimits::MAX_COMMANDS
            || receipt_capacity > AudioLimits::MAX_COMMANDS
            || max_cues > isize::MAX as usize / std::mem::size_of::<Cue>()
        {
            return Err(PracticeError::Capacity);
        }
        Ok(Self {
            max_cues,
            max_overlap,
            max_events_per_render,
            control_capacity,
            receipt_capacity,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Copied admission or render refusal; no allocation is needed to report it.
pub enum PracticeError {
    /// Audio.
    Audio(AudioError),
    /// Capacity.
    Capacity,
    /// Invalid Region.
    InvalidRegion,
    /// Empty.
    Empty,
    /// Unknown Sample.
    UnknownSample,
    /// Invalid Gain.
    InvalidGain,
    /// Duplicate Voice.
    DuplicateVoice,
    /// Overflow.
    Overflow,
    /// Full.
    Full,
    /// Disconnected.
    Disconnected,
    /// Stale Generation.
    StaleGeneration,
    /// A coherent metadata snapshot was unavailable during bounded admission.
    MetadataBusy,
    /// Missed Boundary.
    MissedBoundary,
    /// Receipt Full.
    ReceiptFull,
    /// Event Budget.
    EventBudget,
    /// Already Installed.
    AlreadyInstalled,
    /// Incompatible Mixer.
    IncompatibleMixer,
    /// This program did not opt into runtime rate controls.
    RuntimeRateRequired,
    /// A portable preparation does not describe this immutable program.
    InvalidRatePreparation,
    /// The caller must consume this event through the typed opt-in API.
    EventRequiresOptIn,
    /// Runtime control requires an actually started retained program.
    NotStarted,
}
impl From<AudioError> for PracticeError {
    fn from(value: AudioError) -> Self {
        Self::Audio(value)
    }
}
impl std::fmt::Display for PracticeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "practice error: {self:?}")
    }
}
impl std::error::Error for PracticeError {}

/// Immutable source geometry copied to the cold producer of rate preparations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeRateCueGeometry {
    /// Original song onset.
    pub at: Timestamp,
    /// Original asset frame count.
    pub source_frames: u64,
    /// Original asset sample rate.
    pub source_hz: u32,
}

/// Scalar words in the versioned internal producer preparation.
pub const PRACTICE_RATE_PREPARATION_WORDS: usize = 10;

/// A cold overlap result transferred between the game's trusted producers.
///
/// The geometry signature and scalar checksum detect mismatched custody and
/// malformed numbers. They are not authentication of an untrusted producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeRatePreparation {
    program_id: u64,
    output_hz: u32,
    cue_count: u64,
    rate: Rate,
    max_overlap: usize,
    max_cue_burst: usize,
    geometry_signature: u64,
}

fn signature_word(signature: u64, word: u64) -> u64 {
    (signature ^ word).wrapping_mul(0x100000001b3)
}

fn geometry_signature(cues: impl IntoIterator<Item = PracticeRateCueGeometry>) -> u64 {
    cues.into_iter().fold(0xcbf29ce484222325, |signature, cue| {
        [
            cue.at.as_nanos() as u64,
            cue.source_frames,
            u64::from(cue.source_hz),
        ]
        .into_iter()
        .fold(signature, signature_word)
    })
}

impl PracticeRatePreparation {
    /// Copies a versioned fixed-size payload; never owns cue or PCM storage.
    pub fn to_words(self) -> [u64; PRACTICE_RATE_PREPARATION_WORDS] {
        let mut words = [
            2,
            self.program_id,
            u64::from(self.output_hz),
            self.cue_count,
            self.rate.numerator() as u64,
            self.rate.denominator(),
            self.max_overlap as u64,
            self.geometry_signature,
            self.max_cue_burst as u64,
            0,
        ];
        words[9] = words[..9]
            .iter()
            .copied()
            .fold(0xcbf29ce484222325, signature_word);
        words
    }

    /// Validates numeric extent and version before binding to a live program.
    pub fn from_words(
        words: [u64; PRACTICE_RATE_PREPARATION_WORDS],
    ) -> Result<Self, PracticeError> {
        let invalid = PracticeError::InvalidRatePreparation;
        let rate = Rate::new(words[4] as i64, words[5]).map_err(|_| invalid)?;
        let output_hz = u32::try_from(words[2]).map_err(|_| invalid)?;
        let max_overlap = usize::try_from(words[6]).map_err(|_| invalid)?;
        let max_cue_burst = usize::try_from(words[8]).map_err(|_| invalid)?;
        if words[0] != 2
            || words[1] == 0
            || output_hz == 0
            || words[8] > words[3]
            || (words[3] != 0 && words[8] == 0)
            || rate.numerator() <= 0
            || rate.numerator() as u64 != words[4]
            || rate.denominator() != words[5]
            || words[3] > (isize::MAX as usize / std::mem::size_of::<Cue>()) as u64
            || words[6] > words[3]
            || words[..9]
                .iter()
                .copied()
                .fold(0xcbf29ce484222325, signature_word)
                != words[9]
        {
            return Err(invalid);
        }
        Ok(Self {
            program_id: words[1],
            output_hz,
            cue_count: words[3],
            rate,
            max_overlap,
            max_cue_burst,
            geometry_signature: words[7],
        })
    }
}

/// Prepares future zero-head overlap on the cold game/native producer.
pub fn prepare_practice_rate(
    program_id: u64,
    output_hz: u32,
    cues: &[PracticeRateCueGeometry],
    rate: Rate,
) -> Result<PracticeRatePreparation, PracticeError> {
    if program_id == 0 || output_hz == 0 || rate.numerator() <= 0 {
        return Err(PracticeError::InvalidRatePreparation);
    }
    if cues.len() > isize::MAX as usize / std::mem::size_of::<Cue>() {
        return Err(PracticeError::Capacity);
    }
    let mut events = Vec::new();
    events
        .try_reserve_exact(cues.len().checked_mul(2).ok_or(PracticeError::Overflow)?)
        .map_err(|_| PracticeError::Capacity)?;
    for cue in cues {
        if cue.source_hz == 0 {
            return Err(PracticeError::InvalidRatePreparation);
        }
        if cue.source_frames == 0 {
            continue;
        }
        // An arbitrary segment cut can put cue thresholds at any fraction of
        // the output grid. Round each output lifetime to ns before converting
        // it back to original song extent, so floor(E) never shrinks the bound.
        let lifetime = mul_div_ceil(
            u128::from(cue.source_frames) * u128::from(output_hz),
            u128::from(rate.denominator()),
            u128::from(cue.source_hz) * rate.numerator() as u128,
        )?;
        let output_nanos = mul_div_ceil(lifetime, 1_000_000_000, u128::from(output_hz))?;
        let extent = mul_div_ceil(
            output_nanos,
            rate.numerator() as u128,
            u128::from(rate.denominator()),
        )?;
        let end = i128::from(cue.at.as_nanos())
            .checked_add(i128::try_from(extent).map_err(|_| PracticeError::Overflow)?)
            .ok_or(PracticeError::Overflow)?;
        events.push((i128::from(cue.at.as_nanos()), 1i64));
        events.push((end, -1));
    }
    events.sort_unstable();
    let mut active = 0i64;
    let mut max_overlap = 0usize;
    for &(_, delta) in &events {
        active += delta;
        max_overlap = max_overlap.max(active as usize);
    }
    events.clear();
    // Include empty assets: each onset still consumes retained scheduling
    // work. A whole output frame rounded to active ns bounds every arbitrary
    // fractional segment alignment, just like the voice lifetime envelope.
    let burst_extent = i128::try_from(mul_div_ceil(
        1_000_000_000u128.div_ceil(u128::from(output_hz)),
        rate.numerator() as u128,
        u128::from(rate.denominator()),
    )?)
    .map_err(|_| PracticeError::Overflow)?;
    for cue in cues {
        let start = i128::from(cue.at.as_nanos());
        let end = start
            .checked_add(burst_extent)
            .ok_or(PracticeError::Overflow)?;
        events.push((start, 1));
        events.push((end, -1));
    }
    events.sort_unstable();
    let mut active = 0i64;
    let mut max_cue_burst = 0usize;
    for (_, delta) in events {
        active += delta;
        max_cue_burst = max_cue_burst.max(active as usize);
    }
    Ok(PracticeRatePreparation {
        program_id,
        output_hz,
        cue_count: cues.len() as u64,
        rate,
        max_overlap,
        max_cue_burst,
        geometry_signature: geometry_signature(cues.iter().copied()),
    })
}

/// A portable preparation bound to a coherent live generation/revision.
#[derive(Clone)]
pub struct PreparedPracticeRate {
    preparation: PracticeRatePreparation,
    // Retains the allocation against address reuse. Tokens stay on the cold
    // producer; only copied scalar controls cross the audio ring.
    program: Arc<Program>,
    generation: u64,
    revision: u64,
}
impl std::fmt::Debug for PreparedPracticeRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedPracticeRate")
            .field("preparation", &self.preparation)
            .field("generation", &self.generation)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
impl PartialEq for PreparedPracticeRate {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.program, &other.program)
            && self.preparation == other.preparation
            && self.generation == other.generation
            && self.revision == other.revision
    }
}
impl Eq for PreparedPracticeRate {}

/// Coherent audio-published retained state; no native presentation authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeMetadata {
    /// Current attempt generation.
    pub generation: u64,
    /// Explicit control revision.
    pub revision: u64,
    /// Current applied rate (also while paused).
    pub rate: Rate,
    /// An active PCM frame has actually started the program.
    pub started: bool,
    /// The current retained region has not ended.
    pub active: bool,
}

/// A rate request that could not be applied at its actual active boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeRateRefusal {
    /// The actual attempt no longer matches the prepared generation.
    StaleGeneration,
    /// Another explicit edit superseded the preparation.
    StaleRevision,
    /// The original endpoint was already due at this frame.
    Ended,
    /// Exact phase arithmetic could not represent the new segment.
    Phase(PositiveRatePhaseError),
    /// A live fractional sample head cannot share the new rate denominator.
    VoiceDenominatorCapacity,
    /// Reserved retained BGM slots cannot cover carry and future voices.
    BgmCapacity,
    /// A future single-frame cue burst cannot fit the scheduling work bound.
    EventBudget,
    /// A future cue/end cannot fit the original output frame/deadline grid.
    DeadlineOverflow,
}

/// Actual audio-side outcome, distinct from admission and native presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeRateOutcome {
    /// All heads and the same-attempt phase changed together.
    Applied,
    /// The same reduced rate consumed a revision without adding a segment.
    Noop,
    /// Existing playback continued unchanged.
    Refused(PracticeRateRefusal),
}

/// Actual copied same-attempt rate facts on the original physical frame grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeRateEvent {
    /// Original correlated request identity.
    pub request_id: u64,
    /// Same-attempt generation, unchanged by the rate control.
    pub generation: u64,
    /// Same iteration, unchanged by the rate control.
    pub iteration: u64,
    /// Revision against which the request was prepared.
    pub request_revision: u64,
    /// Resulting explicit revision; unchanged on refusal.
    pub applied_revision: u64,
    /// Actual original physical output frame.
    pub physical_frame: u64,
    /// Actual pause-excluding playback frame.
    pub playback_frame: u64,
    /// Cumulative active elapsed in this attempt, before the cut.
    pub active_elapsed_nanos: i128,
    /// Rate before this control.
    pub previous_rate: Rate,
    /// Requested reduced positive rate.
    pub requested_rate: Rate,
    /// Actual rate after this control, including refusal.
    pub applied_rate: Rate,
    /// Actual audio-side application/noop/refusal.
    pub outcome: PracticeRateOutcome,
}

/// One compound new-attempt region/rate transition, never two controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeRestartEvent {
    /// One actual new-attempt boundary, or the preserved state on refusal.
    pub boundary: PracticeReceipt,
    /// Original explicit preparation revision.
    pub request_revision: u64,
    /// Actual resulting revision.
    pub applied_revision: u64,
    /// Rate before the compound control.
    pub previous_rate: Rate,
    /// Requested original restart rate.
    pub requested_rate: Rate,
    /// Resulting rate, unchanged on refusal.
    pub applied_rate: Rate,
    /// Actual compound outcome.
    pub outcome: PracticeRateOutcome,
}

/// Opt-in events sharing the legacy bounded receipt/projection ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeEvent {
    /// A legacy retained region/pass boundary.
    Boundary(PracticeReceipt),
    /// A same-attempt positive rate outcome.
    Rate(PracticeRateEvent),
    /// One compound region and rate restart.
    Restart(PracticeRestartEvent),
}

/// A typed event proved by generation of an actual target sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectedPracticeEvent {
    /// Original copied audio-side event.
    pub event: PracticeEvent,
    /// Projection on the actually publishing target block.
    pub boundary: super::TargetBoundary,
    /// Absolute generated target sample index.
    pub target_frame: u64,
    /// Target rate used for this projection.
    pub target_rate: u32,
    /// Original physical clock origin.
    pub origin: ClockPoint,
}

/// Conservative original-song nanosecond extent of a zero-phase BGM voice.
///
/// The discrete output lifetime and its inverse original-song extent both
/// round upward, bounding overlap for every exact shared region anchor.
pub fn practice_bgm_voice_extent(
    source_frames: usize,
    source_rate: u32,
    output_rate: u32,
    rate: Rate,
) -> Result<u128, PracticeError> {
    if source_rate == 0 || output_rate == 0 || rate.numerator() <= 0 {
        return Err(PracticeError::InvalidRegion);
    }
    if source_frames == 0 {
        return Ok(0);
    }
    let numerator = rate.numerator() as u128;
    let denominator = u128::from(rate.denominator());
    let output_frames = mul_div_ceil(
        (source_frames as u128)
            .checked_mul(u128::from(output_rate))
            .ok_or(PracticeError::Overflow)?,
        denominator,
        u128::from(source_rate)
            .checked_mul(numerator)
            .ok_or(PracticeError::Overflow)?,
    )?;
    mul_div_ceil(
        output_frames,
        1_000_000_000u128
            .checked_mul(numerator)
            .ok_or(PracticeError::Overflow)?,
        u128::from(output_rate)
            .checked_mul(denominator)
            .ok_or(PracticeError::Overflow)?,
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Cue-local Ceil source selection and its exact rounding correction.
pub struct PracticeSourceSelection {
    /// Frame.
    pub frame: usize,
    /// Requested elapsed nanos.
    pub requested_elapsed_nanos: i128,
    /// Applied elapsed nanos.
    pub applied_elapsed_nanos: i128,
    /// Correction nanos.
    pub correction_nanos: i128,
}
#[derive(Clone, Copy)]
pub(crate) struct Cue {
    pub cue: PracticeCue,
    /// End.
    pub end: i128,
    /// Frames.
    pub frames: usize,
    /// Source rate.
    pub source_rate: u32,
}
pub(crate) struct Program {
    pub cues: Vec<Cue>,
    /// Subtree end.
    pub subtree_end: Vec<i128>,
    /// Limits.
    pub limits: PracticeLimits,
    /// Output rate.
    pub output_rate: u32,
    /// Channels.
    pub channels: u16,
    pub rate: Rate,
    pub preroll: Duration,
    pub open_end: bool,
    pub exact_timing: bool,
    pub validate_deadline: bool,
    pub runtime_program_id: u64,
    pub geometry_signature: u64,
    pub nonempty_cues: Vec<usize>,
    pub initial_overlap: usize,
    pub initial_cue_burst: usize,
}
struct ProgramTiming {
    rate: Rate,
    preroll: Duration,
    open_end: bool,
    exact_timing: bool,
    validate_deadline: bool,
    runtime_program_id: u64,
}
/// Original sample identities and a balanced overlap index. No PCM is copied.
pub struct PreparedPracticeProgram {
    pub(crate) data: Arc<Program>,
}
impl PreparedPracticeProgram {
    /// Validates the supplied original values before changing any owner.
    pub fn new(
        bank: &SampleBank,
        cues: Vec<PracticeCue>,
        limits: PracticeLimits,
    ) -> Result<Self, PracticeError> {
        Self::prepare(
            bank,
            cues,
            limits,
            ProgramTiming {
                rate: Rate::NORMAL,
                preroll: Duration::ZERO,
                open_end: false,
                exact_timing: false,
                validate_deadline: false,
                runtime_program_id: 0,
            },
        )
    }
    /// Prepares immutable positive pitch-changing playback on the original PCM.
    /// The initial region starts at original S; preroll is an output duration.
    /// With open intent, nonrepeating MAX endpoints are conceptual open ends,
    /// including subsequent scrubs; repeating or other endpoints remain finite.
    pub fn new_with_speed(
        bank: &SampleBank,
        cues: Vec<PracticeCue>,
        limits: PracticeLimits,
        rate: Rate,
        output_preroll: Duration,
        open_end: bool,
    ) -> Result<Self, PracticeError> {
        if rate.numerator() <= 0 || output_preroll.as_nanos() < 0 {
            return Err(PracticeError::InvalidRegion);
        }
        let exact_timing = rate != Rate::NORMAL || output_preroll != Duration::ZERO;
        Self::prepare(
            bank,
            cues,
            limits,
            ProgramTiming {
                rate,
                preroll: output_preroll,
                open_end,
                exact_timing,
                validate_deadline: true,
                runtime_program_id: 0,
            },
        )
    }
    /// Opts into exact runtime switching, including an initial NORMAL/P0 setup.
    /// The nonzero identity belongs to the caller's immutable program custody.
    pub fn new_with_runtime_rates(
        bank: &SampleBank,
        cues: Vec<PracticeCue>,
        limits: PracticeLimits,
        rate: Rate,
        output_preroll: Duration,
        open_end: bool,
        program_id: u64,
    ) -> Result<Self, PracticeError> {
        if program_id == 0 || rate.numerator() <= 0 || output_preroll.as_nanos() < 0 {
            return Err(PracticeError::InvalidRatePreparation);
        }
        Self::prepare(
            bank,
            cues,
            limits,
            ProgramTiming {
                rate,
                preroll: output_preroll,
                open_end,
                exact_timing: true,
                validate_deadline: true,
                runtime_program_id: program_id,
            },
        )
    }
    fn prepare(
        bank: &SampleBank,
        cues: Vec<PracticeCue>,
        limits: PracticeLimits,
        timing: ProgramTiming,
    ) -> Result<Self, PracticeError> {
        let ProgramTiming {
            rate,
            preroll,
            open_end,
            exact_timing,
            validate_deadline,
            runtime_program_id,
        } = timing;
        // Revalidate public scalar limits instead of trusting struct construction.
        PracticeLimits::new(
            limits.max_cues,
            limits.max_overlap,
            limits.max_events_per_render,
            limits.control_capacity,
            limits.receipt_capacity,
        )?;
        if cues.len() > limits.max_cues {
            return Err(PracticeError::Capacity);
        }
        let mut prepared = Vec::new();
        prepared
            .try_reserve_exact(cues.len())
            .map_err(|_| PracticeError::Capacity)?;
        let mut voices = Vec::new();
        voices
            .try_reserve_exact(cues.len())
            .map_err(|_| PracticeError::Capacity)?;
        let mut events = Vec::new();
        events
            .try_reserve_exact(cues.len().checked_mul(2).ok_or(PracticeError::Overflow)?)
            .map_err(|_| PracticeError::Capacity)?;
        for cue in cues {
            if !cue.gain.is_finite() {
                return Err(PracticeError::InvalidGain);
            }
            let sample = bank.get(cue.sample).ok_or(PracticeError::UnknownSample)?;
            let duration = ceil(
                i128::try_from(sample.frames()).map_err(|_| PracticeError::Overflow)?
                    * 1_000_000_000,
                i128::from(sample.format().sample_rate()),
            );
            let end = i128::from(cue.at.as_nanos()) + duration;
            voices.push(cue.voice);
            if sample.frames() > 0 {
                events.push((i128::from(cue.at.as_nanos()), 1i64));
                // A future cue starts at a Ceil output frame with head zero.
                // Its discrete lifetime is Ceil(source_frames*out/src), which
                // can exceed its original nanosecond extent. For every exact
                // shared region anchor, rounded start differences are floor or
                // ceil of the original separation in output frames. This
                // envelope therefore bounds overlap without fixing an anchor.
                let voice_extent = if exact_timing {
                    practice_bgm_voice_extent(
                        sample.frames(),
                        sample.format().sample_rate(),
                        bank.format().sample_rate(),
                        rate,
                    )?
                } else {
                    let output_frames = mul_div_ceil(
                        sample.frames() as u128 * u128::from(bank.format().sample_rate()),
                        u128::from(rate.denominator()),
                        u128::from(sample.format().sample_rate()) * rate.numerator() as u128,
                    )?;
                    mul_div_ceil(
                        output_frames,
                        1_000_000_000,
                        u128::from(bank.format().sample_rate()),
                    )?
                };
                let voice_extent =
                    i128::try_from(voice_extent).map_err(|_| PracticeError::Overflow)?;
                events.push((i128::from(cue.at.as_nanos()) + voice_extent, -1));
            }
            prepared.push(Cue {
                cue,
                end,
                frames: sample.frames(),
                source_rate: sample.format().sample_rate(),
            });
        }
        voices.sort_unstable();
        if voices.windows(2).any(|v| v[0] == v[1]) {
            return Err(PracticeError::DuplicateVoice);
        }
        events.sort_unstable();
        let mut active = 0i64;
        for (_, delta) in events {
            active += delta;
            if active > limits.max_overlap as i64 {
                return Err(PracticeError::Capacity);
            }
        }
        prepared.sort_by_key(|cue| cue.cue.at);
        let mut subtree_end = Vec::new();
        subtree_end
            .try_reserve_exact(prepared.len())
            .map_err(|_| PracticeError::Capacity)?;
        subtree_end.resize(prepared.len(), i128::MIN);
        fn build(cues: &[Cue], tree: &mut [i128], lo: usize, hi: usize) -> i128 {
            if lo == hi {
                return i128::MIN;
            }
            let mid = lo + (hi - lo) / 2;
            let left = build(cues, tree, lo, mid);
            let right = build(cues, tree, mid + 1, hi);
            tree[mid] = cues[mid].end.max(left).max(right);
            tree[mid]
        }
        build(&prepared, &mut subtree_end, 0, prepared.len());
        let geometry_signature =
            geometry_signature(prepared.iter().map(|cue| PracticeRateCueGeometry {
                at: cue.cue.at,
                source_frames: cue.frames as u64,
                source_hz: cue.source_rate,
            }));
        let mut nonempty_cues = Vec::new();
        if runtime_program_id != 0 {
            nonempty_cues
                .try_reserve_exact(prepared.len())
                .map_err(|_| PracticeError::Capacity)?;
            nonempty_cues.extend(
                prepared
                    .iter()
                    .enumerate()
                    .filter_map(|(index, cue)| (cue.frames != 0).then_some(index)),
            );
        }
        let mut program = Program {
            cues: prepared,
            subtree_end,
            limits,
            output_rate: bank.format().sample_rate(),
            channels: bank.format().channels(),
            rate,
            preroll,
            open_end,
            exact_timing,
            validate_deadline,
            runtime_program_id,
            geometry_signature,
            nonempty_cues,
            initial_overlap: limits.max_overlap,
            initial_cue_burst: 0,
        };
        if runtime_program_id != 0 {
            let preparation = program.prepare_rate(rate)?;
            program.initial_overlap = preparation.max_overlap;
            program.initial_cue_burst = preparation.max_cue_burst;
        }
        Ok(Self {
            data: Arc::new(program),
        })
    }
    /// Validates the exact original-song interval and representable duration.
    pub fn validate_region(&self, region: PracticeRegion) -> Result<(), PracticeError> {
        self.data.initial_region(region).map(|_| ())
    }
    /// Cue-local source selection from the original asset, never a rounded suffix.
    pub fn source_selection(
        &self,
        voice: VoiceId,
        target: Timestamp,
    ) -> Result<PracticeSourceSelection, PracticeError> {
        let cue = self
            .data
            .cues
            .iter()
            .find(|cue| cue.cue.voice == voice)
            .ok_or(PracticeError::UnknownSample)?;
        let elapsed = i128::from(target.as_nanos()) - i128::from(cue.cue.at.as_nanos());
        if elapsed < 0 {
            return Err(PracticeError::InvalidRegion);
        }
        let frame = ceil(elapsed * i128::from(cue.source_rate), 1_000_000_000);
        if frame >= cue.frames as i128 {
            return Err(PracticeError::InvalidRegion);
        }
        let applied = ceil(frame * 1_000_000_000, i128::from(cue.source_rate));
        Ok(PracticeSourceSelection {
            frame: usize::try_from(frame).map_err(|_| PracticeError::Overflow)?,
            requested_elapsed_nanos: elapsed,
            applied_elapsed_nanos: applied,
            correction_nanos: applied - elapsed,
        })
    }
    /// Configured explicit storage and work bounds.
    pub fn limits(&self) -> PracticeLimits {
        self.data.limits
    }
    /// Immutable selected source speed.
    pub fn rate(&self) -> Rate {
        self.data.rate
    }
    /// Initial physical-output preroll; fresh attempts add no new preroll.
    pub fn output_preroll(&self) -> Duration {
        self.data.preroll
    }
}
impl Program {
    fn prepare_rate(&self, rate: Rate) -> Result<PracticeRatePreparation, PracticeError> {
        if self.runtime_program_id == 0 {
            return Err(PracticeError::RuntimeRateRequired);
        }
        let mut cues = Vec::new();
        cues.try_reserve_exact(self.cues.len())
            .map_err(|_| PracticeError::Capacity)?;
        cues.extend(self.cues.iter().map(|cue| PracticeRateCueGeometry {
            at: cue.cue.at,
            source_frames: cue.frames as u64,
            source_hz: cue.source_rate,
        }));
        prepare_practice_rate(self.runtime_program_id, self.output_rate, &cues, rate)
    }
    pub(crate) fn pass_voice_bound(&self, region: PracticeRegion, global_overlap: usize) -> usize {
        if global_overlap <= self.limits.max_overlap {
            return global_overlap;
        }
        let start = self
            .nonempty_cues
            .partition_point(|index| self.cues[*index].cue.at <= region.start);
        let end = self
            .nonempty_cues
            .partition_point(|index| self.cues[*index].cue.at < region.end);
        let mut voices = end.saturating_sub(start);
        self.overlaps(region.start, |_| voices += 1);
        global_overlap.min(voices)
    }
    pub(crate) fn future_cue_burst(
        &self,
        region: PracticeRegion,
        first: usize,
        global_burst: usize,
    ) -> usize {
        let end = self.cues.partition_point(|cue| cue.cue.at < region.end);
        global_burst.min(end.saturating_sub(first))
    }
    pub(crate) fn pass_event_bound(&self, region: PracticeRegion, global_burst: usize) -> usize {
        let first = self.cues.partition_point(|cue| cue.cue.at <= region.start);
        let burst = self.future_cue_burst(region, first, global_burst);
        let mut start_events = 1 + self.limits.max_overlap;
        if start_events > self.limits.max_events_per_render {
            start_events = 1;
            self.overlaps(region.start, |_| start_events += 1);
        }
        burst.max(start_events)
    }
    fn is_open(&self, region: PracticeRegion) -> bool {
        self.open_end && region.end == Timestamp::MAX && !region.repeat
    }
    fn initial_region(&self, region: PracticeRegion) -> Result<(Timestamp, u64), PracticeError> {
        let (_, mut duration) = self.region(region)?;
        let phase = i128::from(self.preroll.as_nanos())
            .checked_mul(i128::from(self.rate.numerator()))
            .ok_or(PracticeError::Overflow)?
            / i128::from(self.rate.denominator());
        let anchor = i64::try_from(i128::from(region.start.as_nanos()) - phase)
            .map(Timestamp::from_nanos)
            .map_err(|_| PracticeError::Overflow)?;
        i64::try_from(phase).map_err(|_| PracticeError::Overflow)?;
        if self.exact_timing && !self.is_open(region) {
            duration = self.frame_at(region.start, region.end, self.preroll)?;
        }
        if self.exact_timing && self.is_open(region) {
            self.validate_open_cues(region, self.preroll)?;
        }
        Ok((anchor, duration))
    }
    // First output frame where S + trunc((floor(frame*ns/Hz)-P)*N/D)
    // reaches target. Division before multiplication preserves the full range.
    fn frame_at(
        &self,
        start: Timestamp,
        target: Timestamp,
        preroll: Duration,
    ) -> Result<u64, PracticeError> {
        let delta = i128::from(target.as_nanos()) - i128::from(start.as_nanos());
        let n = i128::from(self.rate.numerator());
        let d = i128::from(self.rate.denominator());
        let threshold = if delta >= 0 {
            (delta / n)
                .checked_mul(d)
                .and_then(|q| q.checked_add(ceil((delta % n) * d, n)))
        } else {
            let before = delta - 1;
            before
                .div_euclid(n)
                .checked_mul(d)
                .and_then(|q| q.checked_add((before.rem_euclid(n) * d).div_euclid(n)))
                .and_then(|q| q.checked_add(1))
        }
        .and_then(|q| q.checked_add(i128::from(preroll.as_nanos())))
        .ok_or(PracticeError::Overflow)?;
        if threshold <= 0 {
            return Ok(0);
        }
        u64::try_from(mul_div_ceil(
            threshold as u128,
            u128::from(self.output_rate),
            1_000_000_000,
        )?)
        .map_err(|_| PracticeError::Overflow)
    }
    pub(crate) fn region(&self, region: PracticeRegion) -> Result<(Timestamp, u64), PracticeError> {
        if region.start >= region.end {
            return Err(PracticeError::InvalidRegion);
        }
        let applied = region.start;
        if self.is_open(region) {
            self.validate_open_cues(region, Duration::ZERO)?;
            return Ok((applied, 0));
        }
        if self.exact_timing {
            let duration = self.frame_at(region.start, region.end, Duration::ZERO)?;
            if duration == 0 {
                return Err(PracticeError::InvalidRegion);
            }
            return Ok((applied, duration));
        }
        let duration = ceil(
            (i128::from(region.end.as_nanos()) - i128::from(applied.as_nanos()))
                * i128::from(self.output_rate),
            1_000_000_000,
        );
        if duration <= 0 {
            return Err(PracticeError::InvalidRegion);
        }
        Ok((
            applied,
            u64::try_from(duration).map_err(|_| PracticeError::Overflow)?,
        ))
    }
    fn runtime_region(
        &self,
        region: PracticeRegion,
        rate: Rate,
        preroll: Duration,
    ) -> Result<(PositiveRatePhase, u64), PracticeError> {
        if region.start >= region.end {
            return Err(PracticeError::InvalidRegion);
        }
        let open = self.is_open(region);
        let phase =
            PositiveRatePhase::new(region.start, preroll, rate, (!open).then_some(region.end))
                .map_err(|_| PracticeError::Overflow)?;
        let duration = if open {
            let end = self.cues.partition_point(|cue| cue.cue.at < region.end);
            if let Some(cue) = end.checked_sub(1).and_then(|index| self.cues.get(index)) {
                if cue.cue.at > region.start {
                    Practice::phase_frame(phase, cue.cue.at, self.output_rate)?;
                }
            }
            0
        } else {
            Practice::phase_frame(phase, region.end, self.output_rate)?
        };
        Ok((phase, duration))
    }
    fn validate_open_cues(
        &self,
        region: PracticeRegion,
        preroll: Duration,
    ) -> Result<(), PracticeError> {
        let end = self.cues.partition_point(|cue| cue.cue.at < region.end);
        if let Some(cue) = end.checked_sub(1).and_then(|index| self.cues.get(index)) {
            if cue.cue.at > region.start {
                self.frame_at(region.start, cue.cue.at, preroll)?;
            }
        }
        Ok(())
    }
    pub(crate) fn overlaps(&self, at: Timestamp, mut visit: impl FnMut(usize)) {
        fn walk(p: &Program, lo: usize, hi: usize, at: i128, visit: &mut impl FnMut(usize)) {
            if lo == hi {
                return;
            }
            let mid = lo + (hi - lo) / 2;
            if p.subtree_end[mid] <= at {
                return;
            }
            walk(p, lo, mid, at, visit);
            let cue = p.cues[mid];
            if i128::from(cue.cue.at.as_nanos()) <= at {
                if cue.end > at
                    && ceil(
                        (at - i128::from(cue.cue.at.as_nanos())) * i128::from(cue.source_rate),
                        1_000_000_000,
                    ) < cue.frames as i128
                {
                    visit(mid);
                }
                walk(p, mid + 1, hi, at, visit);
            }
        }
        walk(
            self,
            0,
            self.cues.len(),
            i128::from(at.as_nanos()),
            &mut visit,
        );
    }
}
pub(crate) fn ceil(n: i128, d: i128) -> i128 {
    n.div_euclid(d) + i128::from(n.rem_euclid(d) != 0)
}
// Exact checked product/division without requiring the intermediate product
// to fit. Used in cold preparation and scalar boundary validation only.
fn mul_div_ceil(a: u128, b: u128, d: u128) -> Result<u128, PracticeError> {
    if let Some(product) = a.checked_mul(b) {
        return (product / d)
            .checked_add(u128::from(product % d != 0))
            .ok_or(PracticeError::Overflow);
    }
    let mut q = (a / d).checked_mul(b).ok_or(PracticeError::Overflow)?;
    let a = a % d;
    let mut remainder = 0u128;
    let mut fraction = 0u128;
    for bit in (0..128).rev() {
        fraction = fraction.checked_mul(2).ok_or(PracticeError::Overflow)?;
        if remainder >= d - remainder {
            remainder -= d - remainder;
            fraction = fraction.checked_add(1).ok_or(PracticeError::Overflow)?;
        } else {
            remainder *= 2;
        }
        if b & (1u128 << bit) != 0 {
            if remainder >= d - a {
                remainder -= d - a;
                fraction = fraction.checked_add(1).ok_or(PracticeError::Overflow)?;
            } else {
                remainder += a;
            }
        }
    }
    q = q.checked_add(fraction).ok_or(PracticeError::Overflow)?;
    q.checked_add(u128::from(remainder != 0))
        .ok_or(PracticeError::Overflow)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Ordered request on the pause-excluding playback frame grid.
pub struct PracticeRequest {
    /// Id.
    pub id: u64,
    /// Expected generation.
    pub expected_generation: u64,
    /// At playback frame.
    pub at_playback_frame: u64,
    /// Region.
    pub region: PracticeRegion,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Observable retained-output transition kind.
pub enum PracticeBoundaryKind {
    /// Started.
    Started,
    /// Requested.
    Requested,
    /// Looped.
    Looped,
    /// Ended.
    Ended,
    /// Loop Disabled.
    LoopDisabled,
    /// Next-frame request refused because an explicit edit superseded its revision.
    ControlRejectedRevision,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Ordered rendered boundary evidence, never native presentation evidence.
pub struct PracticeReceipt {
    /// Request id.
    pub request_id: u64,
    /// Generation.
    pub generation: u64,
    /// Iteration.
    pub iteration: u64,
    /// Physical frame.
    pub physical_frame: u64,
    /// Playback frame.
    pub playback_frame: u64,
    /// Requested song time.
    pub requested_song_time: Timestamp,
    /// Applied song time.
    pub applied_song_time: Timestamp,
    /// Correction nanos.
    pub correction_nanos: i64,
    /// Kind.
    pub kind: PracticeBoundaryKind,
}
/// A boundary proved by an actually generated target sample. Neither generated
/// output nor this receipt proves native admission or acoustic presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectedPracticeReceipt {
    /// Original source-frame control and pass facts.
    pub receipt: PracticeReceipt,
    /// Exact source-to-target sample boundary for the publishing target block.
    pub boundary: super::TargetBoundary,
    /// Absolute generated target sample index, not a stream-relative native index.
    pub target_frame: u64,
    /// Target rate at the actual publishing block.
    pub target_rate: u32,
    /// Original physical clock origin for boundary.target_time.
    pub origin: ClockPoint,
}
// Every slot carries only copied atomics. Release-ready/Acquire-ready is the
// same ownership protocol as the command queue; endpoints are sole SPSC owners.
struct Slot<const N: usize> {
    ready: AtomicU8,
    words: [AtomicU64; N],
}
impl<const N: usize> Slot<N> {
    fn new() -> Self {
        Self {
            ready: AtomicU8::new(0),
            words: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
    fn write(&self, words: &[u64]) {
        self.write_state(words, 2);
    }
    fn write_state(&self, words: &[u64], state: u8) {
        for (index, dst) in self.words.iter().enumerate() {
            dst.store(words.get(index).copied().unwrap_or(0), Ordering::Relaxed);
        }
        self.ready.store(state, Ordering::Release);
    }
    fn peek(&self) -> Option<[u64; N]> {
        (self.ready.load(Ordering::Acquire) == 2)
            .then(|| std::array::from_fn(|i| self.words[i].load(Ordering::Relaxed)))
    }
    fn reclaim(&self) {
        self.ready.store(0, Ordering::Release);
    }
}
struct Shared {
    program: Arc<Program>,
    controls: Vec<Slot<14>>,
    receipts: Vec<Slot<31>>,
    controller_alive: AtomicBool,
    audio_alive: AtomicBool,
    generation: AtomicU64,
    revision: AtomicU64,
    metadata_sequence: AtomicU64,
    rate_numerator: AtomicU64,
    rate_denominator: AtomicU64,
    state: AtomicU8,
    runtime_pending: AtomicBool,
    max_overlap: AtomicU64,
    max_cue_burst: AtomicU64,
    control_frame_event_bound: AtomicU64,
}
/// Sole cold/control owner of copied requests and ordered receipts.
pub struct PracticeController {
    shared: Arc<Shared>,
    write: usize,
    read: usize,
    last_id: u64,
    last_frame: u64,
}
/// Sole audio-side endpoint retained until callback quiescence.
pub struct PracticeAudioEndpoint {
    shared: Arc<Shared>,
    read: usize,
    write: usize,
    project: usize,
}
/// Allocates fixed scalar rings before native output starts.
pub fn practice_queue(
    program: &PreparedPracticeProgram,
) -> Result<(PracticeController, PracticeAudioEndpoint), PracticeError> {
    fn slots<const N: usize>(n: usize) -> Result<Vec<Slot<N>>, PracticeError> {
        let mut v = Vec::new();
        v.try_reserve_exact(n)
            .map_err(|_| PracticeError::Capacity)?;
        v.resize_with(n, Slot::new);
        Ok(v)
    }
    let shared = Arc::new(Shared {
        program: Arc::clone(&program.data),
        controls: slots(program.data.limits.control_capacity)?,
        receipts: slots(program.data.limits.receipt_capacity)?,
        controller_alive: AtomicBool::new(true),
        audio_alive: AtomicBool::new(true),
        generation: AtomicU64::new(1),
        revision: AtomicU64::new(0),
        metadata_sequence: AtomicU64::new(0),
        rate_numerator: AtomicU64::new(program.data.rate.numerator() as u64),
        rate_denominator: AtomicU64::new(program.data.rate.denominator()),
        state: AtomicU8::new(2),
        runtime_pending: AtomicBool::new(false),
        max_overlap: AtomicU64::new(program.data.initial_overlap as u64),
        max_cue_burst: AtomicU64::new(program.data.initial_cue_burst as u64),
        control_frame_event_bound: AtomicU64::new(0),
    });
    Ok((
        PracticeController {
            shared: Arc::clone(&shared),
            write: 0,
            read: 0,
            last_id: 0,
            last_frame: 0,
        },
        PracticeAudioEndpoint {
            shared,
            read: 0,
            write: 0,
            project: 0,
        },
    ))
}
impl Shared {
    fn history_retention_pins(&self) -> (Option<u64>, Option<super::TargetTime>) {
        let mut source_pin: Option<u64> = None;
        let mut target_pin: Option<super::TargetTime> = None;
        // The audio endpoint alone writes payloads; the controller only reclaims.
        // Either caller holds its endpoint borrowed throughout this scan. An
        // audio-side scan may retain a reclaimed payload conservatively, while
        // a controller-side scan prevents reclamation until the scan finishes.
        for slot in &self.receipts {
            let ready = slot.ready.load(Ordering::Acquire);
            if ready != 1 && ready != 2 {
                continue;
            }
            let source = slot.words[3].load(Ordering::Relaxed);
            source_pin = Some(source_pin.map_or(source, |pin| pin.min(source)));
            if ready == 2 {
                let target = super::TargetTime::new(
                    slot.words[11].load(Ordering::Relaxed),
                    slot.words[12].load(Ordering::Relaxed),
                    slot.words[13].load(Ordering::Relaxed),
                )
                .expect("audio-side validated exact target time");
                target_pin = Some(target_pin.map_or(target, |pin| {
                    if target
                        .seconds()
                        .cmp(&pin.seconds())
                        .then_with(|| {
                            (u128::from(target.numerator()) * u128::from(pin.denominator())).cmp(
                                &(u128::from(pin.numerator()) * u128::from(target.denominator())),
                            )
                        })
                        .is_lt()
                    {
                        target
                    } else {
                        pin
                    }
                }));
            }
        }
        (source_pin, target_pin)
    }
    // All metadata accesses in this snapshot/write protocol use the same SC
    // order. At most two observations, never an unbounded producer spin. Odd
    // means publication is in progress. The checked sequence never wraps.
    fn metadata(&self) -> Result<PracticeMetadata, PracticeError> {
        self.snapshot().map(|(metadata, _, _, _)| metadata)
    }
    fn snapshot(&self) -> Result<(PracticeMetadata, usize, usize, usize), PracticeError> {
        for _ in 0..2 {
            let before = self.metadata_sequence.load(Ordering::SeqCst);
            if !before.is_multiple_of(2) {
                continue;
            }
            let generation = self.generation.load(Ordering::SeqCst);
            let revision = self.revision.load(Ordering::SeqCst);
            let rate = Rate::new(
                self.rate_numerator.load(Ordering::SeqCst) as i64,
                self.rate_denominator.load(Ordering::SeqCst),
            )
            .map_err(|_| PracticeError::InvalidRatePreparation)?;
            let state = self.state.load(Ordering::SeqCst);
            let max_overlap = self.max_overlap.load(Ordering::SeqCst) as usize;
            let max_cue_burst = self.max_cue_burst.load(Ordering::SeqCst) as usize;
            let control_frame_event_bound =
                self.control_frame_event_bound.load(Ordering::SeqCst) as usize;
            if self.metadata_sequence.load(Ordering::SeqCst) == before {
                return Ok((
                    PracticeMetadata {
                        generation,
                        revision,
                        rate,
                        started: state & 1 != 0,
                        active: state & 2 != 0,
                    },
                    max_overlap,
                    max_cue_burst,
                    control_frame_event_bound,
                ));
            }
        }
        Err(PracticeError::MetadataBusy)
    }
}
impl PracticeController {
    /// Coherent actual audio-side state, without claiming native presentation.
    pub fn metadata(&self) -> Result<PracticeMetadata, PracticeError> {
        self.shared.metadata()
    }
    /// Performs the original geometry scan on a cold native/game thread.
    pub fn prepare_rate(&self, rate: Rate) -> Result<PreparedPracticeRate, PracticeError> {
        let program = &self.shared.program;
        let preparation = program.prepare_rate(rate)?;
        let metadata = self.metadata()?;
        self.bind_rate_preparation(metadata.generation, metadata.revision, preparation)
    }
    /// Binds a trusted producer's scalar result with bounded metadata work only.
    pub fn bind_rate_preparation(
        &self,
        expected_generation: u64,
        expected_revision: u64,
        preparation: PracticeRatePreparation,
    ) -> Result<PreparedPracticeRate, PracticeError> {
        let program = &self.shared.program;
        if program.runtime_program_id == 0 {
            return Err(PracticeError::RuntimeRateRequired);
        }
        let metadata = self.metadata()?;
        if metadata.generation != expected_generation {
            return Err(PracticeError::StaleGeneration);
        }
        if metadata.revision != expected_revision {
            return Err(PracticeError::InvalidRatePreparation);
        }
        if preparation.program_id != program.runtime_program_id
            || preparation.output_hz != program.output_rate
            || preparation.cue_count != program.cues.len() as u64
            || preparation.geometry_signature != program.geometry_signature
        {
            return Err(PracticeError::InvalidRatePreparation);
        }
        Ok(PreparedPracticeRate {
            preparation,
            program: Arc::clone(program),
            generation: expected_generation,
            revision: expected_revision,
        })
    }
    /// Queues one same-attempt change at the next genuinely active frame.
    pub fn try_set_rate_next(
        &mut self,
        id: u64,
        expected_generation: u64,
        rate: &PreparedPracticeRate,
    ) -> Result<(), PracticeError> {
        self.admit_rate(id, expected_generation, None, rate)
    }
    /// Restores a region and rate atomically in one fresh zero-preroll attempt.
    pub fn try_restart_next(
        &mut self,
        id: u64,
        expected_generation: u64,
        region: PracticeRegion,
        rate: &PreparedPracticeRate,
    ) -> Result<(), PracticeError> {
        if region.start >= region.end {
            return Err(PracticeError::InvalidRegion);
        }
        self.admit_rate(id, expected_generation, Some(region), rate)
    }
    fn admit_rate(
        &mut self,
        id: u64,
        expected_generation: u64,
        region: Option<PracticeRegion>,
        rate: &PreparedPracticeRate,
    ) -> Result<(), PracticeError> {
        if !Arc::ptr_eq(&rate.program, &self.shared.program) {
            return Err(PracticeError::InvalidRatePreparation);
        }
        let (metadata, _, _, control_frame_event_bound) = self.shared.snapshot()?;
        if !metadata.started {
            return Err(PracticeError::NotStarted);
        }
        if expected_generation != metadata.generation || rate.generation != expected_generation {
            return Err(PracticeError::StaleGeneration);
        }
        self.bind_rate_preparation(expected_generation, metadata.revision, rate.preparation)?;
        if rate.revision != metadata.revision {
            return Err(PracticeError::InvalidRatePreparation);
        }
        if id == 0 || id <= self.last_id {
            return Err(PracticeError::InvalidRegion);
        }
        if !self.shared.audio_alive.load(Ordering::Acquire) {
            return Err(PracticeError::Disconnected);
        }
        if self.shared.runtime_pending.load(Ordering::Acquire)
            || self
                .shared
                .controls
                .iter()
                .any(|slot| slot.ready.load(Ordering::Acquire) != 0)
            || self
                .shared
                .receipts
                .iter()
                .any(|slot| slot.ready.load(Ordering::Acquire) != 0)
        {
            return Err(PracticeError::Full);
        }
        // The producer cannot predict the next callback's exact cut. Reserve
        // its control event alongside any ordinary frame in the current pass,
        // including unchanged playback after an audio-side refusal.
        let limit = self.shared.program.limits.max_events_per_render;
        if control_frame_event_bound >= limit {
            return Err(PracticeError::EventBudget);
        }
        if let Some(region) = region {
            let mut events = 1usize;
            self.shared.program.overlaps(region.start, |_| {
                events = events.saturating_add(1);
            });
            if events > limit {
                return Err(PracticeError::EventBudget);
            }
        }
        let tag = if region.is_some() { 2 } else { 1 };
        let region = region.unwrap_or(PracticeRegion {
            start: Timestamp::ZERO,
            end: Timestamp::from_nanos(1),
            repeat: false,
        });
        self.shared.runtime_pending.store(true, Ordering::Release);
        self.shared.controls[self.write].write(&[
            id,
            expected_generation,
            0,
            region.start.as_nanos() as u64,
            region.end.as_nanos() as u64,
            u64::from(region.repeat),
            0,
            1,
            rate.revision,
            tag,
            rate.preparation.rate.numerator() as u64,
            rate.preparation.rate.denominator(),
            rate.preparation.max_overlap as u64,
            rate.preparation.max_cue_burst as u64,
        ]);
        self.write = (self.write + 1) % self.shared.controls.len();
        self.last_id = id;
        Ok(())
    }
    /// Earliest original Source and projected Target references in queued
    /// receipts, without popping them. Raw lookahead preserves only Source.
    /// These mathematical references grant no native admission or acknowledgment.
    pub fn history_retention_pins(&self) -> (Option<u64>, Option<super::TargetTime>) {
        self.shared.history_retention_pins()
    }
    /// Immutable source speed of the prepared program, without queue activity.
    pub fn rate(&self) -> Rate {
        self.shared.program.rate
    }
    /// Initial output preroll; later attempts add no new preroll.
    pub fn output_preroll(&self) -> Duration {
        self.shared.program.preroll
    }
    /// Whether nonrepeating MAX regions have conceptual open-end intent.
    pub fn has_open_end(&self) -> bool {
        self.shared.program.open_end
    }
    /// Most recently rendered program generation.
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }
    /// Admits a copied region transition or returns a precise refusal.
    pub fn try_request(&mut self, r: PracticeRequest) -> Result<(), PracticeError> {
        self.admit(r, false, false)
    }
    /// Disables the next repetition without retiring voices or starting an attempt.
    pub fn try_disable_loop(
        &mut self,
        id: u64,
        expected_generation: u64,
        at_playback_frame: u64,
    ) -> Result<(), PracticeError> {
        self.admit(
            PracticeRequest {
                id,
                expected_generation,
                at_playback_frame,
                region: PracticeRegion {
                    start: Timestamp::ZERO,
                    end: Timestamp::from_nanos(1),
                    repeat: false,
                },
            },
            true,
            false,
        )
    }
    /// Requests an exact original region at the next eligible active render
    /// frame. FIFO controls ahead of it retain precedence. Autonomous iterations
    /// do not invalidate admission, but intervening explicit edits do.
    pub fn try_request_next(
        &mut self,
        id: u64,
        expected_generation: u64,
        region: PracticeRegion,
    ) -> Result<(), PracticeError> {
        self.admit(
            PracticeRequest {
                id,
                expected_generation,
                at_playback_frame: 0,
                region,
            },
            false,
            true,
        )
    }
    /// Disables repetition at the next eligible active render frame without
    /// changing the current pass. Autonomous loops do not stale this request.
    pub fn try_disable_loop_next(
        &mut self,
        id: u64,
        expected_generation: u64,
    ) -> Result<(), PracticeError> {
        self.admit(
            PracticeRequest {
                id,
                expected_generation,
                at_playback_frame: 0,
                region: PracticeRegion {
                    start: Timestamp::ZERO,
                    end: Timestamp::from_nanos(1),
                    repeat: false,
                },
            },
            true,
            true,
        )
    }
    fn admit(
        &mut self,
        r: PracticeRequest,
        disable: bool,
        next: bool,
    ) -> Result<(), PracticeError> {
        if self.shared.runtime_pending.load(Ordering::Acquire) {
            return Err(PracticeError::Full);
        }
        if !disable && self.shared.program.runtime_program_id == 0 {
            self.shared.program.region(r.region)?;
        }
        if r.id == 0 || r.id <= self.last_id || !next && r.at_playback_frame < self.last_frame {
            return Err(PracticeError::InvalidRegion);
        }
        let (metadata, max_overlap, max_cue_burst, _) = self.shared.snapshot()?;
        if r.expected_generation != metadata.generation {
            return Err(PracticeError::StaleGeneration);
        }
        if !disable && self.shared.program.runtime_program_id != 0 {
            self.shared
                .program
                .runtime_region(r.region, metadata.rate, Duration::ZERO)?;
            if self.shared.program.pass_voice_bound(r.region, max_overlap)
                > self.shared.program.limits.max_overlap
            {
                return Err(PracticeError::Capacity);
            }
            if self
                .shared
                .program
                .pass_event_bound(r.region, max_cue_burst)
                > self.shared.program.limits.max_events_per_render
            {
                return Err(PracticeError::EventBudget);
            }
        }
        if !self.shared.audio_alive.load(Ordering::Acquire) {
            return Err(PracticeError::Disconnected);
        }
        let slot = &self.shared.controls[self.write];
        if slot.ready.load(Ordering::Acquire) != 0 {
            return Err(PracticeError::Full);
        }
        slot.write(&[
            r.id,
            r.expected_generation,
            r.at_playback_frame,
            r.region.start.as_nanos() as u64,
            r.region.end.as_nanos() as u64,
            u64::from(r.region.repeat),
            u64::from(disable),
            u64::from(next),
            metadata.revision,
            0,
        ]);
        self.write = (self.write + 1) % self.shared.controls.len();
        self.last_id = r.id;
        if !next {
            self.last_frame = r.at_playback_frame;
        }
        Ok(())
    }
    /// Removes one ordered receipt; Empty never overwrites prior evidence.
    pub fn try_pop_receipt(&mut self) -> Result<PracticeReceipt, PracticeError> {
        self.try_pop_projected_receipt()
            .map(|projected| projected.receipt)
    }
    /// Removes one ordered receipt only after actual target-sample generation.
    /// Raw source lookahead and explicit held output never expose a receipt.
    pub fn try_pop_projected_receipt(&mut self) -> Result<ProjectedPracticeReceipt, PracticeError> {
        if self.shared.receipts[self.read]
            .peek()
            .is_some_and(|words| words[9] != 0)
        {
            return Err(PracticeError::EventRequiresOptIn);
        }
        let projected = self.try_pop_projected_event()?;
        let PracticeEvent::Boundary(receipt) = projected.event else {
            unreachable!("sole consumer checked the unchanged event tag")
        };
        Ok(ProjectedPracticeReceipt {
            receipt,
            boundary: projected.boundary,
            target_frame: projected.target_frame,
            target_rate: projected.target_rate,
            origin: projected.origin,
        })
    }
    /// Consumes the next typed event only after genuine target generation.
    pub fn try_pop_event(&mut self) -> Result<PracticeEvent, PracticeError> {
        self.try_pop_projected_event()
            .map(|projected| projected.event)
    }
    /// Consumes a generated typed event, retaining its original projection facts.
    pub fn try_pop_projected_event(&mut self) -> Result<ProjectedPracticeEvent, PracticeError> {
        let slot = &self.shared.receipts[self.read];
        let Some(w) = slot.peek() else {
            return Err(if self.shared.audio_alive.load(Ordering::Acquire) {
                PracticeError::Empty
            } else {
                PracticeError::Disconnected
            });
        };
        let receipt = PracticeReceipt {
            request_id: w[0],
            generation: w[1],
            iteration: w[2],
            physical_frame: w[3],
            playback_frame: w[4],
            requested_song_time: Timestamp::from_nanos(w[5] as i64),
            applied_song_time: Timestamp::from_nanos(w[6] as i64),
            correction_nanos: w[7] as i64,
            kind: match w[8] {
                0 => PracticeBoundaryKind::Started,
                1 => PracticeBoundaryKind::Requested,
                2 => PracticeBoundaryKind::Looped,
                3 => PracticeBoundaryKind::Ended,
                4 => PracticeBoundaryKind::LoopDisabled,
                _ => PracticeBoundaryKind::ControlRejectedRevision,
            },
        };
        let event = if w[9] == 0 {
            PracticeEvent::Boundary(receipt)
        } else {
            let previous_rate = Rate::new(w[18] as i64, w[19])
                .map_err(|_| PracticeError::InvalidRatePreparation)?;
            let requested_rate = Rate::new(w[20] as i64, w[21])
                .map_err(|_| PracticeError::InvalidRatePreparation)?;
            let applied_rate = Rate::new(w[22] as i64, w[23])
                .map_err(|_| PracticeError::InvalidRatePreparation)?;
            let outcome = decode_outcome(w[28], w[29], w[30]);
            if w[9] == 1 {
                PracticeEvent::Rate(PracticeRateEvent {
                    request_id: receipt.request_id,
                    generation: receipt.generation,
                    iteration: receipt.iteration,
                    request_revision: w[24],
                    applied_revision: w[25],
                    physical_frame: receipt.physical_frame,
                    playback_frame: receipt.playback_frame,
                    active_elapsed_nanos: (u128::from(w[26]) | (u128::from(w[27]) << 64)) as i128,
                    previous_rate,
                    requested_rate,
                    applied_rate,
                    outcome,
                })
            } else {
                PracticeEvent::Restart(PracticeRestartEvent {
                    boundary: receipt,
                    request_revision: w[24],
                    applied_revision: w[25],
                    previous_rate,
                    requested_rate,
                    applied_rate,
                    outcome,
                })
            }
        };
        let projected = ProjectedPracticeEvent {
            event,
            boundary: super::TargetBoundary {
                source_frame: receipt.physical_frame,
                target_frame_offset: w[10] as usize,
                target_time: super::TargetTime::new(w[11], w[12], w[13])
                    .expect("audio-side validated exact target time"),
            },
            target_frame: w[14],
            target_rate: w[15] as u32,
            origin: ClockPoint {
                domain: ClockDomainId(u32::try_from(w[16]).map_err(|_| PracticeError::Overflow)?),
                timestamp: Timestamp::from_nanos(w[17] as i64),
            },
        };
        slot.reclaim();
        if w[9] != 0 {
            self.shared.runtime_pending.store(false, Ordering::Release);
        }
        self.read = (self.read + 1) % self.shared.receipts.len();
        Ok(projected)
    }
}
impl Drop for PracticeController {
    fn drop(&mut self) {
        self.shared.controller_alive.store(false, Ordering::Release);
    }
}
impl Drop for PracticeAudioEndpoint {
    fn drop(&mut self) {
        self.shared.audio_alive.store(false, Ordering::Release);
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Control {
    pub request: PracticeRequest,
    pub disable: bool,
    pub next: bool,
    pub revision: u64,
    pub rate: Option<Rate>,
    pub restart: bool,
    pub max_overlap: usize,
    pub max_cue_burst: usize,
}
impl PracticeAudioEndpoint {
    pub(crate) fn history_retention_pins(&self) -> (Option<u64>, Option<super::TargetTime>) {
        self.shared.history_retention_pins()
    }
    pub(crate) fn has_pending_receipts(&self) -> bool {
        self.shared
            .receipts
            .iter()
            .any(|slot| slot.ready.load(Ordering::Acquire) != 0)
    }
    // Raw producer -> projector -> sole public consumer share each slot. The
    // projector never reclaims ownership or frees credits. Public reclamation
    // occurs only after Acquire-observing the final Projected publication.
    pub(crate) fn raw_boundary(&self) -> Option<u64> {
        let slot = &self.shared.receipts[self.project];
        (slot.ready.load(Ordering::Acquire) == 1).then(|| slot.words[3].load(Ordering::Relaxed))
    }
    pub(crate) fn publish_projection(
        &mut self,
        boundary: super::TargetBoundary,
        target_frame: u64,
        target_rate: u32,
        origin: ClockPoint,
    ) {
        let slot = &self.shared.receipts[self.project];
        let fields = [
            boundary.target_frame_offset as u64,
            boundary.target_time.seconds(),
            boundary.target_time.numerator(),
            boundary.target_time.denominator(),
            target_frame,
            u64::from(target_rate),
            u64::from(origin.domain.0),
            origin.timestamp.as_nanos() as u64,
        ];
        for (dst, value) in slot.words[10..].iter().zip(fields) {
            dst.store(value, Ordering::Relaxed);
        }
        slot.ready.store(2, Ordering::Release);
        self.project = (self.project + 1) % self.shared.receipts.len();
    }
    pub(crate) fn matches(&self, p: &PreparedPracticeProgram) -> bool {
        Arc::ptr_eq(&self.shared.program, &p.data)
    }
    pub(crate) fn request(&self, offset: usize) -> Option<Control> {
        if offset >= self.shared.controls.len() {
            return None;
        }
        self.shared.controls[(self.read + offset) % self.shared.controls.len()]
            .peek()
            .map(|w| Control {
                request: PracticeRequest {
                    id: w[0],
                    expected_generation: w[1],
                    at_playback_frame: w[2],
                    region: PracticeRegion {
                        start: Timestamp::from_nanos(w[3] as i64),
                        end: Timestamp::from_nanos(w[4] as i64),
                        repeat: w[5] != 0,
                    },
                },
                disable: w[6] != 0,
                next: w[7] != 0,
                revision: w[8],
                rate: (w[9] != 0).then(|| {
                    Rate::new(w[10] as i64, w[11]).expect("validated copied rate control")
                }),
                restart: w[9] == 2,
                max_overlap: w[12] as usize,
                max_cue_burst: w[13] as usize,
            })
    }
    pub(crate) fn consume(&mut self) {
        self.shared.controls[self.read].reclaim();
        self.read = (self.read + 1) % self.shared.controls.len();
    }
    pub(crate) fn free_receipts(&self) -> usize {
        (0..self.shared.receipts.len())
            .take_while(|i| {
                self.shared.receipts[(self.write + i) % self.shared.receipts.len()]
                    .ready
                    .load(Ordering::Acquire)
                    == 0
            })
            .count()
    }
    fn publish_metadata(&self, cursor: &Cursor) {
        let sequence = self.shared.metadata_sequence.load(Ordering::SeqCst);
        self.shared
            .metadata_sequence
            .store(sequence + 1, Ordering::SeqCst);
        self.shared
            .generation
            .store(cursor.generation, Ordering::SeqCst);
        self.shared
            .revision
            .store(cursor.revision, Ordering::SeqCst);
        self.shared
            .rate_numerator
            .store(cursor.rate.numerator() as u64, Ordering::SeqCst);
        self.shared
            .rate_denominator
            .store(cursor.rate.denominator(), Ordering::SeqCst);
        self.shared.state.store(
            u8::from(cursor.started) | (u8::from(cursor.active) << 1),
            Ordering::SeqCst,
        );
        self.shared
            .max_overlap
            .store(cursor.max_overlap as u64, Ordering::SeqCst);
        self.shared
            .max_cue_burst
            .store(cursor.max_cue_burst as u64, Ordering::SeqCst);
        // Existing boundary publications retain a conservative pass bound;
        // consumed cues need no per-frame metadata updates. Initial preroll
        // can put the current anchor before the selected region start.
        let program = &self.shared.program;
        let mut events = 0;
        if program.runtime_program_id != 0 && cursor.active {
            let first = program
                .cues
                .partition_point(|cue| cue.cue.at <= cursor.anchor);
            events = program.future_cue_burst(cursor.region, first, cursor.max_cue_burst);
            if !cursor.open {
                let mut boundary = 1usize;
                if cursor.region.repeat {
                    program.overlaps(cursor.region.start, |_| {
                        boundary = boundary.saturating_add(1);
                    });
                }
                events = events.max(boundary);
            }
        }
        self.shared
            .control_frame_event_bound
            .store(events as u64, Ordering::SeqCst);
        self.shared
            .metadata_sequence
            .store(sequence + 2, Ordering::SeqCst);
    }
    pub(crate) fn receipt(&mut self, r: PracticeReceipt, cursor: &Cursor) {
        self.publish_metadata(cursor);
        self.shared.receipts[self.write].write_state(
            &[
                r.request_id,
                r.generation,
                r.iteration,
                r.physical_frame,
                r.playback_frame,
                r.requested_song_time.as_nanos() as u64,
                r.applied_song_time.as_nanos() as u64,
                r.correction_nanos as u64,
                match r.kind {
                    PracticeBoundaryKind::Started => 0,
                    PracticeBoundaryKind::Requested => 1,
                    PracticeBoundaryKind::Looped => 2,
                    PracticeBoundaryKind::Ended => 3,
                    PracticeBoundaryKind::LoopDisabled => 4,
                    PracticeBoundaryKind::ControlRejectedRevision => 5,
                },
                0,
            ],
            1,
        );
        self.write = (self.write + 1) % self.shared.receipts.len();
    }
    pub(crate) fn rate_event(
        &mut self,
        decision: &RateDecision,
        physical_frame: u64,
        playback_frame: u64,
    ) {
        let cursor = decision.cursor;
        self.publish_metadata(&cursor);
        let mut words = [0; 31];
        words[0] = decision.control.request.id;
        words[1] = cursor.generation;
        words[2] = cursor.iteration;
        words[3] = physical_frame;
        words[4] = playback_frame;
        words[5] = decision.control.request.region.start.as_nanos() as u64;
        words[6] = cursor.anchor.as_nanos() as u64;
        words[8] = 1; // Requested for the compound boundary's legacy payload.
        if decision.control.restart && matches!(decision.outcome, PracticeRateOutcome::Refused(_)) {
            // This is a correlated unchanged-state marker, never an applied
            // seek. The caller still owns the original pending region target.
            words[5] = words[6];
            words[8] = 5; // ControlRejectedRevision; correction remains zero.
        }
        words[9] = if decision.control.restart { 2 } else { 1 };
        words[18] = decision.previous_rate.numerator() as u64;
        words[19] = decision.previous_rate.denominator();
        words[20] = decision.requested_rate.numerator() as u64;
        words[21] = decision.requested_rate.denominator();
        words[22] = cursor.rate.numerator() as u64;
        words[23] = cursor.rate.denominator();
        words[24] = decision.control.revision;
        words[25] = cursor.revision;
        words[26] = decision.active_elapsed as u64;
        words[27] = ((decision.active_elapsed as u128) >> 64) as u64;
        (words[28], words[29], words[30]) = encode_outcome(decision.outcome);
        self.shared.receipts[self.write].write_state(&words, 1);
        self.write = (self.write + 1) % self.shared.receipts.len();
    }
}

fn encode_outcome(outcome: PracticeRateOutcome) -> (u64, u64, u64) {
    match outcome {
        PracticeRateOutcome::Applied => (0, 0, 0),
        PracticeRateOutcome::Noop => (1, 0, 0),
        PracticeRateOutcome::Refused(refusal) => (
            2,
            match refusal {
                PracticeRateRefusal::StaleRevision => 0,
                PracticeRateRefusal::Ended => 1,
                PracticeRateRefusal::Phase(_) => 2,
                PracticeRateRefusal::VoiceDenominatorCapacity => 3,
                PracticeRateRefusal::BgmCapacity => 4,
                PracticeRateRefusal::DeadlineOverflow => 5,
                PracticeRateRefusal::StaleGeneration => 6,
                PracticeRateRefusal::EventBudget => 7,
            },
            match refusal {
                PracticeRateRefusal::Phase(error) => match error {
                    PositiveRatePhaseError::NegativePreroll => 0,
                    PositiveRatePhaseError::NonPositiveRate => 1,
                    PositiveRatePhaseError::InvalidEnd => 2,
                    PositiveRatePhaseError::NegativeElapsed => 3,
                    PositiveRatePhaseError::RegressingElapsed => 4,
                    PositiveRatePhaseError::Overflow => 5,
                    PositiveRatePhaseError::DenominatorCapacity => 6,
                },
                _ => 0,
            },
        ),
    }
}

fn decode_outcome(outcome: u64, refusal: u64, phase: u64) -> PracticeRateOutcome {
    match outcome {
        0 => PracticeRateOutcome::Applied,
        1 => PracticeRateOutcome::Noop,
        _ => PracticeRateOutcome::Refused(match refusal {
            0 => PracticeRateRefusal::StaleRevision,
            1 => PracticeRateRefusal::Ended,
            2 => PracticeRateRefusal::Phase(match phase {
                0 => PositiveRatePhaseError::NegativePreroll,
                1 => PositiveRatePhaseError::NonPositiveRate,
                2 => PositiveRatePhaseError::InvalidEnd,
                3 => PositiveRatePhaseError::NegativeElapsed,
                4 => PositiveRatePhaseError::RegressingElapsed,
                6 => PositiveRatePhaseError::DenominatorCapacity,
                _ => PositiveRatePhaseError::Overflow,
            }),
            3 => PracticeRateRefusal::VoiceDenominatorCapacity,
            4 => PracticeRateRefusal::BgmCapacity,
            6 => PracticeRateRefusal::StaleGeneration,
            7 => PracticeRateRefusal::EventBudget,
            _ => PracticeRateRefusal::DeadlineOverflow,
        }),
    }
}

/// One read-only result shared by whole-block preflight and real application.
#[derive(Clone, Copy)]
pub(crate) struct RateDecision {
    pub control: Control,
    pub cursor: Cursor,
    pub previous_rate: Rate,
    pub requested_rate: Rate,
    pub active_elapsed: i128,
    pub outcome: PracticeRateOutcome,
    pub cut_due_end: usize,
}

pub(crate) enum PreflightVoiceStep {
    Rate(Rate),
    Reset(Cursor),
    Cue(Cue, Rate),
    Advance,
}
#[derive(Clone, Copy)]
pub(crate) struct Cursor {
    pub region: PracticeRegion,
    /// Anchor.
    pub anchor: Timestamp,
    /// Duration.
    pub duration: u64,
    /// Origin.
    pub origin: u64,
    /// Next cue.
    pub next_cue: usize,
    /// Generation.
    pub generation: u64,
    /// Explicit control revision, distinct from autonomous pass generations.
    pub revision: u64,
    /// Iteration.
    pub iteration: u64,
    /// Started.
    pub started: bool,
    /// Active.
    pub active: bool,
    pub preroll: Duration,
    pub open: bool,
    pub next_cue_frame: i128,
    pub rate: Rate,
    pub phase: Option<PositiveRatePhase>,
    pub max_overlap: usize,
    pub max_cue_burst: usize,
}
pub(crate) struct Practice {
    pub program: PreparedPracticeProgram,
    /// Endpoint.
    pub endpoint: PracticeAudioEndpoint,
    /// Cursor.
    pub cursor: Cursor,
}
impl Practice {
    /// Validates the supplied original values before changing any owner.
    pub fn new(
        program: PreparedPracticeProgram,
        endpoint: PracticeAudioEndpoint,
        region: PracticeRegion,
    ) -> Result<Self, PracticeError> {
        if !endpoint.matches(&program) {
            return Err(PracticeError::IncompatibleMixer);
        }
        let (anchor, duration) = program.data.initial_region(region)?;
        let preroll = program.data.preroll;
        let open = program.data.is_open(region);
        let rate = program.data.rate;
        let max_overlap = program.data.initial_overlap;
        let max_cue_burst = program.data.initial_cue_burst;
        if program.data.runtime_program_id != 0
            && program.data.pass_event_bound(
                PracticeRegion {
                    start: anchor,
                    ..region
                },
                max_cue_burst,
            ) > program.data.limits.max_events_per_render
        {
            return Err(PracticeError::EventBudget);
        }
        let phase = if program.data.runtime_program_id != 0 {
            Some(
                PositiveRatePhase::new(region.start, preroll, rate, (!open).then_some(region.end))
                    .map_err(|_| PracticeError::Overflow)?,
            )
        } else {
            None
        };
        Ok(Self {
            program,
            endpoint,
            cursor: Cursor {
                region,
                anchor,
                duration,
                origin: 0,
                next_cue: 0,
                generation: 1,
                revision: 0,
                iteration: 0,
                started: false,
                active: true,
                preroll,
                open,
                next_cue_frame: 0,
                rate,
                phase,
                max_overlap,
                max_cue_burst,
            },
        })
    }
    // Identical scalar state transition used by preflight and execution.
    /// Fn.
    pub fn step(
        program: &Program,
        c: &mut Cursor,
        frame: u64,
        request: Option<Control>,
    ) -> Result<Option<(u64, PracticeBoundaryKind)>, PracticeError> {
        if let Some(control) = request.filter(|control| control.rate.is_none()) {
            let r = control.request;
            if !control.next && r.at_playback_frame < frame {
                return Err(PracticeError::MissedBoundary);
            }
            if control.next || r.at_playback_frame == frame {
                if control.next && control.revision != c.revision {
                    if !control.disable {
                        i64::try_from(
                            i128::from(c.anchor.as_nanos()) - i128::from(r.region.start.as_nanos()),
                        )
                        .map_err(|_| PracticeError::Overflow)?;
                    }
                    return Ok(Some((r.id, PracticeBoundaryKind::ControlRejectedRevision)));
                }
                if !control.next && r.expected_generation != c.generation {
                    return Err(PracticeError::StaleGeneration);
                }
                c.revision = c.revision.checked_add(1).ok_or(PracticeError::Overflow)?;
                if control.disable {
                    c.region.repeat = false;
                    return Ok(Some((r.id, PracticeBoundaryKind::LoopDisabled)));
                }
                c.region = r.region;
                c.open = program.is_open(r.region);
                c.preroll = Duration::ZERO;
                Self::reset_phase(program, c)?;
                c.generation = c.generation.checked_add(1).ok_or(PracticeError::Overflow)?;
                c.iteration = 0;
                c.origin = frame;
                c.started = true;
                c.active = true;
                c.next_cue = program.cues.partition_point(|x| x.cue.at <= c.anchor);
                Self::set_next_cue_frame(program, c)?;
                return Ok(Some((r.id, PracticeBoundaryKind::Requested)));
            }
        }
        if !c.started {
            c.origin = frame;
            c.started = true;
            c.next_cue = program.cues.partition_point(|x| x.cue.at <= c.anchor);
            Self::set_next_cue_frame(program, c)?;
            return Ok(Some((0, PracticeBoundaryKind::Started)));
        }
        if c.active && !c.open && frame - c.origin >= c.duration {
            c.generation = c.generation.checked_add(1).ok_or(PracticeError::Overflow)?;
            c.iteration = c.iteration.checked_add(1).ok_or(PracticeError::Overflow)?;
            c.origin = frame;
            if !c.region.repeat {
                c.active = false;
                c.anchor = c.region.end;
                return Ok(Some((0, PracticeBoundaryKind::Ended)));
            }
            c.preroll = Duration::ZERO;
            Self::reset_phase(program, c)?;
            c.next_cue = program.cues.partition_point(|x| x.cue.at <= c.anchor);
            Self::set_next_cue_frame(program, c)?;
            return Ok(Some((0, PracticeBoundaryKind::Looped)));
        }
        Ok(None)
    }
    /// Fn.
    pub fn cue_due(program: &Program, c: &Cursor, frame: u64) -> bool {
        c.active
            && program.cues.get(c.next_cue).is_some_and(|x| {
                x.cue.at < c.region.end && c.next_cue_frame <= i128::from(frame - c.origin)
            })
    }
    pub(crate) fn set_next_cue_frame(
        program: &Program,
        c: &mut Cursor,
    ) -> Result<(), PracticeError> {
        c.next_cue_frame = match program
            .cues
            .get(c.next_cue)
            .filter(|cue| cue.cue.at < c.region.end)
        {
            Some(cue) if c.phase.is_some() => i128::from(Self::phase_frame(
                c.phase.unwrap(),
                cue.cue.at,
                program.output_rate,
            )?),
            Some(cue) if program.exact_timing => {
                i128::from(program.frame_at(c.region.start, cue.cue.at, c.preroll)?)
            }
            Some(cue) => ceil(
                (i128::from(cue.cue.at.as_nanos()) - i128::from(c.anchor.as_nanos()))
                    * i128::from(program.output_rate),
                1_000_000_000,
            ),
            None => 0,
        };
        Ok(())
    }
    pub fn advance_cue(program: &Program, c: &mut Cursor) -> Result<(), PracticeError> {
        c.next_cue += 1;
        Self::set_next_cue_frame(program, c)
    }
    pub fn song_at(program: &Program, c: &Cursor, frame: u64) -> Result<Timestamp, PracticeError> {
        if !program.exact_timing || !c.active {
            return Ok(c.anchor);
        }
        if let Some(phase) = c.phase {
            return phase
                .song_at_elapsed(Self::active_elapsed(program, c, frame))
                .map_err(|_| PracticeError::Overflow);
        }
        let elapsed = i128::from(frame - c.origin) * 1_000_000_000
            / i128::from(program.output_rate)
            - i128::from(c.preroll.as_nanos());
        let n = i128::from(program.rate.numerator());
        let d = i128::from(program.rate.denominator());
        let scaled = (elapsed / d)
            .checked_mul(n)
            .and_then(|q| q.checked_add((elapsed % d) * n / d));
        let song = scaled
            .and_then(|q| q.checked_add(i128::from(c.region.start.as_nanos())))
            .ok_or(PracticeError::Overflow)?;
        let song = if c.open {
            song
        } else {
            song.min(i128::from(c.region.end.as_nanos()))
        };
        i64::try_from(song)
            .map(Timestamp::from_nanos)
            .map_err(|_| PracticeError::Overflow)
    }
    pub(crate) fn active_elapsed(program: &Program, c: &Cursor, frame: u64) -> i128 {
        i128::from(frame - c.origin) * 1_000_000_000 / i128::from(program.output_rate)
    }
    pub(crate) fn phase_frame(
        phase: PositiveRatePhase,
        target: Timestamp,
        hz: u32,
    ) -> Result<u64, PracticeError> {
        let elapsed = phase
            .elapsed_at_original_song(target)
            .map_err(|_| PracticeError::Overflow)?;
        let frames = (elapsed / 1_000_000_000)
            .checked_mul(i128::from(hz))
            .and_then(|whole| {
                whole.checked_add(ceil(
                    (elapsed % 1_000_000_000) * i128::from(hz),
                    1_000_000_000,
                ))
            })
            .ok_or(PracticeError::Overflow)?;
        u64::try_from(frames).map_err(|_| PracticeError::Overflow)
    }
    fn reset_phase(program: &Program, c: &mut Cursor) -> Result<(), PracticeError> {
        if c.phase.is_some() {
            let (phase, duration) = program.runtime_region(c.region, c.rate, c.preroll)?;
            c.anchor = phase
                .song_at_elapsed(0)
                .map_err(|_| PracticeError::Overflow)?;
            c.duration = duration;
            c.phase = Some(phase);
        } else {
            (c.anchor, c.duration) = program.region(c.region)?;
        }
        Ok(())
    }
    pub(crate) fn prepare_rate_decision(
        &self,
        frame: u64,
        physical: u64,
        origin: ClockPoint,
    ) -> Result<Option<RateDecision>, PracticeError> {
        let Some(control) = self
            .endpoint
            .request(0)
            .filter(|control| control.rate.is_some())
        else {
            return Ok(None);
        };
        let program = &self.program.data;
        let old = self.cursor;
        let requested_rate = control.rate.unwrap();
        let mut decision = RateDecision {
            control,
            cursor: old,
            previous_rate: old.rate,
            requested_rate,
            active_elapsed: Self::active_elapsed(program, &old, frame),
            outcome: PracticeRateOutcome::Applied,
            cut_due_end: old.next_cue,
        };
        let candidate = (|| {
            if control.request.expected_generation != old.generation {
                return Err(PracticeRateRefusal::StaleGeneration);
            }
            if control.revision != old.revision {
                return Err(PracticeRateRefusal::StaleRevision);
            }
            if !control.restart && (!old.active || !old.open && frame - old.origin >= old.duration)
            {
                return Err(PracticeRateRefusal::Ended);
            }
            let mut cursor = old;
            cursor.revision = cursor
                .revision
                .checked_add(1)
                .ok_or(PracticeRateRefusal::DeadlineOverflow)?;
            if control.restart {
                cursor.region = control.request.region;
                cursor.open = program.is_open(cursor.region);
                cursor.preroll = Duration::ZERO;
                cursor.rate = requested_rate;
                cursor.max_overlap = control.max_overlap;
                cursor.max_cue_burst = control.max_cue_burst;
                cursor.generation = cursor
                    .generation
                    .checked_add(1)
                    .ok_or(PracticeRateRefusal::DeadlineOverflow)?;
                cursor.iteration = 0;
                cursor.origin = frame;
                cursor.active = true;
                Self::reset_phase(program, &mut cursor)
                    .map_err(|_| PracticeRateRefusal::DeadlineOverflow)?;
                cursor.next_cue = program
                    .cues
                    .partition_point(|cue| cue.cue.at <= cursor.anchor);
            } else if requested_rate == old.rate {
                decision.outcome = PracticeRateOutcome::Noop;
            } else {
                let phase = old
                    .phase
                    .ok_or(PracticeRateRefusal::DeadlineOverflow)?
                    .with_rate_at_elapsed(decision.active_elapsed, requested_rate)
                    .map_err(PracticeRateRefusal::Phase)?;
                cursor.rate = requested_rate;
                cursor.max_overlap = control.max_overlap;
                cursor.max_cue_burst = control.max_cue_burst;
                cursor.phase = Some(phase);
                if !cursor.open {
                    cursor.duration =
                        Self::phase_frame(phase, cursor.region.end, program.output_rate)
                            .map_err(|_| PracticeRateRefusal::DeadlineOverflow)?;
                }
            }
            Self::set_next_cue_frame(program, &mut cursor)
                .map_err(|_| PracticeRateRefusal::DeadlineOverflow)?;
            if cursor.open {
                let end = program
                    .cues
                    .partition_point(|cue| cue.cue.at < cursor.region.end);
                if let Some(cue) = end.checked_sub(1).and_then(|index| program.cues.get(index)) {
                    Self::phase_frame(cursor.phase.unwrap(), cue.cue.at, program.output_rate)
                        .map_err(|_| PracticeRateRefusal::DeadlineOverflow)?;
                }
            }
            Self::validate_deadline(program, &cursor, frame, physical, origin)
                .map_err(|_| PracticeRateRefusal::DeadlineOverflow)?;
            Ok(cursor)
        })();
        match candidate {
            Ok(cursor) => decision.cursor = cursor,
            Err(refusal) => decision.outcome = PracticeRateOutcome::Refused(refusal),
        }
        if !control.restart && matches!(decision.outcome, PracticeRateOutcome::Applied) {
            let mut due = old;
            let mut count = 0;
            while Self::cue_due(program, &due, frame) {
                count += 1;
                if count > program.limits.max_events_per_render {
                    return Err(PracticeError::EventBudget);
                }
                Self::advance_cue(program, &mut due)?;
            }
            decision.cut_due_end = due.next_cue;
        }
        Ok(Some(decision))
    }
    /// Fn.
    #[cfg(test)]
    pub fn preflight(&self, start: u64, frames: usize) -> Result<usize, PracticeError> {
        self.preflight_inner(start, frames, None, None, |_| Ok(()))
    }
    pub fn preflight_at(
        &self,
        start: u64,
        frames: usize,
        physical_start: u64,
        origin: ClockPoint,
        decision: Option<RateDecision>,
        visit: impl FnMut(PreflightVoiceStep) -> Result<(), PracticeError>,
    ) -> Result<usize, PracticeError> {
        self.preflight_inner(
            start,
            frames,
            Some((physical_start, origin)),
            decision,
            visit,
        )
    }
    fn preflight_inner(
        &self,
        start: u64,
        frames: usize,
        physical: Option<(u64, ClockPoint)>,
        decision: Option<RateDecision>,
        mut visit: impl FnMut(PreflightVoiceStep) -> Result<(), PracticeError>,
    ) -> Result<usize, PracticeError> {
        let snapshot = (0..self.program.data.limits.control_capacity)
            .take_while(|i| self.endpoint.request(*i).is_some())
            .count();
        let mut c = self.cursor;
        let mut controls = 0;
        let mut receipts = 0;
        let mut events = 0;
        let free_receipts = self.endpoint.free_receipts();
        for offset in 0..frames {
            let frame = start + offset as u64;
            let mut deadline_checked = false;
            if offset == 0 {
                if let Some(decision) = decision {
                    c = decision.cursor;
                    controls += 1;
                    receipts += 1;
                    events += 1;
                    match decision.outcome {
                        PracticeRateOutcome::Applied if decision.control.restart => {
                            visit(PreflightVoiceStep::Reset(c))?;
                            self.program.data.overlaps(c.anchor, |_| events += 1);
                        }
                        PracticeRateOutcome::Applied => visit(PreflightVoiceStep::Rate(c.rate))?,
                        _ => {}
                    }
                }
            }
            loop {
                let r = if controls < snapshot {
                    self.endpoint.request(controls)
                } else {
                    None
                };
                let Some((_, kind)) = Self::step(&self.program.data, &mut c, frame, r)? else {
                    break;
                };
                if let Some((physical_start, origin)) = physical {
                    Self::validate_deadline(
                        &self.program.data,
                        &c,
                        frame,
                        physical_start + offset as u64,
                        origin,
                    )?;
                    deadline_checked = true;
                }
                receipts += 1;
                events += 1;
                if matches!(
                    kind,
                    PracticeBoundaryKind::Requested
                        | PracticeBoundaryKind::LoopDisabled
                        | PracticeBoundaryKind::ControlRejectedRevision
                ) {
                    controls += 1;
                }
                if c.active
                    && !matches!(
                        kind,
                        PracticeBoundaryKind::LoopDisabled
                            | PracticeBoundaryKind::ControlRejectedRevision
                    )
                {
                    self.program.data.overlaps(c.anchor, |_| events += 1);
                }
                if !matches!(
                    kind,
                    PracticeBoundaryKind::LoopDisabled
                        | PracticeBoundaryKind::ControlRejectedRevision
                ) {
                    visit(PreflightVoiceStep::Reset(c))?;
                }
                // A mode control at the exact endpoint must not delay it by a
                // sample. Both ordered receipts belong to this same boundary.
            }
            if offset == 0 && !deadline_checked {
                if let Some((physical_start, origin)) = physical {
                    Self::validate_deadline(&self.program.data, &c, frame, physical_start, origin)?;
                }
            }
            while Self::cue_due(&self.program.data, &c, frame) {
                events += 1;
                visit(PreflightVoiceStep::Cue(
                    self.program.data.cues[c.next_cue],
                    c.rate,
                ))?;
                Self::advance_cue(&self.program.data, &mut c)?;
            }
            visit(PreflightVoiceStep::Advance)?;
            Self::song_at(&self.program.data, &c, frame)?;
            if events > self.program.data.limits.max_events_per_render {
                return Err(PracticeError::EventBudget);
            }
            self.endpoint
                .shared
                .metadata_sequence
                .load(Ordering::SeqCst)
                .checked_add(
                    u64::try_from(receipts)
                        .map_err(|_| PracticeError::Overflow)?
                        .checked_mul(2)
                        .ok_or(PracticeError::Overflow)?,
                )
                .ok_or(PracticeError::Overflow)?;
            if receipts > free_receipts {
                return Err(PracticeError::ReceiptFull);
            }
        }
        Ok(snapshot)
    }
    pub fn validate_deadline(
        program: &Program,
        c: &Cursor,
        frame: u64,
        physical: u64,
        origin: ClockPoint,
    ) -> Result<(), PracticeError> {
        if program.validate_deadline && c.active && !c.open {
            let remaining = c
                .duration
                .checked_sub(frame - c.origin)
                .ok_or(PracticeError::Overflow)?;
            let end = physical
                .checked_add(remaining)
                .ok_or(PracticeError::Overflow)?;
            super::TargetTime::from_frames(end, program.output_rate)?.point(origin)?;
        }
        Ok(())
    }
}
