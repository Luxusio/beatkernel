use beatkernel::input::{
    BackendId, Binding, BindingMap, ButtonEvent, ButtonState, ContactId, DeviceCapabilities,
    DeviceDescriptor, DeviceId, DeviceSelector, DeviceTransport, EventMeta, GameControlId,
    NativeEventMeta, PhysicalControlId, PhysicalInputEvent, Position2, TouchEvent, TouchPhase,
    VirtualInputBackend,
};
use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp};

struct FixtureClock;

impl ClockMapper for FixtureClock {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        if point.domain == ClockDomainId(2) && target == ClockDomainId(1) {
            point
                .timestamp
                .as_nanos()
                .checked_add(1_000)
                .map(Timestamp::from_nanos)
        } else {
            None
        }
    }

    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Virtual binding fixture (no native I/O or latency measurement)");
    let mut backend = VirtualInputBackend::new(ClockDomainId(1));
    for source in [101, 102, 103] {
        backend.register_device(DeviceDescriptor {
            runtime_id: DeviceId(source),
            vendor_id: None,
            product_id: None,
            serial: None,
            name: Some(
                if source == 103 {
                    "virtual touch surface"
                } else {
                    "virtual keyboard"
                }
                .into(),
            ),
            transport: DeviceTransport::Virtual,
            capabilities: DeviceCapabilities {
                button: source != 103,
                touch: source == 103,
                ..Default::default()
            },
        })?;
    }
    let key = PhysicalControlId::keyboard(0x04);
    let surface = PhysicalControlId::HidUsage {
        usage_page: 0x0d,
        usage: 0x04,
    };
    let bindings = BindingMap::from_bindings([
        Binding {
            device: DeviceSelector::Any,
            physical: key,
            game_control: GameControlId(99),
        },
        Binding {
            device: DeviceSelector::Exact(DeviceId(101)),
            physical: key,
            game_control: GameControlId(10),
        },
        Binding {
            device: DeviceSelector::Exact(DeviceId(102)),
            physical: key,
            game_control: GameControlId(20),
        },
        Binding {
            device: DeviceSelector::Exact(DeviceId(103)),
            physical: surface,
            game_control: GameControlId(30),
        },
    ])?;
    for source in [101, 102, 103] {
        let point = ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(10_000),
        };
        let mut meta = EventMeta::new(DeviceId(source), point, 1);
        meta.native = Some(NativeEventMeta {
            backend: BackendId(0),
            code: None,
            timestamp: Some(point),
        });
        let physical = if source == 103 {
            PhysicalInputEvent::Touch(TouchEvent {
                meta,
                control: surface,
                contact: ContactId(8),
                phase: TouchPhase::Down,
                position: Position2 { x: 0.25, y: 0.75 },
                pressure: Some(0.5),
            })
        } else {
            PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: key,
                state: ButtonState::Down,
            })
        };
        backend.push(physical, &FixtureClock)?;
    }
    for physical in backend.drain_events() {
        for game_input in bindings.map(&physical) {
            let meta = game_input.physical.meta();
            println!("game_control={} source={} time={}ns clock={} sequence={} native={:?} origin={:?} physical={:?}",
                game_input.game_control.0, meta.source.0, meta.timestamp.as_nanos(), meta.clock_domain.0,
                meta.sequence, meta.native, meta.original_clock_point, game_input.physical);
        }
    }
    Ok(())
}
