//! Actual fixture vendor decoding through the portable registry and core binding.
use beatkernel::{input::*, time::*};
use beatkernel_platform::input::*;

// Explicit fixture protocol: vendor 0x1234/product 0x0042, report ID 1,
// payload [button_bits, signed_i8_axis]. No generic vendor interpretation.
struct FixtureController;
impl DeviceAdapter for FixtureController {
    fn accepts(&self, device: &DeviceDescriptor) -> bool {
        device.vendor_id == Some(0x1234) && device.product_id == Some(0x0042)
    }
    fn on_report(&mut self, report: &RawHidReportEvent, out: &mut dyn PhysicalInputSink) {
        if report.report_id != Some(1) || report.data.len() != 2 {
            return;
        }
        out.push(PhysicalInputEvent::Button(ButtonEvent {
            meta: report.meta,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(0x1234),
                code: 1,
            },
            state: if report.data[0] & 1 != 0 {
                ButtonState::Down
            } else {
                ButtonState::Up
            },
        }));
        out.push(PhysicalInputEvent::Axis(AxisEvent {
            meta: report.meta,
            control: PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(0x1234),
                code: 2,
            },
            value: f32::from(report.data[1] as i8),
            mode: AxisMode::Absolute,
        }));
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry =
        DeviceAdapterRegistry::new(AdapterRegistryLimits::new(4, 8, 32, 8, 1024, 1024)?);
    registry.register(AdapterId(1), Box::new(|| Box::new(FixtureController)))?;
    registry.attach(DeviceDescriptor {
        runtime_id: DeviceId(42),
        vendor_id: Some(0x1234),
        product_id: Some(0x0042),
        serial: None,
        name: Some("fixture controller".into()),
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            raw_hid: true,
            ..Default::default()
        },
    })?;
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(42)),
        physical: PhysicalControlId::Vendor {
            namespace: VendorNamespaceId(0x1234),
            code: 1,
        },
        game_control: GameControlId(7),
    }])?;
    let report = RawHidReportEvent {
        meta: EventMeta::new(
            DeviceId(42),
            ClockPoint {
                domain: ClockDomainId(1),
                timestamp: Timestamp::from_nanos(100),
            },
            1,
        ),
        report_id: Some(1),
        data: vec![1, 0xfe],
    };
    match registry.route(DeviceId(42), &report)? {
        AdapterRoute::Unhandled => println!("raw unhandled: {report:?}"),
        AdapterRoute::Handled { adapter, events } => {
            for event in events {
                let bound: Vec<_> = bindings.map(&event).collect();
                println!("adapter={adapter:?} physical={event:?} bound={bound:?}");
            }
        }
    }
    registry.remove(DeviceId(42))?;
    Ok(())
}
