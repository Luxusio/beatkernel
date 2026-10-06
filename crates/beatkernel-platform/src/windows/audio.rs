//! Caller-controlled native WASAPI streams with worker-owned COM resources.

#![allow(unsafe_code)]
// Preserve the portable fixed-size, allocation-free audio error representation.
#![allow(clippy::result_large_err)]

use super::clock::QpcClock;
use crate::audio::{
    telemetry::{observe_deadline, status_code, Telemetry},
    *,
};
use beatkernel::{
    audio::Mixer,
    time::{ClockMappingQuality, ClockPoint, Duration, Timestamp},
};
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration as StdDuration, Instant},
};
use windows::{
    core::{w, Interface, BSTR, GUID, HRESULT, PCWSTR},
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::{
            CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, WAIT_FAILED,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Media::Audio::*,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
                COINIT_MULTITHREADED, STGM_READ,
            },
            Threading::{
                AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
                AvSetMmThreadPriority, CreateEventW, GetCurrentProcess, SetEvent,
                WaitForMultipleObjects, AVRT_PRIORITY_CRITICAL, AVRT_PRIORITY_HIGH,
                AVRT_PRIORITY_LOW, AVRT_PRIORITY_NORMAL,
            },
        },
    },
};

const E_FAIL_CODE: i32 = 0x8000_4005u32 as i32;
const E_INVALIDARG_CODE: i32 = 0x8007_0057u32 as i32;
const PCM_GUID: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const FLOAT_GUID: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// MMCSS priority selected explicitly outside buffer filling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasapiPriority {
    /// Low multimedia priority.
    Low,
    /// Normal multimedia priority.
    Normal,
    /// High multimedia priority.
    High,
    /// Critical multimedia priority.
    Critical,
}

/// Explicit native wake policy; no implicit event/timer substitution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasapiWakePolicy {
    /// Native event-driven filling (required for exclusive streams).
    EventDriven,
    /// Shared polling with a positive whole-millisecond interval.
    Timer {
        /// Caller-controlled polling interval.
        poll_interval: Duration,
    },
}

/// Caller-controlled worker scheduling and native wake options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasapiOptions {
    /// Optional MMCSS registration; None explicitly disables it.
    pub mmcss_priority: Option<WasapiPriority>,
    /// Event-driven by default; timer mode must be explicitly selected.
    pub wake_policy: WasapiWakePolicy,
}

impl Default for WasapiOptions {
    fn default() -> Self {
        Self {
            mmcss_priority: Some(WasapiPriority::Normal),
            wake_policy: WasapiWakePolicy::EventDriven,
        }
    }
}

/// Native Windows Audio Session API device/probe boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct WasapiBackend;

impl AudioOutputBackend for WasapiBackend {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioPlatformError> {
        let _apartment = Apartment::new()?;
        let enumerator = enumerator()?;
        let mut defaults = [None, None, None];
        for (index, role) in [eConsole, eMultimedia, eCommunications]
            .into_iter()
            .enumerate()
        {
            // SAFETY: COM apartment and enumerator are live on this thread.
            match unsafe { enumerator.GetDefaultAudioEndpoint(eRender, role) } {
                Ok(device) => defaults[index] = Some(device_id(&device)?),
                Err(error) if error.code().0 == 0x8007_0490u32 as i32 => {}
                Err(error) => return Err(native(error.code().0)),
            }
        }
        // SAFETY: native enumerator returns a live collection of all states.
        let collection =
            unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE(DEVICE_STATEMASK_ALL)) }
                .map_err(win_error)?;
        // SAFETY: live collection, scalar result owned by caller.
        let count = unsafe { collection.GetCount() }.map_err(win_error)?;
        let mut devices = Vec::new();
        devices
            .try_reserve_exact(count as usize)
            .map_err(|_| AudioPlatformError::Capacity)?;
        for index in 0..count {
            // SAFETY: index is within native collection count.
            let device = unsafe { collection.Item(index) }.map_err(win_error)?;
            let id = device_id(&device)?;
            // SAFETY: device is live; property variant and BSTR are RAII-owned.
            let properties = unsafe { device.OpenPropertyStore(STGM_READ) }.map_err(win_error)?;
            // SAFETY: property key is constant and result owns its storage.
            let value =
                unsafe { properties.GetValue(&PKEY_Device_FriendlyName) }.map_err(win_error)?;
            let name = BSTR::try_from(&value).map_err(win_error)?.to_string();
            // SAFETY: live endpoint state query on the owning COM thread.
            let state = unsafe { device.GetState() }.map_err(win_error)?;
            let state = match state {
                DEVICE_STATE_ACTIVE => AudioDeviceState::Active,
                DEVICE_STATE_DISABLED => AudioDeviceState::Disabled,
                DEVICE_STATE_UNPLUGGED => AudioDeviceState::Unplugged,
                DEVICE_STATE_NOTPRESENT => AudioDeviceState::NotPresent,
                _ => return Err(AudioPlatformError::DeviceUnavailable),
            };
            devices.push(AudioDevice {
                default_console: defaults[0].as_ref() == Some(&id),
                default_multimedia: defaults[1].as_ref() == Some(&id),
                default_communications: defaults[2].as_ref() == Some(&id),
                id: AudioDeviceId(id),
                name,
                state,
            });
        }
        Ok(devices)
    }

    fn mix_format(&self, device: &AudioDeviceId) -> Result<DeviceFormat, AudioPlatformError> {
        let _apartment = Apartment::new()?;
        let client = activate(&endpoint(device)?)?;
        // SAFETY: live client returns a CoTaskMem-owned WAVEFORMAT allocation.
        let allocation = CoMemory(unsafe { client.GetMixFormat() }.map_err(win_error)?.cast());
        // SAFETY: allocation comes from GetMixFormat, guaranteeing its declared extent.
        unsafe { read_format(allocation.0.cast()) }
    }

    fn supports_format(
        &self,
        device: &AudioDeviceId,
        mode: AudioStreamMode,
        format: DeviceFormat,
    ) -> Result<FormatSupport, AudioPlatformError> {
        let _apartment = Apartment::new()?;
        probe(&activate(&endpoint(device)?)?, mode, format)
    }

    fn period_constraints(
        &self,
        device: &AudioDeviceId,
        mode: AudioStreamMode,
        format: DeviceFormat,
    ) -> Result<PeriodConstraints, AudioPlatformError> {
        let _apartment = Apartment::new()?;
        constraints(&activate(&endpoint(device)?)?, mode, format, true)
    }
}

