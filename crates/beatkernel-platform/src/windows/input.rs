//! App-owned Windows Raw Input acquisition on the window's control thread.
//!
//! Construction has no registration side effects. The caller owns its HWND,
//! message pump and foreground `DefWindowProc` cleanup. These allocating APIs
//! must not run in an audio real-time callback.

#![allow(unsafe_code)]

use std::{
    collections::BTreeMap, error::Error, fmt, io, marker::PhantomData, mem::size_of, ptr, rc::Rc,
};

use beatkernel::input::{DeviceDescriptor, DeviceId};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, HWND},
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
    UI::{
        Input::{
            GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList,
            GetRegisteredRawInputDevices, RegisterRawInputDevices, RAWINPUTDEVICE,
            RAWINPUTDEVICELIST, RAWINPUTHEADER, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDEV_PAGEONLY,
            RIDEV_REMOVE, RIDI_DEVICEINFO, RIDI_DEVICENAME, RID_DEVICE_INFO, RID_INPUT,
            RIM_TYPEHID, RIM_TYPEKEYBOARD,
        },
        WindowsAndMessaging::GetWindowThreadProcessId,
    },
};

use super::clock::{QpcClock, QpcReceipt};
use crate::raw_input::{
    InputBatch, RawDeviceInfo, RawDeviceKind, RawInputError, RawInputLayout, RawInputPacket,
    RawInputProcessor, MAX_RAW_INPUT_BYTES, MAX_RAW_INPUT_DEVICES,
};

/// Maximum UTF-16 code units accepted for a native interface-path query.
pub const MAX_DEVICE_NAME_CHARS: usize = 32_768;
const QUERY_ATTEMPTS: usize = 3;
const REGISTRATION_FLAGS: u32 = RIDEV_INPUTSINK | RIDEV_DEVNOTIFY;

/// An explicit native API, packet, clock, resource or ownership failure.
#[derive(Debug)]
pub enum WindowsInputError {
    /// An OS failure with the operation and original OS error intact.
    Api {
        /// Native operation that failed.
        operation: &'static str,
        /// Original OS error.
        source: io::Error,
    },
    /// Native clock sampling failed.
    Clock(io::Error),
    /// Portable packet/state processing rejected the acquisition.
    Packet(RawInputError),
    /// Invalid input, conflicting ownership or a bounded query failure.
    Invalid(&'static str),
}

impl fmt::Display for WindowsInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api { operation, source } => write!(f, "{operation}: {source}"),
            Self::Clock(error) => write!(f, "QPC receipt: {error}"),
            Self::Packet(error) => error.fmt(f),
            Self::Invalid(reason) => f.write_str(reason),
        }
    }
}

impl Error for WindowsInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Api { source, .. } | Self::Clock(source) => Some(source),
            Self::Packet(source) => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

impl From<RawInputError> for WindowsInputError {
    fn from(error: RawInputError) -> Self {
        Self::Packet(error)
    }
}

type Result<T> = std::result::Result<T, WindowsInputError>;

/// One explicitly requested HID top-level collection class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawInputUsage {
    /// HID usage page.
    pub page: u16,
    /// HID usage within that page.
    pub usage: u16,
}

impl RawInputUsage {
    /// Generic desktop keyboard class.
    pub const KEYBOARD: Self = Self { page: 1, usage: 6 };
}

/// Thread-affine guard for explicitly registered input collection classes.
///
/// The application must serialize all process-wide registrations, keep its
/// window alive, and grant this guard exclusive ownership until `close`/drop.
/// Close before transfer: Windows has no token to distinguish an identical
/// same-window replacement. Registered-class queries may omit DEVNOTIFY;
/// notification-bit-only changes are also indistinguishable. Other changed
/// flags or windows are left intact.
/// Drop is best-effort; use `close` to observe OS cleanup failures.
pub struct RawInputRegistration {
    window: usize,
    usages: Vec<RawInputUsage>,
    _thread: PhantomData<Rc<()>>,
}

