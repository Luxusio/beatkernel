//! Owning-thread ASIO output with preallocated Mixer rendering and raw diagnostics.
#![allow(unsafe_code)]

use super::control::{check, AsioControl, AsioControlError, AsioLatencies, Status};
use crate::audio::{
    asio::{AsioBlockRenderer, AsioBufferRequest, AsioRenderError},
    telemetry::Telemetry,
    AudioStreamSnapshot, AudioStreamStatus, StreamCounters,
};
use crate::windows::clock::QpcClock;
use beatkernel::audio::{Mixer, RenderReport};
use std::{
    cell::UnsafeCell,
    error::Error,
    ffi::c_void,
    fmt, ptr,
    sync::atomic::{AtomicU64, Ordering},
};

/// Notifications and failures recorded by the native callbacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AsioFaults(pub u32);
impl AsioFaults {
    /// Driver requested reset.
    pub const RESET: Self = Self(1);
    /// Driver requested resynchronization.
    pub const RESYNC: Self = Self(2);
    /// Driver latencies changed.
    pub const LATENCIES_CHANGED: Self = Self(4);
    /// Actual sample rate changed or became invalid.
    pub const RATE_CHANGED: Self = Self(8);
    /// Driver requested unsupported hot buffer resizing.
    pub const BUFFER_SIZE_CHANGED: Self = Self(16);
    /// Driver reported overload; diagnostic only.
    pub const OVERLOAD: Self = Self(32);
    /// Concurrent or reentrant buffer callback rejected.
    pub const REENTRANT_CALLBACK: Self = Self(64);
    /// Driver reported a buffer index other than zero or one.
    pub const INVALID_BUFFER_INDEX: Self = Self(128);
    /// Rust Mixer delivery failed; see the native render error code.
    pub const RENDER_FAILED: Self = Self(256);
    /// Raw clock publication exhausted its generation counter.
    pub const CLOCK_EXHAUSTED: Self = Self(512);
    /// Time-info data was missing or malformed.
    pub const MALFORMED_TIME: Self = Self(1024);
    /// Whether callback processing requires explicit close and reconstruction.
    pub const fn requires_reopen(self) -> bool {
        self.0 & !(Self::OVERLOAD.0 | Self::CLOCK_EXHAUSTED.0) != 0
    }
}

/// Copied native ASIO callback data; system time is not QPC or normalized host time.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AsioCallbackEvent {
    /// SDK buffer index, zero or one for accepted callbacks.
    pub buffer_index: i32,
    /// Unmodified SDK directProcess suggestion; processing is bounded and direct.
    pub direct_process: i32,
    /// SDK time-info validity/change flags; legacy success supplies bits 1 and 2.
    pub flags: u32,
    /// Raw sample position, meaningful only when flags & 2 != 0.
    pub sample_position: u64,
    /// Raw native system nanoseconds, meaningful only when flags & 1 != 0.
    pub system_nanoseconds: u64,
    /// Raw native sample rate, meaningful only when flags & 4 != 0.
    pub sample_rate: f64,
}

/// Raw native diagnostics, kept separate from software buffer preparation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AsioDiagnostics {
    /// Native notification/failure bits, retained until close.
    pub faults: AsioFaults,
    /// First Rust callback failure: 1 bad index, 2 renderer failure, 3 counter exhaustion.
    pub render_error: i32,
    /// Coherent raw event with both native sample position and system-time validity.
    pub event: Option<AsioCallbackEvent>,
}

/// Explicit stream lifecycle; stop is terminal and start is permitted once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioStreamPhase {
    /// Buffer B has been primed, native start has not been called.
    Ready,
    /// Native start succeeded and callbacks may render.
    Running,
    /// Native driver and buffers have been released.
    Stopped,
    /// Rendering/native processing failed; explicit reconstruction is required.
    Failed,
}

/// One successful callback buffer write, pairing its Mixer block and native event.
/// Native flags retain their validity meaning; this is not a normalized host clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AsioBufferObservation {
    /// Copied raw event for exactly this rendering invocation.
    pub event: AsioCallbackEvent,
    /// Actual successful Mixer render used for this native buffer write.
    pub render: RenderReport,
}

