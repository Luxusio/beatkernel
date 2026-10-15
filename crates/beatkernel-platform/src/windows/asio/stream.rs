//! Owning-thread ASIO output with preallocated Mixer rendering and raw diagnostics.
#![allow(unsafe_code)]

use super::control::{check, AsioControl, AsioControlError, AsioLatencies, Status};
use crate::audio::{
    asio::source_callback::{
        source_callback_fill, source_callback_preflight, AsioPlaneDescriptor, AsioSourceOps,
        AsioSourcePermit, AsioSourcePreflight, AsioSourceStep,
    },
    asio::{AsioBlockRenderer, AsioBufferRequest, AsioRenderError},
    source_output_retirement::{
        SourceRetirementError, SourceRetirementPolicy, SourceRetirementSlot,
    },
    telemetry::Telemetry,
    AsioPlanarLayout, AudioStreamSnapshot, AudioStreamStatus, NativeOutputState,
    SourcePlaybackSnapshot, StreamCounters,
};
use crate::windows::clock::QpcClock;
use beatkernel::audio::{
    AudioFormat, InterpolatedTargetTime, Mixer, OutputFrameBasis, RenderReport, SourcePlaybackSpan,
};
use std::{
    cell::UnsafeCell,
    error::Error,
    ffi::c_void,
    fmt,
    mem::ManuallyDrop,
    ptr,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc, Weak,
    },
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
    /// Native start has not been called; complete retained output is unprimed.
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
    /// First positively admitted frame of the original block for this callback.
    pub delivered_first_frame: u64,
    /// Exact native buffer extent, which may be a prefix of the original block.
    pub delivered_frames: u32,
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

/// Generated source history coherent with one ASIO callback publication.
/// Prepared frames describe successful planar writes, including priming; they
/// do not prove audible progress, epoch validity or a native presentation prefix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AsioPlaybackPhaseSnapshot {
    /// ASIO software progress and independently observed lifecycle/diagnostics.
    pub snapshot: AsioStreamSnapshot,
    /// Actual generated physical-to-playback interval selected before rounding.
    pub playback: SourcePlaybackSpan,
    /// Actual physical source frontier generated by the complete Mixer owner.
    pub pulled_source_end: u64,
    /// Matching outer callback generation covering scalar, source and event.
    pub revision: u64,
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
    /// Callback authorization, failed Start or unproved release forbids recovery.
    RecoveryUnavailable,
    /// Original native cleanup error retained alongside the primary operation.
    Cleanup(Arc<AsioControlError>),
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
            Self::RecoveryUnavailable => f.write_str("ASIO Source recovery is unavailable"),
            Self::Cleanup(error) => write!(f, "ASIO stream cleanup: {error}"),
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
            Self::Cleanup(error) => Some(error.as_ref()),
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
        preflight: unsafe extern "C" fn(*mut c_void, i32) -> i32,
        callback: unsafe extern "C" fn(*mut c_void, i32, *const AsioCallbackEvent) -> i32,
        context: *mut c_void,
    ) -> Status;
    fn bk_asio_start(handle: *mut c_void) -> Status;
    fn bk_asio_diagnostics(handle: *mut c_void, output: *mut RawDiagnostics) -> Status;
}

enum RenderOwner {
    Legacy(AsioBlockRenderer),
    Complete(Option<NativeOutputState>),
}
struct SourceControl {
    slot: SourceRetirementSlot,
    geometry: AtomicU32,
    nonce: AtomicU64,
}
impl SourceControl {
    fn new() -> Self {
        Self {
            slot: SourceRetirementSlot::new(),
            geometry: AtomicU32::new(0),
            nonce: AtomicU64::new(0),
        }
    }
    fn authorized(&self) -> bool {
        matches!(
            self.slot.status(self.nonce.load(Ordering::Acquire)),
            Ok(crate::audio::source_output_retirement::SourceRetirementStatus::Authorized)
        )
    }
}
/// Opaque permission request bound to one original native stream allocation.
pub struct SourceRetirementTicket {
    control: Weak<SourceControl>,
    nonce: u64,
}
impl fmt::Debug for SourceRetirementTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceRetirementTicket")
            .field("nonce", &self.nonce)
            .finish_non_exhaustive()
    }
}
/// Callback permission remains distinct from native release and callback drain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRetirementStatus {
    /// The callback has not decided against the actual retained suffix.
    Pending,
    /// The callback retained the live owner and refused this request.
    Refused,
    /// Cancellation won the shared request decision.
    Cancelled,
    /// Sticky permission; native release and callback drain remain separate.
    Authorized,
}
impl From<crate::audio::source_output_retirement::SourceRetirementStatus>
    for SourceRetirementStatus
{
    fn from(value: crate::audio::source_output_retirement::SourceRetirementStatus) -> Self {
        use crate::audio::source_output_retirement::SourceRetirementStatus as S;
        match value {
            S::Pending => Self::Pending,
            S::Refused => Self::Refused,
            S::Cancelled => Self::Cancelled,
            S::Authorized => Self::Authorized,
        }
    }
}
struct SourceRetirementFacts {
    source: AudioFormat,
    layout: AsioPlanarLayout,
    matrix: Option<beatkernel::audio::ChannelMatrix>,
    render_limit: usize,
}
struct RenderState {
    renderer: RenderOwner,
    frames: u32,
    permit: Option<AsioSourcePermit>,
    last_report: Option<RenderReport>,
    rows: [RawOutput; 32],
    prepared_frames: u64,
    buffer_fills: u64,
    version: u64,
    source_retirement_completed: u64,
}
struct RenderContext {
    state: UnsafeCell<RenderState>,
    host_clock: Option<QpcClock>,
    cadence: crate::audio::cadence::Capture,
    telemetry: Telemetry,
    publication: AtomicU64,
    event_values: [AtomicU64; 8],
    source_control: Arc<SourceControl>,
}
// SAFETY: the native bridge's acquire/release nonblocking renderer guard permits
// at most one callback to access state. Owner accesses it only before native start
// or after native close has detached and drained callbacks. Mixer is Send (asserted
// during construction). Shared telemetry contains only atomics. No context pointer
// is retained beyond driver Release and callback-reader drain.
unsafe impl Sync for RenderContext {}

