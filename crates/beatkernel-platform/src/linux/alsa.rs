//! Dedicated nonblocking ALSA PCM writer with explicit device and sizing.
#![allow(unsafe_code)]

use crate::audio::cadence;
mod timing;
pub use cadence::{RenderCadence as AlsaRenderCadence, RenderCadenceError as AlsaCadenceError};
use timing::TimingShared;
pub use timing::{AlsaNativeTimestamp, AlsaTimingSnapshot};

use super::{LinuxError, MonotonicClock};
use crate::audio::{
    AudioStreamSnapshot, AudioStreamStatus, DeviceFormat, SampleEncoding, StreamCounters,
    encode_pcm, telemetry::Telemetry,
};
use beatkernel::{
    audio::{AudioError, Mixer, RenderReport},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    ffi::{CString, c_char, c_int, c_long, c_uint, c_ulong, c_void},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

/// ALSA output/duplex PCM name hint, not a format or availability certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlsaDevice {
    /// Exact ALSA NAME, preserved for explicit caller selection.
    pub name: String,
    /// Unmodified optional UTF-8 description; display code may flatten newlines.
    pub description: Option<String>,
}

/// Queries ALSA output/duplex PCM hints without opening a PCM stream.
///
/// `max_devices` must be 1..=4096 and bounds **all** scanned hints, including
/// input-only hints and duplicates. `max_text_bytes` must be 1..=16384 and
/// bounds each returned native text field, excluding its NUL terminator. A cap
/// violation rejects the whole result instead of returning a truncated list.
/// Exact duplicate NAME values retain the first output/duplex description in
/// native order. No default endpoint is selected and no format is certified.
///
/// Rust traversal and owned metadata are bounded; ALSA constructs its entire
/// native hint list, and allocates extracted strings, before these bounds can
/// be checked. This off-thread query cannot bound that native allocation/work.
///
/// ABI and ownership follow ALSA's [Name Hint Interface](https://www.alsa-project.org/alsa-doc/alsa-lib/group___hint.html).
pub fn alsa_output_devices(
    max_devices: usize,
    max_text_bytes: usize,
) -> Result<Vec<AlsaDevice>, LinuxError> {
    let mut rows = DiscoveryRows::new(max_devices, max_text_bytes)?;
    let api = DiscoveryApi::load()?;
    let mut list = ptr::null_mut();
    // SAFETY: -1 selects all cards; static pcm is NUL-terminated. The output
    // pointer is writable. On error ALSA frees its internal partial list; on
    // success the returned NULL-terminated list becomes HintList's sole owner.
    check("snd_device_name_hint", unsafe {
        (api.hint)(-1, c"pcm".as_ptr(), &mut list)
    })?;
    let list = HintList {
        pointer: list,
        api: &api,
    };
    if list.pointer.is_null() {
        return Err(LinuxError::MalformedInput("ALSA returned a NULL hint list"));
    }
    // Read at most max_devices entries plus the required terminating pointer.
    // A non-null pointer in that extra position reports overflow, never a
    // silently partial list. No hint text is inspected in the extra position.
    for index in 0..=max_devices {
        // SAFETY: ALSA guarantees a NULL-terminated pointer array. We stop at
        // its first NULL, so pointer reads remain within that native allocation.
        let hint = unsafe { *list.pointer.add(index) };
        if hint.is_null() {
            return Ok(rows.devices);
        }
        if index == max_devices {
            return Err(LinuxError::InvalidConfiguration(
                "ALSA discovery hint count exceeds limit",
            ));
        }
        // SAFETY: hint belongs to the still-owned list; IDs are static C strings.
        // Each returned malloc allocation immediately gains a HintText owner.
        let ioid = HintText(unsafe { (api.get)(hint, c"IOID".as_ptr()) });
        let ioid_bytes = ioid.bytes(max_text_bytes)?;
        if hint_is_input(ioid_bytes, max_text_bytes)? {
            rows.admit(None, None, ioid_bytes)?;
            continue;
        }
        // SAFETY: same live hint/IDs; allocations are freed on every later error.
        let name = HintText(unsafe { (api.get)(hint, c"NAME".as_ptr()) });
        // SAFETY: same live hint/IDs; DESC is optional according to ALSA.
        let description = HintText(unsafe { (api.get)(hint, c"DESC".as_ptr()) });
        rows.admit(
            name.bytes(max_text_bytes)?,
            description.bytes(max_text_bytes)?,
            ioid_bytes,
        )?;
    }
    unreachable!("bounded hint traversal returns at terminator or limit")
}

struct DiscoveryRows {
    max_devices: usize,
    max_text_bytes: usize,
    scanned: usize,
    devices: Vec<AlsaDevice>,
}
impl DiscoveryRows {
    fn new(max_devices: usize, max_text_bytes: usize) -> Result<Self, LinuxError> {
        if !(1..=4096).contains(&max_devices) || !(1..=16384).contains(&max_text_bytes) {
            return Err(LinuxError::InvalidConfiguration(
                "ALSA discovery limits must be 1..4096 hints and 1..16384 bytes per text",
            ));
        }
        let mut devices = Vec::new();
        devices
            .try_reserve_exact(max_devices)
            .map_err(|_| discovery_allocation_error())?;
        Ok(Self {
            max_devices,
            max_text_bytes,
            scanned: 0,
            devices,
        })
    }
    fn admit(
        &mut self,
        name: Option<&[u8]>,
        description: Option<&[u8]>,
        ioid: Option<&[u8]>,
    ) -> Result<(), LinuxError> {
        if self.scanned == self.max_devices {
            return Err(LinuxError::InvalidConfiguration(
                "ALSA discovery hint count exceeds limit",
            ));
        }
        self.scanned += 1;
        if hint_is_input(ioid, self.max_text_bytes)? {
            return Ok(());
        }
        let name = hint_utf8(
            name.ok_or(LinuxError::MalformedInput("ALSA output hint has no NAME"))?,
            self.max_text_bytes,
        )?;
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(LinuxError::MalformedInput(
                "ALSA hint NAME is empty or contains controls",
            ));
        }
        let description = description
            .map(|bytes| hint_utf8(bytes, self.max_text_bytes))
            .transpose()?;
        if self.devices.iter().any(|device| device.name == name) {
            return Ok(());
        }
        self.devices.push(AlsaDevice {
            name: owned_hint_text(name)?,
            description: description.map(owned_hint_text).transpose()?,
        });
        Ok(())
    }
}

