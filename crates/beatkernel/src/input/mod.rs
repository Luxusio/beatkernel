//! Typed physical input and game bindings independent of native acquisition.
//!
//! Device IDs are assigned by callers. Event coordinates and axis units follow
//! the device adapter's convention and are preserved without normalization.
//! The virtual backend is a single-owner FIFO for fixtures and control threads;
//! registration, owned payload creation and enqueue may allocate.
//!
//! ```
//! use beatkernel::input::{ButtonEvent, ButtonState, DeviceCapabilities,
//!     DeviceDescriptor, DeviceId, DeviceTransport, EventMeta, PhysicalControlId,
//!     PhysicalInputEvent, VirtualInputBackend};
//! use beatkernel::time::{ClockDomainId, ClockMapper, ClockMappingQuality,
//!     ClockPoint, Timestamp};
//! struct NoMapping;
//! impl ClockMapper for NoMapping {
//!     fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> { None }
//!     fn quality(&self) -> ClockMappingQuality { ClockMappingQuality::Unknown }
//! }
//! let domain = ClockDomainId(1);
//! let source = DeviceId(42);
//! let mut backend = VirtualInputBackend::new(domain);
//! backend.register_device(DeviceDescriptor {
//!     runtime_id: source, vendor_id: None, product_id: None, serial: None,
//!     name: None, transport: DeviceTransport::Virtual,
//!     capabilities: DeviceCapabilities { button: true, ..Default::default() },
//! })?;
//! let event = PhysicalInputEvent::Button(ButtonEvent {
//!     meta: EventMeta::new(source, ClockPoint { domain, timestamp: Timestamp::ZERO }, 0),
//!     control: PhysicalControlId::keyboard(0x04), state: ButtonState::Down,
//! });
//! backend.push(event.clone(), &NoMapping)?;
//! assert_eq!(backend.pop(), Some(event));
//! # Ok::<(), beatkernel::input::VirtualInputError>(())
//! ```

mod adapters;
mod backend;
mod binding;
mod control;
mod device;
mod event;

pub use adapters::{DeviceAdapter, PhysicalInputSink};
pub use backend::{VirtualInputBackend, VirtualInputError};
pub use binding::{
    Binding, BindingError, BindingMap, DeviceSelector, GameControlId, GameInputEvent,
};
pub use control::{BackendId, PhysicalControlId, VendorNamespaceId, KEYBOARD_USAGE_PAGE};
pub use device::{DeviceCapabilities, DeviceDescriptor, DeviceId, DeviceTransport};
pub use event::{
    AxisEvent, AxisMode, ButtonEvent, ButtonState, ContactId, CustomInputEvent, EventMeta,
    NativeEventMeta, PhysicalInputEvent, PointerEvent, PointerMode, PoseEvent, Position2,
    Position3, Quaternion, RawHidReportEvent, TouchEvent, TouchPhase,
};