impl RawInputRegistration {
    /// Registers deduplicated classes to a live current-process/current-thread HWND.
    ///
    /// `window` is an opaque HWND value; zero/foreign-thread windows are rejected.
    /// Existing matching or overlapping page-wide registration is an error,
    /// including a registration already targeting this same HWND. Mouse
    /// acquisition is outside this backend's scope.
    pub fn register(window: usize, usages: &[RawInputUsage]) -> Result<Self> {
        validate_window(window)?;
        if usages.is_empty() || usages.len() > MAX_RAW_INPUT_DEVICES {
            return Err(WindowsInputError::Invalid(
                "invalid registration class count",
            ));
        }
        let mut usages = usages.to_vec();
        usages.sort_unstable();
        usages.dedup();
        for usage in &usages {
            if usage.page == 0 || usage.usage == 0 || (usage.page == 1 && usage.usage == 2) {
                return Err(WindowsInputError::Invalid(
                    "unsupported Raw Input collection",
                ));
            }
        }
        let existing = registered_classes()?;
        for usage in &usages {
            if existing.iter().any(|entry| {
                entry.usUsagePage == usage.page
                    && (entry.usUsage == usage.usage
                        || (entry.usUsage == 0 && entry.dwFlags & RIDEV_PAGEONLY != 0))
            }) {
                return Err(WindowsInputError::Invalid(
                    "Raw Input registration ownership conflict",
                ));
            }
        }
        let entries: Vec<_> = usages
            .iter()
            .map(|usage| RAWINPUTDEVICE {
                usUsagePage: usage.page,
                usUsage: usage.usage,
                dwFlags: REGISTRATION_FLAGS,
                hwndTarget: window as HWND,
            })
            .collect();
        register_classes(&entries)?;
        Ok(Self {
            window,
            usages,
            _thread: PhantomData,
        })
    }

    /// Removes unchanged owned classes with null target, preserving successors.
    ///
    /// This is idempotent after success. On failure the guard retains ownership
    /// information so the caller can retry before destroying its window.
    pub fn close(&mut self) -> Result<()> {
        if self.usages.is_empty() {
            return Ok(());
        }
        let existing = registered_classes()?;
        let removals: Vec<_> = self
            .usages
            .iter()
            .filter(|usage| {
                existing.iter().any(|entry| {
                    entry.usUsagePage == usage.page
                        && entry.usUsage == usage.usage
                        && entry.hwndTarget as usize == self.window
                        // Windows can omit DEVNOTIFY from its query result.
                        // Match every observable bit; application ownership
                        // excludes indistinguishable notification-only changes.
                        && entry.dwFlags & !RIDEV_DEVNOTIFY == RIDEV_INPUTSINK
                })
            })
            .map(|usage| RAWINPUTDEVICE {
                usUsagePage: usage.page,
                usUsage: usage.usage,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: ptr::null_mut(),
            })
            .collect();
        if !removals.is_empty() {
            register_classes(&removals)?;
        }
        self.usages.clear();
        Ok(())
    }
}

impl Drop for RawInputRegistration {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Owned native device information; the interface path is not a friendly name.
#[derive(Clone, Debug)]
pub struct WindowsInputDevice {
    /// Opaque native handle, valid only for this attachment.
    pub handle: usize,
    /// Stable runtime descriptor until retirement.
    pub descriptor: DeviceDescriptor,
    /// Native keyboard or HID discriminator.
    pub kind: RawDeviceKind,
    /// HID top-level collection (keyboard uses the keyboard class).
    pub usage: RawInputUsage,
    /// Device interface path; may contain identifying data, so do not log by default.
    pub interface_path: String,
}

/// One owned native acquisition, including packets with suppressed semantic output.
#[derive(Debug)]
pub struct WindowsInputBatch {
    /// Original bytes, retaining every header/keyboard field and opaque HID report.
    pub packet: Vec<u8>,
    /// Runtime source ID, also present in every emitted event.
    pub device: DeviceId,
    /// Acquisition-start QPC receipt sample; not hardware event time.
    pub receipt: QpcReceipt,
    /// Original MSG.time milliseconds modulo 2^32, if supplied by the caller.
    /// This is message-posted metadata, separate from QPC/canonical timing.
    pub message_time_ms: Option<u32>,
    /// Canonical events and visible pending/filter status.
    pub input: InputBatch,
}

/// Thread-affine native acquisition using one application-shared QPC origin.
///
/// The application provides a live current-thread window and keeps it alive
/// through cleanup. Its message pump follows [`Self::read_raw_input`]'s
/// foreground cleanup contract; the runnable inspector shows the complete pump.
///
/// ```no_run
/// use beatkernel::time::ClockDomainId;
/// use beatkernel_platform::windows::{
///     clock::QpcClock,
///     input::{RawInputRegistration, RawInputUsage, WindowsInput},
/// };
/// # fn configure(app_window: usize) -> Result<(), Box<dyn std::error::Error>> {
/// let shared_clock = QpcClock::new(ClockDomainId(1))?;
/// let mut input = WindowsInput::new(shared_clock); // Does not register.
/// let mut registration =
///     RawInputRegistration::register(app_window, &[RawInputUsage::KEYBOARD])?;
/// let devices = input.enumerate_devices()?;
/// // Run the application's message pump while its window stays alive.
/// registration.close()?; // Observe cleanup failure before destroying the window.
/// # Ok(())
/// # }
/// ```
pub struct WindowsInput {
    clock: QpcClock,
    processor: RawInputProcessor,
    devices: BTreeMap<usize, WindowsInputDevice>,
    _thread: PhantomData<Rc<()>>,
}

impl WindowsInput {
    /// Creates an unregistered backend; copy the application's clock into it.
    pub fn new(clock: QpcClock) -> Self {
        Self {
            processor: RawInputProcessor::new(clock.output_domain()),
            clock,
            devices: BTreeMap::new(),
            _thread: PhantomData,
        }
    }

