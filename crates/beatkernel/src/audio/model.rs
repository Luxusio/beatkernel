use crate::{
    time::{ClockDomainId, Timestamp},
    transport::Rate,
};
use std::fmt;

/// Caller-assigned immutable sample-bank identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleId(pub u64);

/// Caller-assigned active voice identity; duplicate Play replaces that voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(pub u64);

/// Validated rate and channel count for finite interleaved f32 PCM.
///
/// This does not imply device support or automatically convert channel layouts.
///
/// ```
/// use beatkernel::audio::AudioFormat;
/// let format = AudioFormat::new(48_000, 2)?;
/// assert_eq!(format.channels(), 2);
/// assert!(AudioFormat::new(0, 2).is_err());
/// # Ok::<(), beatkernel::audio::AudioError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    sample_rate: u32,
    channels: u16,
}

impl AudioFormat {
    /// Creates a format with nonzero rate and 1–32 interleaved channels.
    ///
    /// Rates are not restricted to a preset list; native support is probed later.
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, AudioError> {
        if sample_rate == 0 || channels == 0 || channels > 32 {
            return Err(AudioError::InvalidFormat);
        }
        Ok(Self {
            sample_rate,
            channels,
        })
    }
    /// Sample frames per second.
    pub const fn sample_rate(self) -> u32 {
        self.sample_rate
    }
    /// Interleaved channel count.
    pub const fn channels(self) -> u16 {
        self.channels
    }
}

/// Off-thread decoded PCM allocation limits, counted as f32 storage bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmLimits {
    max_asset_bytes: usize,
    max_total_bytes: usize,
    max_samples: usize,
}

impl PcmLimits {
    /// Allocation-representability ceiling, rounded down to complete f32 samples.
    ///
    /// Callers choose tighter limits appropriate for their available memory.
    pub const MAX_TOTAL_BYTES: usize =
        (isize::MAX as usize / std::mem::size_of::<f32>()) * std::mem::size_of::<f32>();
    /// Hard sample-bank asset-count ceiling.
    pub const MAX_SAMPLES: usize = 65_536;
    /// Validates explicit nonzero limits before any decoded storage allocation.
    ///
    /// `max_asset_bytes` must not exceed `max_total_bytes`. Smaller caller limits
    /// are accepted; no fixed asset size or sample-count preset is required.
    pub fn new(
        max_asset_bytes: usize,
        max_total_bytes: usize,
        max_samples: usize,
    ) -> Result<Self, AudioError> {
        if max_asset_bytes == 0
            || max_total_bytes == 0
            || max_samples == 0
            || max_asset_bytes > max_total_bytes
            || max_total_bytes > Self::MAX_TOTAL_BYTES
            || max_samples > Self::MAX_SAMPLES
        {
            return Err(AudioError::InvalidCapacity);
        }
        Ok(Self {
            max_asset_bytes,
            max_total_bytes,
            max_samples,
        })
    }
    /// Maximum decoded bytes in one asset.
    pub const fn max_asset_bytes(self) -> usize {
        self.max_asset_bytes
    }
    /// Maximum decoded bytes across the bank.
    pub const fn max_total_bytes(self) -> usize {
        self.max_total_bytes
    }
    /// Maximum number of bank assets.
    pub const fn max_samples(self) -> usize {
        self.max_samples
    }
}

/// Caller-selected bounded queue, mixer storage and callback-work capacities.
///
/// ```
/// use beatkernel::audio::{AudioError, AudioLimits, PcmLimits};
/// let limits = AudioLimits::new(2, 4, 1, 257, 2)?;
/// assert_eq!(limits.queue_capacity(), 2);
/// assert_eq!(limits.pending_capacity(), 1);
/// assert_eq!(AudioLimits::new(0, 4, 1, 257, 2), Err(AudioError::InvalidCapacity));
/// assert_eq!(PcmLimits::new(128, 64, 2), Err(AudioError::InvalidCapacity));
/// # Ok::<(), AudioError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioLimits {
    queue_capacity: usize,
    max_voices: usize,
    pending_capacity: usize,
    max_render_frames: usize,
    max_commands_per_render: usize,
}

