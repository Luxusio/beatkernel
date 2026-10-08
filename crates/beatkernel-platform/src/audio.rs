//! Portable descriptions of native audio devices, requests and applied settings.
//!
//! Enumeration, probing and opening are off the real-time path. Native support
//! must be established by a backend: these types never fabricate a stream.
//! Exact configuration is the default; suggestions are never implicit consent.
//!
//! Native buffer conversion and reported period constraints can be tested
//! without opening a device:
//!
//! ```
//! use beatkernel_platform::audio::*;
//! let format = DeviceFormat::new(48_000, 2,
//!     SampleEncoding::Pcm { container_bits: 16, valid_bits: 16 }, Some(3))?;
//! let mut bytes = [0; 4];
//! encode_pcm(format, &[-1.0, 0.5], &mut bytes)?;
//! assert_eq!(bytes, [0, 128, 0, 64]);
//! let request = AudioStreamRequest::new(AudioDeviceId("explicit-device".into()),
//!     AudioBackendKind::Wasapi, AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
//!     format, BufferRequest::DeviceDefault, PeriodRequest::Frames(128))?;
//! let constraints = PeriodConstraints {
//!     min_frames: Some(48), max_frames: Some(512), fundamental_frames: Some(4),
//!     ..PeriodConstraints::default()
//! };
//! assert_eq!(resolve_period(&request, constraints)?.frames, 128);
//! assert_eq!(validate_buffer_size(&request, 512)?, false);
//! # Ok::<(), AudioPlatformError>(())
//! ```

// Fixed-size error diagnostics keep conversion error paths allocation-free.
#![allow(clippy::result_large_err)]

use beatkernel::{
    audio::{AudioFormat, RenderReport},
    time::{ClockMappingQuality, ClockPoint, Duration},
};
use std::fmt;

/// SDK-free ASIO buffer and sample-rate request validation.
pub mod asio;
pub mod cadence;
pub(crate) mod channel_remix;
mod convert;
#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub(crate) mod mixer_launch;
mod negotiation;
mod output_state;
pub mod presentation;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", test))]
pub(crate) mod telemetry;

pub use convert::encode_pcm;
pub use negotiation::{resolve_period, validate_buffer_size, ResolvedPeriod};
pub use output_state::NativeOutputState;
#[cfg(test)]
pub(crate) use output_state::output_state_fixtures::count_heap_calls;

/// Native audio API selected by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioBackendKind {
    /// Windows Audio Session API.
    Wasapi,
    /// Installed ASIO driver; SDK-combined builds follow GPLv3 distribution terms.
    Asio,
}

/// Explicit shared-mode initialization path, without invisible fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedPeriodPolicy {
    /// Request IAudioClient3 engine-period initialization.
    EnginePeriod,
    /// Explicitly select legacy shared initialization with device-default period.
    DeviceDefault,
}

/// Native session mode selected without fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioStreamMode {
    /// Shared session with the explicitly selected initialization path.
    Shared(SharedPeriodPolicy),
    /// Exclusive event-driven session; buffer and processing period must match.
    Exclusive,
}

/// Stable native endpoint identity; not a chart/audio asset binding.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AudioDeviceId(pub String);

/// Native endpoint state, preserved even when opening is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioDeviceState {
    /// Endpoint can currently be opened.
    Active,
    /// Endpoint is disabled.
    Disabled,
    /// Endpoint is disconnected.
    Unplugged,
    /// Endpoint is not present.
    NotPresent,
}

/// Enumerated endpoint metadata; allocated outside callbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Native stable identity.
    pub id: AudioDeviceId,
    /// Native display name.
    pub name: String,
    /// Endpoint state.
    pub state: AudioDeviceState,
    /// Whether this is the default console endpoint.
    pub default_console: bool,
    /// Whether this is the default multimedia endpoint.
    pub default_multimedia: bool,
    /// Whether this is the default communications endpoint.
    pub default_communications: bool,
}

