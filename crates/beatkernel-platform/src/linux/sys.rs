use beatkernel::time::{ClockDomainId, ClockPoint, Timestamp};
use std::{
    ffi::{c_int, c_long, c_ulong, c_void},
    fmt,
    fs::File,
    io,
    os::fd::AsRawFd,
};

/// Linux native acquisition, configuration or stream failure.
#[derive(Debug)]
pub enum LinuxError {
    /// Native filesystem/syscall failure.
    Io(io::Error),
    /// ABI is currently implemented only for Linux x86_64/aarch64.
    UnsupportedTarget,
    /// Invalid caller-selected configuration.
    InvalidConfiguration(&'static str),
    /// A checked timestamp, sequence or frame count overflowed.
    Overflow,
    /// A native input record/report is malformed or unexpectedly truncated.
    MalformedInput(&'static str),
    /// Dynamic ALSA library or a mandatory symbol is unavailable.
    AlsaUnavailable(&'static str),
    /// ALSA returned a negative errno for the specified operation.
    Alsa {
        /// Native API operation.
        operation: &'static str,
        /// Negative native errno.
        code: i32,
    },
    /// Exact size was unsupported; retry requires explicit rounding opt-in.
    SizeMismatch {
        /// Requested period size.
        requested_period: u32,
        /// Applied native period size.
        applied_period: u32,
        /// Requested buffer size.
        requested_buffer: u32,
        /// Applied native buffer size.
        applied_buffer: u32,
    },
    /// Core mixer refused rendering.
    Mixer(beatkernel::audio::AudioError),
    /// PCM conversion rejected the output buffer or format.
    Conversion(crate::audio::AudioPlatformError),
    /// Native worker panicked, as observed while joining.
    WorkerPanicked,
    /// Stream lifecycle transition is unavailable in this state.
    InvalidLifecycle,
}
impl fmt::Display for LinuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Linux backend: {self:?}")
    }
}
impl std::error::Error for LinuxError {}
impl From<io::Error> for LinuxError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn supported_abi() -> Result<(), LinuxError> {
    if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
        Ok(())
    } else {
        Err(LinuxError::UnsupportedTarget)
    }
}

#[repr(C)]
struct Timespec {
    seconds: c_long,
    nanos: c_long,
}
unsafe extern "C" {
    fn clock_gettime(clock: c_int, result: *mut Timespec) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

/// CLOCK_MONOTONIC reader; caller assigns its domain identity.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock {
    domain: ClockDomainId,
}
impl MonotonicClock {
    /// Creates an explicitly labeled clock without establishing other relations.
    pub const fn new(domain: ClockDomainId) -> Self {
        Self { domain }
    }
    /// Caller-assigned domain identity, without reading the native clock.
    pub const fn domain(self) -> ClockDomainId {
        self.domain
    }
    /// Reads a native monotonic point with checked integer-nanosecond conversion.
    pub fn now(self) -> Result<ClockPoint, LinuxError> {
        supported_abi()?;
        let mut value = Timespec {
            seconds: 0,
            nanos: 0,
        };
        // SAFETY: CLOCK_MONOTONIC is Linux clock 1; value is a writable native
        // timespec on the supported 64-bit Linux ABIs for the full call.
        if unsafe { clock_gettime(1, &mut value) } < 0 {
            return Err(io::Error::last_os_error().into());
        }
        let nanos = i128::from(value.seconds) * 1_000_000_000 + i128::from(value.nanos);
        let nanos = i64::try_from(nanos).map_err(|_| LinuxError::Overflow)?;
        Ok(ClockPoint {
            domain: self.domain,
            timestamp: Timestamp::from_nanos(nanos),
        })
    }
}

// Linux generic ioctl layout used by x86_64 and aarch64: 8 nr, 8 type,
// 14 size, 2 direction bits. These helpers are never used on other ABIs.
pub(crate) const fn request(direction: u32, kind: u8, number: u8, bytes: usize) -> c_ulong {
    ((direction << 30) | ((bytes as u32) << 16) | ((kind as u32) << 8) | number as u32) as c_ulong
}
pub(crate) fn ioctl_bytes(
    file: &File,
    request: c_ulong,
    buffer: &mut [u8],
) -> Result<usize, LinuxError> {
    supported_abi()?;
    // SAFETY: all internal callers compute request size from the supplied
    // initialized buffer and use UAPI ioctls with byte-compatible structures.
    // The File keeps fd valid; ioctl retains no pointer after return.
    let result = unsafe {
        ioctl(
            file.as_raw_fd(),
            request,
            buffer.as_mut_ptr().cast::<c_void>(),
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(result as usize)
    }
}