impl AudioLimits {
    /// Maximum separately allocated queue or pending-command slots.
    pub const MAX_COMMANDS: usize = 65_536;
    /// Maximum simultaneous voices; no implicit voice stealing.
    pub const MAX_VOICES: usize = 4_096;
    /// Hard maximum frames in a single caller render buffer.
    pub const MAX_RENDER_FRAMES: usize = 1_048_576;
    /// Validates independent explicit nonzero capacities.
    ///
    /// The drain budget may exceed queue capacity; actual consumption still
    /// observes only a bounded snapshot. Queue and pending capacity may differ.
    pub fn new(
        queue_capacity: usize,
        max_voices: usize,
        pending_capacity: usize,
        max_render_frames: usize,
        max_commands_per_render: usize,
    ) -> Result<Self, AudioError> {
        if queue_capacity == 0
            || queue_capacity > Self::MAX_COMMANDS
            || max_voices == 0
            || max_voices > Self::MAX_VOICES
            || pending_capacity == 0
            || pending_capacity > Self::MAX_COMMANDS
            || max_render_frames == 0
            || max_render_frames > Self::MAX_RENDER_FRAMES
            || max_commands_per_render == 0
            || max_commands_per_render > Self::MAX_COMMANDS
        {
            return Err(AudioError::InvalidCapacity);
        }
        Ok(Self {
            queue_capacity,
            max_voices,
            pending_capacity,
            max_render_frames,
            max_commands_per_render,
        })
    }
    /// Number of command queue slots.
    pub const fn queue_capacity(self) -> usize {
        self.queue_capacity
    }
    /// Number of preallocated active-voice slots.
    pub const fn max_voices(self) -> usize {
        self.max_voices
    }
    /// Number of preallocated pending commands, distinct from queue slots.
    pub const fn pending_capacity(self) -> usize {
        self.pending_capacity
    }
    /// Maximum frames accepted by one render call.
    pub const fn max_render_frames(self) -> usize {
        self.max_render_frames
    }
    /// Maximum commands drained during one render call.
    pub const fn max_commands_per_render(self) -> usize {
        self.max_commands_per_render
    }
}

/// Immutable output frame-grid origin, clock domain and preallocated capacities.
///
/// A mixer owns its absolute output frame cursor. Callers cannot introduce a
/// fresh timestamp anchor at each render block. Seek affects song position,
/// never this scheduling grid. Native buffer negotiation precedes construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixerConfig {
    format: AudioFormat,
    domain: ClockDomainId,
    origin: Timestamp,
    limits: AudioLimits,
    playback_end_frame: Option<u64>,
}

impl MixerConfig {
    /// Selects already-validated format/capacities and an explicit output origin.
    pub const fn new(
        format: AudioFormat,
        domain: ClockDomainId,
        origin: Timestamp,
        limits: AudioLimits,
    ) -> Self {
        Self {
            format,
            domain,
            origin,
            limits,
            playback_end_frame: None,
        }
    }
    /// Internal output PCM format.
    pub const fn format(self) -> AudioFormat {
        self.format
    }
    /// Domain in which every command's `at` timestamp must be supplied.
    pub const fn domain(self) -> ClockDomainId {
        self.domain
    }
    /// Timestamp corresponding to absolute output frame zero.
    pub const fn origin(self) -> Timestamp {
        self.origin
    }
    /// Explicit bounded storage/work limits.
    pub const fn limits(self) -> AudioLimits {
        self.limits
    }
    /// Sets an exclusive immutable endpoint on the absolute playback frame
    /// grid. Frames at/after it stay silent even after a queue resume request.
    /// Repetition requires a fresh mixer; zero prevents command consumption.
    pub const fn with_playback_end_frame(mut self, end: u64) -> Self {
        self.playback_end_frame = Some(end);
        self
    }
    /// Exclusive playback endpoint, or no finite fence by default.
    pub const fn playback_end_frame(self) -> Option<u64> {
        self.playback_end_frame
    }
}