/// Off-thread software progress plus raw native observations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AsioStreamSnapshot {
    /// Lifecycle observed independently of software telemetry publication.
    pub phase: AsioStreamPhase,
    /// Whether prepared counters and Mixer report form one coherent publication.
    pub telemetry_available: bool,
    /// Successful buffer writes, including pre-start B; not audible frame progress.
    pub prepared_frames: u64,
    /// Successful complete planar block writes, including pre-start B.
    pub buffer_fills: u64,
    /// Last successful core render, even when subsequent native conversion failed.
    pub render: Option<RenderReport>,
    /// Successful callback write coherent with software counters/report.
    /// Absent during B priming, failed delivery or unavailable publication.
    pub buffer_observation: Option<AsioBufferObservation>,
    /// Native observations, cached immediately before close once stopped.
    pub native: AsioDiagnostics,
}

/// Setup, lifecycle or native fault, without implicit rate/device replacement.
#[derive(Debug)]
pub enum AsioStreamError {
    /// Native control/SDK operation or portable buffer negotiation failed.
    Control(AsioControlError),
    /// Fixed Mixer/PCM rendering failed.
    Render(AsioRenderError),
    /// Channel count, duplicate index or signed SDK index was invalid.
    InvalidChannels,
    /// Driver rate differs from the actual Mixer rate.
    RateMismatch {
        /// Actual configured Mixer rate.
        expected: u32,
        /// Actual native reported rate.
        actual: f64,
    },
    /// Operation is invalid for the current terminal/one-start lifecycle.
    InvalidState,
    /// Native callbacks faulted; close and reconstruct explicitly.
    ReopenRequired(AsioDiagnostics),
}
impl fmt::Display for AsioStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(error) => write!(f, "ASIO stream: {error}"),
            Self::Render(error) => write!(f, "ASIO stream: {error}"),
            Self::InvalidChannels => f.write_str("invalid ASIO output channel selection"),
            Self::RateMismatch { expected, actual } => {
                write!(f, "ASIO rate {actual} differs from Mixer rate {expected}")
            }
            Self::InvalidState => f.write_str("invalid ASIO stream lifecycle operation"),
            Self::ReopenRequired(diagnostics) => write!(
                f,
                "ASIO stream requires reopen: faults={}, render={}",
                diagnostics.faults.0, diagnostics.render_error
            ),
        }
    }
}
impl Error for AsioStreamError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Control(error) => Some(error),
            Self::Render(error) => Some(error),
            _ => None,
        }
    }
}
impl From<AsioControlError> for AsioStreamError {
    fn from(error: AsioControlError) -> Self {
        Self::Control(error)
    }
}
impl From<AsioRenderError> for AsioStreamError {
    fn from(error: AsioRenderError) -> Self {
        Self::Render(error)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawOutput {
    channel: i32,
    sample_type: i32,
    width: u32,
    buffer0: *mut c_void,
    buffer1: *mut c_void,
}
impl Default for RawOutput {
    fn default() -> Self {
        Self {
            channel: 0,
            sample_type: 0,
            width: 0,
            buffer0: ptr::null_mut(),
            buffer1: ptr::null_mut(),
        }
    }
}
#[repr(C)]
#[derive(Default)]
struct RawDiagnostics {
    flags: u32,
    render_error: i32,
    clock_available: i32,
    reserved: i32,
    event: AsioCallbackEvent,
}
unsafe extern "C" {
    fn bk_asio_prepare(
        handle: *mut c_void,
        outputs: *mut RawOutput,
        count: i32,
        frames: i32,
        rate: f64,
        callback: unsafe extern "C" fn(*mut c_void, i32, *const AsioCallbackEvent) -> i32,
        context: *mut c_void,
    ) -> Status;
    fn bk_asio_start(handle: *mut c_void) -> Status;
    fn bk_asio_diagnostics(handle: *mut c_void, output: *mut RawDiagnostics) -> Status;
}

struct RenderState {
    renderer: AsioBlockRenderer,
    rows: [RawOutput; 32],
    prepared_frames: u64,
    buffer_fills: u64,
    version: u64,
}
struct RenderContext {
    state: UnsafeCell<RenderState>,
    host_clock: Option<QpcClock>,
    cadence: crate::audio::cadence::Capture,
    telemetry: Telemetry,
    publication: AtomicU64,
    event_values: [AtomicU64; 6],
}
// SAFETY: the native bridge's acquire/release nonblocking renderer guard permits
// at most one callback to access state. Owner accesses it only before native start
// or after native close has detached and drained callbacks. Mixer is Send (asserted
// during construction). Shared telemetry contains only atomics. No context pointer
// is retained beyond driver Release and callback-reader drain.
unsafe impl Sync for RenderContext {}

impl RenderContext {
    // SAFETY required by callers: exclusive pre-start owner or admitted serialized
    // native callback; selected native regions live and are disjoint after prepare.
    unsafe fn fill(
        &self,
        index: i32,
        event: Option<AsioCallbackEvent>,
    ) -> Result<(), AsioRenderError> {
        if !matches!(index, 0 | 1) {
            return Err(AsioRenderError::InvalidBuffers);
        }
        // SAFETY: caller guarantees the single renderer/owner access described above.
        let state = unsafe { &mut *self.state.get() };
        let next_frames = state
            .prepared_frames
            .checked_add(u64::from(state.renderer.frames()))
            .ok_or(AsioRenderError::Capacity)?;
        let next_fills = state
            .buffer_fills
            .checked_add(1)
            .ok_or(AsioRenderError::Capacity)?;
        let next_version = state
            .version
            .checked_add(2)
            .ok_or(AsioRenderError::Capacity)?;
        let count = state.renderer.encodings().len();
        let mut outputs: [&mut [u8]; 32] = std::array::from_fn(|_| &mut [] as &mut [u8]);
        for (output, row) in outputs[..count].iter_mut().zip(&state.rows) {
            let bytes = state.renderer.frames() as usize * row.width as usize;
            let buffer = if index == 0 { row.buffer0 } else { row.buffer1 };
            // SAFETY: prepare validated nonnull, <=isize::MAX representable regions
            // and disjointness of all channels/halves. The trusted driver retains
            // their storage until close; renderer admission prevents concurrent writes.
            *output = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), bytes) };
        }
        self.publication.store(next_version - 1, Ordering::SeqCst);
        // Only actual native callbacks are timed; owner-side B priming has no
        // callback event. Raw ASIO system time is never a QPC substitute.
        let render_start = if event.is_some() {
            self.host_clock
                .and_then(|clock| clock.sample_realtime())
                .map(|receipt| receipt.normalized.timestamp)
        } else {
            None
        };
        let result = state.renderer.render(&mut outputs[..count]);
        if let (Some(_), Some(_), Ok(report)) = (self.host_clock, event, &result) {
            match render_start {
                Some(at) => self
                    .cadence
                    .record(at, report.start_frame, report.frames as u64),
                None => self.cadence.mark_unavailable(),
            }
        }
        if result.is_ok() {
            state.prepared_frames = next_frames;
            state.buffer_fills = next_fills;
        }
        let status = if result.is_err() {
            AudioStreamStatus::Failed { hresult: 2 }
        } else if self
            .telemetry
            .status
            .load(std::sync::atomic::Ordering::SeqCst)
            == 1
        {
            AudioStreamStatus::Running
        } else {
            AudioStreamStatus::Ready
        };
        self.telemetry.publish(
            AudioStreamSnapshot {
                telemetry_available: true,
                status,
                // Private transport of software prepared counts. Public ASIO snapshots
                // rename these and never expose them as native submission/audible position.
                counters: StreamCounters {
                    submitted_frames: state.prepared_frames,
                    buffer_fills: state.buffer_fills,
                    ..Default::default()
                },
                clock: None,
                render: state.renderer.last_render_report(),
            },
            &mut state.version,
        );
        let event = event.filter(|_| result.is_ok());
        let values = event.map_or([0; 6], |event| {
            [
                1,
                u64::from(event.buffer_index as u32)
                    | (u64::from(event.direct_process as u32) << 32),
                u64::from(event.flags),
                event.sample_position,
                event.system_nanoseconds,
                event.sample_rate.to_bits(),
            ]
        });
        for (destination, value) in self.event_values.iter().zip(values) {
            destination.store(value, Ordering::SeqCst);
        }
        self.publication.store(next_version, Ordering::SeqCst);
        result.map(|_| ())
    }
    fn read(&self) -> (AudioStreamSnapshot, Option<AsioBufferObservation>) {
        for _ in 0..3 {
            let before = self.publication.load(Ordering::SeqCst);
            if before == 0 || before & 1 != 0 {
                continue;
            }
            let software = self.telemetry.read();
            let values: [u64; 6] =
                std::array::from_fn(|index| self.event_values[index].load(Ordering::SeqCst));
            if self.publication.load(Ordering::SeqCst) == before && software.telemetry_available {
                let observation = if values[0] == 1 {
                    software.render.map(|render| AsioBufferObservation {
                        render,
                        event: AsioCallbackEvent {
                            buffer_index: values[1] as u32 as i32,
                            direct_process: (values[1] >> 32) as u32 as i32,
                            flags: values[2] as u32,
                            sample_position: values[3],
                            system_nanoseconds: values[4],
                            sample_rate: f64::from_bits(values[5]),
                        },
                    })
                } else {
                    None
                };
                return (software, observation);
            }
        }
        (
            AudioStreamSnapshot {
                telemetry_available: false,
                status: self.telemetry.read().status,
                counters: StreamCounters::default(),
                clock: None,
                render: None,
            },
            None,
        )
    }
}
unsafe extern "C" fn render(
    context: *mut c_void,
    index: i32,
    event: *const AsioCallbackEvent,
) -> i32 {
    if !matches!(index, 0 | 1) {
        return 1;
    }
    // SAFETY: stable boxed context passed to prepare outlives native admission,
    // serialized by the C++ renderer guard. Close drains before freeing the box.
    // SAFETY: C++ passes an initialized event with callback duration lifetime.
    // Copy immediately; no borrowed native pointer enters retained state.
    let event = unsafe { event.as_ref() }.copied();
    let result = unsafe { (&*context.cast::<RenderContext>()).fill(index, event) };
    match result {
        Ok(()) => 0,
        Err(AsioRenderError::Capacity) => 3,
        Err(_) => 2,
    }
}

