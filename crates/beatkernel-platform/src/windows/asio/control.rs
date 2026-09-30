//! Caller-supplied SDK IASIO control, consumed by the optional output stream.
//!
//! Enabled only by `asio-sdk` on Windows MSVC targets. The handle owns one COM
//! STA initialization and one IASIO reference on the opening thread. Control
//! alone installs no callbacks; rate changes and GUI require explicit calls.
#![allow(unsafe_code)]

use super::{AsioDriverRegistration, AsioEnumerationLimits, AsioRegistryError, AsioRegistryView};
use crate::audio::asio::{
    AsioBufferConstraints, AsioConfigurationError, AsioPcmEncoding, AsioPcmError,
    AsioSampleRateRequest,
};
use std::{error::Error, ffi::c_void, fmt, marker::PhantomData, ptr::NonNull, rc::Rc};

/// Native error-code namespace, preserved separately from the operation name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioControlErrorDomain {
    /// Signed COM HRESULT, including changed-apartment failures.
    Com,
    /// Unmodified SDK ASIOError value.
    Asio,
    /// Win32 status, such as an invalid host window.
    Win32,
    /// Native IASIO initialization boolean; zero means rejected.
    InitializationBoolean,
    /// Project bridge validation, allocation or C++ exception status.
    Bridge,
}
/// Control/setup failures, without device replacement or fallback.
#[derive(Debug)]
pub enum AsioControlError {
    /// Supplied registration did not satisfy the SDK-free metadata contract.
    Registration(AsioRegistryError),
    /// Portable buffer/rate validation rejected the request/report.
    Configuration(AsioConfigurationError),
    /// Selected explicit registry view disagrees with process pointer width.
    RegistryViewMismatch {
        /// Original view; no replacement registration is selected.
        view: AsioRegistryView,
    },
    /// A control request named a channel outside the actual driver report.
    InvalidChannel {
        /// Original requested channel index.
        channel: u32,
        /// True selects input, false output.
        input: bool,
    },
    /// A successful native call returned inconsistent or invalid metadata.
    MalformedDriverReport {
        /// Native query whose result failed validation.
        operation: &'static str,
    },
    /// A native call failed with an explicitly identified code namespace.
    Native {
        /// Native/bridge operation that failed.
        operation: &'static str,
        /// Namespace in which code must be interpreted.
        domain: AsioControlErrorDomain,
        /// Unmodified signed native code, or documented bridge status.
        code: i32,
    },
}
impl fmt::Display for AsioControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registration(error) => write!(f, "ASIO registration: {error}"),
            Self::Configuration(error) => write!(f, "ASIO configuration: {error}"),
            Self::RegistryViewMismatch { view } => write!(
                f,
                "ASIO registry view {view:?} does not match process bitness"
            ),
            Self::InvalidChannel { channel, input } => write!(
                f,
                "ASIO channel {channel} input={input} is outside the actual channel report"
            ),
            Self::MalformedDriverReport { operation } => {
                write!(f, "ASIO {operation} returned malformed metadata")
            }
            Self::Native {
                operation,
                domain,
                code,
            } => write!(f, "ASIO {operation} failed: {domain:?} code {code}"),
        }
    }
}
impl Error for AsioControlError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registration(error) => Some(error),
            Self::Configuration(error) => Some(error),
            _ => None,
        }
    }
}
impl From<AsioConfigurationError> for AsioControlError {
    fn from(error: AsioConfigurationError) -> Self {
        Self::Configuration(error)
    }
}