/// Copy-only scheduling command without heap-owning payloads.
///
/// `at` is in [`MixerConfig::domain`], relative to its immutable output origin.
/// It is not song time unless the caller has explicitly mapped the clocks.
/// After an explicit queue pause, this scheduling grid excludes inserted silent
/// frames. Map physical output times through the reported playback cursor before
/// admitting commands; pending targets stay on their original playback grid.
/// Commands are validated on admission/execution; the queue can preserve even
/// rejected scalar payloads. Equal-frame commands retain submission order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioCommand {
    /// Start or replace a voice after validating all command fields.
    Play {
        /// Caller-selected voice identity.
        voice: VoiceId,
        /// Bank asset identity.
        sample: SampleId,
        /// Target output-domain timestamp, rounded upward to its next frame.
        at: Timestamp,
        /// Finite signed gain; negative values invert PCM polarity.
        gain: f32,
    },
    /// Stop an active voice; unknown identities are counted no-ops.
    Stop {
        /// Caller-selected voice identity.
        voice: VoiceId,
        /// Target output-domain timestamp.
        at: Timestamp,
    },
    /// Change pitch-changing rational sample-head speed for all voices.
    SetRate {
        /// Existing normalized rational rate, including zero and reverse.
        rate: Rate,
        /// Target output-domain timestamp.
        at: Timestamp,
    },
    /// Clear voices and set song position, preserving rate and future commands.
    Seek {
        /// New observable song-position anchor.
        song_time: Timestamp,
        /// Target output-domain timestamp; output chronology is not rewound.
        at: Timestamp,
    },
}

impl AudioCommand {
    /// Returns the target timestamp in the configured scheduling domain.
    pub const fn at(self) -> Timestamp {
        match self {
            Self::Play { at, .. }
            | Self::Stop { at, .. }
            | Self::SetRate { at, .. }
            | Self::Seek { at, .. } => at,
        }
    }
}

/// Fixed-size numeric cumulative telemetry, readable outside rendering.
///
/// Counts saturate at u64::MAX rather than wrapping. Device starvation is
/// reported separately by the platform and is not proven by these counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioCounters {
    /// Frames produced by successful render calls.
    pub rendered_frames: u64,
    /// Commands consumed from the input queue.
    pub commands_consumed: u64,
    /// Commands successfully executed.
    pub commands_applied: u64,
    /// Commands executed after their scheduled frame.
    pub late_commands: u64,
    /// Commands rejected because pending storage was full.
    pub pending_full: u64,
    /// Play commands rejected because all voice slots were occupied.
    pub voice_full: u64,
    /// Play commands with unknown bank asset identities.
    pub unknown_samples: u64,
    /// Stop commands with no matching active voice.
    pub unknown_stops: u64,
    /// Non-finite gain commands rejected before modifying voices.
    pub invalid_gains: u64,
    /// Unsupported or unrepresentable rate commands.
    pub invalid_rates: u64,
    /// Commands whose target frame or other time arithmetic was unrepresentable.
    pub invalid_times: u64,
}

/// Fixed render outcome; constructing/returning it requires no allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderReport {
    /// Absolute first output frame of this block.
    pub start_frame: u64,
    /// Number of contiguous frames produced.
    pub frames: usize,
    /// First frame on the scheduling grid, excluding explicit paused silence.
    /// Equal to `start_frame` until the first inserted silent pause frames.
    pub playback_start_frame: u64,
    /// Scheduling frames processed by this block, including an active prefix
    /// before an immutable endpoint; zero during a whole-block explicit pause.
    /// A zero sample-head Rate still advances this scheduling grid.
    pub playback_frames: usize,
    /// Applied pause state at block end, including an immutable endpoint reached
    /// after an active prefix. Only valid nonempty renders adopt requests/fences.
    /// This is render evidence, not proof of native/acoustic presentation.
    pub paused: bool,
    /// Physical exclusive prefix end when the immutable playback endpoint was
    /// first reached by a valid nonempty render. Retained across later reports,
    /// including silence/empty blocks; None for unlimited or not-yet-ended output.
    /// This render evidence does not prove native/acoustic presentation.
    pub playback_end_physical_frame: Option<u64>,
    /// Number of active voices after this block.
    pub active_voices: usize,
    /// Number of pending commands after this block.
    pub pending_commands: usize,
    /// Song-position anchor most recently assigned by Seek (initially zero).
    pub song_position: Timestamp,
    /// Whether the producer has disconnected; no queue ownership is dropped.
    pub producer_disconnected: bool,
    /// Cumulative counters after this block.
    pub counters: AudioCounters,
}