/// Interleaved native sample encoding; widths are not preset sample rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleEncoding {
    /// Signed integer PCM with a 16/24/32-bit container and meaningful high bits.
    Pcm {
        /// Width of each sample's native container in bits.
        container_bits: u16,
        /// Nonzero significant bits, at most the container width.
        valid_bits: u16,
    },
    /// Native IEEE float32.
    Float32,
}

impl SampleEncoding {
    /// Native storage bytes per interleaved channel sample.
    pub const fn bytes_per_sample(self) -> u16 {
        match self {
            Self::Pcm { container_bits, .. } => container_bits / 8,
            Self::Float32 => 4,
        }
    }
}

/// Validated native interleaved format, including explicit speaker positions.
///
/// A missing mask means caller-selected unspecified native ordering. Zero is
/// explicit direct output without speaker assignment. A present nonzero mask
/// must contain exactly one bit per channel. No silent conversion.
///
/// ```
/// use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
/// let f = DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(3))?;
/// assert_eq!(f.block_align(), 8);
/// let direct = DeviceFormat::new(48_000, 8, SampleEncoding::Float32, Some(0))?;
/// assert_eq!(direct.channel_mask(), Some(0));
/// assert!(DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(1)).is_err());
/// assert!(DeviceFormat::new(48_000, 2,
///     SampleEncoding::Pcm { container_bits: 24, valid_bits: 25 }, None).is_err());
/// # Ok::<(), beatkernel_platform::audio::AudioPlatformError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceFormat {
    pcm: AudioFormat,
    encoding: SampleEncoding,
    channel_mask: Option<u32>,
}

impl DeviceFormat {
    /// Validates format widths, channels, mask and native byte-rate arithmetic.
    pub fn new(
        sample_rate: u32,
        channels: u16,
        encoding: SampleEncoding,
        channel_mask: Option<u32>,
    ) -> Result<Self, AudioPlatformError> {
        let pcm = AudioFormat::new(sample_rate, channels)
            .map_err(|_| AudioPlatformError::InvalidFormat)?;
        if let SampleEncoding::Pcm {
            container_bits,
            valid_bits,
        } = encoding
        {
            if !matches!(container_bits, 16 | 24 | 32)
                || valid_bits == 0
                || valid_bits > container_bits
            {
                return Err(AudioPlatformError::InvalidFormat);
            }
        }
        if channel_mask.is_some_and(|mask| {
            mask & !0x0003_ffff != 0 || (mask != 0 && mask.count_ones() != u32::from(channels))
        }) {
            return Err(AudioPlatformError::InvalidFormat);
        }
        let block_align = u32::from(channels) * u32::from(encoding.bytes_per_sample());
        if sample_rate.checked_mul(block_align).is_none() {
            return Err(AudioPlatformError::InvalidFormat);
        }
        Ok(Self {
            pcm,
            encoding,
            channel_mask,
        })
    }
    /// Internal mixing rate and channel count, without implicit conversion.
    pub const fn pcm(self) -> AudioFormat {
        self.pcm
    }
    /// Frames per second.
    pub const fn sample_rate(self) -> u32 {
        self.pcm.sample_rate()
    }
    /// Interleaved channel count.
    pub const fn channels(self) -> u16 {
        self.pcm.channels()
    }
    /// Native sample encoding.
    pub const fn encoding(self) -> SampleEncoding {
        self.encoding
    }
    /// Explicit speaker mask, or unspecified native ordering.
    pub const fn channel_mask(self) -> Option<u32> {
        self.channel_mask
    }
    /// Native bytes in one complete interleaved frame.
    pub const fn block_align(self) -> u16 {
        self.pcm.channels() * self.encoding.bytes_per_sample()
    }
    /// Native bytes per second; checked by construction.
    pub const fn bytes_per_second(self) -> u32 {
        self.sample_rate() * self.block_align() as u32
    }
}