fn hint_utf8(bytes: &[u8], max_text_bytes: usize) -> Result<&str, LinuxError> {
    if bytes.len() > max_text_bytes {
        return Err(LinuxError::MalformedInput(
            "ALSA hint text exceeds selected byte limit",
        ));
    }
    std::str::from_utf8(bytes)
        .map_err(|_| LinuxError::MalformedInput("ALSA hint text is not UTF-8"))
}
fn hint_is_input(ioid: Option<&[u8]>, max_text_bytes: usize) -> Result<bool, LinuxError> {
    match ioid
        .map(|bytes| hint_utf8(bytes, max_text_bytes))
        .transpose()?
    {
        Some("Input") => Ok(true),
        Some("Output") | None => Ok(false),
        Some(_) => Err(LinuxError::MalformedInput("unknown ALSA hint IOID")),
    }
}
fn owned_hint_text(value: &str) -> Result<String, LinuxError> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| discovery_allocation_error())?;
    owned.push_str(value);
    Ok(owned)
}
fn discovery_allocation_error() -> LinuxError {
    LinuxError::Io(std::io::Error::from(std::io::ErrorKind::OutOfMemory))
}

// Kept separate from PCM Api: native stream opening needs no discovery symbols.
struct DiscoveryApi {
    hint: unsafe extern "C" fn(c_int, *const c_char, *mut *mut Handle) -> c_int,
    get: unsafe extern "C" fn(*const c_void, *const c_char) -> *mut c_char,
    free_hint: unsafe extern "C" fn(*mut Handle) -> c_int,
    _library: Library,
}
impl DiscoveryApi {
    fn load() -> Result<Self, LinuxError> {
        let library = Library::open()?;
        // SAFETY: these published ALSA control.h signatures match each symbol.
        // POSIX dlsym returns callable addresses; Library lives until all owners
        // invoking the functions have been dropped.
        let hint = unsafe {
            std::mem::transmute::<
                Handle,
                unsafe extern "C" fn(c_int, *const c_char, *mut *mut Handle) -> c_int,
            >(library.symbol(c"snd_device_name_hint")?)
        };
        // SAFETY: exact control.h signature and the same retained library lifetime.
        let get = unsafe {
            std::mem::transmute::<
                Handle,
                unsafe extern "C" fn(*const c_void, *const c_char) -> *mut c_char,
            >(library.symbol(c"snd_device_name_get_hint")?)
        };
        // SAFETY: exact control.h signature and the same retained library lifetime.
        let free_hint = unsafe {
            std::mem::transmute::<Handle, unsafe extern "C" fn(*mut Handle) -> c_int>(
                library.symbol(c"snd_device_name_free_hint")?,
            )
        };
        Ok(Self {
            hint,
            get,
            free_hint,
            _library: library,
        })
    }
}
struct HintList<'a> {
    pointer: *mut Handle,
    api: &'a DiscoveryApi,
}
impl Drop for HintList<'_> {
    fn drop(&mut self) {
        // SAFETY: matching successful ALSA list allocation, freed once while its
        // library is retained; all HintText owners have already left their scope.
        unsafe {
            (self.api.free_hint)(self.pointer);
        }
    }
}
unsafe extern "C" {
    fn free(pointer: *mut c_void);
}
struct HintText(*mut c_char);
impl HintText {
    fn bytes(&self, max_text_bytes: usize) -> Result<Option<&[u8]>, LinuxError> {
        if self.0.is_null() {
            return Ok(None);
        }
        for length in 0..=max_text_bytes {
            // SAFETY: ALSA returns an allocated NUL-terminated string. Stop at
            // the first NUL, or reject once more than the selected bytes exist.
            if unsafe { *self.0.add(length) } == 0 {
                // SAFETY: the preceding scan established length initialized
                // bytes before NUL; this slice cannot outlive its HintText owner.
                return Ok(Some(unsafe {
                    std::slice::from_raw_parts(self.0.cast(), length)
                }));
            }
        }
        Err(LinuxError::MalformedInput(
            "ALSA hint text exceeds selected byte limit",
        ))
    }
}
impl Drop for HintText {
    fn drop(&mut self) {
        // SAFETY: get_hint returns malloc-owned storage or NULL; libc free is
        // its documented matching deallocator, invoked once for this owner.
        unsafe {
            free(self.0.cast());
        }
    }
}

/// Explicit ALSA PCM request; rates/format never substitute silently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlsaRequest {
    /// Native ALSA endpoint, such as an explicitly selected hw:CARD,DEV.
    pub device: String,
    /// Exact rate/channel/sample encoding, with unspecified speaker mask.
    pub format: DeviceFormat,
    /// Positive requested hardware buffer frames.
    pub buffer_frames: u32,
    /// Positive processing period frames, smaller than the requested buffer.
    pub period_frames: u32,
    /// Explicit permission for nearest supported buffer/period sizes only.
    pub allow_size_rounding: bool,
    /// Caller domain labeling CLOCK_MONOTONIC observations.
    pub monotonic_domain: ClockDomainId,
}

/// Hardware configuration reported independently from the original request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlsaAppliedConfig {
    /// Unmodified caller request.
    pub requested: AlsaRequest,
    /// Applied exact format.
    pub format: DeviceFormat,
    /// Native buffer capacity.
    pub buffer_frames: u32,
    /// Applied processing period.
    pub period_frames: u32,
    /// Whether explicitly authorized rounding changed native sizing.
    pub sizing_adjusted: bool,
    /// Mixer scheduling clock identity, not a hardware presentation clock.
    pub output_domain: ClockDomainId,
    /// Mixer output frame-grid origin.
    pub output_origin: Timestamp,
}

/// Native writer lifecycle, with terminal errno retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlsaStatus {
    /// Open/configured worker is parked until explicit start.
    Ready,
    /// Worker is submitting PCM.
    Running,
    /// Worker was stopped and joined.
    Stopped,
    /// Terminal native or rendering failure; no implicit restart.
    Failed {
        /// ALSA errno when present, zero for nonnative failures.
        code: i32,
    },
    /// Joining observed worker panic.
    WorkerPanicked,
}

/// Independently observed atomic counters; fields are not one coherent snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlsaSnapshot {
    /// Worker lifecycle/terminal native status.
    pub status: AlsaStatus,
    /// Frames successfully submitted to ALSA.
    pub submitted_frames: u64,
    /// Frames consumed from the mixer scheduling grid (including pending writes).
    pub rendered_frames: u64,
    /// Completed period-sized render operations.
    pub renders: u64,
    /// Actual ALSA EPIPE xrun signals, not inferred silence.
    pub xruns: u64,
    /// Actual ALSA ESTRPIPE suspend signals.
    pub suspends: u64,
    /// Terminal native/render errors observed.
    pub failures: u64,
    /// Most recent monotonic software observation, unavailable before rendering.
    pub observed_at: Option<ClockPoint>,
}

