//! Owner-thread IOHIDManager scalar values or explicitly selected raw reports.
use super::{clock::MachClock, ffi};
use beatkernel::input::*;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    ffi::{CString, c_void},
    marker::PhantomData,
    rc::Rc,
    time::Duration,
};

/// Native IOHID acquisition namespace; HID controls retain their usage identities.
pub const IOHID_BACKEND: BackendId = BackendId(0x4d48_4944);
/// An enumerated device with native identity distinct from session identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidDevice {
    /// Canonical device metadata and reconnect-specific session ID.
    pub descriptor: DeviceDescriptor,
    /// Native IORegistry entry identity, when the service reports it.
    pub registry_entry: Option<u64>,
}

/// Keyboard/keypad primary-usage metadata, without opening an HID manager.
/// Registry identity is native metadata, not a runtime/session DeviceId.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardDevice {
    /// Native positive IORegistry identity, resolved again at acquisition.
    pub registry_entry: u64,
    /// Complete optional UTF-8 product name.
    pub name: Option<String>,
    /// Optional USB/HID vendor identity reported by the service.
    pub vendor_id: Option<u16>,
    /// Optional USB/HID product identity reported by the service.
    pub product_id: Option<u16>,
    /// Complete optional native transport description.
    pub transport: Option<String>,
}

struct DiscoveryObject(u32);
impl Drop for DiscoveryObject {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: each iterator/service returned by IOKit owns one handle.
            unsafe { ffi::IOObjectRelease(self.0) };
        }
    }
}

fn discovery_limits(max_devices: usize, max_text_bytes: usize) -> Result<(), HidError> {
    if !(1..=4096).contains(&max_devices) || !(1..=16384).contains(&max_text_bytes) {
        return Err(HidError::Capacity);
    }
    Ok(())
}

fn keyboard_primary_usage(page: Option<u16>, usage: Option<u16>) -> bool {
    page == Some(1) && matches!(usage, Some(6 | 7))
}

/// Enumerates primary Generic Desktop keyboard/keypad services. The limit
/// counts all scanned IOHIDDevice services, including non-keyboards. Overflow
/// or malformed metadata rejects the whole query, never a partial catalog.
/// No manager open, callback, runloop scheduling, or input acquisition occurs.
pub fn keyboard_devices(
    max_devices: usize,
    max_text_bytes: usize,
) -> Result<Vec<KeyboardDevice>, HidError> {
    discovery_limits(max_devices, max_text_bytes)?;
    // SAFETY: static NUL-terminated IOKit service class, Create-rule dictionary.
    let matching = ffi::OwnedRef(unsafe { ffi::IOServiceMatching(c"IOHIDDevice".as_ptr()) });
    if matching.0.is_null() {
        return Err(HidError::Capacity);
    }
    let dictionary = matching.0;
    // The next FFI call consumes this reference on every return path.
    std::mem::forget(matching);
    let mut iterator = 0u32;
    // SAFETY: main port0 means default; live owned dictionary is transferred;
    // iterator is writable uint32. No reference remains for CFRelease here.
    let status = unsafe { ffi::IOServiceGetMatchingServices(0, dictionary, &mut iterator) };
    let iterator = DiscoveryObject(iterator);
    if status != 0 {
        return Err(HidError::Native(status));
    }
    let mut devices = Vec::new();
    if iterator.0 == 0 {
        return Ok(devices);
    }
    let mut scanned = 0usize;
    loop {
        // SAFETY: iterator is an owned live IOKit handle; next service is owned.
        let service = DiscoveryObject(unsafe { ffi::IOIteratorNext(iterator.0) });
        if service.0 == 0 {
            // SAFETY: same live iterator; Apple requires this check after zero.
            if unsafe { ffi::IOIteratorIsValid(iterator.0) } == 0 {
                return Err(HidError::InvalidMetadata);
            }
            break;
        }
        if scanned == max_devices {
            return Err(HidError::Capacity);
        }
        scanned += 1;
        let page = discovery_number(service.0, c"PrimaryUsagePage")?;
        let usage = discovery_number(service.0, c"PrimaryUsage")?;
        if !keyboard_primary_usage(page, usage) {
            continue;
        }
        let mut registry_entry = 0u64;
        // SAFETY: owned service, writable exact uint64 registry identity.
        let status =
            unsafe { ffi::IORegistryEntryGetRegistryEntryID(service.0, &mut registry_entry) };
        if status != 0 {
            return Err(HidError::Native(status));
        }
        if registry_entry == 0 {
            return Err(HidError::InvalidMetadata);
        }
        let device = KeyboardDevice {
            registry_entry,
            name: discovery_string(service.0, c"Product", max_text_bytes)?,
            vendor_id: discovery_number(service.0, c"VendorID")?,
            product_id: discovery_number(service.0, c"ProductID")?,
            transport: discovery_string(service.0, c"Transport", max_text_bytes)?,
        };
        devices.try_reserve(1).map_err(|_| HidError::Capacity)?;
        devices.push(device);
    }
    devices.sort_by_key(|device| device.registry_entry);
    Ok(devices)
}

fn discovery_property(service: u32, key: &std::ffi::CStr) -> Result<ffi::OwnedRef, HidError> {
    // SAFETY: bounded static C key; created CFString owned until query returns.
    let key = ffi::OwnedRef(unsafe {
        ffi::CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), ffi::UTF8)
    });
    if key.0.is_null() {
        return Err(HidError::Capacity);
    }
    // SAFETY: owned service/live key; Create returns a separately owned optional
    // CF property. Default allocator/null and options0 follow IOKitLib.h.
    Ok(ffi::OwnedRef(unsafe {
        ffi::IORegistryEntryCreateCFProperty(service, key.0, std::ptr::null(), 0)
    }))
}

fn discovery_number(service: u32, key: &std::ffi::CStr) -> Result<Option<u16>, HidError> {
    let value = discovery_property(service, key)?;
    if value.0.is_null() {
        return Ok(None);
    }
    // SAFETY: live owned CF property; inspect type before requesting integer.
    if unsafe { ffi::CFGetTypeID(value.0) != ffi::CFNumberGetTypeID() } {
        return Err(HidError::InvalidMetadata);
    }
    let mut number = 0i64;
    // SAFETY: CFNumberSInt64Type4 writes an exact i64, with conversion checked.
    if unsafe { ffi::CFNumberGetValue(value.0, 4, (&mut number as *mut i64).cast()) } == 0 {
        return Err(HidError::InvalidMetadata);
    }
    Ok(Some(
        u16::try_from(number).map_err(|_| HidError::InvalidMetadata)?,
    ))
}

