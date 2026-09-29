//! A deterministic virtual fixture inspector; no native devices are opened.

use beatkernel::input::{
    BackendId, ButtonEvent, ButtonState, DeviceCapabilities, DeviceDescriptor, DeviceId,
    DeviceTransport, EventMeta, NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
    VirtualInputBackend,
};
use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp};
use beatkernel_platform::keyboard::{
    linux_evdev_key, macos_hid_usage, windows_native_code, windows_scan_code, ScanCodePrefix,
    LINUX_KEYBOARD_BACKEND, MACOS_HID_BACKEND, WINDOWS_KEYBOARD_BACKEND,
};

const HOST_CLOCK: ClockDomainId = ClockDomainId(1);
const FIXTURE_CLOCK: ClockDomainId = ClockDomainId(2);

struct FixtureOffsetMapper;

impl ClockMapper for FixtureOffsetMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if from.domain != FIXTURE_CLOCK || to != HOST_CLOCK {
            return None;
        }
        from.timestamp
            .as_nanos()
            .checked_add(1_000)
            .map(Timestamp::from_nanos)
    }

    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Virtual fixture input inspector (no native I/O or latency measurement)");
    let mut queue = VirtualInputBackend::new(HOST_CLOCK);
    let samples: [(&str, DeviceId, BackendId, u32, PhysicalControlId); 3] = [
        (
            "Windows Scan 1 fixture",
            DeviceId(101),
            WINDOWS_KEYBOARD_BACKEND,
            windows_native_code(0x1E, ScanCodePrefix::None),
            windows_scan_code(0x1E, ScanCodePrefix::None),
        ),
        (
            "Linux evdev fixture",
            DeviceId(102),
            LINUX_KEYBOARD_BACKEND,
            30,
            linux_evdev_key(30),
        ),
        (
            "macOS HID fixture",
            DeviceId(103),
            MACOS_HID_BACKEND,
            0x0007_0004,
            macos_hid_usage(0x07, 0x04),
        ),
    ];

    for (label, source, native_backend, native_code, control) in samples {
        assert_eq!(control, PhysicalControlId::keyboard(0x04));
        queue.register_device(DeviceDescriptor {
            runtime_id: source,
            vendor_id: Some(0x1234),
            product_id: Some(0x5678),
            serial: None,
            name: Some(label.into()),
            transport: DeviceTransport::Virtual,
            capabilities: DeviceCapabilities {
                button: true,
                ..Default::default()
            },
        })?;
        let point = ClockPoint {
            domain: FIXTURE_CLOCK,
            timestamp: Timestamp::from_nanos(10_000),
        };
        let mut meta = EventMeta::new(source, point, 1);
        meta.native = Some(NativeEventMeta {
            backend: native_backend,
            code: Some(native_code),
            timestamp: Some(point),
        });
        queue.push(
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control,
                state: ButtonState::Down,
            }),
            &FixtureOffsetMapper,
        )?;
    }

    for event in queue.drain_events() {
        if let PhysicalInputEvent::Button(button) = event {
            println!(
                "source={} control={:?} state={:?} time={}ns clock={} sequence={} native={:?} origin={:?}",
                button.meta.source.0,
                button.control,
                button.state,
                button.meta.timestamp.as_nanos(),
                button.meta.clock_domain.0,
                button.meta.sequence,
                button.meta.native,
                button.meta.original_clock_point,
            );
        }
    }
    Ok(())
}