/// Owned ASIO double buffers and Mixer. Deliberately neither Send nor Sync.
///
/// All public native operations occur on the control's opening thread. Native
/// callbacks use only preallocated state. Drop closes/drains before freeing it.
pub struct AsioStream {
    control: Option<AsioControl>,
    retired: bool,
    context: Box<RenderContext>,
    phase: AsioStreamPhase,
    native: AsioDiagnostics,
    latencies: AsioLatencies,
    sample_rate: u32,
}
impl AsioStream {
    /// Consumes exact driver/Mixer/channel choices, prepares buffers and primes B.
    /// Does not change rate, select another device, start, or show driver UI.
    pub fn prepare(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
    ) -> Result<Self, AsioStreamError> {
        Self::prepare_internal(control, mixer, channels, request, None)
    }

    /// Prepares exact buffers with opt-in direct callback render-entry cadence.
    /// The supplied shared QPC clock retains its application-selected origin;
    /// priming is excluded and cadence is available only after terminal close.
    pub fn prepare_with_clock(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: QpcClock,
    ) -> Result<Self, AsioStreamError> {
        Self::prepare_internal(control, mixer, channels, request, Some(host_clock))
    }

    fn prepare_internal(
        mut control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: Option<QpcClock>,
    ) -> Result<Self, AsioStreamError> {
        fn require_send<T: Send>() {}
        require_send::<AsioBlockRenderer>();
        let format = mixer.configuration().format();
        if channels.len() != usize::from(format.channels())
            || channels.is_empty()
            || channels.len() > 32
            || channels
                .iter()
                .enumerate()
                .any(|(i, channel)| *channel > i32::MAX as u32 || channels[..i].contains(channel))
        {
            return Err(AsioStreamError::InvalidChannels);
        }
        let rate = control.sample_rate()?;
        if rate != f64::from(format.sample_rate()) {
            return Err(AsioStreamError::RateMismatch {
                expected: format.sample_rate(),
                actual: rate,
            });
        }
        let frames = control
            .buffer_constraints()?
            .resolve(request)
            .map_err(AsioControlError::from)?;
        let mut encodings = Vec::new();
        encodings
            .try_reserve_exact(channels.len())
            .map_err(|_| AsioRenderError::Capacity)?;
        let mut rows = [RawOutput::default(); 32];
        for (row, channel) in rows.iter_mut().zip(&channels) {
            let info = control.channel_info(*channel, false)?;
            let encoding = info.pcm_encoding().map_err(AsioRenderError::Pcm)?;
            *row = RawOutput {
                channel: *channel as i32,
                sample_type: info.sample_type,
                width: encoding.bytes_per_sample() as u32,
                ..Default::default()
            };
            encodings.push(encoding);
        }
        let renderer = AsioBlockRenderer::new(mixer, frames, encodings)?;
        let mut stream = Self {
            control: Some(control),
            retired: false,
            context: Box::new(RenderContext {
                host_clock,
                cadence: crate::audio::cadence::Capture::new(),
                state: UnsafeCell::new(RenderState {
                    renderer,
                    rows,
                    prepared_frames: 0,
                    buffer_fills: 0,
                    version: 0,
                }),
                telemetry: Telemetry::new(),
                publication: AtomicU64::new(0),
                event_values: std::array::from_fn(|_| AtomicU64::new(0)),
            }),
            phase: AsioStreamPhase::Ready,
            sample_rate: format.sample_rate(),
            native: AsioDiagnostics::default(),
            latencies: AsioLatencies {
                input_frames: 0,
                output_frames: 0,
            },
        };
        let context = ptr::from_ref(stream.context.as_ref())
            .cast_mut()
            .cast::<c_void>();
        let state = stream.context.state.get_mut();
        // SAFETY: same-thread live control, stable Box and fixed rows, verified
        // positive signed SDK frames/count and actual format. prepare doesn't render;
        // any error drops stream, closing/draining before freeing callback context.
        check(
            unsafe {
                bk_asio_prepare(
                    stream.control.as_ref().unwrap().raw(),
                    state.rows.as_mut_ptr(),
                    channels.len() as i32,
                    frames as i32,
                    rate,
                    render,
                    context,
                )
            },
            "createBuffers",
        )?;
        stream.latencies = stream.control.as_mut().unwrap().latencies()?;
        // SAFETY: native prepare validated all regions; start has not enabled render.
        unsafe { stream.context.fill(1, None) }?;
        Ok(stream)
    }
    /// Driver-reported latency after buffer creation; changes require reopening.
    /// Values are sample frames, not a host-clock relation or acoustic guarantee.
    pub const fn latencies(&self) -> AsioLatencies {
        self.latencies
    }