impl WasapiBackend {
    /// Opens the exact requested endpoint/mode, validates and primes its buffer.
    ///
    /// Mixer ownership transfers to a worker. Errors clean up that worker's
    /// partial resources before returning. Start is a separate acknowledged step.
    pub fn open(
        &self,
        request: AudioStreamRequest,
        mixer: Mixer,
        clock: QpcClock,
        options: WasapiOptions,
    ) -> Result<WasapiStream, AudioPlatformError> {
        self.open_recoverable(request, mixer, clock, options)
            .map_err(|failure| failure.into_parts().0)
    }
    /// Retains recoverable software ownership on preflight/spawn/setup refusal.
    /// Native resources retire on the worker before a joined failure is returned.
    pub fn open_recoverable(
        &self,
        request: AudioStreamRequest,
        mixer: Mixer,
        clock: QpcClock,
        options: WasapiOptions,
    ) -> Result<WasapiStream, beatkernel::audio::MixerOpenFailure<AudioPlatformError>> {
        let basis = mixer.output_frame_basis();
        if let Err(error) = validate_open(&request, &mixer, options) {
            return Err(beatkernel::audio::MixerOpenFailure::new(error, Some(mixer)));
        }
        let control = Arc::new(Control {
            request: AtomicU8::new(0),
            telemetry: Telemetry::new(),
            cadence: crate::audio::cadence::Capture::new(),
        });
        let worker_control = Arc::clone(&control);
        let (opened_tx, opened_rx) = mpsc::sync_channel(1);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let worker =
            crate::audio::mixer_launch::launch_worker(NativeWorkerSpawner, mixer, move |mixer| {
                let mut owned = Some(mixer);
                // Setup result owns all partial COM/native resources locally;
                // an error has dropped them before the failure is sent.
                let setup = Worker::new(request, &mut owned, clock, options);
                let mut worker = match setup {
                    Ok(worker) => worker,
                    Err(error) => {
                        let _ = opened_tx.send(Err(error));
                        return owned;
                    }
                };
                worker.publish(&worker_control.telemetry);
                let duplicate = match worker.control_event.duplicate() {
                    Ok(event) => event,
                    Err(error) => {
                        let mixer = worker.mixer.take();
                        drop(worker);
                        let _ = opened_tx.send(Err(error));
                        return mixer;
                    }
                };
                if opened_tx
                    .send(Ok((worker.configuration.clone(), duplicate)))
                    .is_err()
                {
                    return worker.mixer.take();
                }
                worker.run(&worker_control, &started_tx);
                worker.mixer.take()
            })
            .map_err(|failure| {
                let (_, mixer) = failure.into_parts();
                beatkernel::audio::MixerOpenFailure::new(AudioPlatformError::WorkerFailure, mixer)
            })?;
        match opened_rx.recv() {
            Ok(Ok((configuration, wake))) => Ok(WasapiStream {
                configuration,
                basis,
                options,
                control,
                wake,
                worker: Some(worker),
                recovered_mixer: None,
                retired: false,
                started: started_rx,
                has_started: false,
            }),
            Ok(Err(error)) => Err(crate::audio::mixer_launch::join_open_failure(
                worker,
                error,
                |mixer| mixer,
            )),
            Err(_) => Err(crate::audio::mixer_launch::join_open_failure(
                worker,
                AudioPlatformError::WorkerFailure,
                |mixer| mixer,
            )),
        }
    }
}

fn validate_open(
    request: &AudioStreamRequest,
    mixer: &Mixer,
    options: WasapiOptions,
) -> Result<(), AudioPlatformError> {
    if request.backend() == AudioBackendKind::Asio {
        return Err(AudioPlatformError::BackendUnavailable(
            AudioBackendKind::Asio,
        ));
    }
    validate_options(request, options)?;
    if mixer.config().format() != request.format().pcm() {
        return Err(AudioPlatformError::InvalidFormat);
    }
    Ok(())
}
struct NativeWorkerSpawner;
impl crate::audio::mixer_launch::WorkerSpawner<Option<Mixer>> for NativeWorkerSpawner {
    fn spawn<F>(self, work: F) -> std::io::Result<JoinHandle<Option<Mixer>>>
    where
        F: FnOnce() -> Option<Mixer> + Send + 'static,
    {
        thread::Builder::new()
            .name("beatkernel-wasapi".into())
            .spawn(work)
    }
}

/// A native stream whose terminal stop joins the worker before releasing assets.
pub struct WasapiStream {
    basis: beatkernel::audio::OutputFrameBasis,
    configuration: AppliedStreamConfig,
    options: WasapiOptions,
    control: Arc<Control>,
    wake: Event,
    worker: Option<JoinHandle<Option<Mixer>>>,
    recovered_mixer: Option<Mixer>,
    retired: bool,
    started: mpsc::Receiver<Result<(), i32>>,
    has_started: bool,
}

impl WasapiStream {
    /// Original mixer grid captured before worker priming or native setup.
    pub const fn frame_basis(&self) -> beatkernel::audio::OutputFrameBasis {
        self.basis
    }
    /// Unmodified explicit worker scheduling/wake options.
    pub const fn options(&self) -> WasapiOptions {
        self.options
    }
}

impl AudioOutputStream for WasapiStream {
    fn configuration(&self) -> &AppliedStreamConfig {
        &self.configuration
    }

    fn snapshot(&self) -> AudioStreamSnapshot {
        self.control.telemetry.read()
    }

    fn render_cadence(
        &self,
    ) -> Result<
        Option<crate::audio::cadence::RenderCadence>,
        crate::audio::cadence::RenderCadenceError,
    > {
        // Joining the sole writer also retains its prefix after native failure.
        // Live snapshots cannot establish that this immutable prefix is drained.
        if self.worker.is_some() {
            Ok(None)
        } else {
            self.control
                .cadence
                .summary(self.configuration.format.sample_rate())
                .map(Some)
        }
    }

