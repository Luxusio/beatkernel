//! Narrow C ABI declarations; layouts follow Apple's public headers.
use std::ffi::{c_char, c_void};
pub(super) type Ref = *const c_void;
pub(super) type HidCallback = unsafe extern "C" fn(*mut c_void, i32, *mut c_void, Ref);
// IOHIDBase.h: CFIndex is signed pointer-sized, IOHIDReportType/ID are uint32.
pub(super) type HidTimestampedReportCallback =
    unsafe extern "C" fn(*mut c_void, i32, *mut c_void, u32, u32, *mut u8, isize, u64);
pub(super) type RegisterTimestampedReport =
    unsafe extern "C" fn(Ref, Option<HidTimestampedReportCallback>, *mut c_void);
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
pub(super) fn timestamped_report_registration() -> Option<RegisterTimestampedReport> {
    // SAFETY: Apple's dlfcn.h defines RTLD_DEFAULT as (void*)-2. The name is a
    // static NUL-terminated system IOKit symbol; IOKit is linked for our lifetime.
    let address = unsafe {
        dlsym(
            (-2isize) as *mut c_void,
            c"IOHIDManagerRegisterInputReportWithTimeStampCallback".as_ptr(),
        )
    };
    if address.is_null() {
        return None;
    }
    // SAFETY: exact named Apple public function and callback signature above;
    // Darwin dlsym returns a function address representable as this C function pointer.
    Some(unsafe { std::mem::transmute::<*mut c_void, RegisterTimestampedReport>(address) })
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Timebase {
    pub numer: u32,
    pub denom: u32,
}
unsafe extern "C" {
    pub fn mach_absolute_time() -> u64;
    pub fn mach_timebase_info(info: *mut Timebase) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFRunLoopDefaultMode: Ref;
    pub fn CFRetain(value: Ref) -> Ref;
    pub fn CFRelease(value: Ref);
    pub fn CFGetTypeID(value: Ref) -> usize;
    pub fn CFStringGetTypeID() -> usize;
    pub fn CFNumberGetTypeID() -> usize;
    pub fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32) -> Ref;
    pub fn CFStringGetLength(value: Ref) -> isize;
    pub fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    pub fn CFStringGetCString(value: Ref, buffer: *mut c_char, length: isize, encoding: u32) -> u8;
    pub fn CFNumberGetValue(value: Ref, kind: isize, output: *mut c_void) -> u8;
    pub fn CFSetGetCount(value: Ref) -> isize;
    pub fn CFSetGetValues(value: Ref, output: *mut Ref);
    pub fn CFArrayGetCount(value: Ref) -> isize;
    pub fn CFArrayGetValueAtIndex(value: Ref, index: isize) -> Ref;
    pub fn CFRunLoopGetCurrent() -> Ref;
    pub fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: u8) -> i32;
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    // IOKitLib.h: io_object_t/io_iterator_t are user-space mach_port_t (uint32);
    // GetMatchingServices consumes one CF dictionary reference even on error.
    pub fn IOServiceMatching(name: *const c_char) -> Ref;
    pub fn IOServiceGetMatchingServices(port: u32, matching: Ref, iterator: *mut u32) -> i32;
    pub fn IOIteratorNext(iterator: u32) -> u32;
    pub fn IOIteratorIsValid(iterator: u32) -> u32;
    pub fn IOObjectRelease(object: u32) -> i32;
    pub fn IORegistryEntryCreateCFProperty(
        service: u32,
        key: Ref,
        allocator: Ref,
        options: u32,
    ) -> Ref;
    pub fn IOHIDManagerCreate(allocator: Ref, options: u32) -> Ref;
    pub fn IOHIDManagerSetDeviceMatching(manager: Ref, matching: Ref);
    pub fn IOHIDManagerOpen(manager: Ref, options: u32) -> i32;
    pub fn IOHIDManagerClose(manager: Ref, options: u32) -> i32;
    pub fn IOHIDManagerCopyDevices(manager: Ref) -> Ref;
    pub fn IOHIDManagerScheduleWithRunLoop(manager: Ref, runloop: Ref, mode: Ref);
    pub fn IOHIDManagerUnscheduleFromRunLoop(manager: Ref, runloop: Ref, mode: Ref);
    pub fn IOHIDManagerRegisterInputValueCallback(
        manager: Ref,
        callback: Option<HidCallback>,
        context: *mut c_void,
    );
    pub fn IOHIDManagerRegisterDeviceMatchingCallback(
        manager: Ref,
        callback: Option<HidCallback>,
        context: *mut c_void,
    );
    pub fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: Ref,
        callback: Option<HidCallback>,
        context: *mut c_void,
    );
    pub fn IOHIDDeviceGetProperty(device: Ref, key: Ref) -> Ref;
    pub fn IOHIDDeviceGetService(device: Ref) -> u32;
    pub fn IORegistryEntryGetRegistryEntryID(service: u32, identity: *mut u64) -> i32;
    pub fn IOHIDDeviceCopyMatchingElements(device: Ref, matching: Ref, options: u32) -> Ref;
    pub fn IOHIDElementGetDevice(element: Ref) -> Ref;
    pub fn IOHIDElementGetUsagePage(element: Ref) -> u32;
    pub fn IOHIDElementGetUsage(element: Ref) -> u32;
    pub fn IOHIDElementGetType(element: Ref) -> u32;
    pub fn IOHIDElementGetCookie(element: Ref) -> u32;
    pub fn IOHIDElementIsRelative(element: Ref) -> u8;
    pub fn IOHIDValueGetElement(value: Ref) -> Ref;
    pub fn IOHIDValueGetTimeStamp(value: Ref) -> u64;
    pub fn IOHIDValueGetIntegerValue(value: Ref) -> isize;
    pub fn IOHIDValueGetLength(value: Ref) -> isize;
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct PropertyAddress {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Asbd {
    pub sample_rate: f64,
    pub format_id: u32,
    pub format_flags: u32,
    pub bytes_per_packet: u32,
    pub frames_per_packet: u32,
    pub bytes_per_frame: u32,
    pub channels_per_frame: u32,
    pub bits_per_channel: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct AudioBuffer {
    pub channels: u32,
    pub byte_size: u32,
    pub data: *mut c_void,
}
#[repr(C)]
pub(super) struct AudioBufferList {
    pub count: u32,
    pub buffers: [AudioBuffer; 1],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct SmpteTime {
    pub subframes: i16,
    pub divisor: i16,
    pub counter: u32,
    pub kind: u32,
    pub flags: u32,
    pub hours: i16,
    pub minutes: i16,
    pub seconds: i16,
    pub frames: i16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct AudioTimestamp {
    pub sample_time: f64,
    pub host_time: u64,
    pub rate_scalar: f64,
    pub word_clock_time: u64,
    pub smpte: SmpteTime,
    pub flags: u32,
    pub reserved: u32,
}
pub(super) type IoProc = unsafe extern "C" fn(
    u32,
    *const AudioTimestamp,
    *const AudioBufferList,
    *const AudioTimestamp,
    *mut AudioBufferList,
    *const AudioTimestamp,
    *mut c_void,
) -> i32;
pub(super) type IoProcId = *mut c_void;
pub(super) type PropertyListener =
    unsafe extern "C" fn(u32, u32, *const PropertyAddress, *mut c_void) -> i32;
#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    pub fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    pub fn AudioObjectGetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        output: *mut c_void,
    ) -> i32;
    pub fn AudioObjectSetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        input: *const c_void,
    ) -> i32;
    pub fn AudioObjectAddPropertyListener(
        object: u32,
        address: *const PropertyAddress,
        callback: PropertyListener,
        context: *mut c_void,
    ) -> i32;
    pub fn AudioObjectRemovePropertyListener(
        object: u32,
        address: *const PropertyAddress,
        callback: PropertyListener,
        context: *mut c_void,
    ) -> i32;
    pub fn AudioDeviceCreateIOProcID(
        device: u32,
        callback: IoProc,
        context: *mut c_void,
        id: *mut IoProcId,
    ) -> i32;
    pub fn AudioDeviceDestroyIOProcID(device: u32, id: IoProcId) -> i32;
    pub fn AudioDeviceStart(device: u32, id: IoProcId) -> i32;
    pub fn AudioDeviceStop(device: u32, id: IoProcId) -> i32;
}
pub(super) const UTF8: u32 = 0x0800_0100;
/// Owned CF reference returned by a Create/Copy operation.
pub(super) struct OwnedRef(pub Ref);
impl Drop for OwnedRef {
    fn drop(&mut self) {
        // SAFETY: each nonnull Create/Copy reference is released exactly once.
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}
pub(super) fn cf_string(value: Ref) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: caller provides a borrowed live CF object; inspect its actual type.
    if unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
        return None;
    }
    // SAFETY: validated CFString reference; UTF8 is Apple's CFStringEncoding.
    let size = unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), UTF8) }
        .checked_add(1)?;
    let mut bytes = vec![0u8; usize::try_from(size).ok()?];
    // SAFETY: buffer capacity equals size; CFStringGetCString retains no pointer.
    if unsafe { CFStringGetCString(value, bytes.as_mut_ptr().cast(), size, UTF8) } == 0 {
        return None;
    }
    let end = bytes.iter().position(|byte| *byte == 0)?;
    String::from_utf8(bytes[..end].to_vec()).ok()
}