    /// Relates one actual rendered block to a bounded host presentation interval.
    ///
    /// The caller establishes that this driver's system timestamp uses the
    /// wrapped multimedia timer and supplies honest timer/latency error bounds.
    /// This runs off RT, reads one coherent snapshot, then samples the same QPC
    /// clock used for input. Missing/stale observations never use receipt time
    /// as a substitute for the native switch timestamp. The result uses the
    /// Mixer block's frame identity, not the driver's unrelated sample counter.
    pub fn presentation_observation(
        &mut self,
        anchor: &crate::audio::asio::MultimediaClockAnchor,
        host_clock: &crate::windows::clock::QpcClock,
        latency_error_ns: u64,
        output_origin: beatkernel::time::ClockPoint,
    ) -> Result<
        crate::audio::asio::AsioPresentationObservation,
        crate::audio::asio::AsioPresentationError,
    > {
        use crate::audio::asio::{AsioPresentationError, AsioPresentationObservation};
        let snapshot = self
            .snapshot()
            .map_err(|_| AsioPresentationError::Unavailable)?;
        if snapshot.phase != AsioStreamPhase::Running
            || !snapshot.telemetry_available
            || snapshot.native.faults.0 != 0
            || snapshot.native.render_error != 0
        {
            return Err(AsioPresentationError::Unavailable);
        }
        let observation = snapshot
            .buffer_observation
            .ok_or(AsioPresentationError::Unavailable)?;
        let event = observation.event;
        if event.flags & 3 != 3 || !(0..=1).contains(&event.buffer_index) {
            return Err(AsioPresentationError::Unavailable);
        }
        if event.flags & (16 | 32) != 0
            || (event.flags & 4 != 0 && event.sample_rate != f64::from(self.sample_rate))
        {
            return Err(AsioPresentationError::RateChanged);
        }
        let receipt = host_clock
            .sample()
            .map_err(|_| AsioPresentationError::Unavailable)?;
        let host = anchor.map_wrapped_ns(event.system_nanoseconds, receipt.normalized)?;
        AsioPresentationObservation::from_render(
            observation.render,
            self.sample_rate,
            host,
            self.latencies.output_frames,
            latency_error_ns,
            output_origin,
        )
    }
    /// Starts once. Failure releases the driver and drains callbacks immediately.
    pub fn start(&mut self) -> Result<(), AsioStreamError> {
        if self.phase != AsioStreamPhase::Ready {
            return Err(AsioStreamError::InvalidState);
        }
        let native = match self.diagnostics() {
            Ok(native) => native,
            Err(error) => {
                let _ = self.stop();
                self.phase = AsioStreamPhase::Failed;
                return Err(error.into());
            }
        };
        if native.faults.requires_reopen() {
            let _ = self.stop();
            self.phase = AsioStreamPhase::Failed;
            return Err(AsioStreamError::ReopenRequired(native));
        }
        self.context
            .telemetry
            .status
            .store(1, std::sync::atomic::Ordering::SeqCst);
        // SAFETY: prepared live same-thread control and stable callback context;
        // no mutable access to RenderState is held while callbacks may render.
        let result = check(
            unsafe { bk_asio_start(self.control.as_ref().unwrap().raw()) },
            "start",
        );
        if let Err(error) = result {
            let _ = self.stop();
            self.phase = AsioStreamPhase::Failed;
            return Err(error.into());
        }
        self.phase = AsioStreamPhase::Running;
        let native = match self.diagnostics() {
            Ok(native) => native,
            Err(error) => {
                let _ = self.stop();
                self.phase = AsioStreamPhase::Failed;
                return Err(error.into());
            }
        };
        if native.faults.requires_reopen() {
            let _ = self.stop();
            self.phase = AsioStreamPhase::Failed;
            return Err(AsioStreamError::ReopenRequired(native));
        }
        Ok(())
    }
    fn diagnostics(&mut self) -> Result<AsioDiagnostics, AsioControlError> {
        if let Some(control) = &self.control {
            let mut raw = RawDiagnostics::default();
            // SAFETY: fixed C ABI output lives for this same-thread synchronous read;
            // native diagnostic publication uses atomics and bounded coherent reads.
            check(
                unsafe { bk_asio_diagnostics(control.raw(), &mut raw) },
                "diagnostics",
            )?;
            self.native = AsioDiagnostics {
                faults: AsioFaults(raw.flags),
                render_error: raw.render_error,
                event: (raw.clock_available == 1).then_some(raw.event),
            };
        }
        Ok(self.native)
    }
    /// Reads bounded coherent software progress and raw native diagnostics off-thread
    /// from rendering, on the control owner thread. Native faults set Failed phase.
    pub fn snapshot(&mut self) -> Result<AsioStreamSnapshot, AsioStreamError> {
        let native = self.diagnostics()?;
        let (software, buffer_observation) = self.context.read();
        let phase = if native.faults.requires_reopen()
            || matches!(software.status, AudioStreamStatus::Failed { .. })
        {
            AsioStreamPhase::Failed
        } else {
            self.phase
        };
        Ok(AsioStreamSnapshot {
            phase,
            telemetry_available: software.telemetry_available,
            prepared_frames: software.counters.submitted_frames,
            buffer_fills: software.counters.buffer_fills,
            render: software.render,
            buffer_observation,
            native,
        })
    }
    /// Actual successful callback render-entry cadence after close has detached
    /// and drained callbacks, including a retained prefix following failure.
    /// Returns None while live or when prepared without an explicit clock.
    /// This does not measure driver callback arrival, delivery or acoustic time.
    pub fn render_cadence(
        &self,
    ) -> Result<
        Option<crate::audio::cadence::RenderCadence>,
        crate::audio::cadence::RenderCadenceError,
    > {
        if self.control.is_some() || self.context.host_clock.is_none() {
            Ok(None)
        } else {
            self.context.cadence.summary(self.sample_rate).map(Some)
        }
    }