    fn start(&mut self) -> Result<(), AudioPlatformError> {
        if self.worker.is_none() {
            return Err(AudioPlatformError::WorkerFailure);
        }
        if self.has_started {
            return match self.snapshot().status {
                AudioStreamStatus::Running => Ok(()),
                AudioStreamStatus::Failed { hresult } => Err(native(hresult)),
                _ => Err(AudioPlatformError::WorkerFailure),
            };
        }
        self.control.request.store(1, Ordering::Release);
        let signal_error = self.wake.signal().err();
        // Worker checks atomics at least every 50 ms even if event signaling
        // fails. This acknowledgment wait occurs outside buffer filling.
        match self.started.recv_timeout(StdDuration::from_secs(5)) {
            Ok(Ok(())) => {
                self.has_started = true;
                if let Some(error) = signal_error {
                    let _ = self.stop();
                    Err(error)
                } else {
                    Ok(())
                }
            }
            Ok(Err(code)) => {
                // The negative acknowledgment precedes owner-thread teardown.
                // Join before exposing its original HRESULT to the caller.
                let _ = self.stop();
                if self.snapshot().status == AudioStreamStatus::WorkerPanicked {
                    Err(AudioPlatformError::WorkerFailure)
                } else {
                    Err(native(code))
                }
            }
            Err(_) => {
                let _ = self.stop();
                Err(AudioPlatformError::WorkerFailure)
            }
        }
    }

    fn stop(&mut self) -> Result<(), AudioPlatformError> {
        let Some(worker) = self.worker.take() else {
            return match self.snapshot().status {
                AudioStreamStatus::Failed { hresult } => Err(native(hresult)),
                AudioStreamStatus::WorkerPanicked => Err(AudioPlatformError::WorkerFailure),
                _ => Ok(()),
            };
        };
        self.control.request.store(2, Ordering::Release);
        let signal_error = self.wake.signal().err();
        match worker.join() {
            Ok(mixer) => {
                self.recovered_mixer = mixer;
                self.retired = true;
            }
            Err(_) => {
                self.control.telemetry.status.store(
                    status_code(AudioStreamStatus::WorkerPanicked),
                    Ordering::SeqCst,
                );
                return Err(AudioPlatformError::WorkerFailure);
            }
        }
        if let Some(error) = signal_error {
            return Err(error);
        }
        match self.snapshot().status {
            AudioStreamStatus::Failed { hresult } => Err(native(hresult)),
            _ => Ok(()),
        }
    }
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct Apartment(bool);
impl Apartment {
    fn new() -> Result<Self, AudioPlatformError> {
        // SAFETY: initializes COM for this thread; every success is balanced.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self(true))
        } else if result.0 == 0x8001_0106u32 as i32 {
            // Caller already has a different apartment. Probe objects remain
            // on that caller thread; do not uninitialize its apartment.
            Ok(Self(false))
        } else {
            Err(native(result.0))
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: exactly balances this thread's successful COM init.
            unsafe { CoUninitialize() };
        }
    }
}

struct CoMemory(*mut std::ffi::c_void);
impl Drop for CoMemory {
    fn drop(&mut self) {
        // SAFETY: pointer is a CoTaskMem-owned result (or null), freed once.
        unsafe { CoTaskMemFree(Some(self.0)) };
    }
}

// Store the opaque kernel handle as a scalar so a duplicated control handle can
// transfer through the startup handshake. Each Event owns one distinct handle;
// the worker's original is never closed by the caller's duplicate.
struct Event(isize);
impl Event {
    fn new() -> Result<Self, AudioPlatformError> {
        // SAFETY: creates an unnamed auto-reset event; no borrowed inputs retained.
        let handle = unsafe { CreateEventW(None, false, false, None) }.map_err(win_error)?;
        Ok(Self(handle.0 as isize))
    }
    fn handle(&self) -> HANDLE {
        HANDLE(self.0 as *mut std::ffi::c_void)
    }
    fn signal(&self) -> Result<(), AudioPlatformError> {
        // SAFETY: this owned handle remains live for the synchronous signal.
        unsafe { SetEvent(self.handle()) }.map_err(win_error)
    }
    fn duplicate(&self) -> Result<Self, AudioPlatformError> {
        let mut duplicated = HANDLE::default();
        // SAFETY: process pseudo-handle and source event are live; returned
        // duplicate is independently owned and never aliases handle ownership.
        unsafe {
            let process = GetCurrentProcess();
            DuplicateHandle(
                process,
                self.handle(),
                process,
                &mut duplicated,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }
        .map_err(win_error)?;
        Ok(Self(duplicated.0 as isize))
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: closes this distinct, owned kernel handle exactly once.
        let _ = unsafe { CloseHandle(self.handle()) };
    }
}

struct Mmcss(Option<HANDLE>);
impl Mmcss {
    fn new(priority: WasapiPriority) -> Result<Self, AudioPlatformError> {
        let mut index = 0;
        // SAFETY: registration and teardown occur on this owning worker thread.
        let registration = Self(Some(
            unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut index) }
                .map_err(win_error)?,
        ));
        let priority = match priority {
            WasapiPriority::Low => AVRT_PRIORITY_LOW,
            WasapiPriority::Normal => AVRT_PRIORITY_NORMAL,
            WasapiPriority::High => AVRT_PRIORITY_HIGH,
            WasapiPriority::Critical => AVRT_PRIORITY_CRITICAL,
        };
        // SAFETY: live registration for the current worker thread.
        unsafe {
            AvSetMmThreadPriority(registration.0.expect("live MMCSS registration"), priority)
        }
        .map_err(win_error)?;
        Ok(registration)
    }
    fn revert(&mut self) -> Result<(), i32> {
        if let Some(handle) = self.0.take() {
            // SAFETY: owned registration, reverted once on its worker thread.
            unsafe { AvRevertMmThreadCharacteristics(handle) }.map_err(|error| error.code().0)?;
        }
        Ok(())
    }
}
impl Drop for Mmcss {
    fn drop(&mut self) {
        // SAFETY: live registration is reverted once on its owning thread.
        let _ = self.revert();
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator, AudioPlatformError> {
    // SAFETY: caller owns an initialized COM apartment; interface is local.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(win_error)
}

fn endpoint(id: &AudioDeviceId) -> Result<IMMDevice, AudioPlatformError> {
    if id.0.is_empty() || id.0.contains('\0') {
        return Err(AudioPlatformError::InvalidRequest);
    }
    let mut wide: Vec<u16> = id.0.encode_utf16().collect();
    wide.push(0);
    // SAFETY: wide is null terminated and retained through this synchronous call.
    let device = unsafe { enumerator()?.GetDevice(PCWSTR(wide.as_ptr())) }.map_err(|error| {
        if error.code().0 == 0x8007_0490u32 as i32 {
            AudioPlatformError::DeviceUnavailable
        } else {
            native(error.code().0)
        }
    })?;
    // SAFETY: live endpoint state result is a scalar.
    if unsafe { device.GetState() }.map_err(win_error)? != DEVICE_STATE_ACTIVE {
        return Err(AudioPlatformError::DeviceUnavailable);
    }
    Ok(device)
}

fn device_id(device: &IMMDevice) -> Result<String, AudioPlatformError> {
    // SAFETY: GetId returns null-terminated, CoTaskMem-owned UTF16.
    let pointer = unsafe { device.GetId() }.map_err(win_error)?;
    let allocation = CoMemory(pointer.0.cast());
    if allocation.0.is_null() {
        return Err(AudioPlatformError::DeviceUnavailable);
    }
    // SAFETY: native GetId guarantees a null-terminated UTF16 allocation.
    unsafe { pointer.to_string() }.map_err(|_| AudioPlatformError::DeviceUnavailable)
}

fn activate(device: &IMMDevice) -> Result<IAudioClient, AudioPlatformError> {
    // SAFETY: live endpoint and apartment; interface stays on this thread.
    unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(win_error)
}

fn wave_format(format: DeviceFormat) -> WAVEFORMATEXTENSIBLE {
    let (bits, valid_bits, tag, guid) = match format.encoding() {
        SampleEncoding::Float32 => (32, 32, 3, FLOAT_GUID),
        SampleEncoding::Pcm {
            container_bits,
            valid_bits,
        } => (container_bits, valid_bits, 1, PCM_GUID),
    };
    let extensible = format.channel_mask().is_some() || bits != valid_bits;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: if extensible { 0xfffe } else { tag },
            nChannels: format.channels(),
            nSamplesPerSec: format.sample_rate(),
            nAvgBytesPerSec: format.bytes_per_second(),
            nBlockAlign: format.block_align(),
            wBitsPerSample: bits,
            cbSize: if extensible { 22 } else { 0 },
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: valid_bits,
        },
        dwChannelMask: format.channel_mask().unwrap_or(0),
        SubFormat: guid,
    }
}

