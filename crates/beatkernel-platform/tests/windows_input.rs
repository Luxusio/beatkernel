//! Tests that execute native Windows APIs; Linux builds do not run them.

#![cfg(target_os = "windows")]

use beatkernel::time::{ClockDomainId, ClockMapper};
use beatkernel_platform::{raw_input::WINDOWS_QPC_CLOCK_DOMAIN, windows::clock::QpcClock};

use beatkernel_platform::windows::input::{
    RawInputRegistration, RawInputUsage, WindowsInput, WindowsInputError,
};
use std::{ptr, sync::Mutex};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::{
            GetRegisteredRawInputDevices, RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_DEVNOTIFY,
            RIDEV_INPUTSINK, RIDEV_PAGEONLY, RIDEV_REMOVE,
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW,
            WNDCLASSW,
        },
    },
};

// Raw Input registrations are process-wide, even though each test owns a window.
static REGISTRATION_TESTS: Mutex<()> = Mutex::new(());

struct TestWindow {
    hwnd: HWND,
    instance: HINSTANCE,
    class: Vec<u16>,
}

impl TestWindow {
    fn new(suffix: &str) -> Self {
        let class: Vec<u16> = format!("BeatKernelTest{}_{suffix}", std::process::id())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: null asks for the current module without transferring ownership.
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        assert!(!instance.is_null());
        let descriptor = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        // SAFETY: initialized class has a native callback and a live terminated
        // UTF-16 name; no Rust state is passed through a callback.
        assert_ne!(unsafe { RegisterClassW(&descriptor) }, 0);
        // SAFETY: class and module were registered above, name remains alive;
        // no parent/menu or user data pointers are supplied.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: no window was created; this test owns the class.
            unsafe {
                UnregisterClassW(class.as_ptr(), instance);
            }
            panic!(
                "CreateWindowExW failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Self {
            hwnd,
            instance,
            class,
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: this thread owns the live window/class; registration guards
        // are declared after TestWindow and therefore drop before it.
        unsafe {
            DestroyWindow(self.hwnd);
            UnregisterClassW(self.class.as_ptr(), self.instance);
        }
    }
}

fn registered_target(usage: RawInputUsage) -> Option<usize> {
    let mut count = 0;
    // SAFETY: documented null size query with live writable count.
    unsafe {
        GetRegisteredRawInputDevices(
            ptr::null_mut(),
            &mut count,
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        );
    }
    assert!(count < 4096);
    if count == 0 {
        return None;
    }
    let empty = RAWINPUTDEVICE {
        usUsagePage: 0,
        usUsage: 0,
        dwFlags: 0,
        hwndTarget: ptr::null_mut(),
    };
    let mut entries = vec![empty; count as usize];
    // SAFETY: aligned initialized array sized by the immediate query; tests
    // serialize mutations so its process registration count does not race.
    let actual = unsafe {
        GetRegisteredRawInputDevices(
            entries.as_mut_ptr(),
            &mut count,
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    };
    assert_ne!(actual, u32::MAX);
    assert!(actual as usize <= entries.len());
    entries[..actual as usize]
        .iter()
        .find(|entry| entry.usUsagePage == usage.page && entry.usUsage == usage.usage)
        .map(|entry| entry.hwndTarget as usize)
}

#[test]
fn native_input_invalid_handles_leave_state_empty_and_return_context() {
    let mut input = WindowsInput::new(QpcClock::new(ClockDomainId(101)).unwrap());
    assert!(matches!(
        input.attach_device(0),
        Err(WindowsInputError::Packet(_))
    ));
    assert!(input.read_raw_input(0, Some(u32::MAX)).is_err());
    match input.read_raw_input(usize::MAX, None).unwrap_err() {
        WindowsInputError::Api { operation, source } => {
            assert_eq!(operation, "GetRawInputData(size)");
            assert!(source.raw_os_error().is_some());
        }
        error => panic!("unexpected invalid-handle error: {error}"),
    }
    assert_eq!(input.devices().count(), 0);
}

#[test]
fn native_device_enumeration_uses_real_descriptors_and_owned_results() {
    let mut input = WindowsInput::new(QpcClock::new(ClockDomainId(102)).unwrap());
    let first = input.enumerate_devices().unwrap();
    // A headless/RDP machine is allowed to expose no Raw Input devices; this
    // test does not claim that an empty list proves keyboard acquisition.
    assert!(first.len() <= 4096);
    for device in &first {
        assert_ne!(device.handle, 0);
        assert_ne!(device.descriptor.runtime_id.0, 0);
        assert!(!device.interface_path.is_empty());
        assert_eq!(
            input
                .attach_device(device.handle)
                .unwrap()
                .descriptor
                .runtime_id,
            device.descriptor.runtime_id
        );
    }
    if let Some(device) = first.first() {
        let original = device.descriptor.runtime_id;
        assert_eq!(
            input
                .remove_device(device.handle)
                .unwrap()
                .descriptor
                .runtime_id,
            original
        );
        assert!(
            input
                .attach_device(device.handle)
                .unwrap()
                .descriptor
                .runtime_id
                > original
        );
        assert_eq!(device.descriptor.runtime_id, original);
    }
}

#[test]
fn native_registration_refuses_conflicts_and_cleans_normal_and_error_paths() {
    let _serial = REGISTRATION_TESTS.lock().unwrap();
    let window = TestWindow::new("registration");
    let usage = RawInputUsage {
        page: 0xff00,
        usage: 1,
    };
    assert_eq!(registered_target(usage), None);
    assert!(RawInputRegistration::register(0, &[usage]).is_err());
    assert!(RawInputRegistration::register(window.hwnd as usize, &[]).is_err());
    assert!(RawInputRegistration::register(
        window.hwnd as usize,
        &[RawInputUsage { page: 0, usage: 1 }]
    )
    .is_err());
    let duplicates = vec![usage; 4097];
    let mut guard = RawInputRegistration::register(window.hwnd as usize, &duplicates).unwrap();
    assert_eq!(registered_target(usage), Some(window.hwnd as usize));
    assert!(RawInputRegistration::register(window.hwnd as usize, &[usage]).is_err());
    guard.close().unwrap();
    guard.close().unwrap();
    assert_eq!(registered_target(usage), None);
    fn fail_after_registration(window: usize, usage: RawInputUsage) -> Result<(), &'static str> {
        let _guard = RawInputRegistration::register(window, &[usage]).unwrap();
        Err("injected application failure after successful registration")
    }
    assert!(fail_after_registration(window.hwnd as usize, usage).is_err());
    assert_eq!(registered_target(usage), None);
    let foreign_window = window.hwnd as usize;
    assert!(std::thread::spawn(
        move || RawInputRegistration::register(foreign_window, &[usage]).is_err()
    )
    .join()
    .unwrap());
}

#[test]
fn native_registration_preserves_different_window_successor_and_refuses_page_overlap() {
    let _serial = REGISTRATION_TESTS.lock().unwrap();
    let first = TestWindow::new("first");
    let second = TestWindow::new("second");
    let usage = RawInputUsage {
        page: 0xff00,
        usage: 2,
    };
    let mut guard = RawInputRegistration::register(first.hwnd as usize, &[usage]).unwrap();
    let successor = RAWINPUTDEVICE {
        usUsagePage: usage.page,
        usUsage: usage.usage,
        dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
        hwndTarget: second.hwnd,
    };
    // Deliberately exercises detectable contract violation by an outside owner.
    // SAFETY: valid aligned registration structure and second live HWND.
    assert_ne!(
        unsafe {
            RegisterRawInputDevices(&successor, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
        },
        0
    );
    guard.close().unwrap();
    assert_eq!(registered_target(usage), Some(second.hwnd as usize));
    let removal = RAWINPUTDEVICE {
        dwFlags: RIDEV_REMOVE,
        hwndTarget: ptr::null_mut(),
        ..successor
    };
    // SAFETY: exact successor class is removed with null target, as required.
    assert_ne!(
        unsafe {
            RegisterRawInputDevices(&removal, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
        },
        0
    );
    let page = RAWINPUTDEVICE {
        usUsagePage: usage.page,
        usUsage: 0,
        dwFlags: RIDEV_PAGEONLY | RIDEV_INPUTSINK,
        hwndTarget: second.hwnd,
    };
    // SAFETY: valid page-wide registration intentionally sets up conflict.
    assert_ne!(
        unsafe { RegisterRawInputDevices(&page, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) },
        0
    );
    assert!(RawInputRegistration::register(first.hwnd as usize, &[usage]).is_err());
    let remove_page = RAWINPUTDEVICE {
        dwFlags: RIDEV_REMOVE | RIDEV_PAGEONLY,
        hwndTarget: ptr::null_mut(),
        ..page
    };
    // SAFETY: this test owns this page-wide entry and explicitly removes it.
    assert_ne!(
        unsafe {
            RegisterRawInputDevices(
                &remove_page,
                1,
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            )
        },
        0
    );
}

#[test]
fn native_qpc_samples_share_frequency_origin_and_preserve_receipt_provenance() {
    let output = ClockDomainId(100);
    let clock = QpcClock::new(output).unwrap();
    let first = clock.sample().unwrap();
    let copied = clock;
    let second = copied.sample().unwrap();
    assert!(first.frequency > 0);
    assert_eq!(first.frequency, second.frequency);
    assert!(second.counter >= first.counter);
    assert!(first.normalized.timestamp.as_nanos() >= 0);
    assert!(second.normalized.timestamp >= first.normalized.timestamp);
    assert_eq!(first.native.domain, WINDOWS_QPC_CLOCK_DOMAIN);
    assert_eq!(second.native.domain, WINDOWS_QPC_CLOCK_DOMAIN);
    assert_eq!(first.normalized.domain, output);
    assert_eq!(copied.output_domain(), output);
    assert_eq!(
        clock.mapping().origin_counter(),
        copied.mapping().origin_counter()
    );
    assert_eq!(
        clock.mapping().map(first.native, output),
        Some(first.normalized.timestamp)
    );
    assert_eq!(
        copied.mapping().map(second.native, output),
        Some(second.normalized.timestamp)
    );
}

#[test]
fn native_clock_refuses_to_alias_absolute_and_relative_domains() {
    let error = QpcClock::new(WINDOWS_QPC_CLOCK_DOMAIN).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}