    /// Enumerates keyboard/HID devices and retires attachments absent from the snapshot.
    ///
    /// Mouse entries are deliberately excluded. OS descriptor-query failures
    /// are returned, rather than silently replacing a physical source with a stub.
    /// Arrival/removal races may require the caller to retry this bounded snapshot.
    pub fn enumerate_devices(&mut self) -> Result<Vec<WindowsInputDevice>> {
        let entries = query_array(
            RAWINPUTDEVICELIST {
                hDevice: ptr::null_mut(),
                dwType: 0,
            },
            "GetRawInputDeviceList",
            |pointer, count| {
                // SAFETY: query_array supplies null or an aligned initialized
                // array of count elements and a live count output for this call.
                unsafe {
                    GetRawInputDeviceList(pointer, count, size_of::<RAWINPUTDEVICELIST>() as u32)
                }
            },
        )?;
        let mut snapshot = Vec::new();
        for entry in &entries {
            if entry.dwType == RIM_TYPEKEYBOARD || entry.dwType == RIM_TYPEHID {
                snapshot.push(self.attach_device(entry.hDevice as usize)?);
            }
        }
        let retired: Vec<_> = self
            .devices
            .keys()
            .filter(|handle| {
                !entries
                    .iter()
                    .any(|entry| entry.hDevice as usize == **handle)
            })
            .copied()
            .collect();
        for handle in retired {
            self.remove_device(handle);
        }
        Ok(snapshot)
    }

    /// Handles a keyboard/HID arrival (or lazy first acquisition), preserving active IDs.
    pub fn attach_device(&mut self, handle: usize) -> Result<WindowsInputDevice> {
        if handle == 0 {
            return Err(RawInputError::InvalidDeviceHandle.into());
        }
        if let Some(device) = self.devices.get(&handle) {
            return Ok(device.clone());
        }
        if self.devices.len() >= MAX_RAW_INPUT_DEVICES {
            return Err(RawInputError::DeviceLimit.into());
        }
        let (info, usage) = device_info(handle)?;
        let interface_path = device_path(handle)?;
        let kind = info.kind;
        self.processor.register_device(handle as u64, info)?;
        let descriptor = self
            .processor
            .device(handle as u64)
            .expect("successful registration creates descriptor")
            .clone();
        let device = WindowsInputDevice {
            handle,
            descriptor,
            kind,
            usage,
            interface_path,
        };
        self.devices.insert(handle, device.clone());
        Ok(device)
    }

    /// Handles removal, clearing held/pending state; reconnect receives a fresh ID.
    pub fn remove_device(&mut self, handle: usize) -> Option<WindowsInputDevice> {
        self.processor.unregister_device(handle as u64);
        self.devices.remove(&handle)
    }

    /// Returns currently known device attachments in native-handle order.
    pub fn devices(&self) -> impl Iterator<Item = &WindowsInputDevice> {
        self.devices.values()
    }

