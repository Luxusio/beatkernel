//! Native Windows input inspector with an explicitly synthetic all-host fixture.

use std::{error::Error, io};

use beatkernel::{
    input::{DeviceId, DeviceTransport, PhysicalInputEvent},
    time::{ClockDomainId, ClockMapper},
};
use beatkernel_platform::raw_input::{
    InputBatch, QpcClockMapping, RawDeviceInfo, RawDeviceKind, RawInputData, RawInputLayout,
    RawInputPacket, RawInputProcessor,
};

const HOST_CLOCK: ClockDomainId = ClockDomainId(1);

struct Options {
    fixture: bool,
    seconds: u64,
    hid: Vec<(u16, u16)>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "BeatKernel Windows Raw Input inspector\n\
Usage: windows_input_inspector [--seconds 1..3600] [--hid page:usage]...\n\
       windows_input_inspector --fixture\n\
Native mode requires Windows and opens an app-owned input window (default 10s).\n\
HID numbers are decimal or 0x-prefixed hex; keyboard is always registered.\n\
--fixture uses synthetic packet bytes on every host; it accepts no native options.\n\
QPC is receipt time; MSG.time is posted-message metadata, not hardware time."
        );
        return Ok(());
    }
    let mut options = Options {
        fixture: false,
        seconds: 10,
        hid: Vec::new(),
    };
    let mut native_options = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture" => options.fixture = true,
            "--seconds" => {
                native_options = true;
                options.seconds = args
                    .next()
                    .ok_or_else(|| invalid("--seconds needs a value"))?
                    .parse()
                    .map_err(|_| invalid("--seconds must be an integer from 1 to 3600"))?;
                if !(1..=3600).contains(&options.seconds) {
                    return Err(invalid("--seconds must be an integer from 1 to 3600").into());
                }
            }
            "--hid" => {
                native_options = true;
                let value = args
                    .next()
                    .ok_or_else(|| invalid("--hid needs page:usage"))?;
                let (page, usage) = value
                    .split_once(':')
                    .ok_or_else(|| invalid("--hid needs page:usage"))?;
                fn number(value: &str) -> Result<u16, Box<dyn Error>> {
                    let parsed = if let Some(hex) = value.strip_prefix("0x") {
                        u16::from_str_radix(hex, 16)
                    } else {
                        value.parse()
                    };
                    Ok(parsed.map_err(|_| {
                        invalid("--hid page and usage must fit u16 in decimal or 0x hex")
                    })?)
                }
                let pair = (number(page)?, number(usage)?);
                if pair.0 == 0 || pair.1 == 0 || pair == (1, 2) {
                    return Err(
                        invalid("--hid needs nonzero page/usage; mouse is unsupported").into(),
                    );
                }
                options.hid.push(pair);
            }
            _ => return Err(invalid(&format!("unknown argument: {arg}")).into()),
        }
    }
    if options.fixture {
        if native_options {
            return Err(invalid("--fixture cannot be combined with native options").into());
        }
        fixture()
    } else {
        native(options)
    }
}

fn print_packet(
    bytes: &[u8],
    layout: RawInputLayout,
    source: DeviceId,
    input: &InputBatch,
) -> Result<(), Box<dyn Error>> {
    let packet = RawInputPacket::parse(bytes, layout)?;
    println!("packet source={} native_handle={:#x} sequence={} status={:?} kind={} size={} input_code={:#x}", source.0, packet.header().device_handle, input.sequence, input.status, packet.header().kind, packet.header().size, packet.header().input_code);
    if let RawInputData::Keyboard(key) = packet.data() {
        println!("keyboard make={:#06x} flags={:#06x} reserved={:#06x} vkey={:#06x} message={:#x} extra={:#x}", key.make_code, key.flags, key.reserved, key.virtual_key, key.message, key.extra_information);
    }
    for event in &input.events {
        let meta = event.meta();
        println!(
            "event source={} sequence={} host_ns={} host_clock={} native={:?} origin={:?}",
            meta.source.0,
            meta.sequence,
            meta.timestamp.as_nanos(),
            meta.clock_domain.0,
            meta.native,
            meta.original_clock_point
        );
        match event {
            PhysicalInputEvent::Button(button) => println!(
                "button control={:?} state={:?}",
                button.control, button.state
            ),
            PhysicalInputEvent::RawHidReport(report) => println!(
                "hid report_id={:?} wire_bytes={} preview={:02x?}",
                report.report_id,
                report.data.len(),
                &report.data[..report.data.len().min(32)]
            ),
            _ => unreachable!("Raw Input processor emits only buttons and opaque HID reports"),
        }
    }
    Ok(())
}