fn discovery_string(
    service: u32,
    key: &std::ffi::CStr,
    max_bytes: usize,
) -> Result<Option<String>, HidError> {
    let value = discovery_property(service, key)?;
    if value.0.is_null() {
        return Ok(None);
    }
    // SAFETY: live owned property, actual CF type checked before string calls.
    if unsafe { ffi::CFGetTypeID(value.0) != ffi::CFStringGetTypeID() } {
        return Err(HidError::InvalidMetadata);
    }
    // UTF8 requires at least as many bytes as source UTF16 units. Reject before
    // allocating; remaining storage is fixed to the selected byte cap plus NUL.
    let units = usize::try_from(unsafe { ffi::CFStringGetLength(value.0) })
        .map_err(|_| HidError::InvalidMetadata)?;
    if units > max_bytes {
        return Err(HidError::Capacity);
    }
    let capacity = max_bytes + 1; // Caller validates max_bytes <=16384.
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| HidError::Capacity)?;
    bytes.resize(capacity, 0u8);
    // SAFETY: validated string and fully writable cap+NUL buffer; failure does
    // not produce a truncated successful string, including multibyte overflow.
    if unsafe {
        ffi::CFStringGetCString(
            value.0,
            bytes.as_mut_ptr().cast(),
            capacity as isize,
            ffi::UTF8,
        )
    } == 0
    {
        return Err(HidError::Capacity);
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(HidError::InvalidMetadata)?;
    bytes.truncate(end);
    let text = String::from_utf8(bytes).map_err(|_| HidError::InvalidMetadata)?;
    // Detect embedded NUL without silently dropping the remainder of a string.
    if text.encode_utf16().count() != units {
        return Err(HidError::InvalidMetadata);
    }
    Ok(Some(text))
}
/// Canonical value plus exact native representation not expressible in f32 axes.
#[derive(Clone, Debug, PartialEq)]
pub struct HidSample {
    /// Canonical typed physical event, ready for explicit runtime normalization.
    pub event: PhysicalInputEvent,
    /// Exact mach absolute timestamp returned by IOHIDValueGetTimeStamp.
    pub mach_ticks: u64,
    /// Exact unscaled native integer value.
    pub integer_value: i64,
    /// Device-local element cookie, retained separately from canonical identity.
    pub element_cookie: u32,
}
/// Exact timestamped native input report copied before the callback returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidReport {
    /// Canonical acquisition metadata with original native mach provenance.
    pub meta: EventMeta,
    /// Actual native IOHIDReportType value, retained without reinterpretation.
    pub report_type: u32,
    /// Full native uint32 report ID, before canonical width validation.
    pub report_id: u32,
    /// Exact callback bytes, without inferred prefix stripping.
    pub bytes: Vec<u8>,
    /// Arrival mach absolute ticks supplied by the timestamped callback.
    pub mach_ticks: u64,
}
impl HidReport {
    /// Normalizes ID/layout explicitly while retaining this independent envelope.
    /// Hosts select acceptable native report types before vendor routing.
    pub fn to_raw_report(
        &self,
        layout: crate::input::hid_report::NativeReportLayout,
        max_native_bytes: usize,
    ) -> Result<RawHidReportEvent, crate::input::hid_report::HidReportConversionError> {
        crate::input::hid_report::normalize_report(
            self.meta,
            self.report_id,
            &self.bytes,
            layout,
            max_native_bytes,
        )
    }
}
/// Explicit mutually exclusive acquisition paths; raw mode needs macOS 10.15 API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidInputOptions {
    /// Default typed scalar value path, without timestamped report symbol lookup.
    Values {
        /// Maximum queued canonical values, in 1..=65,536.
        queue_capacity: usize,
    },
    /// Raw-only callback path; scalar callbacks are never also registered.
    Reports {
        /// Maximum queued owned native report envelopes, in 1..=65,536.
        queue_capacity: usize,
        /// Maximum bytes per report, in 1..=1 MiB; capacity product <=64 MiB.
        max_report_bytes: usize,
    },
}
/// Off-callback acquisition failure; no fake successful input is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidError {
    /// Malformed discovery properties/registry identity, or invalidated iterator.
    InvalidMetadata,
    /// Input Monitoring/access permission was rejected by IOKit.
    PermissionDenied,
    /// Timestamped report registration API is unavailable; no receipt-time fallback.
    Unsupported,
    /// Native raw report has invalid length or pointer/device representation.
    InvalidReport,
    /// Raw report exceeds its configured byte capacity.
    ReportCapacity,
    /// Native manager/callback failure, preserving the IOReturn code.
    Native(i32),
    /// Configuration/capacity or runtime device/sequence identities exhausted.
    Capacity,
    /// A native clock point is not representable in integer nanoseconds.
    TimestampOverflow,
    /// Callback queue filled; one or more values were dropped explicitly.
    QueueFull,
    /// Manager was explicitly closed; reopen to resume acquisition.
    Closed,
}
impl std::fmt::Display for HidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::InvalidMetadata => f.write_str("invalid IOHID discovery metadata or invalidated iterator"), Self::PermissionDenied => f.write_str("IOHID input permission denied; grant Input Monitoring access to the host application"), Self::Native(code) => write!(f, "IOHID failure {code:#x}"), Self::Unsupported => f.write_str("timestamped IOHID report API unavailable"), Self::InvalidReport => f.write_str("invalid native IOHID report"), Self::ReportCapacity => f.write_str("native IOHID report exceeds byte capacity"), Self::Capacity => f.write_str("IOHID acquisition capacity exhausted"), Self::TimestampOverflow => f.write_str("IOHID mach timestamp overflow"), Self::QueueFull => f.write_str("IOHID acquisition queue full"), Self::Closed => f.write_str("IOHID manager is closed") }
    }
}
impl std::error::Error for HidError {}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Fixed acquisition counters readable on the manager owner thread.
pub struct HidCounters {
    /// Values converted to canonical samples.
    pub accepted: u64,
    /// Values rejected because the bounded acquisition queue filled.
    pub queue_full: u64,
    /// Unsupported/oversized/non-scalar element layouts skipped explicitly.
    pub unsupported: u64,
    /// Devices retired by a native removal callback.
    pub removed: u64,
    /// Timestamped native reports copied into the bounded raw queue.
    pub reports_accepted: u64,
    /// Native reports dropped because the raw queue filled.
    pub reports_queue_full: u64,
    /// Native reports dropped because their byte length exceeded its cap.
    pub reports_oversized: u64,
    /// Reports rejected for invalid native length, pointer or device sender.
    pub reports_invalid: u64,
    /// Report storage reservation failures on the owner input thread.
    pub reports_allocation_failed: u64,
    /// Reports whose supplied mach timestamp cannot be represented.
    pub reports_timestamp_failed: u64,
}
struct DeviceRecord {
    info: HidDevice,
    sequence: u64,
}
struct State {
    clock: MachClock,
    devices: BTreeMap<usize, DeviceRecord>,
    next_device: u64,
    held: HashSet<(DeviceId, PhysicalControlId)>,
    pending: VecDeque<HidSample>,
    reports: VecDeque<HidReport>,
    report_byte_cap: Option<usize>,
    capacity: usize,
    error: Option<HidError>,
    counters: HidCounters,
}
impl State {
    fn register(&mut self, device: ffi::Ref) {
        if self.devices.contains_key(&(device as usize)) {
            return;
        }
        let Some(next) = self.next_device.checked_add(1) else {
            self.error = Some(HidError::Capacity);
            return;
        };
        let identity = DeviceId(self.next_device);
        self.next_device = next;
        // SAFETY: callback/enumeration supplied a live IOHIDDeviceRef; no ownership transfer.
        let service = unsafe { ffi::IOHIDDeviceGetService(device) };
        let mut registry = 0u64;
        // SAFETY: service is borrowed from this live device; output is writable u64.
        let registry_entry = (service != 0
            && unsafe { ffi::IORegistryEntryGetRegistryEntryID(service, &mut registry) } == 0)
            .then_some(registry);
        let name = property(device, "Product").and_then(|value| ffi::cf_string(value));
        let serial = property(device, "SerialNumber").and_then(|value| ffi::cf_string(value));
        let transport = match property(device, "Transport")
            .and_then(|value| ffi::cf_string(value))
            .as_deref()
        {
            Some("USB") => DeviceTransport::Usb,
            Some("Bluetooth" | "BluetoothLowEnergy") => DeviceTransport::Bluetooth,
            _ => DeviceTransport::Unknown,
        };
        let mut device_capabilities = capabilities(device);
        device_capabilities.raw_hid = self.report_byte_cap.is_some();
        let info = HidDevice {
            descriptor: DeviceDescriptor {
                runtime_id: identity,
                vendor_id: number_property(device, "VendorID"),
                product_id: number_property(device, "ProductID"),
                name,
                serial,
                transport,
                capabilities: device_capabilities,
            },
            registry_entry,
        };
        self.devices
            .insert(device as usize, DeviceRecord { info, sequence: 0 });
    }
    fn value(&mut self, value: ffi::Ref) {
        let element = unsafe { ffi::IOHIDValueGetElement(value) };
        if element.is_null() { self.error = Some(HidError::InvalidMetadata); return; }
        let device = unsafe { ffi::IOHIDElementGetDevice(element) };
        self.value_for_device(value, device);
    }
    fn value_for_device(&mut self, value: ffi::Ref, device: ffi::Ref) {
        // SAFETY: IOKit supplied a live IOHIDValueRef for this callback duration.
        let (element, ticks, length) = unsafe {
            (
                ffi::IOHIDValueGetElement(value),
                ffi::IOHIDValueGetTimeStamp(value),
                ffi::IOHIDValueGetLength(value),
            )
        };
        if element.is_null() || length <= 0 || length > 8 {
            self.counters.unsupported = self.counters.unsupported.saturating_add(1);
            return;
        }
        // SAFETY: element borrowed from live IOHIDValue; all queries are synchronous.
        let (page, usage, kind, cookie, integer, relative) = unsafe {
            (
                ffi::IOHIDElementGetUsagePage(element),
                ffi::IOHIDElementGetUsage(element),
                ffi::IOHIDElementGetType(element),
                ffi::IOHIDElementGetCookie(element),
                ffi::IOHIDValueGetIntegerValue(value) as i64,
                ffi::IOHIDElementIsRelative(element) != 0,
            )
        };
        let (Ok(usage_page), Ok(usage)) = (u16::try_from(page), u16::try_from(usage)) else {
            self.counters.unsupported = self.counters.unsupported.saturating_add(1);
            return;
        };
        if !matches!(kind, 1..=4)
            || (kind == 4 && usage_page != KEYBOARD_USAGE_PAGE && usage_page != 9)
        {
            self.counters.unsupported = self.counters.unsupported.saturating_add(1);
            return;
        }
        let Some(clock) = self.clock.at_ticks(ticks) else {
            self.error = Some(HidError::TimestampOverflow);
            return;
        };
        if device.is_null() { self.error = Some(HidError::InvalidMetadata); return; }
        self.register(device);
        let Some(record) = self.devices.get_mut(&(device as usize)) else {
            return;
        };
        let Some(sequence) = record.sequence.checked_add(1) else {
            self.error = Some(HidError::Capacity);
            return;
        };
        record.sequence = sequence;
        let source = record.info.descriptor.runtime_id;
        let control = PhysicalControlId::HidUsage { usage_page, usage };
        let mut meta = EventMeta::new(source, clock.normalized, sequence);
        meta.native = Some(NativeEventMeta {
            backend: IOHID_BACKEND,
            code: Some(cookie),
            timestamp: Some(clock.native),
        });
        meta.original_clock_point = Some(clock.native);
        let event = if kind == 2 || usage_page == KEYBOARD_USAGE_PAGE || usage_page == 9 {
            let state = if integer == 0 {
                self.held.remove(&(source, control));
                ButtonState::Up
            } else if self.held.insert((source, control)) {
                ButtonState::Down
            } else {
                ButtonState::Repeat
            };
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control,
                state,
            })
        } else {
            PhysicalInputEvent::Axis(AxisEvent {
                meta,
                control,
                value: integer as f32,
                mode: if relative {
                    AxisMode::Relative
                } else {
                    AxisMode::Absolute
                },
            })
        };
        if self.pending.len() == self.capacity {
            self.counters.queue_full = self.counters.queue_full.saturating_add(1);
            self.error = Some(HidError::QueueFull);
            return;
        }
        self.pending.push_back(HidSample {
            event,
            mach_ticks: ticks,
            integer_value: integer,
            element_cookie: cookie,
        });
        self.counters.accepted = self.counters.accepted.saturating_add(1);
    }
    unsafe fn report(
        &mut self,
        device: ffi::Ref,
        report_type: u32,
        report_id: u32,
        bytes: *mut u8,
        length: isize,
        ticks: u64,
    ) {
        let Some(cap) = self.report_byte_cap else {
            return;
        };
        if device.is_null() || length < 0 || (length > 0 && bytes.is_null()) {
            self.counters.reports_invalid = self.counters.reports_invalid.saturating_add(1);
            self.error = Some(HidError::InvalidReport);
            return;
        }
        if device.is_null() { self.error = Some(HidError::InvalidMetadata); return; }
        self.register(device);
        let Some(record) = self.devices.get_mut(&(device as usize)) else {
            return;
        };
        let Some(sequence) = record.sequence.checked_add(1) else {
            self.error = Some(HidError::Capacity);
            return;
        };
        record.sequence = sequence;
        let source = record.info.descriptor.runtime_id;
        let length = length as usize; // nonnegative CFIndex fits usize on this ABI
        if length > cap {
            self.counters.reports_oversized = self.counters.reports_oversized.saturating_add(1);
            self.error = Some(HidError::ReportCapacity);
            return;
        }
        if self.reports.len() == self.capacity {
            self.counters.reports_queue_full = self.counters.reports_queue_full.saturating_add(1);
            self.error = Some(HidError::QueueFull);
            return;
        }
        let Some(clock) = self.clock.at_ticks(ticks) else {
            self.counters.reports_timestamp_failed =
                self.counters.reports_timestamp_failed.saturating_add(1);
            self.error = Some(HidError::TimestampOverflow);
            return;
        };
        let mut owned = Vec::new();
        if owned.try_reserve_exact(length).is_err() {
            self.counters.reports_allocation_failed =
                self.counters.reports_allocation_failed.saturating_add(1);
            self.error = Some(HidError::Capacity);
            return;
        }
        if length != 0 {
            // SAFETY: IOKit supplies this live report buffer for the callback,
            // validated non-null and within explicit length cap. Copy before return.
            owned.extend_from_slice(unsafe { std::slice::from_raw_parts(bytes, length) });
        }
        let mut meta = EventMeta::new(source, clock.normalized, sequence);
        meta.native = Some(NativeEventMeta {
            backend: IOHID_BACKEND,
            code: Some(report_id),
            timestamp: Some(clock.native),
        });
        meta.original_clock_point = Some(clock.native);
        self.reports.push_back(HidReport {
            meta,
            report_type,
            report_id,
            bytes: owned,
            mach_ticks: ticks,
        });
        self.counters.reports_accepted = self.counters.reports_accepted.saturating_add(1);
    }
}