/// Caller-controlled native buffer capacity, independent from processing period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferRequest {
    /// Explicitly accept the device-managed capacity for the selected mode.
    ///
    /// This permits engine-managed buffers without changing an Exact period
    /// request. Actual capacity remains available in AppliedStreamConfig.
    DeviceDefault,
    /// Exact positive frame count unless negotiation was explicitly enabled.
    Frames(u32),
    /// Positive integer nanoseconds; the backend reports native unit rounding.
    Duration(Duration),
}

/// Caller-selected native processing period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodRequest {
    /// Explicit device-default period; not an implicit fallback from EnginePeriod.
    DeviceDefault,
    /// Positive requested processing frames.
    Frames(u32),
    /// Positive integer nanoseconds.
    Duration(Duration),
}

/// Permission to change requested sizes, never device/mode/format substitution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NegotiationPolicy {
    /// Reject any unsupported size or rounding change and return a suggestion.
    #[default]
    Exact,
    /// Explicitly allow supported native period/buffer rounding or alignment.
    AllowSupportedRounding,
}

/// Checked explicit native stream request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioStreamRequest {
    device: AudioDeviceId,
    backend: AudioBackendKind,
    mode: AudioStreamMode,
    format: DeviceFormat,
    buffer: BufferRequest,
    period: PeriodRequest,
    negotiation: NegotiationPolicy,
}

impl AudioStreamRequest {
    /// Validates positive size requests; mode-specific feasibility is probed later.
    ///
    /// Exclusive matching and engine-period constraints require device evidence,
    /// so unsupported combinations return constraints from the native backend.
    pub fn new(
        device: AudioDeviceId,
        backend: AudioBackendKind,
        mode: AudioStreamMode,
        format: DeviceFormat,
        buffer: BufferRequest,
        period: PeriodRequest,
    ) -> Result<Self, AudioPlatformError> {
        if device.0.is_empty()
            || device.0.contains('\0')
            || !match buffer {
                BufferRequest::DeviceDefault => true,
                BufferRequest::Frames(n) => n != 0,
                BufferRequest::Duration(d) => d.as_nanos() > 0,
            }
            || !match period {
                PeriodRequest::DeviceDefault => true,
                PeriodRequest::Frames(n) => n != 0,
                PeriodRequest::Duration(d) => d.as_nanos() > 0,
            }
        {
            return Err(AudioPlatformError::InvalidRequest);
        }
        Ok(Self {
            device,
            backend,
            mode,
            format,
            buffer,
            period,
            negotiation: NegotiationPolicy::Exact,
        })
    }
    /// Explicitly opts into supported sizing changes; defaults to Exact.
    pub fn with_negotiation(mut self, policy: NegotiationPolicy) -> Self {
        self.negotiation = policy;
        self
    }
    /// Requested endpoint identity.
    pub fn device(&self) -> &AudioDeviceId {
        &self.device
    }
    /// Requested native API.
    pub const fn backend(&self) -> AudioBackendKind {
        self.backend
    }
    /// Requested session/initialization mode.
    pub const fn mode(&self) -> AudioStreamMode {
        self.mode
    }
    /// Requested native format.
    pub const fn format(&self) -> DeviceFormat {
        self.format
    }
    /// Requested buffer capacity.
    pub const fn buffer(&self) -> BufferRequest {
        self.buffer
    }
    /// Requested processing period.
    pub const fn period(&self) -> PeriodRequest {
        self.period
    }
    /// Explicit size-negotiation permission.
    pub const fn negotiation(&self) -> NegotiationPolicy {
        self.negotiation
    }
}

/// Native format-probe result; closest is a suggestion, never accepted silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatSupport {
    /// The exact requested format is supported for the selected mode.
    Exact,
    /// Unsupported; an optional native closest format is only advisory.
    Unsupported {
        /// Native suggested alternative, if the API reports one.
        closest: Option<DeviceFormat>,
    },
}