// SAFETY: pointer must be a native format allocation whose physical extent
// includes WAVEFORMATEX and its declared cbSize. GetMixFormat/closest-match APIs
// provide this allocation contract. Packed structures are copied unaligned.
unsafe fn read_format(pointer: *const WAVEFORMATEX) -> Result<DeviceFormat, AudioPlatformError> {
    if pointer.is_null() {
        return Err(AudioPlatformError::InvalidFormat);
    }
    // SAFETY: caller supplies the native WAVEFORMATEX allocation contract.
    let base = unsafe { pointer.read_unaligned() };
    let (tag, valid, mask) = if base.wFormatTag == 0xfffe {
        if base.cbSize < 22 {
            return Err(AudioPlatformError::InvalidFormat);
        }
        // SAFETY: validated cbSize guarantees at least the 22-byte extension.
        let extended = unsafe { pointer.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned() };
        // SAFETY: wValidBitsPerSample is the active member for uncompressed PCM.
        let valid = unsafe { extended.Samples.wValidBitsPerSample };
        let subformat = extended.SubFormat;
        let tag = if subformat == PCM_GUID {
            1
        } else if subformat == FLOAT_GUID {
            3
        } else {
            return Err(AudioPlatformError::InvalidFormat);
        };
        (tag, valid, Some(extended.dwChannelMask))
    } else {
        (base.wFormatTag, base.wBitsPerSample, None)
    };
    let encoding = match tag {
        1 => SampleEncoding::Pcm {
            container_bits: base.wBitsPerSample,
            valid_bits: valid,
        },
        3 if base.wBitsPerSample == 32 && valid == 32 => SampleEncoding::Float32,
        _ => return Err(AudioPlatformError::InvalidFormat),
    };
    let format = DeviceFormat::new(base.nSamplesPerSec, base.nChannels, encoding, mask)?;
    if base.nBlockAlign != format.block_align() || base.nAvgBytesPerSec != format.bytes_per_second()
    {
        return Err(AudioPlatformError::InvalidFormat);
    }
    Ok(format)
}

fn share_mode(mode: AudioStreamMode) -> AUDCLNT_SHAREMODE {
    if mode == AudioStreamMode::Exclusive {
        AUDCLNT_SHAREMODE_EXCLUSIVE
    } else {
        AUDCLNT_SHAREMODE_SHARED
    }
}

fn probe(
    client: &IAudioClient,
    mode: AudioStreamMode,
    format: DeviceFormat,
) -> Result<FormatSupport, AudioPlatformError> {
    let wave = wave_format(format);
    let mut closest = std::ptr::null_mut();
    // SAFETY: live client and full packed format remain valid for this call.
    // Exclusive does not accept a closest-format output pointer.
    let result = unsafe {
        client.IsFormatSupported(
            share_mode(mode),
            &wave.Format,
            (mode != AudioStreamMode::Exclusive).then_some(&mut closest),
        )
    };
    let allocation = CoMemory(closest.cast());
    if result.0 == 0 {
        return Ok(FormatSupport::Exact);
    }
    if result.0 == 1 || result == AUDCLNT_E_UNSUPPORTED_FORMAT {
        let closest = if allocation.0.is_null() {
            None
        } else {
            // SAFETY: IsFormatSupported owns a complete declared format allocation.
            Some(unsafe { read_format(allocation.0.cast()) }?)
        };
        return Ok(FormatSupport::Unsupported { closest });
    }
    Err(native(result.0))
}