/// Actual nonnegative channel counts, each representable by SDK's signed long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioChannels {
    /// Available input channels.
    pub inputs: u32,
    /// Available output channels.
    pub outputs: u32,
}
/// Actual driver-reported latencies in sample frames, not an acoustic guarantee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioLatencies {
    /// Driver input latency in frames.
    pub input_frames: u32,
    /// Driver output latency in frames.
    pub output_frames: u32,
}
/// One validated SDK channel report; native name encoding is not assumed UTF-8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioChannelInfo {
    /// Requested/reported channel index.
    pub channel: u32,
    /// Input versus output selection.
    pub input: bool,
    /// Driver's actual active flag (this slice creates no buffers).
    pub active: bool,
    /// Nonnegative driver-reported channel group.
    pub group: u32,
    /// Unmodified nonnegative SDK sample-type identity; no format support implied.
    pub sample_type: i32,
    name: [u8; 32],
    name_len: usize,
}
impl AsioChannelInfo {
    /// Selects SDK-free PCM conversion from the actual reported type identity.
    /// DSD and unknown identities remain explicit unsupported-type errors.
    pub fn pcm_encoding(&self) -> Result<AsioPcmEncoding, AsioPcmError> {
        AsioPcmEncoding::from_native(self.sample_type)
    }

