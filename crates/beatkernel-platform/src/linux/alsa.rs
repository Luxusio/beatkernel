//! Dedicated nonblocking ALSA PCM writer with explicit device and sizing.
#![allow(unsafe_code)]

use super::{LinuxError, MonotonicClock};
use crate::audio::{encode_pcm, DeviceFormat, SampleEncoding};
use beatkernel::{
    audio::Mixer,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString},
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

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
        }
    }
}

/// Owns the worker lifecycle; worker exclusively owns ALSA handles and Mixer.
pub struct AlsaStream {
    configuration: AlsaAppliedConfig,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<Result<(), LinuxError>>>,
}
impl AlsaStream {
    /// Opens/configures on the worker and waits for its actual startup result.
    /// Render/conversion storage is allocated before this method returns Ready.
    pub fn open(request: AlsaRequest, mixer: Mixer) -> Result<Self, LinuxError> {
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
                        return Ok(());
                    }
                };
                if sender.send(Ok(configuration.clone())).is_err() {
                    return Ok(());
                }
                drop(sender);
                while !worker_shared.start.load(Ordering::Acquire)
                    && !worker_shared.stop.load(Ordering::Acquire)
                {
                    thread::park();
                }
                if worker_shared.stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                worker_shared.status.store(1, Ordering::Release);
                let result = run_worker(
                    &mut pcm,
                    mixer,
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
                result
            })?;
        match receiver.recv() {
            Ok(Ok(configuration)) => Ok(Self {
                configuration,
                shared,
                worker: Some(worker),
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
            Ok(result) => {
                if result.is_ok() {
                    self.shared.status.store(2, Ordering::Release);
                }
                result
            }
            Err(_) => {
                self.shared.status.store(4, Ordering::Release);
                Err(LinuxError::WorkerPanicked)
            }
        }
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

fn increment(counter: &AtomicU64, amount: u64) {
    let old = counter.load(Ordering::Relaxed);
    counter.store(old.saturating_add(amount), Ordering::Relaxed);
}
fn run_worker(
    pcm: &mut NativePcm,
    mut mixer: Mixer,
    config: &AlsaAppliedConfig,
    render: &mut [f32],
    conversion: &mut [u8],
    shared: &Shared,
) -> Result<(), LinuxError> {
    let clock = MonotonicClock::new(config.requested.monotonic_domain);
    let mut pending_offset = config.period_frames as usize;
    let align = usize::from(config.format.block_align());
    while !shared.stop.load(Ordering::Acquire) {
        if pending_offset == config.period_frames as usize {
            mixer.render(render).map_err(LinuxError::Mixer)?;
            encode_pcm(config.format, render, conversion).map_err(LinuxError::Conversion)?;
            shared
                .rendered
                .store(mixer.frame_cursor(), Ordering::Relaxed);
            increment(&shared.renders, 1);
            pending_offset = 0;
        }
        match pcm.write(
            &conversion[pending_offset * align..],
            config.period_frames as usize - pending_offset,
        )? {
            Some(frames) if frames > 0 => {
                pending_offset += frames;
                increment(&shared.submitted, frames as u64);
            }
            _ => {
                pcm.wait()?;
            }
        }
        let point = clock.now()?;
        shared
            .observed
            .store(point.timestamp.as_nanos(), Ordering::Relaxed);
        shared.observed_valid.store(true, Ordering::Release);
    }
    pcm.drop_stream()
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
    sw_apply: unsafe extern "C" fn(Handle, Handle) -> c_int => c"snd_pcm_sw_params",
    prepare: unsafe extern "C" fn(Handle) -> c_int => c"snd_pcm_prepare",
    write: unsafe extern "C" fn(Handle, *const c_void, Frames) -> SignedFrames => c"snd_pcm_writei",
    wait: unsafe extern "C" fn(Handle, c_int) -> c_int => c"snd_pcm_wait",
    drop_stream: unsafe extern "C" fn(Handle) -> c_int => c"snd_pcm_drop",
}

struct NativePcm {
    handle: Handle,
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
        let pcm = Self { handle, api };
        let (buffer, period) = pcm.configure(request)?;
        Ok((pcm, buffer, period))
    }
    fn configure(&self, request: &AlsaRequest) -> Result<(u32, u32), LinuxError> {
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
                check("sw_params", (self.api.sw_apply)(self.handle, sw))?;
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
                ))
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
    fn wait(&mut self) -> Result<(), LinuxError> {
        // SAFETY: valid worker-owned PCM. 20 ms timeout bounds stop observation.
        let result = unsafe { (self.api.wait)(self.handle, 20) };
        if result == -4 {
            return Ok(());
        }
        check("snd_pcm_wait", result)
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
}
