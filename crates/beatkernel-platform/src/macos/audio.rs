//! Native CoreAudio HAL output with exact settings and guarded preallocated IOProc.
use super::{clock::MachClock, ffi};
use crate::audio::{AudioStreamSnapshot, AudioStreamStatus, StreamCounters, telemetry::Telemetry};
use beatkernel::{
    audio::{AudioFormat, ChannelMatrix, FormatConverter, Mixer, RenderReport},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};
const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const DEVICES: u32 = u32::from_be_bytes(*b"dev#");
const STREAMS: u32 = u32::from_be_bytes(*b"stm#");
const STREAM_CONFIG: u32 = u32::from_be_bytes(*b"slay");
const FORMAT: u32 = u32::from_be_bytes(*b"sfmt");
const RATE: u32 = u32::from_be_bytes(*b"nsrt");
const BUFFER: u32 = u32::from_be_bytes(*b"fsiz");
const NAME: u32 = u32::from_be_bytes(*b"name");
const UID: u32 = u32::from_be_bytes(*b"uid ");
const FLOAT: u32 = 1;
const PACKED: u32 = 8;
const PLANAR: u32 = 32;
const MAX_BUFFERS: usize = 32;

/// An output endpoint identified by the exact native AudioDeviceID.
#[derive(Clone, Debug, PartialEq)]
pub struct CoreAudioDevice {
    /// Native AudioDeviceID; no system-default substitution is performed.
    pub id: u32,
    /// Persistent native endpoint UID, when reported.
    pub uid: Option<String>,
    /// Native display name, when reported.
    pub name: Option<String>,
    /// Current device nominal rate, not an exhaustive support list.
    pub nominal_rate: f64,
    /// Current native IO buffer frames.
    pub buffer_frames: u32,
    /// Total reported output channels across native buffers.
    pub output_channels: u32,
}
/// Exact HAL request, distinct from WASAPI shared/exclusive modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreAudioRequest {
    /// Explicit enumerated AudioDeviceID.
    pub device: u32,
    /// Exact sample rate and total output channel count; native float32 only.
    pub format: AudioFormat,
    /// Exact positive native buffer frames; no implicit rounding/fallback.
    pub buffer_frames: u32,
}
/// Readback of the actual native output configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreAudioApplied {
    /// Original exact device/format/buffer request.
    pub request: CoreAudioRequest,
    /// Read-back exact native PCM rate/channels.
    pub format: AudioFormat,
    /// Read-back native IO buffer size.
    pub buffer_frames: u32,
    /// Channel count for each validated native buffer, in stream order.
    pub buffer_channels: Vec<u32>,
    /// Domain of the immutable mixer frame grid; not inferred as mach time.
    pub output_domain: ClockDomainId,
    /// Caller-supplied mixer frame-zero point.
    pub output_origin: Timestamp,
    /// Explicit mach host-time domain of actual presentation timestamps.
    pub native_clock: ClockDomainId,
}
/// Off-callback precise native/configuration/lifecycle failure.
#[derive(Clone, Debug, PartialEq)]
pub enum CoreAudioError {
    /// Callback unregister/drain has not succeeded; mixer transfer is unavailable.
    RecoveryUnavailable,
    /// Native OSStatus and operation retained for diagnosis.
    Native {
        /// API which returned the error.
        operation: &'static str,
        /// Exact native status code.
        code: i32,
    },
    /// Device/frames are zero, or mixer configuration does not match request.
    InvalidRequest,
    /// Requested native device is absent or has no output buffers.
    DeviceUnavailable,
    /// Current virtual stream format is not packed native float32 PCM.
    UnsupportedFormat,
    /// Actual native rate/buffer does not equal the exact request.
    AppliedMismatch {
        /// Read-back nominal sample rate.
        rate: f64,
        /// Read-back IO buffer frames.
        buffer_frames: u32,
    },
    /// Unsupported/malformed stream buffer layout or excessive channel count.
    UnsupportedLayout,
    /// Mixer/scratch capacity is insufficient or allocation failed off-thread.
    Capacity,
    /// Closed streams cannot restart; explicitly open another stream.
    Closed,
    /// Format/rate/buffer changed and the stream requires explicit reopening.
    ConfigurationChanged,
}
impl std::fmt::Display for CoreAudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RecoveryUnavailable => {
                f.write_str("CoreAudio mixer recovery requires confirmed callback retirement")
            }
            Self::Native { operation, code } => write!(f, "{operation} failed: {code:#x}"),
            Self::InvalidRequest => f.write_str("CoreAudio exact request/mixer mismatch"),
            Self::DeviceUnavailable => f.write_str("explicit CoreAudio output device unavailable"),
            Self::UnsupportedFormat => {
                f.write_str("CoreAudio virtual streams require packed native float32 PCM")
            }
            Self::AppliedMismatch {
                rate,
                buffer_frames,
            } => write!(
                f,
                "CoreAudio exact setting mismatch: rate {rate}, frames {buffer_frames}"
            ),
            Self::UnsupportedLayout => f.write_str("unsupported CoreAudio output buffer layout"),
            Self::Capacity => f.write_str("CoreAudio preallocated render capacity insufficient"),
            Self::Closed => f.write_str("CoreAudio stream is closed; explicitly reopen"),
            Self::ConfigurationChanged => {
                f.write_str("CoreAudio configuration changed; explicitly reopen")
            }
        }
    }
}
impl std::error::Error for CoreAudioError {}
/// One actual native presentation observation paired with the logical mixer grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreAudioPresentation {
    /// Native mach absolute ticks from the IOProc output AudioTimeStamp.
    pub host_ticks: u64,
    /// Converted absolute mach nanoseconds when native host-time flag is valid.
    pub native: Option<ClockPoint>,
    /// Actual native sample-frame timestamp when its validity flag is present.
    pub native_sample_frame: Option<f64>,
    /// Logical mixer point for the first rendered frame, independent of mach.
    pub output_grid: Option<ClockPoint>,
    /// Exact native AudioTimeStamp validity flags.
    pub flags: u32,
    /// Core mixer absolute first frame rendered by this callback.
    pub first_frame: u64,
    /// Number of frames rendered by this callback.
    pub frames: u32,
}
/// Fixed atomic stream telemetry; no device/property queries occur in callback.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CoreAudioSnapshot {
    /// Total successful mixer callbacks.
    pub rendered_callbacks: u64,
    /// Total successful output frames.
    pub rendered_frames: u64,
    /// Invalid layouts/capacities/mixer failures or concurrent callback attempts.
    pub callback_failures: u64,
    /// Native format/rate/buffer listener observed a configuration change.
    pub configuration_changed: bool,
    /// Most recent coherent presentation sample, or None during concurrent write.
    pub presentation: Option<CoreAudioPresentation>,
}
struct RenderState {
    mixer: Mixer,
    remix: Option<FormatConverter>,
    scratch: Vec<f32>,
    render_version: u64,
}
struct Context {
    render: UnsafeCell<RenderState>,
    render_telemetry: Telemetry,
    cadence: Box<crate::audio::cadence::Capture>,
    clock: MachClock,
    origin: Timestamp,
    output_domain: ClockDomainId,
    rate: u32,
    channels: u16,
    layout: Vec<u32>,
    buffer_frames: u32,
    enabled: AtomicBool,
    rendering: AtomicBool,
    active: AtomicUsize,
    changed: AtomicBool,
    calls: AtomicU64,
    frames: AtomicU64,
    failures: AtomicU64,
    version: AtomicU64,
    ticks: AtomicU64,
    sample_bits: AtomicU64,
    flags: AtomicU32,
    first_frame: AtomicU64,
    block_frames: AtomicU32,
}
/// Recoverable CoreAudio opening error with retained callback owner for retry.
pub type CoreAudioOpenFailure =
    beatkernel::audio::OutputOpenFailure<CoreAudioError, CoreAudioStream>;