    /// Acquires one WM_INPUT lParam handle, sampling QPC before any native queries.
    ///
    /// Pass original MSG.time when acquiring from a message pump. The handle
    /// must still be valid: call before DefWindowProc consumes the message.
    /// The caller must invoke DefWindowProc exactly once for foreground input,
    /// even when this returns an error, before handling/propagating that error.
    pub fn read_raw_input(
        &mut self,
        handle: usize,
        message_time_ms: Option<u32>,
    ) -> Result<WindowsInputBatch> {
        let receipt = self.clock.sample().map_err(WindowsInputError::Clock)?;
        let bytes = read_packet(handle)?;
        let layout = if size_of::<usize>() == 8 {
            RawInputLayout::Win64
        } else {
            RawInputLayout::Win32
        };
        let packet = RawInputPacket::parse(&bytes, layout)?;
        let device_handle = usize::try_from(packet.header().device_handle).map_err(|_| {
            WindowsInputError::Invalid("device handle exceeds native pointer width")
        })?;
        let device = self.attach_device(device_handle)?.descriptor.runtime_id;
        let input = self
            .processor
            .process(&packet, receipt.native, self.clock.mapping())?;
        Ok(WindowsInputBatch {
            packet: bytes,
            device,
            receipt,
            message_time_ms,
            input,
        })
    }
}

fn validate_window(window: usize) -> Result<()> {
    let mut process = 0;
    // SAFETY: HWND is an opaque OS handle; invalid handles are rejected by the
    // API. process is a live writable u32, not retained beyond the call.
    let thread = unsafe { GetWindowThreadProcessId(window as HWND, &mut process) };
    // SAFETY: these parameterless APIs do not access Rust memory.
    let current_process = unsafe { GetCurrentProcessId() };
    // SAFETY: parameterless query, no retained resources.
    let current_thread = unsafe { GetCurrentThreadId() };
    if window == 0 || thread == 0 || process != current_process || thread != current_thread {
        return Err(WindowsInputError::Invalid(
            "Raw Input target must be a live window on the current process/thread",
        ));
    }
    Ok(())
}

fn registered_classes() -> Result<Vec<RAWINPUTDEVICE>> {
    query_array(
        RAWINPUTDEVICE {
            usUsagePage: 0,
            usUsage: 0,
            dwFlags: 0,
            hwndTarget: ptr::null_mut(),
        },
        "GetRegisteredRawInputDevices",
        |pointer, count| {
            // SAFETY: query_array supplies a correctly aligned writable array
            // or null with a live count and the ABI's exact element size.
            unsafe {
                GetRegisteredRawInputDevices(pointer, count, size_of::<RAWINPUTDEVICE>() as u32)
            }
        },
    )
}

fn register_classes(entries: &[RAWINPUTDEVICE]) -> Result<()> {
    // SAFETY: entries is initialized, aligned, live for the synchronous call;
    // bounded length fits u32. Windows copies these values, retaining no pointer.
    if unsafe {
        RegisterRawInputDevices(
            entries.as_ptr(),
            entries.len() as u32,
            size_of::<RAWINPUTDEVICE>() as u32,
        )
    } == 0
    {
        return Err(api_error("RegisterRawInputDevices"));
    }
    Ok(())
}

/// All elements stay initialized even when Windows writes fewer than requested.
fn query_array<T: Copy>(
    empty: T,
    operation: &'static str,
    mut query: impl FnMut(*mut T, *mut u32) -> u32,
) -> Result<Vec<T>> {
    for _ in 0..QUERY_ATTEMPTS {
        let mut count = 0;
        let result = query(ptr::null_mut(), &mut count);
        if result == u32::MAX {
            let source = io::Error::last_os_error();
            if source.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                return Err(WindowsInputError::Api { operation, source });
            }
        }
        if count as usize > MAX_RAW_INPUT_DEVICES {
            return Err(WindowsInputError::Invalid(
                "native device/class count exceeds 4096",
            ));
        }
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut entries = vec![empty; count as usize];
        let result = query(entries.as_mut_ptr(), &mut count);
        if result == u32::MAX {
            let source = io::Error::last_os_error();
            if source.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                continue;
            }
            return Err(WindowsInputError::Api { operation, source });
        }
        if result as usize > entries.len() || count as usize > entries.len() {
            return Err(WindowsInputError::Invalid(
                "native array query returned an invalid length",
            ));
        }
        entries.truncate(result as usize);
        return Ok(entries);
    }
    Err(WindowsInputError::Invalid(
        "native array changed during three snapshot attempts",
    ))
}