/// Device-provided timing/size constraints; None means not reported by the API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeriodConstraints {
    /// Minimum native buffer duration, when reported by the hardware engine.
    pub min_buffer_duration: Option<Duration>,
    /// Maximum native buffer duration, when reported by the hardware engine.
    pub max_buffer_duration: Option<Duration>,
    /// Wake policy for the buffer-duration query; None when no bounds reported.
    pub buffer_bounds_event_driven: Option<bool>,
    /// Native default engine period in frames, when explicitly reported.
    pub default_frames: Option<u32>,
    /// Default period in nanoseconds.
    pub default_period: Option<Duration>,
    /// Minimum engine period in frames.
    pub min_frames: Option<u32>,
    /// Maximum engine period in frames.
    pub max_frames: Option<u32>,
    /// Required frame-count multiple.
    pub fundamental_frames: Option<u32>,
    /// Required buffer frame-count alignment, if known.
    pub alignment_frames: Option<u32>,
    /// Native minimum period in nanoseconds, where frame bounds are unavailable.
    pub min_period: Option<Duration>,
    /// Native maximum period in nanoseconds, where reported.
    pub max_period: Option<Duration>,
}

/// Applied values reported separately from the original caller request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedStreamConfig {
    /// Unmodified original request, including explicit negotiation policy.
    pub requested: AudioStreamRequest,
    /// Actual format (must match request; changes require explicit new request).
    pub format: DeviceFormat,
    /// Native allocated buffer size in frames.
    pub buffer_frames: u32,
    /// Buffer duration in nanoseconds, with native rounding made visible.
    pub buffer_duration: Duration,
    /// Applied processing period in frames, if available.
    pub period_frames: Option<u32>,
    /// Applied processing period in integer nanoseconds.
    pub period_duration: Duration,
    /// Native-reported stream latency, not physical input-to-audio latency.
    pub stream_latency: Duration,
    /// Whether explicitly authorized native rounding changed requested sizing.
    pub sizing_adjusted: bool,
}

/// Native API reading quality, independent of clock-mapping uncertainty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioClockReadingQuality {
    /// Native API reports an accurate reading (WASAPI S_OK).
    Accurate,
    /// Native API reports degraded measurement accuracy (WASAPI S_FALSE).
    Degraded,
    /// The native reading quality has not been established.
    #[default]
    Unknown,
}

/// Raw device-clock reading, preserving its native units and QPC association.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioClockSnapshot {
    /// Native position; these units are not assumed to be sample frames.
    pub position: u64,
    /// Native position units per second; nonzero on a successful clock reading.
    pub frequency: u64,
    /// IAudioClock QPC association in 100 ns units, not raw QPC ticks.
    pub qpc_100ns: u64,
    /// Native measurement status; no numeric error bound is implied.
    pub reading_quality: AudioClockReadingQuality,
    /// Explicitly mapped host-domain association, if calibration succeeded.
    pub host_point: Option<ClockPoint>,
    /// Quality of the explicit host association.
    pub mapping_quality: ClockMappingQuality,
}

/// Fixed numeric stream state; native failures retain HRESULT without formatting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioStreamStatus {
    /// Opened and primed, not yet running.
    #[default]
    Ready,
    /// Native worker is running.
    Running,
    /// Worker has stopped and been joined.
    Stopped,
    /// Terminal native failure requiring explicit caller reopen.
    Failed {
        /// Raw signed native HRESULT.
        hresult: i32,
    },
    /// Worker panicked; joining observed the failure.
    WorkerPanicked,
}

/// Fixed stream telemetry; starvation is inferred, not hardware-underrun proof.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamCounters {
    /// Total frames successfully submitted to the native endpoint.
    pub submitted_frames: u64,
    /// Total native buffer fills.
    pub buffer_fills: u64,
    /// Last native padding in frames, where meaningful.
    pub padding_frames: u32,
    /// Inferred empty-running-buffer/starvation observations.
    pub inferred_starvations: u64,
    /// Inferred missed processing deadlines.
    pub inferred_deadline_misses: u64,
    /// Numeric native buffer/clock/wait failure count.
    pub native_failures: u64,
}