// All native addresses were validated by prepare across both double-buffer
// halves. The portable step additionally validates this selected full layout.
struct CompleteCallbackOps<'a> {
    context: &'a RenderContext,
    planes: [AsioPlaneDescriptor; 32],
    count: usize,
    event: Option<AsioCallbackEvent>,
    prepared_frames: &'a mut u64,
    buffer_fills: &'a mut u64,
    version: u64,
    next_version: Option<u64>,
    next_frames: u64,
    next_fills: u64,
    report: Option<RenderReport>,
    delivery: Option<(u64, u32)>,
}
impl AsioSourceOps for CompleteCallbackOps<'_> {
    fn check_configuration(&mut self) -> Result<(), i32> {
        Ok(())
    }
    fn buffers(&mut self) -> Result<&[AsioPlaneDescriptor], i32> {
        Ok(&self.planes[..self.count])
    }
    fn begin_publication(&mut self, frames: u32) -> Result<(), i32> {
        if self.context.publication.load(Ordering::SeqCst) == u64::MAX {
            return Err(3);
        }
        let Some(next) = self.version.checked_add(2) else {
            self.context.publication.store(u64::MAX, Ordering::SeqCst);
            return Err(3);
        };
        self.next_frames = self
            .prepared_frames
            .checked_add(u64::from(frames))
            .ok_or(3)?;
        self.next_fills = self.buffer_fills.checked_add(1).ok_or(3)?;
        self.next_version = Some(next);
        self.context.publication.store(next - 1, Ordering::SeqCst);
        Ok(())
    }
    fn end_publication(&mut self) {
        // fill_complete publishes the state/history after this portable call,
        // then closes this same outer generation before returning to native.
    }
    fn copy_plane(
        &mut self,
        plane: AsioPlaneDescriptor,
        samples: &[f32],
        channels: usize,
        channel: usize,
    ) {
        // SAFETY: native prepare established storage validity through drain;
        // portable validation proved exact bounds, disjointness and no PCM alias.
        let output =
            unsafe { std::slice::from_raw_parts_mut(plane.address as *mut u8, plane.byte_size) };
        crate::audio::asio::encode_asio_channel_validated(
            plane.encoding,
            samples,
            channels,
            channel,
            output,
        );
    }
    fn history_request(
        &self,
    ) -> Option<crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest>
    {
        self.context.telemetry.source_retirement_request()
    }
    fn render_pending(
        &mut self,
        state: &mut NativeOutputState,
        frames: usize,
    ) -> Result<RenderReport, i32> {
        let start = if self.event.is_some() {
            self.context
                .host_clock
                .and_then(|clock| clock.sample_realtime())
                .map(|receipt| receipt.normalized.timestamp)
        } else {
            None
        };
        let report = state.render_pending(frames).map_err(|_| 2)?;
        if self.event.is_some() && self.context.host_clock.is_some() {
            match start {
                Some(at) => self
                    .context
                    .cadence
                    .record(at, report.start_frame, frames as u64),
                None => self.context.cadence.mark_unavailable(),
            }
        }
        Ok(report)
    }
    fn publish_generated(&mut self, _state: &NativeOutputState, report: Option<RenderReport>) {
        if report.is_some() {
            self.report = report;
        }
    }
    fn delivered(&mut self, report: RenderReport, first: u64, frames: u32, _end: u64) {
        self.report = Some(report);
        self.delivery = Some((first, frames));
        *self.prepared_frames = self.next_frames;
        *self.buffer_fills = self.next_fills;
    }
}