struct Shared {
    start: AtomicBool,
    stop: AtomicBool,
    status: AtomicU8,
    errno: AtomicI32,
    submitted: AtomicU64,
    rendered: AtomicU64,
    renders: AtomicU64,
    xruns: AtomicU64,
    suspends: AtomicU64,
    failures: AtomicU64,
    observed: AtomicI64,
    observed_valid: AtomicBool,
    timing: TimingShared,
    render_telemetry: Telemetry,
    cadence: cadence::Capture,
}
impl Shared {
    fn new() -> Self {
        Self {
            start: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            status: AtomicU8::new(0),
            errno: AtomicI32::new(0),
            submitted: AtomicU64::new(0),
            rendered: AtomicU64::new(0),
            renders: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            suspends: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            observed: AtomicI64::new(0),
            observed_valid: AtomicBool::new(false),
            timing: TimingShared::new(),
            render_telemetry: Telemetry::new(),
            cadence: cadence::Capture::new(),
        }
    }
}

/// Owns the worker lifecycle; worker exclusively owns ALSA handles and Mixer.
pub struct AlsaStream {
    configuration: AlsaAppliedConfig,
    basis: beatkernel::audio::OutputFrameBasis,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<(Result<(), LinuxError>, Mixer)>>,
    recovered_mixer: Option<Mixer>,
    retired: bool,
}
impl AlsaStream {
    /// Opens/configures on the worker and waits for its actual startup result.
    /// Render/conversion storage is allocated before this method returns Ready.
    pub fn open(request: AlsaRequest, mixer: Mixer) -> Result<Self, LinuxError> {
        let basis = mixer.output_frame_basis();
        super::sys::supported_abi()?;
        if request.device.is_empty()
            || request.device.contains('\0')
            || request.period_frames == 0
            || request.buffer_frames <= request.period_frames
            || request.format.channel_mask().is_some()
        {
            return Err(LinuxError::InvalidConfiguration(
                "explicit device, unspecified channel mask, and 0 < period < buffer required",
            ));
        }
        if mixer.config().format() != request.format.pcm() {
            return Err(LinuxError::InvalidConfiguration(
                "ALSA and mixer formats must match exactly",
            ));
        }
        let shared = Arc::new(Shared::new());
        let worker_shared = Arc::clone(&shared);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("beatkernel-alsa".into())
            .spawn(move || {
                let mut mixer = mixer;
                let opened = NativePcm::open(&request).and_then(|(pcm, buffer, period)| {
                    if period as usize > mixer.config().limits().max_render_frames() {
                        return Err(LinuxError::InvalidConfiguration(
                            "applied period exceeds preallocated mixer render limit",
                        ));
                    }
                    let samples = (period as usize)
                        .checked_mul(usize::from(request.format.channels()))
                        .ok_or(LinuxError::Overflow)?;
                    let bytes = samples
                        .checked_mul(usize::from(request.format.encoding().bytes_per_sample()))
                        .ok_or(LinuxError::Overflow)?;
                    let render = vec![0.0; samples];
                    let conversion = vec![0u8; bytes];
                    let configuration = AlsaAppliedConfig {
                        format: request.format,
                        buffer_frames: buffer,
                        period_frames: period,
                        sizing_adjusted: period != request.period_frames
                            || buffer != request.buffer_frames,
                        output_domain: mixer.config().domain(),
                        output_origin: mixer.config().origin(),
                        requested: request.clone(),
                    };
                    Ok((pcm, configuration, render, conversion))
                });
                let (mut pcm, configuration, mut render, mut conversion) = match opened {
                    Ok(opened) => opened,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return (Ok(()), mixer);
                    }
                };
                if sender.send(Ok(configuration.clone())).is_err() {
                    return (Ok(()), mixer);
                }
                drop(sender);
                while !worker_shared.start.load(Ordering::Acquire)
                    && !worker_shared.stop.load(Ordering::Acquire)
                {
                    thread::park();
                }
                if worker_shared.stop.load(Ordering::Acquire) {
                    return (Ok(()), mixer);
                }
                worker_shared.status.store(1, Ordering::Release);
                // Worker-local unwind/return guard clears timing even on panic.
                let _timing_guard = TimingInvalidation(&worker_shared.timing);
                let result = run_worker(
                    &mut pcm,
                    &mut mixer,
                    &configuration,
                    &mut render,
                    &mut conversion,
                    &worker_shared,
                );
                match &result {
                    Ok(()) => worker_shared.status.store(2, Ordering::Release),
                    Err(error) => {
                        let code = match error {
                            LinuxError::Alsa { code, .. } => *code,
                            _ => 0,
                        };
                        if code == -32 {
                            increment(&worker_shared.xruns, 1);
                        }
                        if code == -86 {
                            increment(&worker_shared.suspends, 1);
                        }
                        increment(&worker_shared.failures, 1);
                        worker_shared.errno.store(code, Ordering::Relaxed);
                        worker_shared.status.store(3, Ordering::Release);
                    }
                }
                (result, mixer)
            })?;
        match receiver.recv() {
            Ok(Ok(configuration)) => Ok(Self {
                configuration,
                basis,
                shared,
                worker: Some(worker),
                recovered_mixer: None,
                retired: false,
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(LinuxError::WorkerPanicked)
            }
        }
    }