fn device_info(handle: usize) -> Result<(RawDeviceInfo, RawInputUsage)> {
    // SAFETY: this ABI structure and union contain only integer fields; zero is
    // a valid initialized bit pattern. No references/owned Rust values exist.
    let mut native: RID_DEVICE_INFO = unsafe { std::mem::zeroed() };
    native.cbSize = size_of::<RID_DEVICE_INFO>() as u32;
    let mut bytes = native.cbSize;
    // SAFETY: native and bytes are live/aligned; exactly cbSize bytes are
    // writable. The opaque device handle is validated by Windows.
    let copied = unsafe {
        GetRawInputDeviceInfoW(
            handle as _,
            RIDI_DEVICEINFO,
            (&mut native as *mut RID_DEVICE_INFO).cast(),
            &mut bytes,
        )
    };
    if copied == u32::MAX {
        return Err(api_error("GetRawInputDeviceInfoW(RIDI_DEVICEINFO)"));
    }
    if copied != size_of::<RID_DEVICE_INFO>() as u32 || bytes != size_of::<RID_DEVICE_INFO>() as u32
    {
        return Err(WindowsInputError::Invalid(
            "native device information has an invalid size",
        ));
    }
    match native.dwType {
        RIM_TYPEKEYBOARD => Ok((
            RawDeviceInfo::new(RawDeviceKind::Keyboard),
            RawInputUsage::KEYBOARD,
        )),
        RIM_TYPEHID => {
            // SAFETY: the OS returned a full RID_DEVICE_INFO and its checked
            // discriminator selects the hid union member; all fields are integers.
            let hid = unsafe { native.Anonymous.hid };
            let mut info = RawDeviceInfo::new(RawDeviceKind::Hid);
            info.vendor_id = Some(
                u16::try_from(hid.dwVendorId)
                    .map_err(|_| WindowsInputError::Invalid("HID vendor ID exceeds u16"))?,
            );
            info.product_id = Some(
                u16::try_from(hid.dwProductId)
                    .map_err(|_| WindowsInputError::Invalid("HID product ID exceeds u16"))?,
            );
            Ok((
                info,
                RawInputUsage {
                    page: hid.usUsagePage,
                    usage: hid.usUsage,
                },
            ))
        }
        kind => Err(RawInputError::UnsupportedType(kind).into()),
    }
}

fn device_path(handle: usize) -> Result<String> {
    for _ in 0..QUERY_ATTEMPTS {
        let mut chars = 0;
        // SAFETY: null is the documented size-query buffer; chars is live and
        // writable. RIDI_DEVICENAME uses UTF-16 character counts, not bytes.
        if unsafe {
            GetRawInputDeviceInfoW(handle as _, RIDI_DEVICENAME, ptr::null_mut(), &mut chars)
        } == u32::MAX
        {
            return Err(api_error("GetRawInputDeviceInfoW(name size)"));
        }
        if chars == 0 || chars as usize > MAX_DEVICE_NAME_CHARS {
            return Err(WindowsInputError::Invalid(
                "native interface path has an invalid character count",
            ));
        }
        // Use u32 storage so even the documented DWORD alignment is preserved.
        let mut words = vec![0u32; (chars as usize).div_ceil(2)];
        let capacity = chars;
        // SAFETY: words gives initialized DWORD-aligned storage for at least
        // chars UTF-16 units. Windows retains no pointer after the call.
        let copied = unsafe {
            GetRawInputDeviceInfoW(
                handle as _,
                RIDI_DEVICENAME,
                words.as_mut_ptr().cast(),
                &mut chars,
            )
        };
        if copied == u32::MAX {
            let source = io::Error::last_os_error();
            if source.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                continue;
            }
            return Err(WindowsInputError::Api {
                operation: "GetRawInputDeviceInfoW(name)",
                source,
            });
        }
        if copied > capacity || chars > capacity {
            return Err(WindowsInputError::Invalid(
                "native interface path exceeds supplied buffer",
            ));
        }
        // SAFETY: words is aligned for u16, initialized, and allocated for at
        // least copied units. The slice cannot outlive words or overlap mutation.
        let units =
            unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u16>(), copied as usize) };
        let length = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        return String::from_utf16(&units[..length])
            .map_err(|_| WindowsInputError::Invalid("native interface path is invalid UTF-16"));
    }
    Err(WindowsInputError::Invalid(
        "native interface path changed during three query attempts",
    ))
}