fn constraints(
    client: &IAudioClient,
    mode: AudioStreamMode,
    format: DeviceFormat,
    event_driven: bool,
) -> Result<PeriodConstraints, AudioPlatformError> {
    let mut default = 0;
    let mut minimum = 0;
    // SAFETY: live client writes two synchronous scalar outputs.
    unsafe { client.GetDevicePeriod(Some(&mut default), Some(&mut minimum)) }.map_err(win_error)?;
    if default <= 0 || minimum <= 0 {
        return Err(AudioPlatformError::InvalidRequest);
    }
    let mut result = PeriodConstraints {
        default_period: Some(Duration::from_nanos(
            default
                .checked_mul(100)
                .ok_or(AudioPlatformError::InvalidRequest)?,
        )),
        min_period: (mode == AudioStreamMode::Exclusive).then_some(Duration::from_nanos(
            minimum
                .checked_mul(100)
                .ok_or(AudioPlatformError::InvalidRequest)?,
        )),
        ..PeriodConstraints::default()
    };
    if mode == AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod) {
        let client3: IAudioClient3 = client.cast().map_err(|_| {
            configuration_error(
                ConfigurationConstraint::EnginePeriodUnavailable,
                result,
                None,
            )
        })?;
        let wave = wave_format(format);
        let (mut default, mut fundamental, mut minimum, mut maximum) = (0, 0, 0, 0);
        // SAFETY: valid format and live client3, synchronous scalar outputs.
        unsafe {
            client3.GetSharedModeEnginePeriod(
                &wave.Format,
                &mut default,
                &mut fundamental,
                &mut minimum,
                &mut maximum,
            )
        }
        .map_err(win_error)?;
        if default == 0 || fundamental == 0 || minimum == 0 || maximum < minimum {
            return Err(AudioPlatformError::InvalidRequest);
        }
        result.default_frames = Some(default);
        result.default_period = Some(frame_duration(default, format.sample_rate()));
        result.min_frames = Some(minimum);
        result.max_frames = Some(maximum);
        result.fundamental_frames = Some(fundamental);
    }
    if let Ok(client2) = client.cast::<IAudioClient2>() {
        let wave = wave_format(format);
        let (mut minimum, mut maximum) = (0, 0);
        // SAFETY: live optional interface, format and scalar outputs.
        match unsafe {
            client2.GetBufferSizeLimits(&wave.Format, event_driven, &mut minimum, &mut maximum)
        } {
            Ok(()) if minimum >= 0 && maximum >= minimum => {
                result.min_buffer_duration = minimum.checked_mul(100).map(Duration::from_nanos);
                result.max_buffer_duration = maximum.checked_mul(100).map(Duration::from_nanos);
                result.buffer_bounds_event_driven = Some(event_driven);
            }
            Err(error) if error.code() == AUDCLNT_E_DEVICE_INVALIDATED => {
                return Err(AudioPlatformError::DeviceInvalidated);
            }
            _ => {}
        }
    }
    Ok(result)
}

fn validate_options(
    request: &AudioStreamRequest,
    options: WasapiOptions,
) -> Result<(), AudioPlatformError> {
    if let WasapiWakePolicy::Timer { poll_interval } = options.wake_policy {
        if request.mode() == AudioStreamMode::Exclusive {
            return Err(configuration_error(
                ConfigurationConstraint::TimerRequiresShared,
                PeriodConstraints::default(),
                None,
            ));
        }
        let nanos = poll_interval.as_nanos();
        if nanos <= 0 || nanos % 1_000_000 != 0 || nanos / 1_000_000 > i64::from(u32::MAX) {
            return Err(AudioPlatformError::InvalidRequest);
        }
    }
    Ok(())
}

fn frame_duration(frames: u32, rate: u32) -> Duration {
    Duration::from_nanos((u64::from(frames) * 1_000_000_000).div_ceil(u64::from(rate)) as i64)
}

fn native_duration(frames: u32, rate: u32) -> i64 {
    // Nearest 100ns duration is WASAPI's frame-alignment retry conversion.
    ((u64::from(frames) * 10_000_000 + u64::from(rate) / 2) / u64::from(rate)) as i64
}

fn win_error(error: windows::core::Error) -> AudioPlatformError {
    native(error.code().0)
}

fn native(code: i32) -> AudioPlatformError {
    match HRESULT(code) {
        AUDCLNT_E_DEVICE_INVALIDATED => AudioPlatformError::DeviceInvalidated,
        AUDCLNT_E_DEVICE_IN_USE => AudioPlatformError::EndpointBusy,
        AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED => AudioPlatformError::ExclusiveDisabled,
        AUDCLNT_E_UNSUPPORTED_FORMAT => AudioPlatformError::FormatUnsupported { closest: None },
        _ => AudioPlatformError::Native { code },
    }
}

fn configuration_error(
    constraint: ConfigurationConstraint,
    constraints: PeriodConstraints,
    frames: Option<u32>,
) -> AudioPlatformError {
    AudioPlatformError::ConfigurationUnsupported {
        constraint,
        constraints,
        suggested_buffer_frames: frames,
        suggested_period_frames: frames,
    }
}

fn validate_native_buffer_size(
    request: &AudioStreamRequest,
    actual_frames: u32,
    reported: PeriodConstraints,
) -> Result<bool, AudioPlatformError> {
    validate_buffer_size(request, actual_frames).map_err(|error| match error {
        AudioPlatformError::ConfigurationUnsupported {
            constraint,
            suggested_buffer_frames,
            suggested_period_frames,
            ..
        } => AudioPlatformError::ConfigurationUnsupported {
            constraint,
            constraints: reported,
            suggested_buffer_frames,
            suggested_period_frames,
        },
        other => other,
    })
}

struct Worker {
    client: IAudioClient,
    renderer: IAudioRenderClient,
    audio_clock: IAudioClock,
    render_event: Event,
    control_event: Event,
    mixer: Option<Mixer>,
    scratch: Vec<f32>,
    configuration: AppliedStreamConfig,
    options: WasapiOptions,
    clock: QpcClock,
    clock_frequency: u64,
    snapshot: AudioStreamSnapshot,
    version: u64,
    last_qpc: Option<u64>,
    _mmcss: Option<Mmcss>,
    // Declared last: interfaces/registration drop before CoUninitialize.
    _apartment: Apartment,
}