// Public IOHIDDevicePlugIn.h v1.5 queue interface. These are COM pointer-to-
// vtable interfaces; UUID selection fixes the complete layout, not slot guesses.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct UuidBytes(pub [u8; 16]);
#[repr(C)]
pub(super) struct UnknownInterface {
    pub reserved: *mut c_void,
    pub query: unsafe extern "C" fn(*mut c_void, UuidBytes, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "C" fn(*mut c_void) -> u32,
    pub release: unsafe extern "C" fn(*mut c_void) -> u32,
}
#[repr(C)]
pub(super) struct PluginInterface {
    pub unknown: UnknownInterface,
    pub version: u16,
    pub revision: u16,
    pub probe: unsafe extern "C" fn(*mut c_void, Ref, u32, *mut i32) -> i32,
    pub start: unsafe extern "C" fn(*mut c_void, Ref, u32) -> i32,
    pub stop: unsafe extern "C" fn(*mut c_void) -> i32,
}
pub(super) type HidReportCallback =
    unsafe extern "C" fn(*mut c_void, i32, *mut c_void, u32, u32, *mut u8, isize);
#[repr(C)]
pub(super) struct DeviceInterface {
    pub unknown: UnknownInterface,
    pub open: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub close: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub get_property: unsafe extern "C" fn(*mut c_void, Ref, *mut Ref) -> i32,
    pub set_property: unsafe extern "C" fn(*mut c_void, Ref, Ref) -> i32,
    pub get_async_source: unsafe extern "C" fn(*mut c_void, *mut Ref) -> i32,
    pub copy_elements: unsafe extern "C" fn(*mut c_void, Ref, *mut Ref, u32) -> i32,
    pub set_value: unsafe extern "C" fn(
        *mut c_void,
        Ref,
        Ref,
        u32,
        Option<HidCallback>,
        *mut c_void,
        u32,
    ) -> i32,
    pub get_value: unsafe extern "C" fn(
        *mut c_void,
        Ref,
        *mut Ref,
        u32,
        Option<HidCallback>,
        *mut c_void,
        u32,
    ) -> i32,
    pub set_report_callback: unsafe extern "C" fn(
        *mut c_void,
        *mut u8,
        isize,
        Option<HidReportCallback>,
        *mut c_void,
        u32,
    ) -> i32,
    pub set_report: unsafe extern "C" fn(
        *mut c_void,
        u32,
        u32,
        *const u8,
        isize,
        u32,
        Option<HidReportCallback>,
        *mut c_void,
        u32,
    ) -> i32,
    pub get_report: unsafe extern "C" fn(
        *mut c_void,
        u32,
        u32,
        *mut u8,
        *mut isize,
        u32,
        Option<HidReportCallback>,
        *mut c_void,
        u32,
    ) -> i32,
}
#[repr(C)]
pub(super) struct QueueInterface {
    pub unknown: UnknownInterface,
    pub get_async_source: unsafe extern "C" fn(*mut c_void, *mut Ref) -> i32,
    pub set_depth: unsafe extern "C" fn(*mut c_void, u32, u32) -> i32,
    pub get_depth: unsafe extern "C" fn(*mut c_void, *mut u32) -> i32,
    pub add_element: unsafe extern "C" fn(*mut c_void, Ref, u32) -> i32,
    pub remove_element: unsafe extern "C" fn(*mut c_void, Ref, u32) -> i32,
    pub contains_element: unsafe extern "C" fn(*mut c_void, Ref, *mut u8, u32) -> i32,
    pub start: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub stop: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    pub set_callback: unsafe extern "C" fn(*mut c_void, Option<HidCallback>, *mut c_void) -> i32,
    pub copy_next_value: unsafe extern "C" fn(*mut c_void, *mut Ref, u32, u32) -> i32,
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFUUIDCreateFromUUIDBytes(allocator: Ref, bytes: UuidBytes) -> Ref;
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    pub fn IOCreatePlugInInterfaceForService(
        service: u32,
        plugin_type: Ref,
        interface_type: Ref,
        interface: *mut *mut *mut PluginInterface,
        score: *mut i32,
    ) -> i32;
}