impl RenderContext {
    // The same native render guard must remain held from preflight through fill.
    // A Continue permit is consumed once; a later submitted request is deferred.
    unsafe fn preflight(&self, index: i32) -> i32 {
        if !matches!(index, 0 | 1) {
            return 2;
        }
        // SAFETY: caller owns the pre-start state or the native render guard.
        let state = unsafe { &mut *self.state.get() };
        state.permit = None;
        match &state.renderer {
            RenderOwner::Legacy(_) => 0,
            RenderOwner::Complete(Some(owner)) => match source_callback_preflight(
                owner,
                &self.source_control.slot,
                &self.source_control.geometry,
            ) {
                AsioSourcePreflight::Continue(permit) => {
                    state.permit = Some(permit);
                    0
                }
                AsioSourcePreflight::Retire => 1,
                AsioSourcePreflight::Failure(error) => error.code.max(2),
            },
            RenderOwner::Complete(None) => 2,
        }
    }
    unsafe fn fill_complete(
        &self,
        index: i32,
        event: Option<AsioCallbackEvent>,
        state: &mut RenderState,
    ) -> Result<(), AsioRenderError> {
        let permit = state
            .permit
            .take()
            .ok_or(AsioRenderError::InvalidConfiguration)?;
        let RenderOwner::Complete(Some(owner)) = &mut state.renderer else {
            return Err(AsioRenderError::MixerUnavailable);
        };
        let layout = match owner.layout() {
            Some(crate::audio::NativeOutputLayout::AsioPlanar(layout)) => layout,
            _ => return Err(AsioRenderError::InvalidConfiguration),
        };
        let count = layout.channels().len();
        let mut planes = [AsioPlaneDescriptor::default(); 32];
        for ((plane, row), &(_, encoding)) in planes[..count]
            .iter_mut()
            .zip(&state.rows)
            .zip(layout.channels())
        {
            let byte_size = (state.frames as usize)
                .checked_mul(row.width as usize)
                .ok_or(AsioRenderError::Capacity)?;
            if row.sample_type != encoding.native_type()
                || row.width as usize != encoding.bytes_per_sample()
            {
                return Err(AsioRenderError::InvalidBuffers);
            }
            *plane = AsioPlaneDescriptor {
                channel: u32::try_from(row.channel).map_err(|_| AsioRenderError::InvalidBuffers)?,
                encoding,
                byte_size,
                address: if index == 0 { row.buffer0 } else { row.buffer1 } as usize,
            };
        }
        let mut ops = CompleteCallbackOps {
            context: self,
            planes,
            count,
            event,
            prepared_frames: &mut state.prepared_frames,
            buffer_fills: &mut state.buffer_fills,
            version: state.version,
            next_version: None,
            next_frames: 0,
            next_fills: 0,
            report: state.last_report,
            delivery: None,
        };
        let result = source_callback_fill(
            owner,
            permit,
            &mut ops,
            &mut state.source_retirement_completed,
        );
        let next_version = ops.next_version;
        let report = ops.report;
        let delivery = ops.delivery;
        drop(ops);
        if let Some(next_version) = next_version {
            state.last_report = report;
            let success = matches!(result, AsioSourceStep::Delivered { .. });
            let status = if !success {
                AudioStreamStatus::Failed { hresult: 2 }
            } else if self.telemetry.status.load(Ordering::SeqCst) == 1 {
                AudioStreamStatus::Running
            } else {
                AudioStreamStatus::Ready
            };
            self.telemetry.publish_with_source_history(
                Some(AudioStreamSnapshot {
                    telemetry_available: true,
                    status,
                    counters: StreamCounters {
                        submitted_frames: state.prepared_frames,
                        buffer_fills: state.buffer_fills,
                        ..Default::default()
                    },
                    clock: None,
                    render: report,
                }),
                owner.mixer().source_playback_history(),
                &mut state.version,
            );
            let values = match (event, delivery) {
                (Some(event), Some((first, frames))) if success => [
                    1,
                    u64::from(event.buffer_index as u32)
                        | (u64::from(event.direct_process as u32) << 32),
                    u64::from(event.flags),
                    event.sample_position,
                    event.system_nanoseconds,
                    event.sample_rate.to_bits(),
                    first,
                    u64::from(frames),
                ],
                _ => [0; 8],
            };
            for (destination, value) in self.event_values.iter().zip(values) {
                destination.store(value, Ordering::SeqCst);
            }
            self.publication.store(next_version, Ordering::SeqCst);
        }
        match result {
            AsioSourceStep::Delivered { .. } => Ok(()),
            AsioSourceStep::Failure(error) if error.code == 3 => Err(AsioRenderError::Capacity),
            AsioSourceStep::Failure(_) => Err(AsioRenderError::InvalidBuffers),
        }
    }
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
        if matches!(state.renderer, RenderOwner::Complete(_)) {
            // SAFETY: this is the same admitted, serialized access as fill.
            return unsafe { self.fill_complete(index, event, state) };
        }
        let RenderOwner::Legacy(renderer) = &mut state.renderer else {
            unreachable!()
        };
        if self.publication.load(Ordering::SeqCst) == u64::MAX {
            return Err(AsioRenderError::Capacity);
        }
        let Some(next_version) = state.version.checked_add(2) else {
            // Permanently invalidate old callback/source proof before any render.
            self.publication.store(u64::MAX, Ordering::SeqCst);
            return Err(AsioRenderError::Capacity);
        };
        let next_frames = state
            .prepared_frames
            .checked_add(u64::from(renderer.frames()))
            .ok_or(AsioRenderError::Capacity)?;
        let next_fills = state
            .buffer_fills
            .checked_add(1)
            .ok_or(AsioRenderError::Capacity)?;
        let count = renderer.encodings().len();
        let mut outputs: [&mut [u8]; 32] = std::array::from_fn(|_| &mut [] as &mut [u8]);
        for (output, row) in outputs[..count].iter_mut().zip(&state.rows) {
            let bytes = renderer.frames() as usize * row.width as usize;
            let buffer = if index == 0 { row.buffer0 } else { row.buffer1 };
            // SAFETY: prepare validated nonnull, <=isize::MAX representable regions
            // and disjointness of all channels/halves. The trusted driver retains
            // their storage until close; renderer admission prevents concurrent writes.
            *output = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), bytes) };
        }
        renderer.validate_buffers(&outputs[..count])?;
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
        let result = match self.telemetry.source_retirement_request() {
            Some(request) => renderer
                .apply_source_history_retirement(request, &mut state.source_retirement_completed)
                .and_then(|_| renderer.render(&mut outputs[..count])),
            None => renderer.render(&mut outputs[..count]),
        };
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
        self.telemetry.publish_with_source_history(
            Some(AudioStreamSnapshot {
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
                render: renderer.last_render_report(),
            }),
            renderer.source_playback_history(),
            &mut state.version,
        );
        let event = event.filter(|_| result.is_ok());
        let values = event.map_or([0; 8], |event| {
            [
                1,
                u64::from(event.buffer_index as u32)
                    | (u64::from(event.direct_process as u32) << 32),
                u64::from(event.flags),
                event.sample_position,
                event.system_nanoseconds,
                event.sample_rate.to_bits(),
                renderer
                    .last_render_report()
                    .map_or(0, |report| report.start_frame),
                u64::from(renderer.frames()),
            ]
        });
        for (destination, value) in self.event_values.iter().zip(values) {
            destination.store(value, Ordering::SeqCst);
        }
        self.publication.store(next_version, Ordering::SeqCst);
        result.map(|_| ())
    }
    fn read_source_playback_at(
        &self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
    ) -> Option<(SourcePlaybackSnapshot, Option<AsioBufferObservation>, u64)> {
        self.read_source_playback_with(
            query,
            expected_source_rate,
            expected_initial_physical,
            expected_initial_playback,
            |_| {},
        )
    }
    fn read_source_playback_with(
        &self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
        mut before_recheck: impl FnMut(usize),
    ) -> Option<(SourcePlaybackSnapshot, Option<AsioBufferObservation>, u64)> {
        for attempt in 0..3 {
            let before = self.publication.load(Ordering::SeqCst);
            if before == 0 || before & 1 != 0 {
                continue;
            }
            let source = self.telemetry.read_source_playback_at(
                query,
                expected_source_rate,
                expected_initial_physical,
                expected_initial_playback,
            );
            let values =
                std::array::from_fn(|index| self.event_values[index].load(Ordering::SeqCst));
            before_recheck(attempt);
            if self.publication.load(Ordering::SeqCst) == before {
                let source = source?;
                let observation = buffer_observation(source.snapshot, values);
                return Some((source, observation, before));
            }
        }
        None
    }
    fn read(&self) -> (AudioStreamSnapshot, Option<AsioBufferObservation>) {
        for _ in 0..3 {
            let before = self.publication.load(Ordering::SeqCst);
            if before == 0 || before & 1 != 0 {
                continue;
            }
            let software = self.telemetry.read();
            let values: [u64; 8] =
                std::array::from_fn(|index| self.event_values[index].load(Ordering::SeqCst));
            if self.publication.load(Ordering::SeqCst) == before && software.telemetry_available {
                let observation = buffer_observation(software, values);
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
fn buffer_observation(
    software: AudioStreamSnapshot,
    values: [u64; 8],
) -> Option<AsioBufferObservation> {
    if values[0] != 1 {
        return None;
    }
    software.render.map(|render| AsioBufferObservation {
        render,
        delivered_first_frame: values[6],
        delivered_frames: values[7] as u32,
        event: AsioCallbackEvent {
            buffer_index: values[1] as u32 as i32,
            direct_process: (values[1] >> 32) as u32 as i32,
            flags: values[2] as u32,
            sample_position: values[3],
            system_nanoseconds: values[4],
            sample_rate: f64::from_bits(values[5]),
        },
    })
}

fn stream_snapshot(
    phase: AsioStreamPhase,
    native: AsioDiagnostics,
    software: AudioStreamSnapshot,
    buffer_observation: Option<AsioBufferObservation>,
) -> AsioStreamSnapshot {
    let phase = if native.faults.requires_reopen()
        || matches!(software.status, AudioStreamStatus::Failed { .. })
    {
        AsioStreamPhase::Failed
    } else {
        phase
    };
    AsioStreamSnapshot {
        phase,
        telemetry_available: software.telemetry_available,
        prepared_frames: software.counters.submitted_frames,
        buffer_fills: software.counters.buffer_fills,
        render: software.render,
        buffer_observation,
        native,
    }
}

unsafe extern "C" fn preflight(context: *mut c_void, index: i32) -> i32 {
    // SAFETY: bridge retains its reader and render guard until the paired fill
    // returns; this stable context survives native close and callback drain.
    unsafe { (&*context.cast::<RenderContext>()).preflight(index) }
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
    basis: beatkernel::audio::OutputFrameBasis,
    control: Option<AsioControl>,
    retired: bool,
    context: ManuallyDrop<Box<RenderContext>>,
    phase: AsioStreamPhase,
    native: AsioDiagnostics,
    latencies: AsioLatencies,
    sample_rate: u32,
    buffer_frames: u32,
    source_facts: Option<SourceRetirementFacts>,
    native_release_unproved: bool,
    latest_cleanup: Option<Arc<AsioControlError>>,
    failed_start: bool,
}
/// Original prepare error with software mixer or an owner pending callback retirement.
pub type AsioPrepareFailure = beatkernel::audio::OutputOpenFailure<AsioStreamError, AsioStream>;
/// Original prepare error retaining complete remix, PCM, history and source owner.
pub type AsioStatePrepareFailure =
    beatkernel::audio::OutputOpenFailure<AsioStreamError, AsioStream, NativeOutputState>;
fn validate_channels(
    channels: &[u32],
    format: beatkernel::audio::AudioFormat,
) -> Result<(), AsioStreamError> {
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
    Ok(())
}
fn recover_before_context(
    control: AsioControl,
    original: AsioStreamError,
    mixer: Option<Mixer>,
) -> AsioPrepareFailure {
    let close = control.close_with_retirement();
    let failure = AsioPrepareFailure::recovered(original, mixer);
    match close.error {
        Some(error) => failure.with_cleanup_error(AsioStreamError::Control(error)),
        None => failure,
    }
}
fn recover_after_prepare(mut stream: AsioStream, original: AsioStreamError) -> AsioPrepareFailure {
    let close = stream
        .control
        .take()
        .expect("prepare owns its control")
        .close_with_retirement();
    stream.retired = close.retired;
    stream.phase = AsioStreamPhase::Failed;
    stream.native_release_unproved = !close.retired || close.error.is_some();
    stream.latest_cleanup = close.error.map(Arc::new);
    let cleanup = stream
        .latest_cleanup
        .as_ref()
        .map(|error| AsioStreamError::Cleanup(Arc::clone(error)));
    let failure = if stream.retired && !stream.native_release_unproved {
        match beatkernel::audio::StoppedMixerSource::take_stopped_mixer(&mut stream) {
            Ok(mixer) => AsioPrepareFailure::recovered(original, mixer),
            Err(error) => {
                return AsioPrepareFailure::pending(original, stream)
                    .with_cleanup_error(cleanup.unwrap_or(error));
            }
        }
    } else {
        AsioPrepareFailure::pending(original, stream)
    };
    match cleanup {
        Some(error) => failure.with_cleanup_error(error),
        None => failure,
    }
}
trait NativeLifecycleOps {
    fn diagnostics(
        &mut self,
        control: Option<&AsioControl>,
        cached: AsioDiagnostics,
    ) -> Result<AsioDiagnostics, AsioControlError>;
    fn start(
        &mut self,
        control: Option<&AsioControl>,
        context: &RenderContext,
    ) -> Result<(), AsioControlError>;
    fn close(&mut self, control: &mut Option<AsioControl>) -> Option<super::control::CloseOutcome>;
}
trait NativePrepareOps: NativeLifecycleOps {
    fn sample_rate(&mut self, control: Option<&mut AsioControl>) -> Result<f64, AsioControlError>;
    fn buffer_constraints(
        &mut self,
        control: Option<&mut AsioControl>,
    ) -> Result<crate::audio::asio::AsioBufferConstraints, AsioControlError>;
    fn channel_info(
        &mut self,
        control: Option<&mut AsioControl>,
        channel: u32,
    ) -> Result<super::control::AsioChannelInfo, AsioControlError>;
    fn latencies(
        &mut self,
        control: Option<&mut AsioControl>,
    ) -> Result<AsioLatencies, AsioControlError>;
    fn prepare(
        &mut self,
        control: Option<&AsioControl>,
        outputs: &mut [RawOutput],
        frames: u32,
        rate: f64,
        context: *mut c_void,
    ) -> Result<(), AsioControlError>;
}
struct NativeOps;
impl NativeLifecycleOps for NativeOps {
    fn diagnostics(
        &mut self,
        control: Option<&AsioControl>,
        cached: AsioDiagnostics,
    ) -> Result<AsioDiagnostics, AsioControlError> {
        let Some(control) = control else {
            return Ok(cached);
        };
        let mut raw = RawDiagnostics::default();
        // SAFETY: exact C ABI initialized output and live same-thread handle.
        check(
            unsafe { bk_asio_diagnostics(control.raw(), &mut raw) },
            "diagnostics",
        )?;
        Ok(AsioDiagnostics {
            faults: AsioFaults(raw.flags),
            render_error: raw.render_error,
            event: (raw.clock_available == 1).then_some(raw.event),
        })
    }
    fn start(
        &mut self,
        control: Option<&AsioControl>,
        _context: &RenderContext,
    ) -> Result<(), AsioControlError> {
        // SAFETY: public construction retains the prepared opening-thread control.
        check(
            unsafe { bk_asio_start(control.expect("prepared native control").raw()) },
            "start",
        )
    }
    fn close(&mut self, control: &mut Option<AsioControl>) -> Option<super::control::CloseOutcome> {
        control.take().map(AsioControl::close_with_retirement)
    }
}
impl NativePrepareOps for NativeOps {
    fn sample_rate(&mut self, control: Option<&mut AsioControl>) -> Result<f64, AsioControlError> {
        control.expect("native control").sample_rate()
    }
    fn buffer_constraints(
        &mut self,
        control: Option<&mut AsioControl>,
    ) -> Result<crate::audio::asio::AsioBufferConstraints, AsioControlError> {
        control.expect("native control").buffer_constraints()
    }
    fn channel_info(
        &mut self,
        control: Option<&mut AsioControl>,
        channel: u32,
    ) -> Result<super::control::AsioChannelInfo, AsioControlError> {
        control
            .expect("native control")
            .channel_info(channel, false)
    }
    fn latencies(
        &mut self,
        control: Option<&mut AsioControl>,
    ) -> Result<AsioLatencies, AsioControlError> {
        control.expect("native control").latencies()
    }
    fn prepare(
        &mut self,
        control: Option<&AsioControl>,
        outputs: &mut [RawOutput],
        frames: u32,
        rate: f64,
        context: *mut c_void,
    ) -> Result<(), AsioControlError> {
        // SAFETY: fixed prevalidated metadata, positive signed native frames,
        // stable callback context retained through reported detach and drain.
        check(
            unsafe {
                bk_asio_prepare(
                    control.expect("native control").raw(),
                    outputs.as_mut_ptr(),
                    outputs.len() as i32,
                    frames as i32,
                    rate,
                    preflight,
                    render,
                    context,
                )
            },
            "createBuffers",
        )
    }
}
fn retirement_error(error: SourceRetirementError) -> AsioStreamError {
    match error {
        SourceRetirementError::InvalidTicket => AsioStreamError::InvalidState,
        SourceRetirementError::NoncesExhausted => AsioRenderError::Capacity.into(),
    }
}
impl AsioStream {
    /// Prepares complete ownership and source history without filling either half.
    /// The actual buffer B is filled only immediately before the native Start.
    pub fn prepare_state_recoverable(
        control: AsioControl,
        state: NativeOutputState,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: QpcClock,
        history_headroom: usize,
    ) -> Result<Self, AsioStatePrepareFailure> {
        Self::prepare_state_with(
            Some(control),
            state,
            &channels,
            request,
            None,
            Some(host_clock),
            Some(history_headroom),
            &mut NativeOps,
        )
    }
    /// Complete preparation with an explicit same-rate output matrix.
    pub fn prepare_state_remixed_recoverable(
        control: AsioControl,
        state: NativeOutputState,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        matrix: beatkernel::audio::ChannelMatrix,
        host_clock: QpcClock,
        history_headroom: usize,
    ) -> Result<Self, AsioStatePrepareFailure> {
        Self::prepare_state_with(
            Some(control),
            state,
            &channels,
            request,
            Some(matrix),
            Some(host_clock),
            Some(history_headroom),
            &mut NativeOps,
        )
    }
    fn prepare_state_with(
        control: Option<AsioControl>,
        state: NativeOutputState,
        channels: &[u32],
        request: AsioBufferRequest,
        matrix: Option<beatkernel::audio::ChannelMatrix>,
        host_clock: Option<QpcClock>,
        history_headroom: Option<usize>,
        ops: &mut impl NativePrepareOps,
    ) -> Result<Self, AsioStatePrepareFailure> {
        fn require_send<T: Send>() {}
        require_send::<NativeOutputState>();
        let basis = state.output_frame_basis();
        let source = state.mixer().configuration().format();
        // Install the actual owner before any native probe so every failed
        // consumed close can return a real pending stream for HWND custody.
        let mut stream = Self {
            basis,
            control,
            retired: false,
            context: ManuallyDrop::new(Box::new(RenderContext {
                state: UnsafeCell::new(RenderState {
                    renderer: RenderOwner::Complete(Some(state)),
                    frames: 0,
                    permit: None,
                    last_report: None,
                    rows: [RawOutput::default(); 32],
                    prepared_frames: 0,
                    buffer_fills: 0,
                    version: 0,
                    source_retirement_completed: 0,
                }),
                host_clock,
                cadence: crate::audio::cadence::Capture::new(),
                telemetry: Telemetry::new(),
                publication: AtomicU64::new(0),
                event_values: std::array::from_fn(|_| AtomicU64::new(0)),
                source_control: Arc::new(SourceControl::new()),
            })),
            phase: AsioStreamPhase::Ready,
            native: AsioDiagnostics::default(),
            latencies: AsioLatencies {
                input_frames: 0,
                output_frames: 0,
            },
            sample_rate: source.sample_rate(),
            buffer_frames: 0,
            source_facts: None,
            native_release_unproved: false,
            latest_cleanup: None,
            failed_start: false,
        };
        let staged = (|| -> Result<(), AsioStreamError> {
            let target = if matrix.is_some() {
                AudioFormat::new(
                    source.sample_rate(),
                    u16::try_from(channels.len()).map_err(|_| AsioStreamError::InvalidChannels)?,
                )
                .map_err(|_| AsioStreamError::InvalidChannels)?
            } else {
                source
            };
            validate_channels(channels, target)?;
            if let Some(matrix) = matrix.as_ref() {
                crate::audio::channel_remix::validate(source, target, matrix)
                    .map_err(AsioRenderError::Core)?;
            }
            let rate = ops.sample_rate(stream.control.as_mut())?;
            if rate != f64::from(source.sample_rate()) {
                return Err(AsioStreamError::RateMismatch {
                    expected: source.sample_rate(),
                    actual: rate,
                });
            }
            let frames = ops
                .buffer_constraints(stream.control.as_mut())?
                .resolve(request)
                .map_err(AsioControlError::from)?;
            let mut rows = [RawOutput::default(); 32];
            let mut layout_rows = [(0, crate::audio::asio::AsioPcmEncoding::Float32Lsb); 32];
            for ((row, layout), &channel) in rows.iter_mut().zip(&mut layout_rows).zip(channels) {
                let info = ops.channel_info(stream.control.as_mut(), channel)?;
                if info.channel != channel || info.input {
                    return Err(AsioStreamError::InvalidChannels);
                }
                let encoding = info.pcm_encoding().map_err(AsioRenderError::Pcm)?;
                *row = RawOutput {
                    channel: channel as i32,
                    sample_type: info.sample_type,
                    width: encoding.bytes_per_sample() as u32,
                    ..Default::default()
                };
                *layout = (channel, encoding);
            }
            let layout = AsioPlanarLayout::new(target, &layout_rows[..channels.len()])
                .map_err(AsioRenderError::Core)?;
            let state = stream.context.state.get_mut();
            let RenderOwner::Complete(Some(owner)) = &mut state.renderer else {
                unreachable!()
            };
            owner
                .validate_reconfigure_asio(&layout, matrix.as_ref(), frames as usize)
                .map_err(AsioRenderError::Core)?;
            // New history begins at the Mixer's generated frontier. It cannot
            // describe an older retained suffix whose original Source history
            // was never captured; refuse before changing the complete owner.
            if owner.pending_frames() != 0 && owner.mixer().source_playback_history().is_none() {
                return Err(
                    AsioRenderError::Core(beatkernel::audio::AudioError::InvalidFormat).into(),
                );
            }
            if let Some(headroom) = history_headroom {
                owner
                    .prepare_growing_native_source_history(frames, headroom)
                    .map_err(AsioRenderError::Core)?;
            }
            owner
                .reconfigure_asio(layout, matrix, frames as usize)
                .map_err(AsioRenderError::Core)?;
            stream.source_facts = Some(SourceRetirementFacts {
                source,
                layout,
                matrix: owner.channel_matrix().cloned(),
                render_limit: owner.mixer().configuration().limits().max_render_frames(),
            });
            let telemetry = match owner.mixer().source_playback_history() {
                Some(history) => Telemetry::with_source_history_and_retirement(history, basis)
                    .map_err(AsioRenderError::Core)?,
                None => Telemetry::new(),
            };
            state.frames = frames;
            state.rows = rows;
            stream.buffer_frames = frames;
            stream.context.telemetry = telemetry;
            let pointer = ptr::from_ref(stream.context.as_ref())
                .cast_mut()
                .cast::<c_void>();
            let state = stream.context.state.get_mut();
            ops.prepare(
                stream.control.as_ref(),
                &mut state.rows[..channels.len()],
                frames,
                rate,
                pointer,
            )?;
            stream.latencies = ops.latencies(stream.control.as_mut())?;
            Ok(())
        })();
        if let Err(original) = staged {
            let cleanup = stream.stop_with(ops).err();
            stream.phase = AsioStreamPhase::Failed;
            let cleanup = stream
                .latest_cleanup
                .as_ref()
                .map(|error| AsioStreamError::Cleanup(Arc::clone(error)))
                .or(cleanup);
            let failure = if stream.retired && !stream.native_release_unproved {
                match stream.take_stopped_output_state() {
                    Ok(state) => AsioStatePrepareFailure::recovered_state(original, state),
                    Err(error) => {
                        return Err(AsioStatePrepareFailure::pending_state(original, stream)
                            .with_cleanup_error(cleanup.unwrap_or(error)))
                    }
                }
            } else {
                AsioStatePrepareFailure::pending_state(original, stream)
            };
            return Err(match cleanup {
                Some(error) => failure.with_cleanup_error(error),
                None => failure,
            });
        }
        Ok(stream)
    }
    /// Retries only actual remaining native control; a consumed failed close
    /// cannot invent new release proof or erase the original cleanup diagnostic.
    pub fn retry_state_prepare_cleanup(
        failure: &mut AsioStatePrepareFailure,
    ) -> Result<bool, &AsioStreamError> {
        failure.retry_retirement(|stream| {
            stream.stop()?;
            stream.take_stopped_output_state()
        })
    }
    /// Transfers the unique complete owner only after native release and drain.
    pub fn take_stopped_output_state(
        &mut self,
    ) -> Result<Option<NativeOutputState>, AsioStreamError> {
        if !self.retired || self.control.is_some() || self.native_release_unproved {
            return Err(AsioStreamError::RecoveryUnavailable);
        }
        // SAFETY: retirement detached and drained callbacks; exclusive owner.
        let state = unsafe { &mut *self.context.state.get() };
        match &mut state.renderer {
            RenderOwner::Complete(owner) => Ok(owner.take()),
            RenderOwner::Legacy(_) => Err(AsioStreamError::InvalidState),
        }
    }
    /// Exact negotiated frame count; complete Ready may still have no fills.
    pub fn buffer_frames(&self) -> u32 {
        self.buffer_frames
    }
    /// Original applied output matrix, without callback state access.
    pub fn source_channel_matrix(&self) -> Option<&beatkernel::audio::ChannelMatrix> {
        self.source_facts
            .as_ref()
            .and_then(|facts| facts.matrix.as_ref())
    }
    /// Sticky absence of successful native release, separate from callback drain.
    pub const fn native_release_unproved(&self) -> bool {
        self.native_release_unproved
    }
    /// Latest original native cleanup error, even when the primary operation failed.
    pub fn latest_cleanup_error(&self) -> Option<&AsioControlError> {
        self.latest_cleanup.as_deref()
    }
    fn source_authorized(&self) -> bool {
        self.context.source_control.authorized()
    }
    /// Current Source liveness. Authorization/uncertain admission is a narrow
    /// recovery refusal; unrelated native errors keep their original variants.
    pub fn source_liveness(&self) -> Result<(), AsioStreamError> {
        // Only a healthy stream's authorization race is a deferrable lifecycle
        // refusal. Never replace an actual native/cleanup failure with that token.
        if let Some(error) = &self.latest_cleanup {
            return Err(AsioStreamError::Cleanup(Arc::clone(error)));
        }
        if self.native.faults.requires_reopen() || self.native.render_error != 0 {
            return Err(AsioStreamError::ReopenRequired(self.native));
        }
        if matches!(
            self.context.telemetry.read().status,
            AudioStreamStatus::Failed { .. }
        ) {
            return Err(AsioStreamError::InvalidState);
        }
        if self.source_authorized() || self.failed_start || self.native_release_unproved {
            return Err(AsioStreamError::RecoveryUnavailable);
        }
        if !matches!(
            self.phase,
            AsioStreamPhase::Ready | AsioStreamPhase::Running
        ) {
            return Err(AsioStreamError::InvalidState);
        }
        Ok(())
    }
    /// Publishes one retirement request against cold validated target facts.
    /// DriverPreferred has unknown future geometry and therefore requires no tail.
    pub fn submit_source_conditional_retirement(
        &mut self,
        channels: &[u32],
        request: AsioBufferRequest,
        matrix: Option<&beatkernel::audio::ChannelMatrix>,
    ) -> Result<Option<SourceRetirementTicket>, AsioStreamError> {
        let facts = self
            .source_facts
            .as_ref()
            .ok_or(AsioStreamError::InvalidState)?;
        let target = AudioFormat::new(
            facts.source.sample_rate(),
            u16::try_from(channels.len()).map_err(|_| AsioStreamError::InvalidChannels)?,
        )
        .map_err(|_| AsioStreamError::InvalidChannels)?;
        validate_channels(channels, target)?;
        match matrix {
            Some(matrix) => crate::audio::channel_remix::validate(facts.source, target, matrix)
                .map_err(AsioRenderError::Core)?,
            None if target != facts.source => {
                return Err(AsioRenderError::InvalidConfiguration.into())
            }
            None => {}
        }
        let geometry = match request {
            AsioBufferRequest::DriverPreferred => 0,
            AsioBufferRequest::Frames(frames)
                if frames != 0 && frames as usize <= facts.render_limit =>
            {
                frames
            }
            _ => return Err(AsioRenderError::Capacity.into()),
        };
        let same = facts
            .layout
            .channels()
            .iter()
            .map(|&(channel, _)| channel)
            .eq(channels.iter().copied())
            && facts.matrix.as_ref() == matrix;
        let policy = if same && geometry != 0 {
            SourceRetirementPolicy::PreservePausedSuffix
        } else {
            SourceRetirementPolicy::RequireEmptySuffix
        };
        if self.source_authorized() {
            return Ok(None);
        }
        self.source_liveness()?;
        let control = &self.context.source_control;
        let nonce = control
            .slot
            .submit_prepared(policy, || {
                control.geometry.store(geometry, Ordering::Relaxed);
            })
            .map_err(retirement_error)?;
        if let Some(nonce) = nonce {
            control.nonce.store(nonce, Ordering::Release);
        }
        Ok(nonce.map(|nonce| SourceRetirementTicket {
            control: Arc::downgrade(control),
            nonce,
        }))
    }
    fn ticket_nonce(&self, ticket: &SourceRetirementTicket) -> Result<u64, AsioStreamError> {
        if !ticket
            .control
            .ptr_eq(&Arc::downgrade(&self.context.source_control))
        {
            return Err(AsioStreamError::InvalidState);
        }
        Ok(ticket.nonce)
    }
    /// Reads/consumes a response; sticky authorization survives terminal cleanup.
    pub fn poll_source_conditional_retirement(
        &mut self,
        ticket: &SourceRetirementTicket,
    ) -> Result<SourceRetirementStatus, AsioStreamError> {
        let nonce = self.ticket_nonce(ticket)?;
        let slot = &self.context.source_control.slot;
        if slot.status(nonce).map_err(retirement_error)?
            != crate::audio::source_output_retirement::SourceRetirementStatus::Authorized
        {
            self.source_liveness()?;
        }
        slot.poll(nonce).map(Into::into).map_err(retirement_error)
    }
    /// Cancellation and callback authorization compete in the same atomic word.
    pub fn cancel_source_conditional_retirement(
        &mut self,
        ticket: &SourceRetirementTicket,
    ) -> Result<SourceRetirementStatus, AsioStreamError> {
        let nonce = self.ticket_nonce(ticket)?;
        let status = self
            .context
            .source_control
            .slot
            .cancel(nonce)
            .map_err(retirement_error)?;
        if status != crate::audio::source_output_retirement::SourceRetirementStatus::Authorized {
            self.source_liveness()?;
        }
        Ok(status.into())
    }
    /// Original physical mixer frame basis captured before native probes and priming.
    pub const fn frame_basis(&self) -> beatkernel::audio::OutputFrameBasis {
        self.basis
    }
    /// Consumes exact driver/Mixer/channel choices, prepares buffers and primes B.
    /// Does not change rate, select another device, start, or show driver UI.
    pub fn prepare(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
    ) -> Result<Self, AsioStreamError> {
        Self::prepare_recoverable(control, mixer, channels, request)
            .map_err(|failure| failure.into_parts().0)
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
        Self::prepare_with_clock_recoverable(control, mixer, channels, request, host_clock)
            .map_err(|failure| failure.into_parts().0)
    }

    /// Prepares source history from the actual negotiated native buffer size.
    /// Existing history and the supplied QPC clock retain their original origins.
    pub fn prepare_with_clock_and_source_history(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: QpcClock,
        headroom: usize,
    ) -> Result<Self, AsioStreamError> {
        Self::prepare_with_clock_and_source_history_recoverable(
            control, mixer, channels, request, host_clock, headroom,
        )
        .map_err(|failure| failure.into_parts().0)
    }

    /// Preserves current software state or the actual partial callback owner on error.
    pub fn prepare_recoverable(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
    ) -> Result<Self, AsioPrepareFailure> {
        Self::prepare_internal(control, mixer, channels, request, None, None, None)
    }
    /// Recoverable preparation with the original opt-in host cadence clock.
    pub fn prepare_with_clock_recoverable(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: QpcClock,
    ) -> Result<Self, AsioPrepareFailure> {
        Self::prepare_internal(
            control,
            mixer,
            channels,
            request,
            Some(host_clock),
            None,
            None,
        )
    }

    /// Recoverable preparation with source history sized before native priming.
    pub fn prepare_with_clock_and_source_history_recoverable(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: QpcClock,
        headroom: usize,
    ) -> Result<Self, AsioPrepareFailure> {
        Self::prepare_internal(
            control,
            mixer,
            channels,
            request,
            Some(host_clock),
            None,
            Some(headroom),
        )
    }
    /// Prepares an explicit same-rate matrix in selected driver-channel order.
    /// Optional QPC cadence keeps its original caller-selected origin.
    pub fn prepare_remixed_recoverable(
        control: AsioControl,
        mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        matrix: beatkernel::audio::ChannelMatrix,
        host_clock: Option<QpcClock>,
    ) -> Result<Self, AsioPrepareFailure> {
        Self::prepare_internal(
            control,
            mixer,
            channels,
            request,
            host_clock,
            Some(matrix),
            None,
        )
    }
    /// Retries only while actual control/retirement evidence remains available.
    /// A consumed control without proof cannot be treated as a successful close.
    pub fn retry_prepare_cleanup(
        failure: &mut AsioPrepareFailure,
    ) -> Result<bool, &AsioStreamError> {
        failure.retry_retirement(|stream| {
            if stream.control.is_none() && !stream.retired {
                return Err(AsioStreamError::InvalidState);
            }
            stream.stop()?;
            beatkernel::audio::StoppedMixerSource::take_stopped_mixer(stream)
        })
    }
    fn prepare_internal(
        mut control: AsioControl,
        mut mixer: Mixer,
        channels: Vec<u32>,
        request: AsioBufferRequest,
        host_clock: Option<QpcClock>,
        matrix: Option<beatkernel::audio::ChannelMatrix>,
        history_headroom: Option<usize>,
    ) -> Result<Self, AsioPrepareFailure> {
        let basis = mixer.output_frame_basis();
        fn require_send<T: Send>() {}
        require_send::<AsioBlockRenderer>();
        let format = mixer.configuration().format();
        let staged = (|| -> Result<_, AsioStreamError> {
            let target = if let Some(matrix) = &matrix {
                let count =
                    u16::try_from(channels.len()).map_err(|_| AsioStreamError::InvalidChannels)?;
                let target = AudioFormat::new(format.sample_rate(), count)
                    .map_err(|_| AsioStreamError::InvalidChannels)?;
                crate::audio::channel_remix::validate(format, target, matrix)
                    .map_err(AsioRenderError::Core)?;
                target
            } else {
                format
            };
            validate_channels(&channels, target)?;
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
            Ok((rate, frames, encodings, rows, target))
        })();
        let (rate, frames, encodings, rows, target) = match staged {
            Ok(staged) => staged,
            Err(error) => return Err(recover_before_context(control, error, Some(mixer))),
        };
        if let Some(headroom) = history_headroom {
            if let Err(error) =
                crate::audio::source_playback_publication::prepare_native_source_history(
                    &mut mixer, frames, headroom,
                )
            {
                return Err(recover_before_context(
                    control,
                    AsioStreamError::Render(AsioRenderError::Core(error)),
                    Some(mixer),
                ));
            }
        }
        let telemetry = match mixer.source_playback_history() {
            Some(history) => match Telemetry::with_source_history_and_retirement(history, basis) {
                Ok(telemetry) => telemetry,
                Err(error) => {
                    return Err(recover_before_context(
                        control,
                        AsioStreamError::Render(AsioRenderError::Core(error)),
                        Some(mixer),
                    ))
                }
            },
            None => Telemetry::new(),
        };
        let rendered = match matrix {
            Some(matrix) => {
                AsioBlockRenderer::new_remixed_recoverable(mixer, frames, target, encodings, matrix)
            }
            None => AsioBlockRenderer::new_recoverable(mixer, frames, encodings),
        };
        let renderer = match rendered {
            Ok(renderer) => renderer,
            Err(failure) => {
                let (error, mixer) = failure.into_parts();
                return Err(recover_before_context(
                    control,
                    AsioStreamError::Render(error),
                    mixer,
                ));
            }
        };

        let mut stream = Self {
            basis,
            control: Some(control),
            retired: false,
            context: ManuallyDrop::new(Box::new(RenderContext {
                host_clock,
                cadence: crate::audio::cadence::Capture::new(),
                state: UnsafeCell::new(RenderState {
                    renderer: RenderOwner::Legacy(renderer),
                    frames,
                    permit: None,
                    last_report: None,
                    rows,
                    prepared_frames: 0,
                    buffer_fills: 0,
                    version: 0,
                    source_retirement_completed: 0,
                }),
                telemetry,
                publication: AtomicU64::new(0),
                event_values: std::array::from_fn(|_| AtomicU64::new(0)),
                source_control: Arc::new(SourceControl::new()),
            })),
            phase: AsioStreamPhase::Ready,
            sample_rate: format.sample_rate(),
            buffer_frames: frames,
            source_facts: None,
            native_release_unproved: false,
            latest_cleanup: None,
            failed_start: false,
            native: AsioDiagnostics::default(),
            latencies: AsioLatencies {
                input_frames: 0,
                output_frames: 0,
            },
        };
        let prepared = (|| -> Result<(), AsioStreamError> {
            let context = ptr::from_ref(stream.context.as_ref())
                .cast_mut()
                .cast::<c_void>();
            let state = stream.context.state.get_mut();
            // SAFETY: same-thread live control, stable Box and fixed rows, verified
            // positive signed SDK frames/count and actual format. prepare doesn't render;
            // errors attempt reported retirement before context release; unproven
            // retirement retains the actual context in a pending owner.
            check(
                unsafe {
                    bk_asio_prepare(
                        stream.control.as_ref().unwrap().raw(),
                        state.rows.as_mut_ptr(),
                        channels.len() as i32,
                        frames as i32,
                        rate,
                        preflight,
                        render,
                        context,
                    )
                },
                "createBuffers",
            )?;
            stream.latencies = stream.control.as_mut().unwrap().latencies()?;
            // SAFETY: native prepare validated all regions; start has not enabled render.
            unsafe { stream.context.fill(1, None) }?;
            Ok(())
        })();
        if let Err(error) = prepared {
            return Err(recover_after_prepare(stream, error));
        }
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
        AsioPresentationObservation::from_delivery(
            observation.render,
            observation.delivered_first_frame,
            observation.delivered_frames as usize,
            self.sample_rate,
            host,
            self.latencies.output_frames,
            latency_error_ns,
            output_origin,
        )
    }
    /// Starts once; complete B priming occurs immediately before actual Start.
    /// The primary operation is returned and any cleanup error remains inspectable.
    pub fn start(&mut self) -> Result<(), AsioStreamError> {
        self.start_with(&mut NativeOps)
    }
    fn start_with(&mut self, ops: &mut impl NativeLifecycleOps) -> Result<(), AsioStreamError> {
        if self.phase != AsioStreamPhase::Ready {
            return Err(AsioStreamError::InvalidState);
        }
        if self.source_authorized() || self.native_release_unproved || self.failed_start {
            return Err(AsioStreamError::RecoveryUnavailable);
        }
        let native = match ops.diagnostics(self.control.as_ref(), self.native) {
            Ok(native) => {
                self.native = native;
                native
            }
            Err(error) => return self.fail_start_before_attempt(error.into(), ops),
        };
        if native.faults.requires_reopen() || native.render_error != 0 {
            return self.fail_start_before_attempt(AsioStreamError::ReopenRequired(native), ops);
        }
        if self.source_facts.is_some() {
            // SAFETY: native Start has not enabled callbacks; owner is exclusive.
            let disposition = unsafe { self.context.preflight(1) };
            if disposition != 0 {
                let error = if disposition == 1 {
                    AsioStreamError::RecoveryUnavailable
                } else {
                    AsioRenderError::InvalidConfiguration.into()
                };
                return self.fail_start_before_attempt(error, ops);
            }
            // SAFETY: prepare established both native halves; preflight minted
            // this owner's single-use permit. The driver is still unstarted.
            if let Err(error) = unsafe { self.context.fill(1, None) } {
                return self.fail_start_before_attempt(error.into(), ops);
            }
        }
        self.context.telemetry.status.store(1, Ordering::SeqCst);
        // No mutable callback-state borrow crosses native Start: A may execute
        // synchronously here and consumes the prefix following the B prime.
        if let Err(error) = ops.start(self.control.as_ref(), &self.context) {
            self.failed_start = true;
            let _ = self.stop_with(ops);
            self.phase = AsioStreamPhase::Failed;
            return Err(error.into());
        }
        self.phase = AsioStreamPhase::Running;
        let native = match ops.diagnostics(self.control.as_ref(), self.native) {
            Ok(native) => {
                self.native = native;
                native
            }
            Err(error) => {
                self.failed_start = true;
                let _ = self.stop_with(ops);
                self.phase = AsioStreamPhase::Failed;
                return Err(error.into());
            }
        };
        if native.faults.requires_reopen() || native.render_error != 0 {
            self.failed_start = true;
            let _ = self.stop_with(ops);
            self.phase = AsioStreamPhase::Failed;
            return Err(AsioStreamError::ReopenRequired(native));
        }
        Ok(())
    }
    fn fail_start_before_attempt(
        &mut self,
        error: AsioStreamError,
        ops: &mut impl NativeLifecycleOps,
    ) -> Result<(), AsioStreamError> {
        let _ = self.stop_with(ops);
        self.phase = AsioStreamPhase::Failed;
        Err(error)
    }
    fn diagnostics(&mut self) -> Result<AsioDiagnostics, AsioControlError> {
        let native = NativeOps.diagnostics(self.control.as_ref(), self.native)?;
        self.native = native;
        Ok(native)
    }
    /// Reads bounded coherent software progress and raw native diagnostics off-thread
    /// from rendering, on the control owner thread. Native faults set Failed phase.
    pub fn snapshot(&mut self) -> Result<AsioStreamSnapshot, AsioStreamError> {
        let native = self.diagnostics()?;
        let (software, buffer_observation) = self.context.read();
        Ok(stream_snapshot(
            self.phase,
            native,
            software,
            buffer_observation,
        ))
    }
    /// Reads exact generated source facts and the copied callback event within
    /// one outer generation. Lifecycle and native diagnostics remain independent.
    /// Callers must separately authenticate the native epoch and presentation;
    /// software preparation, including B priming, grants no audible frontier.
    pub fn source_playback_snapshot_at(
        &mut self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
    ) -> Result<Option<AsioPlaybackPhaseSnapshot>, AsioStreamError> {
        self.diagnostics()?;
        self.source_liveness()?;
        Ok(self.source_playback_view_at(
            query,
            expected_source_rate,
            expected_initial_physical,
            expected_initial_playback,
        ))
    }
    /// Reads one coherent source/render/callback publication without SDK calls.
    /// Lifecycle and diagnostics are cached owner state, not fresh native fault
    /// evidence or presentation permission. Priming can have no callback event.
    pub fn source_playback_view_at(
        &self,
        query: &InterpolatedTargetTime,
        expected_source_rate: u32,
        expected_initial_physical: u64,
        expected_initial_playback: u64,
    ) -> Option<AsioPlaybackPhaseSnapshot> {
        if self.failed_start || self.native_release_unproved || self.source_authorized() {
            return None;
        }
        let view = self
            .context
            .read_source_playback_at(
                query,
                expected_source_rate,
                expected_initial_physical,
                expected_initial_playback,
            )
            .map(
                |(source, observation, revision)| AsioPlaybackPhaseSnapshot {
                    snapshot: stream_snapshot(
                        self.phase,
                        self.native,
                        source.snapshot,
                        observation,
                    ),
                    playback: source.playback,
                    pulled_source_end: source.pulled_source_end,
                    revision,
                },
            );
        if self.failed_start || self.native_release_unproved || self.source_authorized() {
            None
        } else {
            view
        }
    }
    /// Queues a mathematical Source retention minimum for the admitted owning
    /// callback. Cached lifecycle/diagnostics do not authenticate presentation.
    pub fn request_source_history_retirement(
        &self,
        expected_basis: OutputFrameBasis,
        source_identity: (u32, u64, u64),
        source_pin: u64,
    ) -> Result<Option<u64>, AsioStreamError> {
        if expected_basis != self.basis || source_identity.0 != self.sample_rate {
            return Err(AsioStreamError::Render(AsioRenderError::Core(
                beatkernel::audio::AudioError::InvalidFormat,
            )));
        }
        self.source_liveness()?;
        if !matches!(
            self.phase,
            AsioStreamPhase::Ready | AsioStreamPhase::Running
        ) {
            return Err(AsioStreamError::InvalidState);
        }
        if self.native.faults.requires_reopen() || self.native.render_error != 0 {
            return Err(AsioStreamError::ReopenRequired(self.native));
        }
        for _ in 0..3 {
            let before = self.context.publication.load(Ordering::SeqCst);
            if before == 0 || before & 1 != 0 {
                continue;
            }
            let software = self.context.telemetry.read();
            if self.context.publication.load(Ordering::SeqCst) != before {
                continue;
            }
            if !software.telemetry_available {
                return Ok(None);
            }
            if matches!(software.status, AudioStreamStatus::Failed { .. }) {
                return Err(AsioStreamError::InvalidState);
            }
            if self.source_authorized() || self.failed_start {
                return Err(AsioStreamError::RecoveryUnavailable);
            }
            return self
                .context
                .telemetry
                .request_source_history_retirement(expected_basis, source_identity, source_pin)
                .map_err(|error| AsioStreamError::Render(AsioRenderError::Core(error)));
        }
        Ok(None)
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
        if !self.retired || self.control.is_some() || self.context.host_clock.is_none() {
            Ok(None)
        } else {
            self.context.cadence.summary(self.sample_rate).map(Some)
        }
    }

    /// Terminal stop/dispose/Release, retaining cleanup failure separately from
    /// callback retirement. A consumed failed handle supplies no later retry proof.
    pub fn stop(&mut self) -> Result<(), AsioStreamError> {
        self.stop_with(&mut NativeOps)
    }
    fn stop_with(&mut self, ops: &mut impl NativeLifecycleOps) -> Result<(), AsioStreamError> {
        // Always ask the actual production operations whether a native owner
        // remains. The test seam supplies real lifecycle effects without an SDK.
        let diagnostic = ops.diagnostics(self.control.as_ref(), self.native);
        if let Ok(native) = &diagnostic {
            self.native = *native;
        }
        let outcome = ops.close(&mut self.control);
        if let Some(outcome) = outcome {
            self.retired = outcome.retired;
            if !outcome.retired || outcome.error.is_some() {
                self.native_release_unproved = true;
            }
            if let Some(error) = outcome.error {
                self.latest_cleanup = Some(Arc::new(error));
            }
            if self.retired && self.failed_start {
                // SAFETY: only detached, drained callbacks permit owner access.
                // B may have played even when native Start invoked no callback.
                let state = unsafe { &mut *self.context.state.get() };
                if state.prepared_frames != 0 {
                    if let RenderOwner::Complete(Some(owner)) = &mut state.renderer {
                        owner.mark_admission_uncertain();
                    }
                }
            }
            self.phase = if self.native_release_unproved
                || diagnostic.is_err()
                || self.native.faults.requires_reopen()
            {
                AsioStreamPhase::Failed
            } else {
                AsioStreamPhase::Stopped
            };
        }
        if let Err(error) = diagnostic {
            return Err(error.into());
        }
        if self.native_release_unproved {
            return Err(self
                .latest_cleanup
                .as_ref()
                .map_or(AsioStreamError::RecoveryUnavailable, |error| {
                    AsioStreamError::Cleanup(Arc::clone(error))
                }));
        }
        if self.native.faults.requires_reopen() {
            return Err(AsioStreamError::ReopenRequired(self.native));
        }
        Ok(())
    }
}
impl Drop for AsioStream {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let outcome = control.close_with_retirement();
            self.retired = outcome.retired;
            if !outcome.retired || outcome.error.is_some() {
                self.native_release_unproved = true;
            }
        }
        if self.retired {
            // SAFETY: reported detach/drain is sufficient for Rust storage;
            // HWND release requires the separate sticky native-release proof.
            unsafe {
                ManuallyDrop::drop(&mut self.context);
            }
        }
        // Unproved callback drain deliberately retains the actual context.
    }
}

#[cfg(test)]
#[path = "stream/source_playback_view_fixtures.rs"]
mod source_playback_view_fixtures;
#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "stream/source_history_retirement_fixtures.rs"]
mod source_history_retirement_fixtures;

impl beatkernel::audio::StoppedMixerSource for AsioStream {
    type Error = AsioStreamError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        if !self.retired || self.control.is_some() || self.native_release_unproved {
            return Err(AsioStreamError::InvalidState);
        }
        // SAFETY: successful close detached and drained callback admission. The
        // control is gone and this exclusive owner is the sole remaining accessor.
        let state = unsafe { &mut *self.context.state.get() };
        match &mut state.renderer {
            RenderOwner::Legacy(renderer) => Ok(renderer.take_mixer()),
            RenderOwner::Complete(_) => Err(AsioStreamError::InvalidState),
        }
    }
}

#[cfg(test)]
#[path = "stream/open_failure_fixtures.rs"]
mod open_failure_fixtures;

#[cfg(test)]
#[path = "stream/retained_source_fixtures.rs"]
mod retained_source_fixtures;