/// Allocation-free setup/render validation errors; formatting belongs off RT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// Zero rate/channels or more than 32 channels.
    InvalidFormat,
    /// A capacity is zero, exceeds its hard bound or has inconsistent limits.
    InvalidCapacity,
    /// Offline storage allocation failed.
    AllocationFailed,
    /// Requested decoded or bank storage exceeds its configured limit.
    PcmCapacity,
    /// A PCM sample is not finite.
    NonFiniteSample,
    /// Sample channels differ from the chosen output channels.
    ChannelMismatch,
    /// Duplicate caller-selected sample-bank identity.
    DuplicateSample,
    /// Output or source storage is not aligned to complete interleaved frames.
    InvalidBuffer,
    /// A render block exceeds its configured frame limit.
    RenderCapacity,
    /// Checked time, frame or allocation arithmetic overflowed.
    Overflow,
    /// Startup scheduling was requested on an ordinary queue.
    StartGateUnavailable,
    /// This gated queue already owns an immutable start target.
    StartGateAlreadyArmed,
    /// The relevant endpoint disconnected before startup admission/application.
    StartGateDisconnected,
    /// The exact physical start target is behind the rendered frontier.
    StartGateMissed,
    /// Manual pause intersects the initial target and would move first playback.
    StartGatePaused,
    /// Rate cannot be represented by the configured sample-head arithmetic.
    UnsupportedRate,
    /// Retained program receipt capacity exhausted before any render mutation.
    PracticeReceiptFull,
    /// Retained program event budget exhausted before any render mutation.
    PracticeEventBudget,
    /// Retained program request is stale, late, invalid or overflows.
    PracticeControl,
    /// A fresh conversion owner cannot reinterpret pending original-output proofs.
    PracticeProjectionPending,
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidFormat => "audio rate/channels must be nonzero; at most 32 channels",
            Self::InvalidCapacity => {
                "audio capacity is zero, inconsistent or exceeds its documented limit"
            }
            Self::AllocationFailed => "audio storage allocation failed",
            Self::PcmCapacity => "decoded PCM exceeds configured storage capacity",
            Self::NonFiniteSample => "PCM samples must be finite",
            Self::ChannelMismatch => {
                "PCM channels differ from output; convert explicitly before loading"
            }
            Self::DuplicateSample => "sample-bank identity already exists",
            Self::InvalidBuffer => "audio buffer must contain complete interleaved frames",
            Self::RenderCapacity => "render block exceeds configured frame capacity",
            Self::Overflow => "audio time/frame/storage arithmetic overflow",
            Self::StartGateUnavailable => "queue has no initial start gate",
            Self::StartGateAlreadyArmed => "initial start target is immutable",
            Self::StartGateDisconnected => "startup endpoint disconnected",
            Self::StartGateMissed => "initial physical start frame was missed",
            Self::StartGatePaused => "manual pause intersects initial start frame",
            Self::PracticeReceiptFull => "practice receipt capacity exhausted",
            Self::PracticeEventBudget => "practice event budget exhausted",
            Self::PracticeControl => "practice control is stale, late or invalid",
            Self::PracticeProjectionPending => {
                "practice receipts must drain before transferring to a new conversion owner"
            }
            Self::UnsupportedRate => "rate is not representable by this mixer",
        })
    }
}
impl std::error::Error for AudioError {}