impl Worker {
    fn new(
        request: AudioStreamRequest,
        mixer: &mut Option<Mixer>,
        clock: QpcClock,
        options: WasapiOptions,
    ) -> Result<Self, AudioPlatformError> {
        let mixer_config = mixer
            .as_ref()
            .ok_or(AudioPlatformError::RecoveryUnavailable)?
            .config();
        let apartment = Apartment::new()?;
        let device = endpoint(request.device())?;
        let render_event = Event::new()?;
        let mut client = activate(&device)?;
        if let FormatSupport::Unsupported { closest } =
            probe(&client, request.mode(), request.format())?
        {
            return Err(AudioPlatformError::FormatUnsupported { closest });
        }
        let event_driven = options.wake_policy == WasapiWakePolicy::EventDriven;
        let reported = constraints(&client, request.mode(), request.format(), event_driven)?;
        let resolved = resolve_period(&request, reported)?;
        let flags = if event_driven {
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        } else {
            0
        };
        let wave = wave_format(request.format());
        let mut period_frames = None;
        let mut period_duration = reported
            .default_period
            .ok_or(AudioPlatformError::InvalidRequest)?;
        let mut adjusted = resolved.adjusted;
        if request.mode() == AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod) {
            let client3: IAudioClient3 = client.cast().map_err(|_| {
                configuration_error(
                    ConfigurationConstraint::EnginePeriodUnavailable,
                    reported,
                    None,
                )
            })?;
            // SAFETY: owning COM thread, full format and scalar initialization;
            // generated error allocation is outside the fill boundary.
            unsafe {
                client3.InitializeSharedAudioStream(flags, resolved.frames, &wave.Format, None)
            }
            .map_err(win_error)?;
            if event_driven {
                // SAFETY: register immediately after successful initialization,
                // before any fallible configuration checks or interface release.
                unsafe { client.SetEventHandle(render_event.handle()) }.map_err(win_error)?;
            }
            let mut actual_format = std::ptr::null_mut();
            let mut actual_period = 0;
            // SAFETY: live initialized interface and synchronous outputs.
            unsafe {
                client3.GetCurrentSharedModeEnginePeriod(&mut actual_format, &mut actual_period)
            }
            .map_err(win_error)?;
            let allocation = CoMemory(actual_format.cast());
            // SAFETY: GetCurrentSharedModeEnginePeriod supplies declared format allocation.
            let engine_format = unsafe { read_format(allocation.0.cast()) }?;
            if actual_period == 0 || engine_format.sample_rate() != request.format().sample_rate() {
                return Err(AudioPlatformError::InvalidFormat);
            }
            if actual_period != resolved.frames {
                if request.negotiation() == NegotiationPolicy::Exact {
                    return Err(configuration_error(
                        ConfigurationConstraint::PeriodBounds,
                        reported,
                        Some(actual_period),
                    ));
                }
                adjusted = true;
            }
            period_frames = Some(actual_period);
            period_duration = frame_duration(actual_period, request.format().sample_rate());
        } else {
            let (buffer_duration, periodicity) = if request.mode() == AudioStreamMode::Exclusive {
                let value = native_duration(resolved.frames, request.format().sample_rate());
                period_frames = Some(resolved.frames);
                period_duration = Duration::from_nanos(value * 100);
                (value, value)
            } else if event_driven {
                // Microsoft's SHARED EVENTCALLBACK contract requires BOTH zero.
                (0, 0)
            } else {
                let value = match request.buffer() {
                    BufferRequest::DeviceDefault => 0,
                    BufferRequest::Frames(frames) => {
                        native_duration(frames, request.format().sample_rate())
                    }
                    BufferRequest::Duration(duration) => {
                        ((i128::from(duration.as_nanos()) + 99) / 100) as i64
                    }
                };
                (value, 0)
            };
            // SAFETY: worker owns live COM client; packed format is complete.
            let initialize = unsafe {
                (client.vtable().Initialize)(
                    client.as_raw(),
                    share_mode(request.mode()),
                    flags,
                    buffer_duration,
                    periodicity,
                    &wave.Format,
                    std::ptr::null(),
                )
            };
            if initialize == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
                && request.mode() == AudioStreamMode::Exclusive
            {
                // SAFETY: WASAPI explicitly permits GetBufferSize after this alignment failure.
                let aligned = unsafe { client.GetBufferSize() }.map_err(win_error)?;
                if aligned == 0 {
                    return Err(AudioPlatformError::InvalidRequest);
                }
                if request.negotiation() == NegotiationPolicy::Exact {
                    return Err(configuration_error(
                        ConfigurationConstraint::BufferAlignment,
                        reported,
                        Some(aligned),
                    ));
                }
                // Failed initialization must be released before reactivation.
                drop(client);
                client = activate(&device)?;
                let value = native_duration(aligned, request.format().sample_rate());
                // SAFETY: fresh client, caller explicitly allowed one alignment retry.
                unsafe {
                    client.Initialize(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        flags,
                        value,
                        value,
                        &wave.Format,
                        None,
                    )
                }
                .map_err(win_error)?;
                period_frames = Some(aligned);
                period_duration = Duration::from_nanos(value * 100);
                adjusted = true;
            } else if initialize.is_err() {
                return Err(native(initialize.0));
            }
            if event_driven {
                // SAFETY: successfully initialized client receives its live
                // worker event before any later configuration rejection.
                unsafe { client.SetEventHandle(render_event.handle()) }.map_err(win_error)?;
            }
        }
        // SAFETY: client initialized on this thread; native scalar configuration.
        let buffer_frames = unsafe { client.GetBufferSize() }.map_err(win_error)?;
        adjusted |= validate_native_buffer_size(&request, buffer_frames, reported)?;
        if buffer_frames as usize > mixer_config.limits().max_render_frames() {
            return Err(AudioPlatformError::Capacity);
        }
        if request.mode() == AudioStreamMode::Exclusive && period_frames != Some(buffer_frames) {
            return Err(configuration_error(
                ConfigurationConstraint::ExclusiveBufferEqualsPeriod,
                reported,
                Some(buffer_frames),
            ));
        }
        // SAFETY: initialized client returns latency in 100ns units.
        let latency = unsafe { client.GetStreamLatency() }.map_err(win_error)?;
        let latency = latency
            .checked_mul(100)
            .filter(|value| *value >= 0)
            .ok_or(AudioPlatformError::InvalidRequest)?;
        let control_event = Event::new()?;
        // SAFETY: initialized client supplies owner-thread COM services.
        let renderer: IAudioRenderClient = unsafe { client.GetService() }.map_err(win_error)?;
        // SAFETY: initialized client supplies owner-thread COM services.
        let audio_clock: IAudioClock = unsafe { client.GetService() }.map_err(win_error)?;
        // SAFETY: service is live and writes a scalar frequency.
        let clock_frequency = unsafe { audio_clock.GetFrequency() }.map_err(win_error)?;
        if clock_frequency == 0 {
            return Err(AudioPlatformError::InvalidRequest);
        }
        let sample_count = (buffer_frames as usize)
            .checked_mul(usize::from(request.format().channels()))
            .ok_or(AudioPlatformError::Capacity)?;
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(sample_count)
            .map_err(|_| AudioPlatformError::Capacity)?;
        scratch.resize(sample_count, 0.0);
        let mmcss = options.mmcss_priority.map(Mmcss::new).transpose()?;
        let configuration = AppliedStreamConfig {
            format: request.format(),
            buffer_frames,
            buffer_duration: frame_duration(buffer_frames, request.format().sample_rate()),
            period_frames,
            period_duration,
            stream_latency: Duration::from_nanos(latency),
            sizing_adjusted: adjusted,
            requested: request,
        };
        let mut worker = Self {
            client,
            renderer,
            audio_clock,
            render_event,
            control_event,
            mixer: mixer.take(),
            scratch,
            configuration,
            options,
            clock,
            clock_frequency,
            snapshot: AudioStreamSnapshot {
                telemetry_available: true,
                status: AudioStreamStatus::Ready,
                counters: StreamCounters::default(),
                clock: None,
                render: None,
            },
            version: 0,
            last_qpc: None,
            _mmcss: mmcss,
            _apartment: apartment,
        };
        if let Err(error) = worker.fill(false, None) {
            *mixer = worker.mixer.take();
            drop(worker);
            return Err(native(error));
        }
        Ok(worker)
    }

    fn run(&mut self, control: &Control, started: &mpsc::SyncSender<Result<(), i32>>) {
        let mut running = false;
        let mut last_timer = Instant::now();
        loop {
            match control.request.load(Ordering::Acquire) {
                2 => break,
                1 if !running => {
                    // SAFETY: native Start is called on the client owning thread.
                    // Raw HRESULT avoids projected error work in this loop.
                    let result = unsafe { (self.client.vtable().Start)(self.client.as_raw()) };
                    if result.is_err() {
                        self.fail(result.0);
                        self.publish(&control.telemetry);
                        let _ = started.send(Err(result.0));
                        break;
                    }
                    running = true;
                    // Prefill/Ready time is not a running deadline interval.
                    self.last_qpc = None;
                    self.snapshot.status = AudioStreamStatus::Running;
                    self.publish(&control.telemetry);
                    let _ = started.send(Ok(()));
                    last_timer = Instant::now();
                }
                _ => {}
            }
            let handles = [self.control_event.handle(), self.render_event.handle()];
            let timeout = match (running, self.options.wake_policy) {
                (true, WasapiWakePolicy::Timer { poll_interval }) => {
                    (poll_interval.as_nanos() / 1_000_000).min(50) as u32
                }
                _ => 50,
            };
            // SAFETY: both handles are live worker-owned events. Bounded waits
            // ensure shutdown recovers even if signaling the duplicate fails.
            let wait = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
            if wait == WAIT_FAILED {
                self.fail(E_FAIL_CODE);
                break;
            }
            if control.request.load(Ordering::Acquire) == 2 {
                break;
            }
            let should_fill = running
                && match self.options.wake_policy {
                    WasapiWakePolicy::EventDriven => wait.0 == WAIT_OBJECT_0.0 + 1,
                    WasapiWakePolicy::Timer { poll_interval } => {
                        let due =
                            last_timer.elapsed().as_nanos() >= poll_interval.as_nanos() as u128;
                        if due {
                            last_timer = Instant::now();
                        }
                        due
                    }
                };
            if wait != WAIT_TIMEOUT && wait != WAIT_OBJECT_0 && wait.0 != WAIT_OBJECT_0.0 + 1 {
                self.fail(E_FAIL_CODE);
                break;
            }
            if should_fill {
                if let Err(code) = self.fill(true, Some(&control.cadence)) {
                    self.fail(code);
                    break;
                }
                self.publish(&control.telemetry);
            } else if running && wait == WAIT_TIMEOUT {
                // A lost endpoint may stop signaling events. Observe its raw
                // clock on bounded timeouts rather than waiting indefinitely
                // for a callback that can no longer arrive.
                if let Err(code) = self.read_clock(false) {
                    self.fail(code);
                    break;
                }
                self.publish(&control.telemetry);
            }
        }
        if running {
            // SAFETY: Stop runs on the owning COM thread after fill quiescence.
            let result = unsafe { (self.client.vtable().Stop)(self.client.as_raw()) };
            if result.is_err() && !matches!(self.snapshot.status, AudioStreamStatus::Failed { .. })
            {
                self.fail(result.0);
            }
        }
        if !matches!(self.snapshot.status, AudioStreamStatus::Failed { .. }) {
            self.snapshot.status = AudioStreamStatus::Stopped;
        }
        if let Some(mmcss) = &mut self._mmcss {
            if let Err(code) = mmcss.revert() {
                self.fail(code);
            }
        }
        self.publish(&control.telemetry);
    }

    // Real-time fill boundary: no projected COM errors, allocation, release of
    // owning assets/interfaces, locks, logging or channels occur in this method.
    fn fill(
        &mut self,
        running: bool,
        cadence: Option<&crate::audio::cadence::Capture>,
    ) -> Result<(), i32> {
        let mixer = self.mixer.as_mut().ok_or(E_INVALIDARG_CODE)?;
        let exclusive = self.configuration.requested.mode() == AudioStreamMode::Exclusive;
        let mut padding = 0;
        if !exclusive {
            // SAFETY: live owner-thread client; output is a stack scalar.
            let result = unsafe {
                (self.client.vtable().GetCurrentPadding)(self.client.as_raw(), &mut padding)
            };
            if result.is_err() {
                return Err(result.0);
            }
            if padding > self.configuration.buffer_frames {
                return Err(E_INVALIDARG_CODE);
            }
        }
        self.snapshot.counters.padding_frames = padding;
        if running && !exclusive && padding == 0 && self.snapshot.counters.buffer_fills != 0 {
            self.snapshot.counters.inferred_starvations = self
                .snapshot
                .counters
                .inferred_starvations
                .saturating_add(1);
        }
        let frames = self.configuration.buffer_frames - padding;
        if frames != 0 {
            let mut pointer = std::ptr::null_mut();
            // SAFETY: frame count is bounded by the negotiated native capacity;
            // renderer writes one pointer, retained only until matching release.
            let result = unsafe {
                (self.renderer.vtable().GetBuffer)(self.renderer.as_raw(), frames, &mut pointer)
            };
            if result.is_err() {
                return Err(result.0);
            }
            let mut lease = BufferLease {
                renderer: &self.renderer,
                frames,
                released: false,
            };
            let count = frames as usize * usize::from(self.configuration.format.channels());
            let byte_count = frames as usize * usize::from(self.configuration.format.block_align());
            let render = if pointer.is_null() {
                Err(E_INVALIDARG_CODE)
            } else {
                // Direct render-entry QPC, never the device presentation clock.
                // Ready prefill has no capture; failure is diagnostic only.
                let render_start = cadence
                    .and_then(|_| self.clock.sample_realtime())
                    .map(|receipt| receipt.normalized.timestamp);
                match mixer.render(&mut self.scratch[..count]) {
                    Ok(report) => {
                        if let Some(cadence) = cadence {
                            match render_start {
                                Some(at) => {
                                    cadence.record(at, report.start_frame, report.frames as u64);
                                }
                                None => cadence.mark_unavailable(),
                            }
                        }
                        // SAFETY: GetBuffer guarantees writable storage for the
                        // requested complete frames. Exact extent is bounded by
                        // negotiated capacity; no pointer survives ReleaseBuffer.
                        let bytes = unsafe { std::slice::from_raw_parts_mut(pointer, byte_count) };
                        match encode_pcm(self.configuration.format, &self.scratch[..count], bytes) {
                            Ok(()) => Ok(report),
                            Err(_) => Err(E_INVALIDARG_CODE),
                        }
                    }
                    Err(_) => Err(E_INVALIDARG_CODE),
                }
            };
            // SAFETY: every successful GetBuffer has exactly one ReleaseBuffer.
            // On local failure submit silence, never uninitialized native bytes.
            let release = lease.release(if render.is_ok() {
                0
            } else {
                AUDCLNT_BUFFERFLAGS_SILENT.0 as u32
            });
            if release.is_err() {
                return Err(release.0);
            }
            let report = render?;
            self.snapshot.render = Some(report);
            self.snapshot.counters.submitted_frames = self
                .snapshot
                .counters
                .submitted_frames
                .saturating_add(u64::from(frames));
            self.snapshot.counters.buffer_fills =
                self.snapshot.counters.buffer_fills.saturating_add(1);
        }
        self.read_clock(running)
    }

    fn read_clock(&mut self, running: bool) -> Result<(), i32> {
        let mut position = 0;
        let mut qpc = 0;
        // SAFETY: live owner-thread audio clock writes two stack scalars. Keep
        // raw S_FALSE rather than erasing reading quality in a projected Result.
        let result = unsafe {
            (self.audio_clock.vtable().GetPosition)(
                self.audio_clock.as_raw(),
                &mut position,
                &mut qpc,
            )
        };
        if result.is_err() {
            return Err(result.0);
        }
        let origin = i128::from(self.clock.mapping().origin_counter()) * 1_000_000_000
            / i128::from(self.clock.mapping().frequency());
        let host = i128::from(qpc) * 100 - origin;
        let host_point = i64::try_from(host).ok().map(|nanos| ClockPoint {
            domain: self.clock.output_domain(),
            timestamp: Timestamp::from_nanos(nanos),
        });
        self.snapshot.clock = Some(AudioClockSnapshot {
            position,
            frequency: self.clock_frequency,
            qpc_100ns: qpc,
            host_point,
            reading_quality: match result.0 {
                0 => AudioClockReadingQuality::Accurate,
                1 => AudioClockReadingQuality::Degraded,
                _ => AudioClockReadingQuality::Unknown,
            },
            // Native accuracy status and conversion quantization do not establish
            // a bound for the complete device-to-host measurement relation.
            mapping_quality: ClockMappingQuality::Unknown,
        });
        if running && observe_deadline(&mut self.last_qpc, qpc, self.configuration.period_duration)
        {
            self.snapshot.counters.inferred_deadline_misses = self
                .snapshot
                .counters
                .inferred_deadline_misses
                .saturating_add(1);
        }
        Ok(())
    }

    fn fail(&mut self, code: i32) {
        self.snapshot.status = AudioStreamStatus::Failed { hresult: code };
        self.snapshot.counters.native_failures =
            self.snapshot.counters.native_failures.saturating_add(1);
    }

    fn publish(&mut self, telemetry: &Telemetry) {
        telemetry.publish(self.snapshot, &mut self.version);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.snapshot.status == AudioStreamStatus::Running {
            // SAFETY: panic fallback runs on the owner thread after scoped
            // native buffer leases have unwound and released their buffers.
            let _ = unsafe { (self.client.vtable().Stop)(self.client.as_raw()) };
        }
    }
}

struct BufferLease<'a> {
    renderer: &'a IAudioRenderClient,
    frames: u32,
    released: bool,
}

impl BufferLease<'_> {
    fn release(&mut self, flags: u32) -> HRESULT {
        self.released = true;
        // SAFETY: one lease is constructed for each successful GetBuffer and
        // marks itself released before the one matching native call.
        unsafe {
            (self.renderer.vtable().ReleaseBuffer)(self.renderer.as_raw(), self.frames, flags)
        }
    }
}

impl Drop for BufferLease<'_> {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.release(AUDCLNT_BUFFERFLAGS_SILENT.0 as u32);
        }
    }
}

struct Control {
    request: AtomicU8,
    telemetry: Telemetry,
    cadence: crate::audio::cadence::Capture,
}

#[cfg(test)]
mod tests;

impl beatkernel::audio::StoppedMixerSource for WasapiStream {
    type Error = AudioPlatformError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        if !self.retired || self.worker.is_some() {
            return Err(AudioPlatformError::RecoveryUnavailable);
        }
        Ok(self.recovered_mixer.take())
    }
}

#[cfg(test)]
#[path = "audio/open_failure_fixtures.rs"]
mod open_failure_fixtures;