    /// Exact bytes before the validated terminal NUL, at most 31 bytes.
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Status {
    domain: i32,
    code: i32,
}
#[repr(C)]
#[derive(Default)]
struct RawChannel {
    channel: i32,
    input: i32,
    active: i32,
    group: i32,
    sample_type: i32,
    name: [u8; 32],
}
unsafe extern "C" {
    fn bk_asio_open(clsid: *const u16, host: usize, out: *mut *mut c_void) -> Status;
    fn bk_asio_close(handle: *mut c_void) -> Status;
    fn bk_asio_channels(handle: *mut c_void, inputs: *mut i32, outputs: *mut i32) -> Status;
    fn bk_asio_buffer(
        handle: *mut c_void,
        min: *mut i32,
        max: *mut i32,
        preferred: *mut i32,
        granularity: *mut i32,
    ) -> Status;
    fn bk_asio_latencies(handle: *mut c_void, input: *mut i32, output: *mut i32) -> Status;
    fn bk_asio_rate(handle: *mut c_void, rate: *mut f64) -> Status;
    fn bk_asio_probe_rate(handle: *mut c_void, rate: f64) -> Status;
    fn bk_asio_set_rate(handle: *mut c_void, rate: f64) -> Status;
    fn bk_asio_channel(handle: *mut c_void, index: i32, input: i32, out: *mut RawChannel)
        -> Status;
    fn bk_asio_control_panel(handle: *mut c_void) -> Status;
}
pub(super) fn check(status: Status, operation: &'static str) -> Result<(), AsioControlError> {
    if status.domain == 0 && status.code == 0 {
        return Ok(());
    }
    let domain = match status.domain {
        1 => AsioControlErrorDomain::Com,
        2 => AsioControlErrorDomain::Asio,
        3 => AsioControlErrorDomain::Win32,
        4 => AsioControlErrorDomain::InitializationBoolean,
        _ => AsioControlErrorDomain::Bridge,
    };
    Err(AsioControlError::Native {
        operation,
        domain,
        code: status.code,
    })
}
fn malformed(operation: &'static str) -> AsioControlError {
    AsioControlError::MalformedDriverReport { operation }
}

/// Owning-thread IASIO control reference; deliberately neither Send nor Sync.
///
/// Drop releases IASIO before balancing COM on the opening thread. `close`
/// additionally reports bridge cleanup failure. This type installs no callbacks
/// and exposes no buffers/start/stream. Driver code is native trusted code.
pub struct AsioControl {
    handle: Option<NonNull<c_void>>,
    registration: AsioDriverRegistration,
    _owner_thread: PhantomData<Rc<()>>,
}
impl AsioControl {
    /// Opens exactly this validated registration and initializes IASIO with the
    /// caller's explicit optional HWND. Nonzero handles are checked with IsWindow.
    /// Changed COM apartment and incompatible explicit registry views reject.
    ///
    /// # Safety
    /// The selected installed driver must be trusted native code. A supplied
    /// HWND must denote a valid caller-controlled window and remain alive until
    /// this control is closed/dropped; init may retain it. Native IsWindow is only
    /// an opening-time check, not a window-lifetime guarantee. None passes null.
    pub unsafe fn open(
        registration: &AsioDriverRegistration,
        host_window: Option<usize>,
    ) -> Result<Self, AsioControlError> {
        let registration = AsioDriverRegistration::from_values(
            &registration.name,
            registration.description.as_deref(),
            &registration.id.clsid,
            registration.id.view,
            AsioEnumerationLimits {
                max_drivers: 1,
                max_value_units: 32768,
            },
        )
        .map_err(AsioControlError::Registration)?;
        let view = registration.id.view;
        if matches!(view, AsioRegistryView::Bits32) && usize::BITS != 32
            || matches!(view, AsioRegistryView::Bits64) && usize::BITS != 64
        {
            return Err(AsioControlError::RegistryViewMismatch { view });
        }
        let mut clsid = [0u16; 39];
        for (target, byte) in clsid.iter_mut().zip(registration.id.clsid.bytes()) {
            *target = u16::from(byte);
        }
        let mut handle = std::ptr::null_mut();
        // SAFETY: validated canonical ASCII CLSID occupies 38 UTF-16 units plus
        // zero terminator; writable handle slot and caller's HWND contract hold.
        // Bridge retains no Rust pointer, balances failed setup, and owns success.
        check(
            unsafe { bk_asio_open(clsid.as_ptr(), host_window.unwrap_or(0), &mut handle) },
            "CoInitializeEx/CoCreateInstance/IASIO::init",
        )?;
        let handle = NonNull::new(handle).ok_or_else(|| malformed("open"))?;
        Ok(Self {
            handle: Some(handle),
            registration,
            _owner_thread: PhantomData,
        })
    }
    pub(super) fn raw(&self) -> *mut c_void {
        self.handle.expect("live owning control").as_ptr()
    }
    /// Immutable canonical registration used for this exact open.
    pub fn registration(&self) -> &AsioDriverRegistration {
        &self.registration
    }
    /// Queries current native channel counts; no arbitrary channel ceiling added.
    pub fn channels(&mut self) -> Result<AsioChannels, AsioControlError> {
        let (mut inputs, mut outputs) = (0, 0);
        // SAFETY: exclusive owner is on its opening thread; fixed-width initialized
        // outputs live for the synchronous SDK call and are not retained.
        check(
            unsafe { bk_asio_channels(self.raw(), &mut inputs, &mut outputs) },
            "getChannels",
        )?;
        if inputs < 0 || outputs < 0 || (inputs == 0 && outputs == 0) {
            return Err(malformed("getChannels"));
        }
        Ok(AsioChannels {
            inputs: inputs as u32,
            outputs: outputs as u32,
        })
    }
    /// Queries and validates actual SDK buffer constraints; creates no buffers.
    pub fn buffer_constraints(&mut self) -> Result<AsioBufferConstraints, AsioControlError> {
        let (mut min, mut max, mut preferred, mut granularity) = (0, 0, 0, 0);
        // SAFETY: live same-thread owner and four initialized fixed-width outputs;
        // SDK long conversion stays entirely in the MSVC C++ bridge.
        check(
            unsafe {
                bk_asio_buffer(
                    self.raw(),
                    &mut min,
                    &mut max,
                    &mut preferred,
                    &mut granularity,
                )
            },
            "getBufferSize",
        )?;
        Ok(AsioBufferConstraints::from_raw(
            min,
            max,
            preferred,
            granularity,
        )?)
    }
    /// Queries actual input/output frame latencies; does not establish acoustics.
    pub fn latencies(&mut self) -> Result<AsioLatencies, AsioControlError> {
        let (mut input, mut output) = (0, 0);
        // SAFETY: live same-thread owner, initialized fixed-size scalar outputs.
        check(
            unsafe { bk_asio_latencies(self.raw(), &mut input, &mut output) },
            "getLatencies",
        )?;
        if input < 0 || output < 0 {
            return Err(malformed("getLatencies"));
        }
        Ok(AsioLatencies {
            input_frames: input as u32,
            output_frames: output as u32,
        })
    }
    /// Queries the actual current finite positive rate; native no-clock errors remain.
    pub fn sample_rate(&mut self) -> Result<f64, AsioControlError> {
        let mut rate = 0.0;
        // SAFETY: live same-thread owner and initialized IEEE f64 output.
        check(
            unsafe { bk_asio_rate(self.raw(), &mut rate) },
            "getSampleRate",
        )?;
        if !rate.is_finite() || rate <= 0.0 {
            return Err(malformed("getSampleRate"));
        }
        Ok(rate)
    }
    /// Probes a validated request; unsupported rates retain native ASE_NoClock.
    /// ExternalClock is forwarded explicitly as zero, without selecting a rate.
    pub fn can_sample_rate(
        &mut self,
        request: AsioSampleRateRequest,
    ) -> Result<(), AsioControlError> {
        let rate = request.native_value()?;
        // SAFETY: live same-thread owner; portable request validation excludes
        // negative/nonfinite rates and only explicit ExternalClock yields zero.
        check(
            unsafe { bk_asio_probe_rate(self.raw(), rate) },
            "canSampleRate",
        )
    }
    /// Explicitly requests the selected Hertz value or external synchronization.
    /// Driver acceptance is reported; actual current rate must be queried anew.
    pub fn set_sample_rate(
        &mut self,
        request: AsioSampleRateRequest,
    ) -> Result<(), AsioControlError> {
        let rate = request.native_value()?;
        // SAFETY: live same-thread owner and validated explicit scalar request.
        check(
            unsafe { bk_asio_set_rate(self.raw(), rate) },
            "setSampleRate",
        )
    }
    /// Queries one channel using current counts and bounded native name storage.
    pub fn channel_info(
        &mut self,
        channel: u32,
        input: bool,
    ) -> Result<AsioChannelInfo, AsioControlError> {
        let counts = self.channels()?;
        if channel >= if input { counts.inputs } else { counts.outputs } {
            return Err(AsioControlError::InvalidChannel { channel, input });
        }
        let mut raw = RawChannel::default();
        // SAFETY: channel checked against the nonnegative signed-long count;
        // same-thread owner and project-owned fixed-width struct live through call.
        // SDK layout/array extent is consumed only by C++, never reinterpreted here.
        check(
            unsafe { bk_asio_channel(self.raw(), channel as i32, i32::from(input), &mut raw) },
            "getChannelInfo",
        )?;
        if raw.channel != channel as i32
            || raw.input != i32::from(input)
            || !matches!(raw.active, 0 | 1)
            || raw.group < 0
            || raw.sample_type < 0
        {
            return Err(malformed("getChannelInfo"));
        }
        let name_len = raw
            .name
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| malformed("getChannelInfo"))?;
        Ok(AsioChannelInfo {
            channel,
            input,
            active: raw.active != 0,
            group: raw.group as u32,
            sample_type: raw.sample_type,
            name: raw.name,
            name_len,
        })
    }
    /// Explicitly invokes the selected driver's control panel on this owner thread.
    /// May display/block in native UI; it is never called automatically.
    pub fn control_panel(&mut self) -> Result<(), AsioControlError> {
        // SAFETY: live same-thread initialized driver; caller's window lifetime
        // obligation from open still holds. No Rust callback or pointer retained.
        check(unsafe { bk_asio_control_panel(self.raw()) }, "controlPanel")
    }
    /// Releases IASIO then balances this handle's COM initialization on this thread.
    /// Consumes control even when native cleanup reports failure; no implicit retry.
    pub fn close(mut self) -> Result<(), AsioControlError> {
        let handle = self.handle.take().expect("live owning control");
        // SAFETY: uniquely owned pointer, same opening thread guaranteed by the
        // !Send/!Sync marker; bridge consumes it once and balances COM afterward.
        check(
            unsafe { bk_asio_close(handle.as_ptr()) },
            "Release/CoUninitialize",
        )
    }
}
impl Drop for AsioControl {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: unique same-thread owner consumes bridge allocation once;
            // native reference is released before its COM initialization balance.
            let _ = unsafe { bk_asio_close(handle.as_ptr()) };
        }
    }
}