    /// Original mixer grid captured before this stream renders or submits frames.
    pub const fn frame_basis(&self) -> beatkernel::audio::OutputFrameBasis {
        self.basis
    }
    /// Exact applied settings and output scheduling grid.
    pub const fn configuration(&self) -> &AlsaAppliedConfig {
        &self.configuration
    }
    /// Starts once. A stopped/failed stream requires an explicit new open.
    pub fn start(&mut self) -> Result<(), LinuxError> {
        let worker = self.worker.as_ref().ok_or(LinuxError::InvalidLifecycle)?;
        if self.shared.start.swap(true, Ordering::AcqRel) {
            return Err(LinuxError::InvalidLifecycle);
        }
        if self.shared.stop.load(Ordering::Acquire) {
            return Err(LinuxError::InvalidLifecycle);
        }
        worker.thread().unpark();
        Ok(())
    }
    /// Requests stop and joins. Native waits have a fixed 20 ms bound.
    /// Returning a worker error still guarantees join and native-handle teardown.
    pub fn stop(&mut self) -> Result<(), LinuxError> {
        self.shared.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker.thread().unpark();
        match worker.join() {
            Ok((result, mixer)) => {
                self.recovered_mixer = Some(mixer);
                self.retired = true;
                self.shared.timing.invalidate();
                if result.is_ok() {
                    self.shared.status.store(2, Ordering::Release);
                }
                result
            }
            Err(_) => {
                self.shared.timing.invalidate();
                self.shared.status.store(4, Ordering::Release);
                Err(LinuxError::WorkerPanicked)
            }
        }
    }
    /// Last successful core mixer report, retained after stop or native failure.
    /// It proves core rendering, not native submission or acoustic presentation.
    /// None before a successful render or while publication is unavailable.
    pub fn last_render_report(&self) -> Option<RenderReport> {
        self.shared.render_telemetry.read().render
    }
    /// Separately coherent native status; unavailable before Start/after stop,
    /// during publication or after an unavailable/failed native query.
    /// Native PREPARED may yield raw fields with no played-frame estimate.
    /// No native calls, allocations or locks occur on the caller.
    pub fn timing_snapshot(&self) -> Option<AlsaTimingSnapshot> {
        if self.shared.stop.load(Ordering::SeqCst) || self.shared.status.load(Ordering::SeqCst) != 1
        {
            return None;
        }
        let snapshot = self
            .shared
            .timing
            .snapshot(self.configuration.requested.monotonic_domain);
        if self.shared.stop.load(Ordering::SeqCst) || self.shared.status.load(Ordering::SeqCst) != 1
        {
            return None;
        }
        snapshot
    }
    /// Successful render-start cadence, available only after stop joins the worker.
    /// Includes startup fill bursts; this is not native delivery/acoustic jitter.
    pub fn render_cadence(&self) -> Result<Option<AlsaRenderCadence>, AlsaCadenceError> {
        if self.worker.is_some() {
            return Ok(None);
        }
        self.shared
            .cadence
            .summary(self.configuration.format.pcm().sample_rate())
            .map(Some)
    }
    /// Reads fixed atomic observations outside the writer.
    pub fn snapshot(&self) -> AlsaSnapshot {
        let status = match self.shared.status.load(Ordering::Acquire) {
            0 => AlsaStatus::Ready,
            1 => AlsaStatus::Running,
            2 => AlsaStatus::Stopped,
            3 => AlsaStatus::Failed {
                code: self.shared.errno.load(Ordering::Relaxed),
            },
            _ => AlsaStatus::WorkerPanicked,
        };
        AlsaSnapshot {
            status,
            submitted_frames: self.shared.submitted.load(Ordering::Relaxed),
            rendered_frames: self.shared.rendered.load(Ordering::Relaxed),
            renders: self.shared.renders.load(Ordering::Relaxed),
            xruns: self.shared.xruns.load(Ordering::Relaxed),
            suspends: self.shared.suspends.load(Ordering::Relaxed),
            failures: self.shared.failures.load(Ordering::Relaxed),
            observed_at: self
                .shared
                .observed_valid
                .load(Ordering::Acquire)
                .then(|| ClockPoint {
                    domain: self.configuration.requested.monotonic_domain,
                    timestamp: Timestamp::from_nanos(self.shared.observed.load(Ordering::Relaxed)),
                }),
        }
    }
}
impl Drop for AlsaStream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct TimingInvalidation<'a>(&'a TimingShared);
impl Drop for TimingInvalidation<'_> {
    fn drop(&mut self) {
        self.0.invalidate();
    }
}

fn increment(counter: &AtomicU64, amount: u64) {
    let old = counter.load(Ordering::Relaxed);
    counter.store(old.saturating_add(amount), Ordering::Relaxed);
}
fn run_worker(
    pcm: &mut NativePcm,
    mixer: &mut Mixer,
    config: &AlsaAppliedConfig,
    render: &mut [f32],
    conversion: &mut [u8],
    shared: &Shared,
) -> Result<(), LinuxError> {
    let clock = MonotonicClock::new(config.requested.monotonic_domain);
    let mut pending_offset = config.period_frames as usize;
    let mut render_version = 0u64;
    let align = usize::from(config.format.block_align());
    while !shared.stop.load(Ordering::Acquire) {
        if pending_offset == config.period_frames as usize {
            let render_start = clock.now()?;
            let report =
                render_and_publish(mixer, render, &shared.render_telemetry, &mut render_version)
                    .map_err(LinuxError::Mixer)?;
            shared.cadence.record(
                render_start.timestamp,
                report.start_frame,
                report.frames as u64,
            );
            encode_pcm(config.format, render, conversion).map_err(LinuxError::Conversion)?;
            let rendered = shared
                .rendered
                .load(Ordering::Relaxed)
                .checked_add(u64::try_from(report.frames).map_err(|_| LinuxError::Overflow)?)
                .ok_or(LinuxError::Overflow)?;
            shared.rendered.store(rendered, Ordering::Relaxed);
            increment(&shared.renders, 1);
            pending_offset = 0;
        }
        match pcm.write(
            &conversion[pending_offset * align..],
            config.period_frames as usize - pending_offset,
        )? {
            Some(frames) if frames > 0 => {
                pending_offset += frames;
                let submitted = shared
                    .submitted
                    .load(Ordering::Relaxed)
                    .checked_add(frames as u64)
                    .ok_or(LinuxError::Overflow)?;
                shared.submitted.store(submitted, Ordering::Relaxed);
            }
            _ => {
                if !pcm.wait()? {
                    continue;
                }
            }
        }
        shared.timing.invalidate();
        if let Some(timing) = pcm.timing(&clock, shared.submitted.load(Ordering::Relaxed))? {
            shared.timing.publish(timing);
        }
        let point = clock.now()?;
        shared
            .observed
            .store(point.timestamp.as_nanos(), Ordering::Relaxed);
        shared.observed_valid.store(true, Ordering::Release);
    }
    pcm.drop_stream()
}

// Publishes only successful core rendering, before conversion/native admission.
fn render_and_publish(
    mixer: &mut Mixer,
    output: &mut [f32],
    telemetry: &Telemetry,
    version: &mut u64,
) -> Result<RenderReport, AudioError> {
    let report = mixer.render(output)?;
    telemetry.publish(
        AudioStreamSnapshot {
            telemetry_available: true,
            status: AudioStreamStatus::Running,
            counters: StreamCounters::default(),
            clock: None,
            render: Some(report),
        },
        version,
    );
    Ok(report)
}