/// Off-thread stream telemetry snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStreamSnapshot {
    /// Whether counters/clock/render form one coherent worker publication.
    ///
    /// A bounded read colliding with publication may return false. In that
    /// case counters are unavailable (zero placeholders), clock/render are
    /// absent, and status is independently observed. Retry outside rendering.
    pub telemetry_available: bool,
    /// Worker lifecycle/terminal failure state.
    pub status: AudioStreamStatus,
    /// Native counters with inferred metrics explicitly labeled.
    pub counters: StreamCounters,
    /// Most recent successful device-clock reading.
    pub clock: Option<AudioClockSnapshot>,
    /// Most recent core mixer report, if a buffer was rendered.
    pub render: Option<RenderReport>,
}

/// Native request constraint requiring an explicit caller configuration choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationConstraint {
    /// Reported native buffer capacity differs from the explicit size request.
    BufferSize,
    /// Timer-driven filling is supported only for shared sessions.
    TimerRequiresShared,
    /// Exclusive event-driven buffering requires identical buffer/period.
    ExclusiveBufferEqualsPeriod,
    /// Engine-period shared mode is unavailable; explicitly choose legacy path.
    EnginePeriodUnavailable,
    /// Engine-period initialization does not independently select buffer capacity.
    EngineManagedBuffer,
    /// Legacy shared path requires explicitly selecting the default period.
    LegacyDeviceDefaultPeriod,
    /// Requested size is outside reported bounds or required multiples.
    PeriodBounds,
    /// Native exclusive buffer alignment requires caller-authorized resizing.
    BufferAlignment,
}

/// Precise off-thread platform failure with no implicit mode/device replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioPlatformError {
    /// Worker retirement is not confirmed, so mixer ownership cannot be transferred.
    RecoveryUnavailable,
    /// Format widths/rate/channels/mask/byte-rate are invalid.
    InvalidFormat,
    /// Empty device identity or nonpositive requested size.
    InvalidRequest,
    /// Exact requested endpoint is absent or unavailable.
    DeviceUnavailable,
    /// Requested backend is unavailable on this host.
    BackendUnavailable(AudioBackendKind),
    /// Legacy licensing failure retained for callers of earlier backend versions.
    /// Current WASAPI code reports unavailable ASIO rather than this variant.
    AsioLicenseUnresolved,
    /// Exact requested format is unavailable; optional closest is advisory.
    FormatUnsupported {
        /// Optional suggested format, not automatically opened.
        closest: Option<DeviceFormat>,
    },
    /// Native sizing/mode constraint with optional supported frame suggestion.
    ConfigurationUnsupported {
        /// Constraint requiring caller action.
        constraint: ConfigurationConstraint,
        /// Device-provided bounds/alignment, with unknown fields left None.
        constraints: PeriodConstraints,
        /// Supported buffer-size suggestion in frames, if known.
        suggested_buffer_frames: Option<u32>,
        /// Supported processing-period suggestion in frames, if known.
        suggested_period_frames: Option<u32>,
    },
    /// Endpoint is busy/exclusive access is unavailable.
    EndpointBusy,
    /// Exclusive mode is disabled for this endpoint.
    ExclusiveDisabled,
    /// Endpoint was invalidated; caller must explicitly reopen.
    DeviceInvalidated,
    /// Native API failed outside a more specific classified failure.
    Native {
        /// Raw HRESULT or native numeric failure value.
        code: i32,
    },
    /// Worker startup or shutdown/join failed.
    WorkerFailure,
    /// Capacity or allocation was insufficient during off-thread setup.
    Capacity,
}