fn fixture() -> Result<(), Box<dyn Error>> {
    println!("SYNTHETIC Raw Input fixture; no native acquisition or latency measurement");
    let mapping = QpcClockMapping::new(1_000_000_000, 1000, HOST_CLOCK)?;
    for layout in [RawInputLayout::Win32, RawInputLayout::Win64] {
        let mut processor = RawInputProcessor::new(HOST_CLOCK);
        for (handle, kind) in [
            (11, RawDeviceKind::Keyboard),
            (22, RawDeviceKind::Keyboard),
            (33, RawDeviceKind::Hid),
        ] {
            let mut info = RawDeviceInfo::new(kind);
            info.transport = DeviceTransport::Virtual;
            info.name = Some("Synthetic packet device".into());
            processor.register_device(handle, info)?;
        }
        fn packet(layout: RawInputLayout, kind: u32, handle: u64, body: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend(kind.to_le_bytes());
            bytes.extend(((layout.header_size() + body.len()) as u32).to_le_bytes());
            match layout {
                RawInputLayout::Win32 => {
                    bytes.extend((handle as u32).to_le_bytes());
                    bytes.extend(0u32.to_le_bytes());
                }
                RawInputLayout::Win64 => {
                    bytes.extend(handle.to_le_bytes());
                    bytes.extend(0u64.to_le_bytes());
                }
            }
            bytes.extend(body);
            bytes
        }
        for (index, (handle, flags)) in [(11, 0u16), (22, 0), (11, 0), (11, 1), (22, 1)]
            .iter()
            .enumerate()
        {
            let mut body = Vec::new();
            for field in [0x1eu16, *flags, 0, 0x41] {
                body.extend(field.to_le_bytes());
            }
            body.extend((if *flags == 0 { 0x100u32 } else { 0x101 }).to_le_bytes());
            body.extend(0u32.to_le_bytes());
            let bytes = packet(layout, 1, *handle, &body);
            let point = mapping.point(2000 + index as i64)?;
            println!("fixture layout={layout:?} receipt_counter={} frequency=1000000000 native_ns={} host_ns={}", 2000 + index, point.timestamp.as_nanos(), mapping.map(point, HOST_CLOCK).unwrap().as_nanos());
            let input =
                processor.process(&RawInputPacket::parse(&bytes, layout)?, point, &mapping)?;
            print_packet(
                &bytes,
                layout,
                processor.device(*handle).unwrap().runtime_id,
                &input,
            )?;
        }
        let bytes = packet(
            layout,
            2,
            33,
            &[3, 0, 0, 0, 2, 0, 0, 0, 7, 0xaa, 0xbb, 0, 0xcc, 0xdd],
        );
        let input = processor.process(
            &RawInputPacket::parse(&bytes, layout)?,
            mapping.point(3000)?,
            &mapping,
        )?;
        print_packet(
            &bytes,
            layout,
            processor.device(33).unwrap().runtime_id,
            &input,
        )?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn native(_options: Options) -> Result<(), Box<dyn Error>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native Raw Input requires Windows; use --fixture for synthetic packets",
    )
    .into())
}

#[cfg(target_os = "windows")]
fn native(options: Options) -> Result<(), Box<dyn Error>> {
    native_windows::run(options)
}

#[cfg(target_os = "windows")]
mod native_windows {
    use super::*;
    use beatkernel_platform::windows::{
        clock::QpcClock,
        input::{RawInputRegistration, RawInputUsage, WindowsInput, WindowsInputDevice},
    };
    use std::{
        collections::BTreeSet,
        ptr,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{HINSTANCE, HWND},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
            RegisterClassW, TranslateMessage, UnregisterClassW, GIDC_ARRIVAL, GIDC_REMOVAL, MSG,
            PM_REMOVE, WM_CLOSE, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_QUIT, WNDCLASSW,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    };

    struct Window {
        hwnd: HWND,
        instance: HINSTANCE,
        class: Vec<u16>,
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        if message == WM_CLOSE {
            // SAFETY: posts only to this thread. Preserve the live window until
            // the pump closes registration, including synchronously sent close.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            }
            return 0;
        }
        // SAFETY: the native caller supplies window/message values. This
        // callback owns no Rust state and cannot unwind across the ABI.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    impl Window {
        fn new() -> Result<Self, Box<dyn Error>> {
            let class: Vec<u16> = format!("BeatKernelInputInspector{}", std::process::id())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: documented null query gets this executable's module.
            let instance = unsafe { GetModuleHandleW(ptr::null()) };
            if instance.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            let descriptor = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: ptr::null_mut(),
                hCursor: ptr::null_mut(),
                hbrBackground: ptr::null_mut(),
                lpszMenuName: ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            // SAFETY: initialized class, terminated live name, native callback
            // without any Rust callback state or unwind across the ABI.
            if unsafe { RegisterClassW(&descriptor) } == 0 {
                return Err(io::Error::last_os_error().into());
            }
            let mut window = Self {
                hwnd: ptr::null_mut(),
                instance,
                class,
            };
            let title: Vec<u16> = "BeatKernel input inspector: press keys here"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: live registered class/module and UTF-16 strings, no menu,
            // parent or user-data pointers. Drop owns partial failure cleanup.
            window.hwnd = unsafe {
                CreateWindowExW(
                    0,
                    window.class.as_ptr(),
                    title.as_ptr(),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    100,
                    100,
                    640,
                    240,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    instance,
                    ptr::null(),
                )
            };
            if window.hwnd.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            Ok(window)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: current thread owns this window/class; registration guard
            // is constructed after Window and cleaned before destruction.
            unsafe {
                if !self.hwnd.is_null() {
                    DestroyWindow(self.hwnd);
                }
                UnregisterClassW(self.class.as_ptr(), self.instance);
            }
        }
    }