#[cfg(test)]
fn validate_open(request: &CoreAudioRequest, mixer: &Mixer) -> Result<(), CoreAudioError> {
    validate_open_with_matrix(request, mixer, None)
}
fn validate_open_with_matrix(
    request: &CoreAudioRequest,
    mixer: &Mixer,
    matrix: Option<&ChannelMatrix>,
) -> Result<(), CoreAudioError> {
    if request.device == 0 || request.buffer_frames == 0 {
        return Err(CoreAudioError::InvalidRequest);
    }
    if let Some(matrix) = matrix {
        crate::audio::channel_remix::validate(mixer.config().format(), request.format, matrix)
            .map_err(|_| CoreAudioError::InvalidRequest)?;
    } else if mixer.config().format() != request.format {
        return Err(CoreAudioError::InvalidRequest);
    }
    if mixer.config().limits().max_render_frames() < request.buffer_frames as usize {
        return Err(CoreAudioError::Capacity);
    }
    Ok(())
}
/// Owned HAL IOProc and stable callback state, with explicit start/close lifecycle.
/// Mutable mixer storage is accessed only by one guarded native callback.
/// This owner is intentionally !Send/!Sync; callback telemetry uses atomics.
pub struct CoreAudioStream {
    basis: beatkernel::audio::OutputFrameBasis,
    applied: CoreAudioApplied,
    context: Option<Box<Context>>,
    recovered_mixer: Option<Mixer>,
    retired: bool,
    proc_id: ffi::IoProcId,
    listeners: Vec<(u32, ffi::PropertyAddress)>,
    started: bool,
    owner: PhantomData<Rc<()>>,
    final_snapshot: CoreAudioSnapshot,
    final_render_report: Option<RenderReport>,
    final_cadence: Option<Box<crate::audio::cadence::Capture>>,
}
impl CoreAudioStream {
    /// Original physical mixer frame basis captured before any native operation.
    pub const fn frame_basis(&self) -> beatkernel::audio::OutputFrameBasis {
        self.basis
    }
    /// Query the OS default media output without changing or opening a stream.
    pub fn default_output_device() -> Result<u32, CoreAudioError> {
        // AudioHardware.h: system object 1, global UInt32 'dOut' property.
        let id: u32 = scalar(1, address(u32::from_be_bytes(*b"dOut"), GLOBAL))?;
        if id == 0 {
            return Err(CoreAudioError::DeviceUnavailable);
        }
        Ok(id)
    }