// Opaque C objects are never dereferenced by Rust; they remain worker-owned.
type Handle = *mut c_void;
type Frames = c_ulong;
type SignedFrames = c_long;
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(name: *const c_char, flags: c_int) -> Handle;
    fn dlsym(library: Handle, name: *const c_char) -> Handle;
    fn dlclose(library: Handle) -> c_int;
}
struct Library(Handle);
impl Library {
    fn open() -> Result<Self, LinuxError> {
        // SAFETY: static NUL-terminated soname, RTLD_NOW=2; handle is owned here.
        let handle = unsafe { dlopen(c"libasound.so.2".as_ptr(), 2) };
        if handle.is_null() {
            Err(LinuxError::AlsaUnavailable("libasound.so.2"))
        } else {
            Ok(Self(handle))
        }
    }
    fn symbol(&self, name: &'static std::ffi::CStr) -> Result<Handle, LinuxError> {
        // SAFETY: library remains loaded; static name is NUL-terminated. The
        // caller binds each symbol only to its published ALSA C signature.
        let symbol = unsafe { dlsym(self.0, name.as_ptr()) };
        if symbol.is_null() {
            Err(LinuxError::AlsaUnavailable(
                name.to_str().unwrap_or("required ALSA symbol"),
            ))
        } else {
            Ok(symbol)
        }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: exactly this owned reference is closed once, after function use.
        unsafe {
            dlclose(self.0);
        }
    }
}

macro_rules! functions {
    ($($field:ident : $signature:ty => $name:literal),* $(,)?) => {
        struct Api { $($field: $signature,)* _library: Library }
        impl Api {
            fn load() -> Result<Self, LinuxError> {
                let library = Library::open()?;
                Ok(Self { $( $field: {
                    let symbol = library.symbol($name)?;
                    // SAFETY: Linux POSIX dlsym returns a callable address;
                    // the exact declared signature comes from ALSA pcm.h.
                    unsafe { std::mem::transmute::<Handle, $signature>(symbol) }
                }, )* _library: library })
            }
        }
    }
}
functions! {
    open: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, c_int) -> c_int => c"snd_pcm_open",
    close: unsafe extern "C" fn(Handle) -> c_int => c"snd_pcm_close",
    hw_malloc: unsafe extern "C" fn(*mut Handle) -> c_int => c"snd_pcm_hw_params_malloc",
    hw_free: unsafe extern "C" fn(Handle) -> () => c"snd_pcm_hw_params_free",
    hw_any: unsafe extern "C" fn(Handle, Handle) -> c_int => c"snd_pcm_hw_params_any",
    access: unsafe extern "C" fn(Handle, Handle, c_int) -> c_int => c"snd_pcm_hw_params_set_access",
    format: unsafe extern "C" fn(Handle, Handle, c_int) -> c_int => c"snd_pcm_hw_params_set_format",
    format_value: unsafe extern "C" fn(*const c_char) -> c_int => c"snd_pcm_format_value",
    channels: unsafe extern "C" fn(Handle, Handle, c_uint) -> c_int => c"snd_pcm_hw_params_set_channels",
    rate: unsafe extern "C" fn(Handle, Handle, c_uint, c_int) -> c_int => c"snd_pcm_hw_params_set_rate",
    period_near: unsafe extern "C" fn(Handle, Handle, *mut Frames, *mut c_int) -> c_int => c"snd_pcm_hw_params_set_period_size_near",
    buffer_near: unsafe extern "C" fn(Handle, Handle, *mut Frames) -> c_int => c"snd_pcm_hw_params_set_buffer_size_near",
    apply: unsafe extern "C" fn(Handle, Handle) -> c_int => c"snd_pcm_hw_params",
    get_period: unsafe extern "C" fn(Handle, *mut Frames, *mut c_int) -> c_int => c"snd_pcm_hw_params_get_period_size",
    get_buffer: unsafe extern "C" fn(Handle, *mut Frames) -> c_int => c"snd_pcm_hw_params_get_buffer_size",
    sw_malloc: unsafe extern "C" fn(*mut Handle) -> c_int => c"snd_pcm_sw_params_malloc",
    sw_free: unsafe extern "C" fn(Handle) -> () => c"snd_pcm_sw_params_free",
    sw_current: unsafe extern "C" fn(Handle, Handle) -> c_int => c"snd_pcm_sw_params_current",
    avail_min: unsafe extern "C" fn(Handle, Handle, Frames) -> c_int => c"snd_pcm_sw_params_set_avail_min",
    start_threshold: unsafe extern "C" fn(Handle, Handle, Frames) -> c_int => c"snd_pcm_sw_params_set_start_threshold",
    tstamp_mode: unsafe extern "C" fn(Handle, Handle, c_int) -> c_int => c"snd_pcm_sw_params_set_tstamp_mode",
    tstamp_type: unsafe extern "C" fn(Handle, Handle, c_int) -> c_int => c"snd_pcm_sw_params_set_tstamp_type",
    get_tstamp_mode: unsafe extern "C" fn(Handle, *mut c_int) -> c_int => c"snd_pcm_sw_params_get_tstamp_mode",
    get_tstamp_type: unsafe extern "C" fn(Handle, *mut c_int) -> c_int => c"snd_pcm_sw_params_get_tstamp_type",
    status_malloc: unsafe extern "C" fn(*mut Handle) -> c_int => c"snd_pcm_status_malloc",
    status_free: unsafe extern "C" fn(Handle) -> () => c"snd_pcm_status_free",
    status: unsafe extern "C" fn(Handle, Handle) -> c_int => c"snd_pcm_status",
    status_state: unsafe extern "C" fn(Handle) -> c_int => c"snd_pcm_status_get_state",
    status_delay: unsafe extern "C" fn(Handle) -> SignedFrames => c"snd_pcm_status_get_delay",
    status_avail: unsafe extern "C" fn(Handle) -> Frames => c"snd_pcm_status_get_avail",
    status_htstamp: unsafe extern "C" fn(Handle, *mut NativeTimespec) -> () => c"snd_pcm_status_get_htstamp",
    sw_apply: unsafe extern "C" fn(Handle, Handle) -> c_int => c"snd_pcm_sw_params",
    prepare: unsafe extern "C" fn(Handle) -> c_int => c"snd_pcm_prepare",
    write: unsafe extern "C" fn(Handle, *const c_void, Frames) -> SignedFrames => c"snd_pcm_writei",
    wait: unsafe extern "C" fn(Handle, c_int) -> c_int => c"snd_pcm_wait",
    drop_stream: unsafe extern "C" fn(Handle) -> c_int => c"snd_pcm_drop",
}

