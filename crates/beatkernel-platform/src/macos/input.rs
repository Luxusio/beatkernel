//! Owner-thread IOHIDManager device enumeration and canonical value acquisition.
use super::{clock::MachClock, ffi};
use beatkernel::input::*;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    ffi::{c_void, CString},
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
/// Off-callback acquisition failure; no fake successful input is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidError {
    /// Input Monitoring/access permission was rejected by IOKit.
    PermissionDenied,
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
        match self { Self::PermissionDenied => f.write_str("IOHID input permission denied; grant Input Monitoring access to the host application"), Self::Native(code) => write!(f, "IOHID failure {code:#x}"), Self::Capacity => f.write_str("IOHID acquisition capacity exhausted"), Self::TimestampOverflow => f.write_str("IOHID mach timestamp overflow"), Self::QueueFull => f.write_str("IOHID acquisition queue full"), Self::Closed => f.write_str("IOHID manager is closed") }
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
        let info = HidDevice {
            descriptor: DeviceDescriptor {
                runtime_id: identity,
                vendor_id: number_property(device, "VendorID"),
                product_id: number_property(device, "ProductID"),
                name,
                serial,
                transport,
                capabilities: capabilities(device),
            },
            registry_entry,
        };
        self.devices
            .insert(device as usize, DeviceRecord { info, sequence: 0 });
    }
    fn value(&mut self, value: ffi::Ref) {
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
        let (device, page, usage, kind, cookie, integer, relative) = unsafe {
            (
                ffi::IOHIDElementGetDevice(element),
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
}

/// Native manager pinned to its creating runloop/thread, intentionally !Send/!Sync.
/// All callbacks run only while this owner polls that runloop. Input callback
/// allocations are off the audio path; the bounded value queue reports overflow.
pub struct HidInput {
    manager: ffi::OwnedRef,
    runloop: ffi::OwnedRef,
    state: Box<State>,
    owner_thread: PhantomData<Rc<()>>,
    closed: bool,
}
impl HidInput {
    /// Opens all HID devices without seizure on the current thread's runloop.
    /// `first_device` reserves a caller-controlled range of session identities.
    pub fn open(
        clock: MachClock,
        first_device: DeviceId,
        queue_capacity: usize,
    ) -> Result<Self, HidError> {
        if queue_capacity == 0 {
            return Err(HidError::Capacity);
        }
        // SAFETY: null allocator selects CF default; no pointer ownership arguments.
        let manager = ffi::OwnedRef(unsafe { ffi::IOHIDManagerCreate(std::ptr::null(), 0) });
        if manager.0.is_null() {
            return Err(HidError::Capacity);
        }
        // SAFETY: current runloop is a borrowed live reference retained for this owner.
        let runloop = ffi::OwnedRef(unsafe { ffi::CFRetain(ffi::CFRunLoopGetCurrent()) });
        let mut pending = VecDeque::new();
        pending
            .try_reserve_exact(queue_capacity)
            .map_err(|_| HidError::Capacity)?;
        let state = Box::new(State {
            clock,
            devices: BTreeMap::new(),
            next_device: first_device.0,
            held: HashSet::new(),
            pending,
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
            ffi::IOHIDManagerRegisterInputValueCallback(input.manager.0, Some(value), context);
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
    /// Pumps at most the requested duration or one runloop source on owner thread.
    /// Other sources registered in this default runloop mode may also run.
    pub fn poll(&mut self, timeout: Duration) -> Result<(), HidError> {
        if self.closed {
            return Err(HidError::Closed);
        }
        // SAFETY: owner-thread runloop with callbacks pointing at this stable box.
        unsafe { ffi::CFRunLoopRunInMode(ffi::kCFRunLoopDefaultMode, timeout.as_secs_f64(), 1) };
        self.state.error.take().map_or(Ok(()), Err)
    }
    /// Copies active devices; removal retires IDs and reconnect allocates new IDs.
    pub fn devices(&self) -> Vec<HidDevice> {
        self.state
            .devices
            .values()
            .map(|record| record.info.clone())
            .collect()
    }
    /// Removes the next acquisition-order sample without sorting native time.
    pub fn pop(&mut self) -> Option<HidSample> {
        self.state.pending.pop_front()
    }
    /// Current bounded queue/callback diagnostics.
    pub const fn counters(&self) -> HidCounters {
        self.state.counters
    }
}
impl HidInput {
    /// Unschedules/unregisters callbacks before closing; reports native failure.
    /// Pending samples remain drainable, but polling requires an open manager.
    pub fn close(&mut self) -> Result<(), HidError> {
        if self.closed {
            return Ok(());
        }
        // SAFETY: owner thread only. Unschedule/unregister before close and before
        // state destruction so no later runloop callback can reference its box.
        unsafe {
            ffi::IOHIDManagerUnscheduleFromRunLoop(
                self.manager.0,
                self.runloop.0,
                ffi::kCFRunLoopDefaultMode,
            );
            ffi::IOHIDManagerRegisterInputValueCallback(self.manager.0, None, std::ptr::null_mut());
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
        check(unsafe { ffi::IOHIDManagerClose(self.manager.0, 0) })
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