fn read_packet(handle: usize) -> Result<Vec<u8>> {
    if handle == 0 {
        return Err(WindowsInputError::Invalid("zero WM_INPUT handle"));
    }
    for _ in 0..QUERY_ATTEMPTS {
        let mut bytes = 0;
        // SAFETY: null is the documented size-query pointer; bytes is live and
        // writable. The OS validates the opaque input handle.
        let result = unsafe {
            GetRawInputData(
                handle as _,
                RID_INPUT,
                ptr::null_mut(),
                &mut bytes,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if result == u32::MAX {
            return Err(api_error("GetRawInputData(size)"));
        }
        if result != 0
            || bytes < size_of::<RAWINPUTHEADER>() as u32
            || bytes as usize > MAX_RAW_INPUT_BYTES
        {
            return Err(WindowsInputError::Invalid(
                "native Raw Input size exceeds valid bounds",
            ));
        }
        let capacity = bytes;
        let mut words = vec![0u32; (bytes as usize).div_ceil(4)];
        // SAFETY: initialized DWORD-aligned storage contains at least bytes
        // writable bytes; exact native header size is supplied. No pointer retained.
        let copied = unsafe {
            GetRawInputData(
                handle as _,
                RID_INPUT,
                words.as_mut_ptr().cast(),
                &mut bytes,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if copied == u32::MAX {
            let source = io::Error::last_os_error();
            if source.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                continue;
            }
            return Err(WindowsInputError::Api {
                operation: "GetRawInputData(packet)",
                source,
            });
        }
        if copied < size_of::<RAWINPUTHEADER>() as u32 || copied > capacity || bytes > capacity {
            return Err(WindowsInputError::Invalid(
                "native Raw Input copy returned an invalid size",
            ));
        }
        // SAFETY: bytes are initialized inside words; copied cannot exceed
        // allocated capacity. Copying owns the result before the backing words drop.
        return Ok(unsafe {
            std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), copied as usize)
        }
        .to_vec());
    }
    Err(WindowsInputError::Invalid(
        "native packet changed during three query attempts",
    ))
}

fn api_error(operation: &'static str) -> WindowsInputError {
    WindowsInputError::Api {
        operation,
        source: io::Error::last_os_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::SetLastError;

    #[test]
    fn array_queries_use_actual_count_and_reject_bounds_before_allocation() {
        let values = query_array(0u32, "fixture", |pointer, count| {
            // SAFETY: the test closure is called only by query_array with its
            // live count and initialized storage. The queried capacity is 3.
            unsafe {
                if pointer.is_null() {
                    *count = 3;
                    return 0;
                }
                assert_eq!(*count, 3);
                *pointer = 91;
                *count = 1;
            }
            1
        })
        .unwrap();
        assert_eq!(values, vec![91]);
        let mut calls = 0;
        assert!(query_array(0u32, "fixture", |pointer, count| {
            calls += 1;
            assert!(pointer.is_null());
            // SAFETY: count is the query's live writable u32.
            unsafe {
                *count = 4097;
            }
            0
        })
        .is_err());
        assert_eq!(calls, 1);
        assert!(query_array(0u32, "fixture", |pointer, count| {
            // SAFETY: count is live, and no elements are accessed here.
            unsafe {
                *count = 1;
            }
            if pointer.is_null() {
                0
            } else {
                2
            }
        })
        .is_err());
    }

    #[test]
    fn array_retry_exhaustion_is_bounded_and_os_failures_keep_context() {
        let mut fills = 0;
        let error = query_array(0u32, "fixture retry", |pointer, count| {
            // SAFETY: count is live; SetLastError changes only this thread's OS
            // error slot. No fake pointer is dereferenced.
            unsafe {
                *count = 1;
                if pointer.is_null() {
                    return 0;
                }
                fills += 1;
                SetLastError(ERROR_INSUFFICIENT_BUFFER);
            }
            u32::MAX
        })
        .unwrap_err();
        assert_eq!(fills, 3);
        assert!(matches!(error, WindowsInputError::Invalid(_)));
        let error = query_array(0u32, "fixture access", |_, _| {
            // SAFETY: parameterless change to this thread's last-error slot.
            unsafe {
                SetLastError(5);
            }
            u32::MAX
        })
        .unwrap_err();
        match error {
            WindowsInputError::Api { operation, source } => {
                assert_eq!(operation, "fixture access");
                assert_eq!(source.raw_os_error(), Some(5));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