/// Actual CFRunLoopRunInMode completion, distinct from callback queue emptiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidPollCompletion {
    /// The requested mode has no remaining sources or timers.
    Finished,
    /// The native runloop was explicitly stopped.
    Stopped,
    /// The requested duration elapsed; this is not a native queue drain proof.
    TimedOut,
    /// One source was handled; further native work may remain.
    HandledSource,
}
fn poll_completion(code: i32) -> Result<HidPollCompletion, HidError> {
    // Apple CFRunLoop.h defines these four return reasons.
    match code {
        1 => Ok(HidPollCompletion::Finished),
        2 => Ok(HidPollCompletion::Stopped),
        3 => Ok(HidPollCompletion::TimedOut),
        4 => Ok(HidPollCompletion::HandledSource),
        _ => Err(HidError::Native(code)),
    }
}

// Checked public queue interface: IOHIDDevicePlugIn.h (HIDFamily v1.5).
// https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDDevicePlugIn.h
const QUEUE_UNDERRUN: i32 = 0xe000_02e7u32 as i32;
fn decode_queue_result(status: i32, present: bool) -> Result<bool, HidError> {
    match (status, present) {
        (0, true) => Ok(true),
        (QUEUE_UNDERRUN, false) => Ok(false),
        (0, false) | (QUEUE_UNDERRUN, true) => Err(HidError::InvalidMetadata),
        _ => { check(status)?; Err(HidError::InvalidMetadata) }
    }
}
/// Cold checked-queue initialization failure with first cleanup failure retained.
/// Legacy callback APIs continue returning the Copy HidError type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueuedInputError {
    /// Original queue initialization error.
    pub primary: HidError,
    /// First failure while closing every partially acquired native lease.
    pub cleanup: Option<HidError>,
}
impl From<HidError> for QueuedInputError {
    fn from(primary: HidError) -> Self { Self { primary, cleanup: None } }
}
impl std::fmt::Display for QueuedInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.primary)?;
        if let Some(cleanup) = self.cleanup { write!(f, "; IOHID queue cleanup: {cleanup}")?; }
        Ok(())
    }
}
impl std::error::Error for QueuedInputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(&self.primary) }
}
struct QueueSweep {
    drained: Vec<bool>,
    cursor: usize,
    remaining: usize,
    before: Option<beatkernel::time::ClockPoint>,
}
impl QueueSweep {
    fn new(count: usize) -> Result<Self, HidError> {
        if count == 0 || count > 64 { return Err(HidError::Capacity); }
        let mut drained = Vec::new();
        drained.try_reserve_exact(count).map_err(|_| HidError::Capacity)?;
        drained.resize(count, false);
        Ok(Self { drained, cursor: 0, remaining: count, before: None })
    }
    fn begin(&mut self, before: beatkernel::time::ClockPoint) {
        if self.before.is_none() {
            self.drained.fill(false);
            self.cursor = 0;
            self.remaining = self.drained.len();
            self.before = Some(before);
        }
    }
    fn index(&mut self) -> usize {
        let index = self.cursor;
        self.cursor = (self.cursor + 1) % self.drained.len();
        index
    }
    fn empty(&mut self, index: usize) -> Option<beatkernel::time::ClockPoint> {
        if !self.drained[index] { self.drained[index] = true; self.remaining -= 1; }
        if self.remaining == 0 { self.before.take() } else { None }
    }
}
trait QueueLease {
    fn stop(&mut self) -> Result<(), HidError>;
    fn close_device(&mut self) -> Result<(), HidError>;
    fn release(&mut self);
}
#[derive(Default)]
struct QueueCleanup {
    started: bool,
    opened: bool,
    retained: Option<HidError>,
    released: bool,
}
impl QueueCleanup {
    fn close(&mut self, lease: &mut impl QueueLease) -> Result<(), HidError> {
        if let Some(error) = self.retained { return Err(error); }
        if self.released { return Ok(()); }
        let mut failure = None;
        if self.started {
            match lease.stop() { Ok(()) => self.started = false, Err(error) => failure = Some(error) }
        }
        if self.opened {
            match lease.close_device() { Ok(()) => self.opened = false, Err(error) => { failure.get_or_insert(error); } }
        }
        if let Some(error) = failure { self.retained = Some(error); return Err(error); }
        lease.release();
        self.released = true;
        Ok(())
    }
}
struct NativeLease {
    plugin: *mut *mut ffi::PluginInterface,
    device: *mut *mut ffi::DeviceInterface,
    queue: *mut *mut ffi::QueueInterface,
}
impl QueueLease for NativeLease {
    fn stop(&mut self) -> Result<(), HidError> {
        check(unsafe { ((**self.queue).stop)(self.queue.cast(), 0) })
    }
    fn close_device(&mut self) -> Result<(), HidError> {
        check(unsafe { ((**self.device).close)(self.device.cast(), 0) })
    }
    fn release(&mut self) {
        // SAFETY: each successful QueryInterface/Create owns one COM reference;
        // no queue callbacks were scheduled, and checked stop/close completed.
        unsafe {
            if !self.queue.is_null() && !(*self.queue).is_null() { ((**self.queue).unknown.release)(self.queue.cast()); }
            if !self.device.is_null() && !(*self.device).is_null() { ((**self.device).unknown.release)(self.device.cast()); }
            if !self.plugin.is_null() && !(*self.plugin).is_null() { ((**self.plugin).unknown.release)(self.plugin.cast()); }
        }
    }
}
struct NativeQueue {
    id: DeviceId,
    device: ffi::Ref,
    owned_device: ffi::OwnedRef,
    plugin: *mut *mut ffi::PluginInterface,
    native_device: *mut *mut ffi::DeviceInterface,
    queue: *mut *mut ffi::QueueInterface,
    cleanup: QueueCleanup,
}
impl NativeQueue {
    fn open(device: ffi::Ref, id: DeviceId, depth: usize) -> Result<Self, QueuedInputError> {
        let mut owner = Self {
            id, device, owned_device: ffi::OwnedRef(unsafe { ffi::CFRetain(device) }),
            plugin: std::ptr::null_mut(), native_device: std::ptr::null_mut(), queue: std::ptr::null_mut(),
            cleanup: QueueCleanup::default(),
        };
        let result = (|| -> Result<(), HidError> {
            // These UUIDs select complete public layouts, not private objects.
            let device_type = ffi::OwnedRef(unsafe { ffi::CFUUIDCreateFromUUIDBytes(std::ptr::null(), ffi::UuidBytes([0x7d,0xde,0xec,0xa8,0xa7,0xb4,0x11,0xda,0x8a,0x0e,0x00,0x14,0x51,0x97,0x58,0xef])) });
            let plugin_type = ffi::OwnedRef(unsafe { ffi::CFUUIDCreateFromUUIDBytes(std::ptr::null(), ffi::UuidBytes([0xc2,0x44,0xe8,0x58,0x10,0x9c,0x11,0xd4,0x91,0xd4,0x00,0x50,0xe4,0xc6,0x42,0x6f])) });
            if device_type.0.is_null() || plugin_type.0.is_null() { return Err(HidError::Capacity); }
            let mut score = 0;
            // SAFETY: exact live selected service; writable public COM output.
            check(unsafe { ffi::IOCreatePlugInInterfaceForService(ffi::IOHIDDeviceGetService(device), device_type.0, plugin_type.0, &mut owner.plugin, &mut score) })?;
            if owner.plugin.is_null() || unsafe { (*owner.plugin).is_null() } { return Err(HidError::InvalidMetadata); }
            let mut native = std::ptr::null_mut();
            let status = unsafe { ((**owner.plugin).unknown.query)(owner.plugin.cast(), ffi::UuidBytes([0x47,0x4b,0xdc,0x8e,0x9f,0x4a,0x11,0xda,0xb3,0x66,0x00,0x0d,0x93,0x6d,0x06,0xd2]), &mut native) };
            owner.native_device = native.cast();
            check(status)?;
            if owner.native_device.is_null() || unsafe { (*owner.native_device).is_null() } { return Err(HidError::InvalidMetadata); }
            check(unsafe { ((**owner.native_device).open)(owner.native_device.cast(), 0) })?;
            owner.cleanup.opened = true;
            let mut queue = std::ptr::null_mut();
            let status = unsafe { ((**owner.plugin).unknown.query)(owner.plugin.cast(), ffi::UuidBytes([0x2e,0xc7,0x8b,0xdb,0x9f,0x4e,0x11,0xda,0xb6,0x5c,0x00,0x0d,0x93,0x6d,0x06,0xd2]), &mut queue) };
            owner.queue = queue.cast();
            check(status)?;
            if owner.queue.is_null() || unsafe { (*owner.queue).is_null() } { return Err(HidError::InvalidMetadata); }
            check(unsafe { ((**owner.queue).set_depth)(owner.queue.cast(), u32::try_from(depth).map_err(|_| HidError::Capacity)?, 0) })?;
            let mut elements = std::ptr::null();
            let status = unsafe { ((**owner.native_device).copy_elements)(owner.native_device.cast(), std::ptr::null(), &mut elements, 0) };
            let elements = ffi::OwnedRef(elements);
            check(status)?;
            if elements.0.is_null() { return Err(HidError::InvalidMetadata); }
            let count = unsafe { ffi::CFArrayGetCount(elements.0) };
            if !(0..=65536).contains(&count) { return Err(HidError::Capacity); }
            let mut added = 0usize;
            for index in 0..count {
                let element = unsafe { ffi::CFArrayGetValueAtIndex(elements.0, index) };
                if element.is_null() { return Err(HidError::InvalidMetadata); }
                let kind = unsafe { ffi::IOHIDElementGetType(element) };
                let page = unsafe { ffi::IOHIDElementGetUsagePage(element) };
                if matches!(kind, 1..=4) && (kind != 4 || page == u32::from(KEYBOARD_USAGE_PAGE) || page == 9) {
                    check(unsafe { ((**owner.queue).add_element)(owner.queue.cast(), element, 0) })?;
                    added += 1;
                }
            }
            if added == 0 { return Err(HidError::InvalidMetadata); }
            // A failed start may have partially activated native delivery; stop
            // is still attempted during initialization cleanup.
            owner.cleanup.started = true;
            check(unsafe { ((**owner.queue).start)(owner.queue.cast(), 0) })?;
            Ok(())
        })();
        if let Err(primary) = result {
            return Err(QueuedInputError { primary, cleanup: owner.close().err() });
        }
        Ok(owner)
    }
    fn next(&mut self) -> Result<Option<ffi::OwnedRef>, HidError> {
        let mut value = std::ptr::null();
        // SAFETY: checked UUID queue, worker-local owner, nonblocking timeout0.
        let status = unsafe { ((**self.queue).copy_next_value)(self.queue.cast(), &mut value, 0, 0) };
        let value = ffi::OwnedRef(value);
        if decode_queue_result(status, !value.0.is_null())? { Ok(Some(value)) } else { Ok(None) }
    }
    fn close(&mut self) -> Result<(), HidError> {
        let mut lease = NativeLease { plugin: self.plugin, device: self.native_device, queue: self.queue };
        self.cleanup.close(&mut lease)?;
        self.queue = std::ptr::null_mut(); self.native_device = std::ptr::null_mut(); self.plugin = std::ptr::null_mut();
        Ok(())
    }

}
impl Drop for NativeQueue {
    fn drop(&mut self) {
        let _ = self.close();
        if self.cleanup.retained.is_some() {
            std::mem::forget(std::mem::replace(&mut self.owned_device, ffi::OwnedRef(std::ptr::null())));
        }
    }
}