    /// Enumerates actual render devices; it never selects or opens the default.
    pub fn devices() -> Result<Vec<CoreAudioDevice>, CoreAudioError> {
        let identities = u32_property(1, address(DEVICES, GLOBAL))?;
        let mut devices = Vec::new();
        for id in identities {
            let layout = native_layout(id)?;
            if layout.is_empty() {
                continue;
            }
            devices.push(CoreAudioDevice {
                id,
                uid: string_property(id, UID),
                name: string_property(id, NAME),
                nominal_rate: scalar(id, address(RATE, GLOBAL))?,
                buffer_frames: scalar(id, address(BUFFER, GLOBAL))?,
                output_channels: layout.iter().sum(),
            });
        }
        Ok(devices)
    }
    /// Applies exact device-global rate/buffer settings, validates actual virtual
    /// stream ASBDs/layout and registers an owned callback without starting it.
    /// Mixer format/capacity must already match; its output grid origin/domain
    /// stays explicit and is paired with observed native presentation telemetry.
    pub fn open(
        request: CoreAudioRequest,
        clock: MachClock,
        mixer: Mixer,
    ) -> Result<Self, CoreAudioError> {
        Self::open_recoverable(request, clock, mixer).map_err(|failure| failure.into_parts().0)
    }
    /// Retains software ownership or the actual partial callback owner on failure.
    /// Device-global setting changes are not rolled back by this operation.
    pub fn open_recoverable(
        request: CoreAudioRequest,
        clock: MachClock,
        mixer: Mixer,
    ) -> Result<Self, CoreAudioOpenFailure> {
        Self::open_impl(request, clock, mixer, None)
    }
    /// Opens with an explicit channel matrix at the original source/device rate.
    /// Native layouts and original frame/pause/end/retirement evidence remain exact.
    pub fn open_remixed_recoverable(
        request: CoreAudioRequest,
        clock: MachClock,
        mixer: Mixer,
        matrix: ChannelMatrix,
    ) -> Result<Self, CoreAudioOpenFailure> {
        Self::open_impl(request, clock, mixer, Some(matrix))
    }
    fn open_impl(
        request: CoreAudioRequest,
        clock: MachClock,
        mixer: Mixer,
        matrix: Option<ChannelMatrix>,
    ) -> Result<Self, CoreAudioOpenFailure> {
        let basis = mixer.output_frame_basis();
        if let Err(error) = validate_open_with_matrix(&request, &mixer, matrix.as_ref()) {
            return Err(CoreAudioOpenFailure::recovered(error, Some(mixer)));
        }
        let staged = (|| -> Result<_, CoreAudioError> {
            if !u32_property(1, address(DEVICES, GLOBAL))?.contains(&request.device) {
                return Err(CoreAudioError::DeviceUnavailable);
            }
            let initial_layout = native_layout(request.device)?;
            if initial_layout.is_empty() {
                return Err(CoreAudioError::DeviceUnavailable);
            }
            if initial_layout.iter().sum::<u32>() != u32::from(request.format.channels()) {
                return Err(CoreAudioError::UnsupportedLayout);
            }
            let requested_rate = f64::from(request.format.sample_rate());
            let prior_rate: f64 = scalar(request.device, address(RATE, GLOBAL))?;
            if prior_rate != requested_rate {
                set_scalar(request.device, address(RATE, GLOBAL), requested_rate)?;
            }
            let prior_frames: u32 = scalar(request.device, address(BUFFER, GLOBAL))?;
            if prior_frames != request.buffer_frames {
                set_scalar(
                    request.device,
                    address(BUFFER, GLOBAL),
                    request.buffer_frames,
                )?;
            }
            let actual_rate: f64 = scalar(request.device, address(RATE, GLOBAL))?;
            let actual_frames: u32 = scalar(request.device, address(BUFFER, GLOBAL))?;
            if actual_rate != requested_rate || actual_frames != request.buffer_frames {
                return Err(CoreAudioError::AppliedMismatch {
                    rate: actual_rate,
                    buffer_frames: actual_frames,
                });
            }
            let streams = u32_property(request.device, address(STREAMS, OUTPUT))?;
            if streams.is_empty() {
                return Err(CoreAudioError::DeviceUnavailable);
            }
            let mut expected = Vec::new();
            for stream in &streams {
                let format: ffi::Asbd = scalar(*stream, address(FORMAT, GLOBAL))?;
                validate_format(format, requested_rate)?;
                if format.format_flags & PLANAR != 0 {
                    for _ in 0..format.channels_per_frame {
                        expected.push(1);
                    }
                } else {
                    expected.push(format.channels_per_frame);
                }
            }
            let layout = native_layout(request.device)?;
            if layout != expected
                || layout.len() > MAX_BUFFERS
                || layout.iter().sum::<u32>() != u32::from(request.format.channels())
            {
                return Err(CoreAudioError::UnsupportedLayout);
            }
            let config = mixer.config();
            let samples = (request.buffer_frames as usize)
                .checked_mul(usize::from(request.format.channels()))
                .ok_or(CoreAudioError::Capacity)?;
            let mut scratch = Vec::new();
            scratch
                .try_reserve_exact(samples)
                .map_err(|_| CoreAudioError::Capacity)?;
            scratch.resize(samples, 0.0);
            let remix = matrix
                .map(|matrix| {
                    crate::audio::channel_remix::prepare(
                        config,
                        request.format,
                        matrix,
                        actual_frames as usize,
                    )
                    .map_err(|_| CoreAudioError::Capacity)
                })
                .transpose()?;
            let applied = CoreAudioApplied {
                request,
                format: request.format,
                buffer_frames: actual_frames,
                buffer_channels: layout.clone(),
                output_domain: config.domain(),
                output_origin: config.origin(),
                native_clock: clock.native_domain(),
            };
            Ok((applied, layout, scratch, streams, remix))
        })();
        let (applied, layout, scratch, streams, remix) = match staged {
            Ok(staged) => staged,
            Err(error) => return Err(CoreAudioOpenFailure::recovered(error, Some(mixer))),
        };
        let config = mixer.config();
        let context = Box::new(Context {
            render: UnsafeCell::new(RenderState {
                mixer,
                remix,
                scratch,
                render_version: 0,
            }),
            render_telemetry: Telemetry::new(),
            cadence: Box::new(crate::audio::cadence::Capture::new()),
            clock,
            origin: config.origin(),
            output_domain: config.domain(),
            rate: request.format.sample_rate(),
            channels: request.format.channels(),
            layout,
            buffer_frames: applied.buffer_frames,
            enabled: AtomicBool::new(false),
            rendering: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            changed: AtomicBool::new(false),
            calls: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            version: AtomicU64::new(0),
            ticks: AtomicU64::new(0),
            sample_bits: AtomicU64::new(0),
            flags: AtomicU32::new(0),
            first_frame: AtomicU64::new(0),
            block_frames: AtomicU32::new(0),
        });
        let mut stream = Self {
            basis,
            applied,
            context: Some(context),
            recovered_mixer: None,
            retired: false,
            proc_id: std::ptr::null_mut(),
            listeners: Vec::new(),
            started: false,
            owner: PhantomData,
            final_snapshot: CoreAudioSnapshot::default(),
            final_render_report: None,
            final_cadence: None,
        };
        let registration = (|| -> Result<(), CoreAudioError> {
            let pointer = stream.context.as_deref().expect("context installed") as *const Context
                as *mut c_void;
            // SAFETY: stable box contains callback-exclusive UnsafeCell storage and
            // atomics; registration lifetime is retained until successful destruction.
            check("AudioDeviceCreateIOProcID", unsafe {
                ffi::AudioDeviceCreateIOProcID(
                    request.device,
                    render_callback,
                    pointer,
                    &mut stream.proc_id,
                )
            })?;
            if stream.proc_id.is_null() {
                return Err(CoreAudioError::DeviceUnavailable);
            }
            let mut properties = vec![
                (request.device, address(RATE, GLOBAL)),
                (request.device, address(BUFFER, GLOBAL)),
                (request.device, address(STREAM_CONFIG, OUTPUT)),
                (request.device, address(STREAMS, OUTPUT)),
            ];
            properties.extend(streams.into_iter().map(|id| (id, address(FORMAT, GLOBAL))));
            for (id, address) in properties {
                // SAFETY: stable callback context, native object/address and correct
                // listener ABI; every successful registration is tracked for removal.
                check("AudioObjectAddPropertyListener", unsafe {
                    ffi::AudioObjectAddPropertyListener(
                        id,
                        &address,
                        configuration_changed,
                        pointer,
                    )
                })?;
                stream.listeners.push((id, address));
            }
            Ok(())
        })();
        if let Err(error) = registration {
            let mut failure = CoreAudioOpenFailure::pending(error, stream);
            let _ = Self::retry_open_cleanup(&mut failure);
            return Err(failure);
        }
        Ok(stream)
    }
    /// Retry failed-open retirement, extracting only after actual stop/unregister/drain.
    pub fn retry_open_cleanup(failure: &mut CoreAudioOpenFailure) -> Result<bool, &CoreAudioError> {
        failure.retry_retirement(|stream| {
            stream.stop()?;
            beatkernel::audio::StoppedMixerSource::take_stopped_mixer(stream)
        })
    }
    /// Returns original request and applied native layout/domain separately.
    pub const fn configuration(&self) -> &CoreAudioApplied {
        &self.applied
    }
    /// Starts this exact native IOProc once; closed streams require reopening.
    pub fn start(&mut self) -> Result<(), CoreAudioError> {
        let context = self.context.as_deref().ok_or(CoreAudioError::Closed)?;
        if context.changed.load(Ordering::Acquire) {
            return Err(CoreAudioError::ConfigurationChanged);
        }
        if self.started {
            return Ok(());
        }
        context.enabled.store(true, Ordering::Release);
        // SAFETY: registered exact IOProc and stable callback box remain owned.
        let result = check("AudioDeviceStart", unsafe {
            ffi::AudioDeviceStart(self.applied.request.device, self.proc_id)
        });
        if result.is_err() {
            context.enabled.store(false, Ordering::Release);
        } else {
            self.started = true;
        }
        result
    }
    /// Stops and destroys callback/listener registrations before releasing mixer,
    /// scratch, queue and assets. Failure leaves the context owned for retry.
    pub fn stop(&mut self) -> Result<(), CoreAudioError> {
        let Some(context) = self.context.as_deref() else {
            return Ok(());
        };
        context.enabled.store(false, Ordering::Release);
        if self.started {
            // SAFETY: exact live device/proc pair; callback memory remains owned.
            check("AudioDeviceStop", unsafe {
                ffi::AudioDeviceStop(self.applied.request.device, self.proc_id)
            })?;
            self.started = false;
        }
        let pointer = context as *const Context as *mut c_void;
        while let Some(&(id, address)) = self.listeners.last() {
            // SAFETY: removes only this owned listener and stable context pair.
            check("AudioObjectRemovePropertyListener", unsafe {
                ffi::AudioObjectRemovePropertyListener(id, &address, configuration_changed, pointer)
            })?;
            self.listeners.pop();
        }
        if !self.proc_id.is_null() {
            // SAFETY: disabled/stopped IOProc removed before dropping its context;
            // unregister prevents future callbacks using this client pointer.
            check("AudioDeviceDestroyIOProcID", unsafe {
                ffi::AudioDeviceDestroyIOProcID(self.applied.request.device, self.proc_id)
            })?;
            self.proc_id = std::ptr::null_mut();
        }
        // Existing callbacks may finish after stop; retain storage until every
        // in-flight IOProc/listener has exited. Waiting is control-thread only.
        while context.active.load(Ordering::Acquire) != 0 {
            std::thread::yield_now();
        }
        self.final_snapshot = self.snapshot();
        self.final_render_report = self.last_render_report();
        let context = self
            .context
            .take()
            .expect("context retained through callback drain");
        let context = *context;
        let render = context.render.into_inner();
        self.recovered_mixer = Some(render.mixer);
        self.final_cadence = Some(context.cadence);
        self.retired = true;
        Ok(())
    }
    /// Direct pre-Mixer mach cadence, available after successful unregister/drain.
    /// Summary allocation stays off callbacks; this is not presentation timing.
    pub fn render_cadence(
        &self,
    ) -> Result<
        Option<crate::audio::cadence::RenderCadence>,
        crate::audio::cadence::RenderCadenceError,
    > {
        match self.final_cadence.as_deref() {
            Some(capture) => capture.summary(self.applied.format.sample_rate()).map(Some),
            None => Ok(None),
        }
    }
    /// Last successful core render, retained after successful stop.
    /// This is execution history, not native delivery or current clock timing.
    /// Reads are bounded and perform no allocation, native calls or locking.
    pub fn last_render_report(&self) -> Option<RenderReport> {
        match self.context.as_deref() {
            Some(context) => context.render_telemetry.read().render,
            None => self.final_render_report,
        }
    }
    /// Reads bounded atomic numeric/presentation telemetry outside the callback.
    pub fn snapshot(&self) -> CoreAudioSnapshot {
        let Some(context) = self.context.as_deref() else {
            return self.final_snapshot;
        };
        let mut result = CoreAudioSnapshot {
            rendered_callbacks: context.calls.load(Ordering::Relaxed),
            rendered_frames: context.frames.load(Ordering::Relaxed),
            callback_failures: context.failures.load(Ordering::Relaxed),
            configuration_changed: context.changed.load(Ordering::Acquire),
            presentation: None,
        };
        for _ in 0..3 {
            let before = context.version.load(Ordering::SeqCst);
            if before == 0 || before & 1 != 0 {
                continue;
            }
            let host_ticks = context.ticks.load(Ordering::SeqCst);
            let flags = context.flags.load(Ordering::SeqCst);
            let first_frame = context.first_frame.load(Ordering::SeqCst);
            let sample = f64::from_bits(context.sample_bits.load(Ordering::SeqCst));
            let frames = context.block_frames.load(Ordering::SeqCst);
            if context.version.load(Ordering::SeqCst) != before {
                continue;
            }
            let time = i128::from(context.origin.as_nanos())
                + i128::from(first_frame) * 1_000_000_000 / i128::from(context.rate);
            result.presentation = Some(CoreAudioPresentation {
                host_ticks,
                native: (flags & 2 != 0)
                    .then(|| context.clock.native_point(host_ticks))
                    .flatten(),
                native_sample_frame: (flags & 1 != 0).then_some(sample),
                output_grid: i64::try_from(time).ok().map(|time| ClockPoint {
                    domain: context.output_domain,
                    timestamp: Timestamp::from_nanos(time),
                }),
                flags,
                first_frame,
                frames,
            });
            break;
        }
        result
    }
}
impl Drop for CoreAudioStream {
    fn drop(&mut self) {
        if self.stop().is_err() {
            // Native registration may still call us: intentionally retain the
            // context/assets rather than freeing memory behind a native pointer.
            if let Some(context) = self.context.take() {
                let _ = Box::into_raw(context);
            }
        }
    }
}
struct Active<'a>(&'a Context);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Release);
    }
}
unsafe extern "C" fn configuration_changed(
    _: u32,
    _: u32,
    _: *const ffi::PropertyAddress,
    pointer: *mut c_void,
) -> i32 {
    // SAFETY: listener registration retains this stable Context until removal and
    // in-flight callback quiescence; only atomic fields are touched here.
    let context = unsafe { &*pointer.cast::<Context>() };
    context.active.fetch_add(1, Ordering::Acquire);
    let _active = Active(context);
    context.changed.store(true, Ordering::Release);
    0
}
unsafe extern "C" fn render_callback(
    _: u32,
    _: *const ffi::AudioTimestamp,
    _: *const ffi::AudioBufferList,
    _: *const ffi::AudioTimestamp,
    output: *mut ffi::AudioBufferList,
    time: *const ffi::AudioTimestamp,
    pointer: *mut c_void,
) -> i32 {
    // SAFETY: owned stable Context; native destruction precedes allocation drop.
    let context = unsafe { &*pointer.cast::<Context>() };
    context.active.fetch_add(1, Ordering::Acquire);
    let _active = Active(context);
    if !context.enabled.load(Ordering::Acquire) {
        return 0;
    }
    if context.changed.load(Ordering::Acquire)
        || context
            .rendering
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
    {
        context.failures.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    // SAFETY: rendering flag provides exclusive mutable access to UnsafeCell;
    // control thread never accesses render state while callback is registered.
    let state = unsafe { &mut *context.render.get() };
    // SAFETY: CoreAudio supplies writable initialized output buffer descriptors;
    // helper validates native counts/layout/size/alignment before PCM access.
    let result = unsafe { render_buffers(context, state, output, time) };
    if result.is_err() {
        context.failures.fetch_add(1, Ordering::Relaxed);
    }
    context.rendering.store(false, Ordering::Release);
    0
}
unsafe fn render_buffers(
    context: &Context,
    state: &mut RenderState,
    output: *mut ffi::AudioBufferList,
    time: *const ffi::AudioTimestamp,
) -> Result<(), ()> {
    if output.is_null() {
        return Err(());
    }
    // SAFETY: native nonnull AudioBufferList includes count descriptors; count is
    // bounded and must match the prevalidated native stream layout.
    let count = unsafe { (*output).count as usize };
    if count != context.layout.len() || count == 0 || count > MAX_BUFFERS {
        return Err(());
    }
    // SAFETY: CoreAudio flexible-array allocation holds count AudioBuffers. Only
    // shared descriptor reads occur; no simultaneous mutable descriptor alias.
    let buffers = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*output).buffers).cast::<ffi::AudioBuffer>(),
            count,
        )
    };
    let mut frames = None;
    for (index, buffer) in buffers.iter().enumerate() {
        if buffer.channels != context.layout[index]
            || buffer.channels == 0
            || buffer.data.is_null()
            || (buffer.data as usize) % std::mem::align_of::<f32>() != 0
        {
            return Err(());
        }
        let bytes_per_frame = buffer.channels.checked_mul(4).ok_or(())?;
        if buffer.byte_size % bytes_per_frame != 0 {
            return Err(());
        }
        let count = buffer.byte_size / bytes_per_frame;
        if count == 0
            || count > context.buffer_frames
            || frames.is_some_and(|frames| frames != count)
        {
            return Err(());
        }
        frames = Some(count);
        let start = buffer.data as usize;
        let end = start.checked_add(buffer.byte_size as usize).ok_or(())?;
        for earlier in &buffers[..index] {
            let previous_start = earlier.data as usize;
            let previous_end = previous_start
                .checked_add(earlier.byte_size as usize)
                .ok_or(())?;
            if start < previous_end && previous_start < end {
                return Err(());
            }
        }
    }
    let frames = frames.ok_or(())?;
    let channels = usize::from(context.channels);
    let samples = (frames as usize).checked_mul(channels).ok_or(())?;
    if samples > state.scratch.len() {
        return Err(());
    }
    let render_start = context
        .clock
        .sample_realtime()
        .map(|sample| sample.normalized.timestamp);
    let report = crate::audio::channel_remix::render(
        &mut state.mixer,
        &mut state.remix,
        &mut state.scratch[..samples],
    )
    .map_err(|_| ())?;
    match render_start {
        Some(at) => context
            .cadence
            .record(at, report.start_frame, report.frames as u64),
        None => context.cadence.mark_unavailable(),
    }
    // Publish actual completed core rendering before native buffer delivery.
    // Callback guard grants exclusive access to the local version counter.
    context.render_telemetry.publish(
        AudioStreamSnapshot {
            telemetry_available: true,
            status: AudioStreamStatus::Running,
            counters: StreamCounters::default(),
            clock: None,
            render: Some(report),
        },
        &mut state.render_version,
    );
    let mut first_channel = 0usize;
    for buffer in buffers {
        let buffer_channels = buffer.channels as usize;
        // SAFETY: native buffer is writable and float32 aligned with validated
        // count; disjoint extents checked above and no slice escapes callback.
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                buffer.data.cast::<f32>(),
                frames as usize * buffer_channels,
            )
        };
        crate::audio::channel_remix::copy_channel_group(
            &state.scratch[..samples],
            channels,
            first_channel,
            buffer_channels,
            output,
        )
        .map_err(|_| ())?;
        first_channel += buffer_channels;
    }
    context.calls.fetch_add(1, Ordering::Relaxed);
    context
        .frames
        .fetch_add(u64::from(frames), Ordering::Relaxed);
    // SAFETY: nullable native AudioTimeStamp is valid for this IOProc duration.
    let time = if time.is_null() {
        ffi::AudioTimestamp::default()
    } else {
        unsafe { *time }
    };
    context.version.fetch_add(1, Ordering::SeqCst);
    context.ticks.store(time.host_time, Ordering::SeqCst);
    context
        .sample_bits
        .store(time.sample_time.to_bits(), Ordering::SeqCst);
    context.flags.store(time.flags, Ordering::SeqCst);
    context
        .first_frame
        .store(report.start_frame, Ordering::SeqCst);
    context.block_frames.store(frames, Ordering::SeqCst);
    context.version.fetch_add(1, Ordering::SeqCst);
    Ok(())
}
fn check(operation: &'static str, code: i32) -> Result<(), CoreAudioError> {
    if code == 0 {
        Ok(())
    } else {
        Err(CoreAudioError::Native { operation, code })
    }
}
fn address(selector: u32, scope: u32) -> ffi::PropertyAddress {
    ffi::PropertyAddress {
        selector,
        scope,
        element: 0,
    }
}
fn scalar<T: Default>(object: u32, address: ffi::PropertyAddress) -> Result<T, CoreAudioError> {
    let mut value = T::default();
    let expected = u32::try_from(std::mem::size_of::<T>()).map_err(|_| CoreAudioError::Capacity)?;
    let mut size = expected;
    // SAFETY: private callers choose exact ABI property types (u32/f64/Asbd);
    // output has exact aligned expected storage and native API respects size.
    check("AudioObjectGetPropertyData", unsafe {
        ffi::AudioObjectGetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut T).cast(),
        )
    })?;
    if size != expected {
        return Err(CoreAudioError::UnsupportedLayout);
    }
    Ok(value)
}
fn set_scalar<T>(
    object: u32,
    address: ffi::PropertyAddress,
    value: T,
) -> Result<(), CoreAudioError> {
    // SAFETY: private callers pass exact u32/f64 property ABI types, retained for
    // this synchronous API call only; native code does not retain input storage.
    check("AudioObjectSetPropertyData", unsafe {
        ffi::AudioObjectSetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            std::mem::size_of::<T>() as u32,
            (&value as *const T).cast(),
        )
    })
}
fn property_storage(
    object: u32,
    address: ffi::PropertyAddress,
) -> Result<(Vec<u64>, usize), CoreAudioError> {
    let mut size = 0u32;
    // SAFETY: valid address and writable output size; no qualifier.
    check("AudioObjectGetPropertyDataSize", unsafe {
        ffi::AudioObjectGetPropertyDataSize(object, &address, 0, std::ptr::null(), &mut size)
    })?;
    // Bound off-thread native metadata allocation to 1MiB; aligned u64 storage
    // accommodates the flexible AudioBufferList and scalar arrays on macOS.
    if size > 1_048_576 {
        return Err(CoreAudioError::Capacity);
    }
    let mut storage = vec![0u64; (size as usize).div_ceil(8)];
    let capacity = size;
    // SAFETY: storage is aligned, writable and holds at least capacity bytes;
    // API updates size without retaining our pointer. Dynamic growth errors out.
    check("AudioObjectGetPropertyData", unsafe {
        ffi::AudioObjectGetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            storage.as_mut_ptr().cast(),
        )
    })?;
    if size > capacity {
        return Err(CoreAudioError::UnsupportedLayout);
    }
    Ok((storage, size as usize))
}
fn u32_property(object: u32, address: ffi::PropertyAddress) -> Result<Vec<u32>, CoreAudioError> {
    let (storage, size) = property_storage(object, address)?;
    if size % 4 != 0 {
        return Err(CoreAudioError::UnsupportedLayout);
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: aligned storage contains size initialized bytes, divisible by u32.
    Ok(unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u32>(), size / 4) }.to_vec())
}
fn native_layout(device: u32) -> Result<Vec<u32>, CoreAudioError> {
    let (storage, size) = property_storage(device, address(STREAM_CONFIG, OUTPUT))?;
    let offset = std::mem::offset_of!(ffi::AudioBufferList, buffers);
    if size < offset {
        return Err(CoreAudioError::UnsupportedLayout);
    }
    let pointer = storage.as_ptr().cast::<ffi::AudioBufferList>();
    // SAFETY: aligned storage holds the count field validated by size above.
    let count = unsafe { (*pointer).count as usize };
    if count > MAX_BUFFERS
        || offset
            .checked_add(count * std::mem::size_of::<ffi::AudioBuffer>())
            .is_none_or(|needed| needed > size)
    {
        return Err(CoreAudioError::UnsupportedLayout);
    }
    // SAFETY: exact flexible-array extent was validated against returned size.
    let buffers = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*pointer).buffers).cast::<ffi::AudioBuffer>(),
            count,
        )
    };
    let channels: Vec<_> = buffers.iter().map(|buffer| buffer.channels).collect();
    if channels
        .iter()
        .any(|channels| *channels == 0 || *channels > 32)
        || channels.iter().sum::<u32>() > 32
    {
        return Err(CoreAudioError::UnsupportedLayout);
    }
    Ok(channels)
}
fn validate_format(format: ffi::Asbd, sample_rate: f64) -> Result<(), CoreAudioError> {
    let noninterleaved = format.format_flags & PLANAR != 0;
    let bytes = if noninterleaved {
        4
    } else {
        format
            .channels_per_frame
            .checked_mul(4)
            .ok_or(CoreAudioError::UnsupportedFormat)?
    };
    if format.format_id != u32::from_be_bytes(*b"lpcm")
        || format.format_flags & (FLOAT | PACKED) != FLOAT | PACKED
        || format.format_flags & !(FLOAT | PACKED | PLANAR) != 0
        || format.sample_rate != sample_rate
        || format.channels_per_frame == 0
        || format.channels_per_frame > 32
        || format.frames_per_packet != 1
        || format.bits_per_channel != 32
        || format.bytes_per_frame != bytes
        || format.bytes_per_packet != bytes
    {
        return Err(CoreAudioError::UnsupportedFormat);
    }
    Ok(())
}
fn string_property(device: u32, selector: u32) -> Option<String> {
    let mut value: ffi::Ref = std::ptr::null();
    let mut size = std::mem::size_of::<ffi::Ref>() as u32;
    // SAFETY: pointer-sized CFStringRef output; these name/UID properties return
    // a retained CF object which the caller releases after copying its text.
    if unsafe {
        ffi::AudioObjectGetPropertyData(
            device,
            &address(selector, GLOBAL),
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut ffi::Ref).cast(),
        )
    } != 0
        || value.is_null()
    {
        return None;
    }
    let owned = ffi::OwnedRef(value);
    ffi::cf_string(owned.0)
}

impl beatkernel::audio::StoppedMixerSource for CoreAudioStream {
    type Error = CoreAudioError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        if !self.retired || self.context.is_some() {
            return Err(CoreAudioError::RecoveryUnavailable);
        }
        Ok(self.recovered_mixer.take())
    }
}

#[cfg(test)]
#[path = "audio/open_failure_fixtures.rs"]
mod open_failure_fixtures;