    /// Terminal stop/dispose/Release, always attempting cleanup after diagnostics errors.
    /// A repeated stop is harmless; no implicit restart or Mixer rewind occurs.
    pub fn stop(&mut self) -> Result<(), AsioStreamError> {
        if self.control.is_none() {
            return Ok(());
        }
        let diagnostic = self.diagnostics();
        let close = self.control.take().unwrap().close();
        self.retired = close.is_ok();
        self.phase =
            if close.is_err() || diagnostic.is_err() || self.native.faults.requires_reopen() {
                AsioStreamPhase::Failed
            } else {
                AsioStreamPhase::Stopped
            };
        diagnostic?;
        close?;
        if self.native.faults.requires_reopen() {
            return Err(AsioStreamError::ReopenRequired(self.native));
        }
        Ok(())
    }
}
impl Drop for AsioStream {
    fn drop(&mut self) {
        // Close detaches and drains callback admission before context's field drop.
        if let Some(control) = self.control.take() {
            let _ = control.close();
        }
    }
}

#[cfg(test)]
mod tests;

impl beatkernel::audio::StoppedMixerSource for AsioStream {
    type Error = AsioStreamError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        if !self.retired || self.control.is_some() {
            return Err(AsioStreamError::InvalidState);
        }
        // SAFETY: successful close detached and drained callback admission. The
        // control is gone and this exclusive owner is the sole remaining accessor.
        let state = unsafe { &mut *self.context.state.get() };
        Ok(state.renderer.take_mixer())
    }
}