// snd_htimestamp_t is struct timespec. Supported Linux LP64 targets use
// signed 64-bit time_t and long; sys::supported_abi gates native opening.
#[repr(C)]
struct NativeTimespec {
    seconds: c_long,
    nanoseconds: c_long,
}

struct NativePcm {
    handle: Handle,
    native_status: Handle,
    timestamp_mode: c_int,
    timestamp_type: c_int,
    api: Api,
}
impl NativePcm {
    fn open(request: &AlsaRequest) -> Result<(Self, u32, u32), LinuxError> {
        let api = Api::load()?;
        let endpoint = CString::new(request.device.as_str())
            .map_err(|_| LinuxError::InvalidConfiguration("NUL in ALSA endpoint"))?;
        let mut handle = ptr::null_mut();
        // SAFETY: valid writable output handle, live NUL-terminated endpoint;
        // playback stream=0, SND_PCM_NONBLOCK=1. All ownership stays on worker.
        check("snd_pcm_open", unsafe {
            (api.open)(&mut handle, endpoint.as_ptr(), 0, 1)
        })?;
        let mut pcm = Self {
            handle,
            api,
            native_status: ptr::null_mut(),
            timestamp_mode: 0,
            timestamp_type: 0,
        };
        let (buffer, period) = pcm.configure(request)?;
        // SAFETY: writable output pointer; the worker owns this matching status
        // allocation until Drop, including every later setup/return failure.
        check("snd_pcm_status_malloc", unsafe {
            (pcm.api.status_malloc)(&mut pcm.native_status)
        })?;
        Ok((pcm, buffer, period))
    }
    fn configure(&mut self, request: &AlsaRequest) -> Result<(u32, u32), LinuxError> {
        let mut hw = ptr::null_mut();
        // SAFETY: ALSA output allocation pointer is writable; successful params
        // remains valid through each matching API call and is freed exactly once.
        check("hw_params_malloc", unsafe { (self.api.hw_malloc)(&mut hw) })?;
        let result = self.configure_hw(request, hw);
        // SAFETY: hw is the successful matching ALSA allocation, no longer used.
        unsafe {
            (self.api.hw_free)(hw);
        }
        let (buffer, period) = result?;
        let mut sw = ptr::null_mut();
        // SAFETY: matching ALSA output allocation pointer is writable.
        check("sw_params_malloc", unsafe { (self.api.sw_malloc)(&mut sw) })?;
        // SAFETY: live PCM and matching params pointers; scalar frame values
        // have been read back from the successfully configured native stream.
        let result = unsafe {
            (|| {
                check("sw_params_current", (self.api.sw_current)(self.handle, sw))?;
                check(
                    "sw_params_avail_min",
                    (self.api.avail_min)(self.handle, sw, Frames::from(period)),
                )?;
                check(
                    "sw_params_start_threshold",
                    (self.api.start_threshold)(self.handle, sw, Frames::from(buffer - period)),
                )?;
                check(
                    "sw_params_set_tstamp_mode",
                    (self.api.tstamp_mode)(self.handle, sw, 1),
                )?;
                check(
                    "sw_params_set_tstamp_type",
                    (self.api.tstamp_type)(self.handle, sw, 1),
                )?;
                check("sw_params", (self.api.sw_apply)(self.handle, sw))?;
                // Reload applied parameters rather than inspecting request storage.
                check(
                    "sw_params_current timestamp readback",
                    (self.api.sw_current)(self.handle, sw),
                )?;
                check(
                    "sw_params_get_tstamp_mode",
                    (self.api.get_tstamp_mode)(sw, &mut self.timestamp_mode),
                )?;
                check(
                    "sw_params_get_tstamp_type",
                    (self.api.get_tstamp_type)(sw, &mut self.timestamp_type),
                )?;
                if self.timestamp_mode != 1 || self.timestamp_type != 1 {
                    return Err(LinuxError::InvalidConfiguration(
                        "ALSA requires applied ENABLE/MONOTONIC timestamps",
                    ));
                }
                check("snd_pcm_prepare", (self.api.prepare)(self.handle))
            })()
        };
        // SAFETY: sw is the matching allocation and is no longer used.
        unsafe {
            (self.api.sw_free)(sw);
        }
        result?;
        Ok((buffer, period))
    }
    fn configure_hw(
        &self,
        request: &AlsaRequest,
        params: Handle,
    ) -> Result<(u32, u32), LinuxError> {
        let format_name = match request.format.encoding() {
            SampleEncoding::Float32 => c"FLOAT_LE",
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            } => c"S16_LE",
            SampleEncoding::Pcm {
                container_bits: 24,
                valid_bits: 24,
            } => c"S24_3LE",
            SampleEncoding::Pcm {
                container_bits: 32,
                valid_bits: 32,
            } => c"S32_LE",
            _ => {
                return Err(LinuxError::InvalidConfiguration(
                    "ALSA backend requires all container bits valid",
                ));
            }
        };
        let mut period = Frames::from(request.period_frames);
        let mut buffer = Frames::from(request.buffer_frames);
        let mut direction = 0;
        // SAFETY: PCM/params are live matching ALSA owners, output variables are
        // writable native snd_pcm_uframes_t/int. Format names are static CStr.
        unsafe {
            check("hw_params_any", (self.api.hw_any)(self.handle, params))?;
            check(
                "hw_params_access",
                (self.api.access)(self.handle, params, 3),
            )?; // RW_INTERLEAVED
            let format = (self.api.format_value)(format_name.as_ptr());
            check(
                "hw_params_format",
                (self.api.format)(self.handle, params, format),
            )?;
            check(
                "hw_params_channels",
                (self.api.channels)(self.handle, params, u32::from(request.format.channels())),
            )?;
            check(
                "hw_params_rate",
                (self.api.rate)(self.handle, params, request.format.sample_rate(), 0),
            )?;
            check(
                "hw_params_period_size_near",
                (self.api.period_near)(self.handle, params, &mut period, &mut direction),
            )?;
            check(
                "hw_params_buffer_size_near",
                (self.api.buffer_near)(self.handle, params, &mut buffer),
            )?;
            // Reject a near result before applying when exact was requested.
            validate_sizes(request, buffer, period, direction)?;
            check("hw_params", (self.api.apply)(self.handle, params))?;
            direction = 0;
            check(
                "hw_params_get_period_size",
                (self.api.get_period)(params, &mut period, &mut direction),
            )?;
            check(
                "hw_params_get_buffer_size",
                (self.api.get_buffer)(params, &mut buffer),
            )?;
        }
        validate_sizes(request, buffer, period, direction)
    }
    fn write(&mut self, bytes: &[u8], frames: usize) -> Result<Option<usize>, LinuxError> {
        // SAFETY: initialized interleaved storage covers exactly the requested
        // frames for the configured format and remains live throughout writei.
        let result =
            unsafe { (self.api.write)(self.handle, bytes.as_ptr().cast(), frames as Frames) };
        if result == -11 || result == -4 {
            return Ok(None);
        }
        if result < 0 {
            return Err(LinuxError::Alsa {
                operation: "snd_pcm_writei",
                code: result as i32,
            });
        }
        if result as usize > frames {
            return Err(LinuxError::InvalidConfiguration(
                "ALSA returned more written frames than submitted",
            ));
        }
        Ok(Some(result as usize))
    }
    fn wait(&mut self) -> Result<bool, LinuxError> {
        // SAFETY: valid worker-owned PCM. 20 ms timeout bounds stop observation.
        let result = unsafe { (self.api.wait)(self.handle, 20) };
        if result == -4 || result == -11 {
            return Ok(false);
        }
        check("snd_pcm_wait", result)?;
        Ok(true)
    }
    fn timing(
        &mut self,
        clock: &MonotonicClock,
        submitted: u64,
    ) -> Result<Option<AlsaTimingSnapshot>, LinuxError> {
        let started = clock.now()?;
        // SAFETY: live worker-owned PCM and preallocated matching status object.
        let result = unsafe { (self.api.status)(self.handle, self.native_status) };
        let finished = clock.now()?;
        if result == -11 || result == -4 {
            return Ok(None);
        }
        check("snd_pcm_status", result)?;
        let mut stamp = NativeTimespec {
            seconds: 0,
            nanoseconds: 0,
        };
        // SAFETY: successful status initialized its matching opaque object; all
        // getters use that same object. Timespec output has the gated LP64 ABI.
        let (state, delay, available) = unsafe {
            (self.api.status_htstamp)(self.native_status, &mut stamp);
            (
                (self.api.status_state)(self.native_status),
                (self.api.status_delay)(self.native_status),
                (self.api.status_avail)(self.native_status),
            )
        };
        Ok(Some(timing::interpret(
            state,
            submitted,
            delay as i64,
            available as u64,
            AlsaNativeTimestamp {
                seconds: stamp.seconds as i64,
                nanoseconds: stamp.nanoseconds as i64,
            },
            started,
            finished,
            self.timestamp_mode,
            self.timestamp_type,
        )))
    }
    fn drop_stream(&mut self) -> Result<(), LinuxError> {
        // SAFETY: worker owns a live PCM; drop terminates output, retaining handle.
        check("snd_pcm_drop", unsafe {
            (self.api.drop_stream)(self.handle)
        })
    }
}
impl Drop for NativePcm {
    fn drop(&mut self) {
        // SAFETY: worker owns handle exclusively; close precedes library unload.
        unsafe {
            if !self.native_status.is_null() {
                (self.api.status_free)(self.native_status);
            }
            (self.api.close)(self.handle);
        }
    }
}
fn check(operation: &'static str, code: i32) -> Result<(), LinuxError> {
    if code < 0 {
        Err(LinuxError::Alsa { operation, code })
    } else {
        Ok(())
    }
}
fn validate_sizes(
    request: &AlsaRequest,
    buffer: Frames,
    period: Frames,
    direction: i32,
) -> Result<(u32, u32), LinuxError> {
    let buffer = u32::try_from(buffer).map_err(|_| LinuxError::Overflow)?;
    let period = u32::try_from(period).map_err(|_| LinuxError::Overflow)?;
    if period == 0 || buffer <= period || direction != 0 {
        return Err(LinuxError::InvalidConfiguration(
            "ALSA period must be integral and smaller than buffer",
        ));
    }
    if !request.allow_size_rounding
        && (period != request.period_frames || buffer != request.buffer_frames)
    {
        return Err(LinuxError::SizeMismatch {
            requested_period: request.period_frames,
            applied_period: period,
            requested_buffer: request.buffer_frames,
            applied_buffer: buffer,
        });
    }
    Ok((buffer, period))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_filters_input_and_preserves_first_exact_output_name() {
        let mut rows = DiscoveryRows::new(4, 32).unwrap();
        rows.admit(None, None, Some(b"Input")).unwrap();
        rows.admit(
            Some(b"hw:CARD=Exact,DEV=1"),
            Some(b"Stereo\nOutput"),
            Some(b"Output"),
        )
        .unwrap();
        rows.admit(Some(b"default"), None, None).unwrap();
        rows.admit(Some(b"hw:CARD=Exact,DEV=1"), Some(b"duplicate"), None)
            .unwrap();
        assert_eq!(
            rows.devices,
            vec![
                AlsaDevice {
                    name: "hw:CARD=Exact,DEV=1".into(),
                    description: Some("Stereo\nOutput".into())
                },
                AlsaDevice {
                    name: "default".into(),
                    description: None
                },
            ]
        );
        assert_eq!(rows.scanned, 4);
    }

    #[test]
    fn discovery_rejects_caps_without_truncation_and_counts_input_hints() {
        for (devices, bytes) in [(0, 1), (4097, 1), (1, 0), (1, 16385)] {
            assert!(matches!(
                DiscoveryRows::new(devices, bytes),
                Err(LinuxError::InvalidConfiguration(_))
            ));
        }
        let mut rows = DiscoveryRows::new(1, 6).unwrap();
        rows.admit(None, None, Some(b"Input")).unwrap();
        assert!(matches!(
            rows.admit(Some(b"hw:0"), None, None),
            Err(LinuxError::InvalidConfiguration(_))
        ));
        assert!(rows.devices.is_empty());
        let mut rows = DiscoveryRows::new(2, 6).unwrap();
        rows.admit(Some(b"abcdef"), Some(b"123456"), None).unwrap();
        assert!(rows.admit(Some(b"abcdefg"), None, None).is_err());
        assert_eq!(rows.devices.len(), 1);
        assert_eq!(rows.devices[0].name, "abcdef");
    }

    #[test]
    fn discovery_rejects_malformed_native_text_and_unknown_direction() {
        for name in [&b""[..], &b"a\nb"[..], &b"a\0b"[..], &b"\xff"[..]] {
            let mut rows = DiscoveryRows::new(1, 32).unwrap();
            assert!(matches!(
                rows.admit(Some(name), None, None),
                Err(LinuxError::MalformedInput(_))
            ));
        }
        let mut rows = DiscoveryRows::new(1, 32).unwrap();
        assert!(rows.admit(Some(b"name"), Some(b"\xff"), None).is_err());
        let mut rows = DiscoveryRows::new(1, 32).unwrap();
        assert!(rows.admit(Some(b"name"), None, Some(b"Unknown")).is_err());
        let mut rows = DiscoveryRows::new(1, 32).unwrap();
        assert!(rows.admit(None, None, None).is_err());
    }

    #[test]
    fn native_hint_text_scan_honors_byte_boundary_and_empty_or_absent_values() {
        // Borrowed fixtures bypass Drop because these are not malloc allocations.
        let empty = std::mem::ManuallyDrop::new(HintText(c"".as_ptr().cast_mut()));
        assert_eq!(empty.bytes(1).unwrap(), Some(&b""[..]));
        let text = std::mem::ManuallyDrop::new(HintText(c"four".as_ptr().cast_mut()));
        assert_eq!(text.bytes(4).unwrap(), Some(&b"four"[..]));
        assert!(text.bytes(3).is_err());
        let absent = HintText(ptr::null_mut());
        assert_eq!(absent.bytes(1).unwrap(), None);
    }

    fn request() -> AlsaRequest {
        AlsaRequest {
            device: "explicit-endpoint".into(),
            format: DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap(),
            buffer_frames: 256,
            period_frames: 64,
            allow_size_rounding: false,
            monotonic_domain: ClockDomainId(1),
        }
    }

    #[test]
    fn unsupported_exact_sizing_returns_applied_suggestion_without_consent() {
        assert!(matches!(
            validate_sizes(&request(), 288, 72, 0),
            Err(LinuxError::SizeMismatch {
                requested_period: 64,
                applied_period: 72,
                requested_buffer: 256,
                applied_buffer: 288
            })
        ));
        let mut rounded = request();
        rounded.allow_size_rounding = true;
        assert_eq!(validate_sizes(&rounded, 288, 72, 0).unwrap(), (288, 72));
    }

    #[test]
    fn degenerate_sizing_is_rejected_even_with_rounding_permission() {
        let mut request = request();
        request.allow_size_rounding = true;
        assert!(validate_sizes(&request, 64, 64, 0).is_err());
        assert!(validate_sizes(&request, 256, 0, 0).is_err());
        assert!(validate_sizes(&request, 256, 64, 1).is_err());
    }

    #[test]
    fn real_mixer_reports_expose_execution_rejections_and_survive_stop() {
        use beatkernel::audio::{
            AudioCommand, AudioFormat, AudioLimits, MixerConfig, PcmLimits, PcmSample, SampleBank,
            SampleId, VoiceId, command_queue,
        };
        let requested = request();
        let shared = Arc::new(Shared::new());
        let mut stream = AlsaStream {
            configuration: AlsaAppliedConfig {
                format: requested.format,
                buffer_frames: requested.buffer_frames,
                period_frames: requested.period_frames,
                sizing_adjusted: false,
                output_domain: ClockDomainId(9),
                output_origin: Timestamp::ZERO,
                requested,
            },
            basis: beatkernel::audio::OutputFrameBasis::new(
                ClockPoint {
                    domain: ClockDomainId(9),
                    timestamp: Timestamp::ZERO,
                },
                48_000,
                0,
            )
            .unwrap(),
            shared: Arc::clone(&shared),
            worker: None, // Pure facade fixture: no native thread/PCM/device.
            recovered_mixer: None,
            retired: false,
        };
        assert_eq!(stream.last_render_report(), None);
        let format = AudioFormat::new(48_000, 2).unwrap();
        let pcm_limits = PcmLimits::new(1024, 4096, 4).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.5; 64], pcm_limits).unwrap(),
        )
        .unwrap();
        let limits = AudioLimits::new(8, 1, 8, 8, 8).unwrap();
        let (mut producer, consumer) = command_queue(8).unwrap();
        for (voice, sample) in [(1, 1), (2, 1), (3, 99)] {
            producer
                .try_push(AudioCommand::Play {
                    voice: VoiceId(voice),
                    sample: SampleId(sample),
                    at: Timestamp::ZERO,
                    gain: 1.0,
                })
                .unwrap();
        }
        let mut mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(9), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        let mut version = 0;
        let mut output = [0.0; 8];
        let first = render_and_publish(
            &mut mixer,
            &mut output,
            &shared.render_telemetry,
            &mut version,
        )
        .unwrap();
        assert_eq!(first.counters.voice_full, 1);
        assert_eq!(first.counters.unknown_samples, 1);
        assert_eq!(first.active_voices, 1);
        assert_eq!(first.frames, 4);
        assert_eq!(first.counters.rendered_frames, 4);
        assert_eq!(stream.last_render_report(), Some(first));
        assert!(output.iter().all(|sample| *sample == 0.5));
        assert!(!first.producer_disconnected);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::from_nanos(1),
                gain: 1.0,
            })
            .unwrap();
        drop(producer);
        let second = render_and_publish(
            &mut mixer,
            &mut output,
            &shared.render_telemetry,
            &mut version,
        )
        .unwrap();
        assert_eq!(second.start_frame, 4);
        assert_eq!(second.counters.rendered_frames, 8);
        assert_eq!(second.counters.late_commands, 1);
        assert_eq!(second.counters.voice_full, 1);
        assert!(second.producer_disconnected);
        assert_eq!(stream.last_render_report(), Some(second));
        let published_version = version;
        assert!(
            render_and_publish(
                &mut mixer,
                &mut [0.0; 18],
                &shared.render_telemetry,
                &mut version
            )
            .is_err()
        );
        assert_eq!(version, published_version);
        assert_eq!(stream.last_render_report(), Some(second));
        // The internal carrier exposes no clock or synthetic native counters.
        let carrier = shared.render_telemetry.read();
        assert_eq!(carrier.clock, None);
        assert_eq!(carrier.counters, StreamCounters::default());
        shared.errno.store(-32, Ordering::Release);
        shared.status.store(3, Ordering::Release);
        assert_eq!(stream.last_render_report(), Some(second));
        stream.stop().unwrap();
        assert_eq!(stream.last_render_report(), Some(second));
        assert_eq!(stream.timing_snapshot(), None);
    }
}

impl beatkernel::audio::StoppedMixerSource for AlsaStream {
    type Error = LinuxError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        if !self.retired || self.worker.is_some() {
            return Err(LinuxError::InvalidLifecycle);
        }
        Ok(self.recovered_mixer.take())
    }
}
#[cfg(test)]
#[path = "alsa/recovery_fixtures.rs"]
mod recovery_fixtures;

#[cfg(test)]
#[path = "alsa/frame_basis_fixtures.rs"]
mod frame_basis_fixtures;