/// Native manager pinned to its creating runloop/thread, intentionally !Send/!Sync.
/// All callbacks run only while this owner polls that runloop. Input callback
/// allocations are off the audio path; bounded value/report queues report overflow.
pub struct HidInput {
    manager: ffi::OwnedRef,
    runloop: ffi::OwnedRef,
    state: Box<State>,
    owner_thread: PhantomData<Rc<()>>,
    closed: bool,
    report_registration: Option<ffi::RegisterTimestampedReport>,
    queued: bool,
    queues: Vec<NativeQueue>,
    sweep: Option<QueueSweep>,
}
impl HidInput {
    /// Opens all HID devices without seizure on the current thread's runloop.
    /// `first_device` reserves a caller-controlled range of session identities.
    pub fn open(
        clock: MachClock,
        first_device: DeviceId,
        queue_capacity: usize,
    ) -> Result<Self, HidError> {
        Self::open_with_options(
            clock,
            first_device,
            HidInputOptions::Values { queue_capacity },
        )
    }
    /// Opens only timestamped raw reports; missing macOS API is explicit Unsupported.
    pub fn open_reports(
        clock: MachClock,
        first_device: DeviceId,
        queue_capacity: usize,
        max_report_bytes: usize,
    ) -> Result<Self, HidError> {
        Self::open_with_options(
            clock,
            first_device,
            HidInputOptions::Reports {
                queue_capacity,
                max_report_bytes,
            },
        )
    }
    /// Selects one acquisition mode on this creating thread's runloop.
    pub fn open_with_options(
        clock: MachClock,
        first_device: DeviceId,
        options: HidInputOptions,
    ) -> Result<Self, HidError> {
        Self::open_mode(clock, first_device, options, false)
    }
    /// Opens an owner-thread manager for explicit checked synchronous queues.
    /// No scalar callback is registered; configure the pinned selection before polling.
    pub fn open_queued(clock: MachClock, first_device: DeviceId, queue_capacity: usize) -> Result<Self, HidError> {
        Self::open_mode(clock, first_device, HidInputOptions::Values { queue_capacity }, true)
    }
    fn open_mode(clock: MachClock, first_device: DeviceId, options: HidInputOptions, queued: bool) -> Result<Self, HidError> {
        let (queue_capacity, report_byte_cap) = match options {
            HidInputOptions::Values { queue_capacity } => (queue_capacity, None),
            HidInputOptions::Reports {
                queue_capacity,
                max_report_bytes,
            } => (queue_capacity, Some(max_report_bytes)),
        };
        if queue_capacity == 0 || queue_capacity > 65_536 {
            return Err(HidError::Capacity);
        }
        let report_registration = if let Some(bytes) = report_byte_cap {
            if bytes == 0
                || bytes > 1_048_576
                || queue_capacity
                    .checked_mul(bytes)
                    .is_none_or(|total| total > 67_108_864)
            {
                return Err(HidError::Capacity);
            }
            Some(ffi::timestamped_report_registration().ok_or(HidError::Unsupported)?)
        } else {
            None
        };
        // SAFETY: null allocator selects CF default; no pointer ownership arguments.
        let manager = ffi::OwnedRef(unsafe { ffi::IOHIDManagerCreate(std::ptr::null(), 0) });
        if manager.0.is_null() {
            return Err(HidError::Capacity);
        }
        // SAFETY: current runloop is a borrowed live reference retained for this owner.
        let runloop = ffi::OwnedRef(unsafe { ffi::CFRetain(ffi::CFRunLoopGetCurrent()) });
        let mut pending = VecDeque::new();
        if report_byte_cap.is_none() {
            pending
                .try_reserve_exact(queue_capacity)
                .map_err(|_| HidError::Capacity)?;
        }
        let mut reports = VecDeque::new();
        if report_byte_cap.is_some() {
            reports
                .try_reserve_exact(queue_capacity)
                .map_err(|_| HidError::Capacity)?;
        }
        let state = Box::new(State {
            clock,
            devices: BTreeMap::new(),
            next_device: first_device.0,
            held: HashSet::new(),
            pending,
            reports,
            report_byte_cap,
            capacity: queue_capacity,
            error: None,
            counters: HidCounters::default(),
        });
        let mut input = Self {
            manager,
            runloop,
            state,
            owner_thread: PhantomData,
            closed: false,
            report_registration,
            queued,
            queues: Vec::new(),
            sweep: None,
        };
        let context = (&mut *input.state as *mut State).cast();
        // SAFETY: boxed context has stable address, remains owned until unschedule,
        // unregister and close; callbacks dispatched only on this owner runloop.
        unsafe {
            ffi::IOHIDManagerSetDeviceMatching(input.manager.0, std::ptr::null());
            ffi::IOHIDManagerRegisterDeviceMatchingCallback(
                input.manager.0,
                Some(matched),
                context,
            );
            ffi::IOHIDManagerRegisterDeviceRemovalCallback(input.manager.0, Some(removed), context);
            if let Some(register) = input.report_registration {
                register(input.manager.0, Some(report), context);
            } else if !queued {
                ffi::IOHIDManagerRegisterInputValueCallback(input.manager.0, Some(value), context);
            }
            ffi::IOHIDManagerScheduleWithRunLoop(
                input.manager.0,
                input.runloop.0,
                ffi::kCFRunLoopDefaultMode,
            );
        }
        // SAFETY: live scheduled manager with stable registered callbacks; no seizure.
        check(unsafe { ffi::IOHIDManagerOpen(input.manager.0, 0) })?;
        // SAFETY: Copy returns an owned nullable CFSet of live device references.
        let devices = ffi::OwnedRef(unsafe { ffi::IOHIDManagerCopyDevices(input.manager.0) });
        if !devices.0.is_null() {
            // SAFETY: set is live and its count controls the allocated output capacity.
            let count = unsafe { ffi::CFSetGetCount(devices.0) };
            let mut values =
                vec![std::ptr::null(); usize::try_from(count).map_err(|_| HidError::Capacity)?];
            // SAFETY: values has exactly count writable Ref elements.
            unsafe { ffi::CFSetGetValues(devices.0, values.as_mut_ptr()) };
            for device in values {
                input.state.register(device);
            }
        }
        if let Some(error) = input.state.error.take() {
            return Err(error);
        }
        Ok(input)
    }
    /// Installs one checked native queue for every exact admitted runtime identity.
    /// Only a fresh queued manager may configure the roster; no callback migration.
    pub fn select_queued_devices(&mut self, selected: &[DeviceId]) -> Result<(), QueuedInputError> {
        if self.closed { return Err(HidError::Closed.into()); }
        if !self.queued || !self.queues.is_empty() || selected.is_empty() || selected.len() > 64 { return Err(HidError::Capacity.into()); }
        // Validate/allocate the complete pinned roster before acquiring native leases.
        let mut devices = Vec::new();
        devices.try_reserve_exact(selected.len()).map_err(|_| HidError::Capacity)?;
        for (index, &id) in selected.iter().enumerate() {
            if selected[..index].contains(&id) { return Err(HidError::InvalidMetadata.into()); }
            let (&device, _) = self.state.devices.iter().find(|(_, record)| record.info.descriptor.runtime_id == id).ok_or(HidError::InvalidMetadata)?;
            devices.push((device as ffi::Ref, id));
        }
        let sweep = QueueSweep::new(selected.len())?;
        let mut queues = Vec::new();
        queues.try_reserve_exact(selected.len()).map_err(|_| HidError::Capacity)?;
        for (device, id) in devices {
            match NativeQueue::open(device, id, self.state.capacity.min(1024)) {
                Ok(queue) => queues.push(queue),
                Err(mut error) => {
                    for queue in &mut queues {
                        if let Err(cleanup) = queue.close() { error.cleanup.get_or_insert(cleanup); }
                    }
                    return Err(error);
                }
            }
        }
        self.sweep = Some(sweep);
        self.queues = queues;
        Ok(())
    }
    /// Bounded fair checked queue sweep. Cut describes the observation BEFORE
    /// this sweep, and appears only after every selected queue returned Underrun.
    /// Kernel/driver losses not exposed by the native API remain unmeasured.
    pub fn poll_queued(&mut self, quantum: usize) -> Result<Option<beatkernel::time::ClockPoint>, HidError> {
        if self.closed { return Err(HidError::Closed); }
        if !self.queued || self.queues.is_empty() || quantum == 0 || quantum > 65536 { return Err(HidError::Capacity); }
        let sweep = self.sweep.as_mut().ok_or(HidError::InvalidMetadata)?;
        if sweep.before.is_none() {
            let before = self.state.clock.sample().map_err(|_| HidError::TimestampOverflow)?.normalized;
            sweep.begin(before);
        }
        for _ in 0..quantum {
            let index = sweep.index();
            if sweep.drained[index] { continue; }
            let queue = &mut self.queues[index];
            if self.state.devices.get(&(queue.device as usize)).is_none_or(|record| record.info.descriptor.runtime_id != queue.id) { return Err(HidError::Closed); }
            match queue.next()? {
                Some(value) => {
                    self.state.value_for_device(value.0, queue.device);
                    if let Some(error) = self.state.error.take() { return Err(error); }
                }
                None => { if let Some(cut) = sweep.empty(index) { return Ok(Some(cut)); } }
            }
        }
        Ok(None)
    }
    /// Pumps at most the requested duration or one runloop source on owner thread.
    /// Other sources registered in this default runloop mode may also run.
    pub fn poll(&mut self, timeout: Duration) -> Result<(), HidError> {
        self.poll_completion(timeout).map(|_| ())
    }
    /// Returns the native runloop reason, which alone does not prove native queues empty.
    pub fn poll_completion(&mut self, timeout: Duration) -> Result<HidPollCompletion, HidError> {
        if self.closed {
            return Err(HidError::Closed);
        }
        // SAFETY: owner-thread runloop with callbacks pointing at this stable box.
        let reason = unsafe {
            ffi::CFRunLoopRunInMode(ffi::kCFRunLoopDefaultMode, timeout.as_secs_f64(), 1)
        };
        if let Some(error) = self.state.error.take() {
            return Err(error);
        }
        poll_completion(reason)
    }
    /// Copies active devices; removal retires IDs and reconnect allocates new IDs.
    pub fn devices(&self) -> Vec<HidDevice> {
        self.state
            .devices
            .values()
            .map(|record| record.info.clone())
            .collect()
    }
    /// Copies at most two registry matches without copying names or allocating.
    /// Two matches suffice to report ambiguity; this does not select a device.
    /// No references to callback-owned records escape across runloop pumping.
    pub fn registry_candidates(&self, registry: u64) -> [Option<(DeviceId, bool)>; 2] {
        let mut matches = self
            .state
            .devices
            .values()
            .filter(|record| record.info.registry_entry == Some(registry))
            .map(|record| {
                (
                    record.info.descriptor.runtime_id,
                    record.info.descriptor.capabilities.button,
                )
            });
        [matches.next(), matches.next()]
    }
    /// Checks an exact pinned identity without copying device metadata.
    pub fn has_registry_attachment(&self, registry: u64, device: DeviceId) -> bool {
        self.state.devices.values().any(|record| {
            record.info.registry_entry == Some(registry)
                && record.info.descriptor.runtime_id == device
        })
    }
    /// Removes the next acquisition-order sample without sorting native time.
    pub fn pop(&mut self) -> Option<HidSample> {
        self.state.pending.pop_front()
    }
    /// Removes the next raw-only envelope in received callback order.
    /// Default scalar mode always returns None; layout conversion stays explicit.
    pub fn pop_report(&mut self) -> Option<HidReport> {
        self.state.reports.pop_front()
    }
    /// Current bounded queue/callback diagnostics.
    pub const fn counters(&self) -> HidCounters {
        self.state.counters
    }
}
impl HidInput {
    /// Unschedules/unregisters callbacks before closing; reports native failure.
    /// Pending samples/reports remain drainable, but polling requires an open manager.
    pub fn close(&mut self) -> Result<(), HidError> {
        if self.closed {
            return Ok(());
        }
        let mut queue_error = None;
        for queue in &mut self.queues {
            if let Err(error) = queue.close() { queue_error.get_or_insert(error); }
        }
        // SAFETY: owner thread only. Unschedule/unregister before close and before
        // state destruction so no later runloop callback can reference its box.
        unsafe {
            ffi::IOHIDManagerUnscheduleFromRunLoop(
                self.manager.0,
                self.runloop.0,
                ffi::kCFRunLoopDefaultMode,
            );
            if let Some(register) = self.report_registration {
                register(self.manager.0, None, std::ptr::null_mut());
            } else {
                ffi::IOHIDManagerRegisterInputValueCallback(
                    self.manager.0,
                    None,
                    std::ptr::null_mut(),
                );
            }
            ffi::IOHIDManagerRegisterDeviceMatchingCallback(
                self.manager.0,
                None,
                std::ptr::null_mut(),
            );
            ffi::IOHIDManagerRegisterDeviceRemovalCallback(
                self.manager.0,
                None,
                std::ptr::null_mut(),
            );
        }
        self.closed = true;
        // SAFETY: manager remains owned; callbacks no longer retain our context.
        let manager_close = check(unsafe { ffi::IOHIDManagerClose(self.manager.0, 0) });
        queue_error.map_or(manager_close, Err)
    }
}
impl Drop for HidInput {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
unsafe extern "C" fn matched(context: *mut c_void, status: i32, _: *mut c_void, device: ffi::Ref) {
    // SAFETY: callback context is stable owned State, exclusively dispatched on
    // its creating runloop; owner holds no State borrow while pumping sources.
    let state = unsafe { &mut *context.cast::<State>() };
    if let Err(error) = check(status) {
        state.error = Some(error);
        return;
    }
    if !device.is_null() {
        state.register(device);
    }
}
unsafe extern "C" fn removed(context: *mut c_void, status: i32, _: *mut c_void, device: ffi::Ref) {
    // SAFETY: same owner-thread/stable State callback contract as matched.
    let state = unsafe { &mut *context.cast::<State>() };
    if let Err(error) = check(status) {
        state.error = Some(error);
        return;
    }
    if let Some(record) = state.devices.remove(&(device as usize)) {
        state
            .held
            .retain(|(source, _)| *source != record.info.descriptor.runtime_id);
        state.counters.removed = state.counters.removed.saturating_add(1);
    }
}
unsafe extern "C" fn value(context: *mut c_void, status: i32, _: *mut c_void, value: ffi::Ref) {
    // SAFETY: same owner-thread/stable State callback contract as matched.
    let state = unsafe { &mut *context.cast::<State>() };
    if let Err(error) = check(status) {
        state.error = Some(error);
        return;
    }
    if !value.is_null() {
        state.value(value);
    }
}
unsafe extern "C" fn report(
    context: *mut c_void,
    status: i32,
    sender: *mut c_void,
    report_type: u32,
    report_id: u32,
    bytes: *mut u8,
    length: isize,
    ticks: u64,
) {
    // SAFETY: exact timestamped IOHID ABI. Manager/device forwarding supplies
    // IOHIDDeviceRef sender; context is boxed, owner-thread-only State until
    // unschedule/unregister. Buffer is borrowed for this callback duration.
    let state = unsafe { &mut *context.cast::<State>() };
    if let Err(error) = check(status) {
        state.error = Some(error);
        return;
    }
    // SAFETY: native sender/buffer validity belongs to the callback ABI;
    // State additionally validates null/negative/oversized representations.
    unsafe {
        state.report(
            sender.cast_const(),
            report_type,
            report_id,
            bytes,
            length,
            ticks,
        )
    };
}
fn check(status: i32) -> Result<(), HidError> {
    match status as u32 {
        0 => Ok(()),
        0xe000_02e2 | 0xe000_02c1 => Err(HidError::PermissionDenied),
        _ => Err(HidError::Native(status)),
    }
}
fn property(device: ffi::Ref, key: &str) -> Option<ffi::Ref> {
    let name = CString::new(key).ok()?;
    // SAFETY: valid NUL-terminated key; Create reference is owned until query ends.
    let key = ffi::OwnedRef(unsafe {
        ffi::CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), ffi::UTF8)
    });
    if key.0.is_null() {
        return None;
    }
    // SAFETY: live enumerated device and CFString key; property is borrowed from device.
    let value = unsafe { ffi::IOHIDDeviceGetProperty(device, key.0) };
    (!value.is_null()).then_some(value)
}
fn number_property(device: ffi::Ref, key: &str) -> Option<u16> {
    let value = property(device, key)?;
    // SAFETY: inspect actual CF type before interpreting it as a number.
    if unsafe { ffi::CFGetTypeID(value) != ffi::CFNumberGetTypeID() } {
        return None;
    }
    let mut number = 0i64;
    // SAFETY: CFNumberSInt64Type=4; writable i64 exactly matches its output representation.
    if unsafe { ffi::CFNumberGetValue(value, 4, (&mut number as *mut i64).cast()) } == 0 {
        return None;
    }
    u16::try_from(number).ok()
}
fn capabilities(device: ffi::Ref) -> DeviceCapabilities {
    let mut result = DeviceCapabilities::default();
    // SAFETY: Copy creates an owned optional element array for this live device.
    let elements =
        ffi::OwnedRef(unsafe { ffi::IOHIDDeviceCopyMatchingElements(device, std::ptr::null(), 0) });
    if elements.0.is_null() {
        return result;
    }
    // SAFETY: native array count bounds subsequent element access.
    let count = unsafe { ffi::CFArrayGetCount(elements.0) };
    for index in 0..count {
        // SAFETY: index is within this live array of IOHIDElementRef values.
        let (kind, page) = unsafe {
            let element = ffi::CFArrayGetValueAtIndex(elements.0, index);
            (
                ffi::IOHIDElementGetType(element),
                ffi::IOHIDElementGetUsagePage(element),
            )
        };
        if matches!(kind, 1..=4)
            && (kind != 4 || page == u32::from(KEYBOARD_USAGE_PAGE) || page == 9)
        {
            if kind == 2 || page == u32::from(KEYBOARD_USAGE_PAGE) || page == 9 {
                result.button = true;
            } else {
                result.axis = true;
            }
        }
    }
    result
}

#[cfg(test)]
mod discovery_fixtures {
    use super::*;

    #[test]
    fn primary_keyboard_keypad_and_discovery_limits_are_explicit() {
        assert!(keyboard_primary_usage(Some(1), Some(6)));
        assert!(keyboard_primary_usage(Some(1), Some(7)));
        assert!(!keyboard_primary_usage(Some(1), Some(2)));
        assert!(!keyboard_primary_usage(Some(7), Some(6)));
        assert!(!keyboard_primary_usage(None, Some(6)));
        assert!(discovery_limits(1, 1).is_ok());
        assert!(discovery_limits(4096, 16384).is_ok());
        for (count, bytes) in [(0, 1), (4097, 1), (1, 0), (1, 16385)] {
            assert_eq!(discovery_limits(count, bytes), Err(HidError::Capacity));
        }
    }
}

#[cfg(test)]
#[path = "input_fixtures.rs"]
mod input_fixtures;
