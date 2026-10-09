//! Cold original-asset programs and copied, bounded retained-output controls.
use super::{AudioError, AudioLimits, SampleBank, SampleId, VoiceId};
use crate::time::{ClockDomainId, ClockPoint, Timestamp};
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
                let output_frames = ceil(
                    sample.frames() as i128 * i128::from(bank.format().sample_rate()),
                    i128::from(sample.format().sample_rate()),
                );
                let voice_extent = ceil(
                    output_frames * 1_000_000_000,
                    i128::from(bank.format().sample_rate()),
                );
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
        Ok(Self {
            data: Arc::new(Program {
                cues: prepared,
                subtree_end,
                limits,
                output_rate: bank.format().sample_rate(),
                channels: bank.format().channels(),
            }),
        })
    }
    /// Validates the exact original-song interval and representable duration.
    pub fn validate_region(&self, region: PracticeRegion) -> Result<(), PracticeError> {
        self.data.region(region).map(|_| ())
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
}
impl Program {
    pub(crate) fn region(&self, region: PracticeRegion) -> Result<(Timestamp, u64), PracticeError> {
        if region.start >= region.end {
            return Err(PracticeError::InvalidRegion);
        }
        let applied = region.start;
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
    fn write(&self, words: [u64; 10]) {
        self.write_state(words, 2);
    }
    fn write_state(&self, words: [u64; 10], state: u8) {
        for (dst, src) in self.words.iter().zip(words) {
            dst.store(src, Ordering::Relaxed);
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
    controls: Vec<Slot<10>>,
    receipts: Vec<Slot<18>>,
    controller_alive: AtomicBool,
    audio_alive: AtomicBool,
    generation: AtomicU64,
    revision: AtomicU64,
    metadata_sequence: AtomicU64,
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
    // All metadata accesses in this snapshot/write protocol use the same SC
    // order. At most two observations, never an unbounded producer spin. Odd
    // means publication is in progress. The checked sequence never wraps.
    fn metadata(&self) -> Result<(u64, u64), PracticeError> {
        for _ in 0..2 {
            let before = self.metadata_sequence.load(Ordering::SeqCst);
            if !before.is_multiple_of(2) {
                continue;
            }
            let generation = self.generation.load(Ordering::SeqCst);
            let revision = self.revision.load(Ordering::SeqCst);
            if self.metadata_sequence.load(Ordering::SeqCst) == before {
                return Ok((generation, revision));
            }
        }
        Err(PracticeError::MetadataBusy)
    }
}
impl PracticeController {
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
        if !disable {
            self.shared.program.region(r.region)?;
        }
        if r.id == 0 || r.id <= self.last_id || !next && r.at_playback_frame < self.last_frame {
            return Err(PracticeError::InvalidRegion);
        }
        let (generation, revision) = self.shared.metadata()?;
        if r.expected_generation != generation {
            return Err(PracticeError::StaleGeneration);
        }
        if !self.shared.audio_alive.load(Ordering::Acquire) {
            return Err(PracticeError::Disconnected);
        }
        let slot = &self.shared.controls[self.write];
        if slot.ready.load(Ordering::Acquire) != 0 {
            return Err(PracticeError::Full);
        }
        slot.write([
            r.id,
            r.expected_generation,
            r.at_playback_frame,
            r.region.start.as_nanos() as u64,
            r.region.end.as_nanos() as u64,
            u64::from(r.region.repeat),
            u64::from(disable),
            u64::from(next),
            revision,
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
        let projected = ProjectedPracticeReceipt {
            receipt,
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
}
impl PracticeAudioEndpoint {
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
    pub(crate) fn receipt(&mut self, r: PracticeReceipt, revision: u64) {
        let sequence = self.shared.metadata_sequence.load(Ordering::SeqCst);
        self.shared
            .metadata_sequence
            .store(sequence + 1, Ordering::SeqCst);
        self.shared.generation.store(r.generation, Ordering::SeqCst);
        self.shared.revision.store(revision, Ordering::SeqCst);
        self.shared
            .metadata_sequence
            .store(sequence + 2, Ordering::SeqCst);
        self.shared.receipts[self.write].write_state(
            [
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
        let (anchor, duration) = program.data.region(region)?;
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
        if let Some(control) = request {
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
                let (anchor, duration) = program.region(r.region)?;
                c.region = r.region;
                c.anchor = anchor;
                c.duration = duration;
                c.generation = c.generation.checked_add(1).ok_or(PracticeError::Overflow)?;
                c.iteration = 0;
                c.origin = frame;
                c.started = true;
                c.active = true;
                c.next_cue = program.cues.partition_point(|x| x.cue.at <= anchor);
                return Ok(Some((r.id, PracticeBoundaryKind::Requested)));
            }
        }
        if !c.started {
            c.origin = frame;
            c.started = true;
            c.next_cue = program.cues.partition_point(|x| x.cue.at <= c.anchor);
            return Ok(Some((0, PracticeBoundaryKind::Started)));
        }
        if c.active && frame - c.origin >= c.duration {
            c.generation = c.generation.checked_add(1).ok_or(PracticeError::Overflow)?;
            c.iteration = c.iteration.checked_add(1).ok_or(PracticeError::Overflow)?;
            c.origin = frame;
            if !c.region.repeat {
                c.active = false;
                c.anchor = c.region.end;
                return Ok(Some((0, PracticeBoundaryKind::Ended)));
            }
            c.next_cue = program.cues.partition_point(|x| x.cue.at <= c.anchor);
            return Ok(Some((0, PracticeBoundaryKind::Looped)));
        }
        Ok(None)
    }
    /// Fn.
    pub fn cue_due(program: &Program, c: &Cursor, frame: u64) -> bool {
        c.active
            && program.cues.get(c.next_cue).is_some_and(|x| {
                x.cue.at < c.region.end
                    && ceil(
                        (i128::from(x.cue.at.as_nanos()) - i128::from(c.anchor.as_nanos()))
                            * i128::from(program.output_rate),
                        1_000_000_000,
                    ) <= i128::from(frame - c.origin)
            })
    }
    /// Fn.
    pub fn preflight(&self, start: u64, frames: usize) -> Result<usize, PracticeError> {
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
            loop {
                let r = if controls < snapshot {
                    self.endpoint.request(controls)
                } else {
                    None
                };
                let Some((_, kind)) = Self::step(&self.program.data, &mut c, frame, r)? else {
                    break;
                };
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
                // A mode control at the exact endpoint must not delay it by a
                // sample. Both ordered receipts belong to this same boundary.
            }
            while Self::cue_due(&self.program.data, &c, frame) {
                events += 1;
                c.next_cue += 1;
            }
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
}