impl fmt::Display for AudioPlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecoveryUnavailable => {
                f.write_str("audio mixer recovery requires confirmed worker retirement")
            }
            Self::InvalidFormat => f.write_str("invalid native sample format or speaker layout"),
            Self::InvalidRequest => {
                f.write_str("audio device identity and requested sizes must be nonempty/positive")
            }
            Self::DeviceUnavailable => f.write_str("requested audio endpoint is unavailable"),
            Self::BackendUnavailable(kind) => {
                write!(f, "requested {kind:?} backend is unavailable")
            }
            Self::AsioLicenseUnresolved => {
                f.write_str("ASIO licensing/distribution path remains unresolved")
            }
            Self::FormatUnsupported { closest } => write!(
                f,
                "requested format unsupported; advisory alternative: {closest:?}"
            ),
            Self::ConfigurationUnsupported {
                constraint,
                constraints,
                suggested_buffer_frames,
                suggested_period_frames,
            } => write!(
                f,
                "unsupported audio configuration: {constraint:?}; constraints: {constraints:?}; advisory buffer/period frames: {suggested_buffer_frames:?}/{suggested_period_frames:?}"
            ),
            Self::EndpointBusy => {
                f.write_str("audio endpoint is busy or exclusive access unavailable")
            }
            Self::ExclusiveDisabled => {
                f.write_str("exclusive mode disabled for requested endpoint")
            }
            Self::DeviceInvalidated => {
                f.write_str("audio endpoint invalidated; explicit reopen required")
            }
            Self::Native { code } => write!(f, "native audio failure {code:#x}"),
            Self::WorkerFailure => f.write_str("audio worker startup/shutdown/join failed"),
            Self::Capacity => f.write_str("audio setup capacity/allocation failed"),
        }
    }
}
impl std::error::Error for AudioPlatformError {}

/// Off-thread device and arbitrary-format probing boundary.
///
/// A finite list of tested formats must not be advertised as complete support.
/// Native stream construction can take ownership of a concrete core mixer;
/// that backend-specific ownership is not hidden behind a generic callback.
pub trait AudioOutputBackend {
    /// Enumerates native render identities/states and default-role metadata.
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioPlatformError>;
    /// Reads the native engine mix format for this exact device.
    fn mix_format(&self, device: &AudioDeviceId) -> Result<DeviceFormat, AudioPlatformError>;
    /// Probes an arbitrary valid caller format in the explicitly requested mode.
    fn supports_format(
        &self,
        device: &AudioDeviceId,
        mode: AudioStreamMode,
        format: DeviceFormat,
    ) -> Result<FormatSupport, AudioPlatformError>;
    /// Reads reported native period/alignment constraints for this format/mode.
    fn period_constraints(
        &self,
        device: &AudioDeviceId,
        mode: AudioStreamMode,
        format: DeviceFormat,
    ) -> Result<PeriodConstraints, AudioPlatformError>;
}

/// Off-thread lifecycle/readback boundary for an actually opened native stream.
///
/// Stop must wake, stop and join the worker before releasing its owned assets.
/// Implementations must also enforce this ownership order when dropped.
pub trait AudioOutputStream {
    /// Returns original request and actual native configuration separately.
    fn configuration(&self) -> &AppliedStreamConfig;
    /// Reads fixed-size numeric telemetry outside rendering.
    fn snapshot(&self) -> AudioStreamSnapshot;
    /// Starts an already primed stream, returning a precise startup failure.
    fn start(&mut self) -> Result<(), AudioPlatformError>;
    /// Stops and joins the worker before its queue/mixer/assets can be destroyed.
    fn stop(&mut self) -> Result<(), AudioPlatformError>;
    /// Optional direct render-start cadence after stop/join; None means unavailable.
    /// Summaries may allocate off the render worker and do not prove acoustic jitter.
    fn render_cadence(
        &self,
    ) -> Result<Option<cadence::RenderCadence>, cadence::RenderCadenceError> {
        Ok(None)
    }
}