    fn print_device(device: &WindowsInputDevice) {
        println!(
            "device={} kind={:?} usage={:?} vendor={:?} product={:?} name={:?}",
            device.descriptor.runtime_id.0,
            device.kind,
            device.usage,
            device.descriptor.vendor_id,
            device.descriptor.product_id,
            device.descriptor.name
        );
    }

    pub(super) fn run(options: Options) -> Result<(), Box<dyn Error>> {
        let clock = QpcClock::new(HOST_CLOCK)?;
        let window = Window::new()?;
        let mut usages = vec![RawInputUsage::KEYBOARD];
        usages.extend(options.hid.iter().map(|(page, usage)| RawInputUsage {
            page: *page,
            usage: *usage,
        }));
        let mut registration = RawInputRegistration::register(window.hwnd as usize, &usages)?;
        let mut input = WindowsInput::new(clock);
        println!("NATIVE Windows Raw Input inspector; QPC receipt time, not hardware latency");
        let mut printed_devices = BTreeSet::new();
        for device in input.enumerate_devices()? {
            print_device(&device);
            printed_devices.insert(device.descriptor.runtime_id);
        }
        let deadline = Instant::now() + Duration::from_secs(options.seconds);
        let mut acquisitions = 0u64;
        'pump: while Instant::now() < deadline {
            // SAFETY: MSG has only integer/opaque-pointer fields, so zero is a
            // valid bit pattern. PeekMessage fills it before inspection.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: live aligned writable MSG and no retained pointer.
            while Instant::now() < deadline
                && unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
            {
                if message.message == WM_QUIT
                    || (message.message == WM_CLOSE && message.hwnd == window.hwnd)
                {
                    // Do not dispatch WM_CLOSE: cleanup registration first.
                    break 'pump;
                }
                if message.hwnd == window.hwnd && message.message == WM_INPUT {
                    let result = input.read_raw_input(message.lParam as usize, Some(message.time));
                    if message.wParam & 0xff == 0 {
                        // SAFETY: this is the actual foreground message from
                        // this window's queue. Acquisition finishes first;
                        // cleanup runs exactly once before handling any error.
                        unsafe {
                            DefWindowProcW(
                                message.hwnd,
                                message.message,
                                message.wParam,
                                message.lParam,
                            );
                        }
                    }
                    // Acquisition can attach a device even when the packet is
                    // rejected later, so describe new sources on both paths.
                    for device in input.devices() {
                        if printed_devices.insert(device.descriptor.runtime_id) {
                            print_device(device);
                        }
                    }
                    match result {
                        Ok(batch) => {
                            acquisitions += 1;
                            println!("receipt counter={} frequency={} native_ns={} native_clock={} host_ns={} host_clock={} posted_ms={:?}", batch.receipt.counter, batch.receipt.frequency, batch.receipt.native.timestamp.as_nanos(), batch.receipt.native.domain.0, batch.receipt.normalized.timestamp.as_nanos(), batch.receipt.normalized.domain.0, batch.message_time_ms);
                            let layout = if std::mem::size_of::<usize>() == 8 {
                                RawInputLayout::Win64
                            } else {
                                RawInputLayout::Win32
                            };
                            print_packet(&batch.packet, layout, batch.device, &batch.input)?;
                        }
                        Err(error) => eprintln!("input rejected: {error}"),
                    }
                    continue; // Foreground cleanup has already dispatched it.
                }
                if message.hwnd == window.hwnd && message.message == WM_INPUT_DEVICE_CHANGE {
                    match message.wParam as u32 {
                        GIDC_ARRIVAL => match input.attach_device(message.lParam as usize) {
                            Ok(device) => {
                                println!("device arrived={}", device.descriptor.runtime_id.0);
                                if printed_devices.insert(device.descriptor.runtime_id) {
                                    print_device(&device);
                                }
                            }
                            Err(error) => eprintln!("arrival rejected: {error}"),
                        },
                        GIDC_REMOVAL => {
                            if let Some(device) = input.remove_device(message.lParam as usize) {
                                println!("device removed={}", device.descriptor.runtime_id.0);
                            }
                        }
                        _ => eprintln!("unknown device notification={}", message.wParam),
                    }
                }
                // SAFETY: actual initialized message from this thread's queue;
                // stateless callback delegates normal messages to DefWindowProc;
                // WM_CLOSE only posts quit until registration cleanup finishes.
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if Instant::now() >= deadline {
                    break 'pump;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        registration.close()?;
        println!("native acquisitions={acquisitions}; owned registrations closed");
        Ok(())
    }
}
