use crate::time::{ClockDomainId, ClockPoint, Timestamp};

use super::{BackendId, DeviceId, PhysicalControlId, VendorNamespaceId};

/// Native acquisition provenance retained independently of normalized time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeEventMeta {
    /// The native acquisition backend.
    pub backend: BackendId,
    /// The native event code, when available.
    pub code: Option<u32>,
    /// The original native clock point, when available.
    pub timestamp: Option<ClockPoint>,
}

/// Source identity, clock information and acquisition order for every event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventMeta {
    /// The device that acquired the input.
    pub source: DeviceId,
    /// The event timestamp in `clock_domain`.
    pub timestamp: Timestamp,
    /// The clock domain of `timestamp`.
    pub clock_domain: ClockDomainId,
    /// The source acquisition sequence; equal values allow report fanout.
    pub sequence: u64,
    /// Optional native acquisition provenance.
    pub native: Option<NativeEventMeta>,
    /// The incoming clock point before normalization, if one has been preserved.
    pub original_clock_point: Option<ClockPoint>,
}

impl EventMeta {
    /// Constructs metadata with no native provenance or saved clock origin.
    pub const fn new(source: DeviceId, point: ClockPoint, sequence: u64) -> Self {
        Self {
            source,
            timestamp: point.timestamp,
            clock_domain: point.domain,
            sequence,
            native: None,
            original_clock_point: None,
        }
    }
}

/// An unmodified two-dimensional position or displacement in adapter units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position2 {
    /// The horizontal coordinate or displacement.
    pub x: f32,
    /// The vertical coordinate or displacement.
    pub y: f32,
}

/// An unmodified three-dimensional position in adapter coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position3 {
    /// The x coordinate.
    pub x: f32,
    /// The y coordinate.
    pub y: f32,
    /// The z coordinate.
    pub z: f32,
}

/// An unmodified orientation quaternion in the adapter's convention.
///
/// The core does not normalize the quaternion or impose a coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    /// The x component.
    pub x: f32,
    /// The y component.
    pub y: f32,
    /// The z component.
    pub z: f32,
    /// The scalar component.
    pub w: f32,
}

/// A button transition or repeat indication.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonState {
    /// The button was pressed.
    Down,
    /// The button was released.
    Up,
    /// The acquisition source reported a held-button repeat.
    Repeat,
}

/// Whether an axis sample represents a position or a displacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AxisMode {
    /// A position in adapter-defined units.
    Absolute,
    /// A displacement in adapter-defined units.
    Relative,
}

/// A contact identity scoped to a device and touch surface.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContactId(
    /// The adapter-assigned contact identity.
    pub u64,
);

/// The reported phase of one touch contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TouchPhase {
    /// The contact began.
    Down,
    /// The contact moved or changed.
    Move,
    /// The contact ended normally.
    Up,
    /// The contact was canceled.
    Cancel,
}

/// Whether a pointer sample represents a position or a displacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerMode {
    /// A position in adapter-defined coordinates.
    Absolute,
    /// A displacement in adapter-defined coordinates.
    Relative,
}

/// A typed physical button event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The physical button identity.
    pub control: PhysicalControlId,
    /// The reported transition or repeat.
    pub state: ButtonState,
}

/// A typed physical axis sample with an unmodified value.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The physical axis identity.
    pub control: PhysicalControlId,
    /// The unmodified sample in adapter-defined units.
    pub value: f32,
    /// Whether the sample is absolute or relative.
    pub mode: AxisMode,
}

/// A touch contact sample on a physical surface.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The physical touch surface identity.
    pub control: PhysicalControlId,
    /// The contact identity within this device and surface.
    pub contact: ContactId,
    /// The reported contact phase.
    pub phase: TouchPhase,
    /// The unmodified position in adapter-defined coordinates.
    pub position: Position2,
    /// The unmodified pressure in adapter-defined units, when available.
    pub pressure: Option<f32>,
}

/// A pointer position or displacement.
#[derive(Clone, Debug, PartialEq)]
pub struct PointerEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The physical pointer identity.
    pub control: PhysicalControlId,
    /// The unmodified position or displacement.
    pub position: Position2,
    /// Whether the sample is absolute or relative.
    pub mode: PointerMode,
}

/// A three-dimensional pose with unmodified position and orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct PoseEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The physical pose control identity.
    pub control: PhysicalControlId,
    /// The unmodified position in adapter coordinates.
    pub position: Position3,
    /// The unmodified orientation in the adapter's convention.
    pub orientation: Quaternion,
}

/// An exact owned HID report payload for adapter parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawHidReportEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The separate report ID, when used by the device.
    pub report_id: Option<u8>,
    /// Exact payload bytes excluding the separately stored report ID.
    pub data: Vec<u8>,
}

/// An exact vendor-defined payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomInputEvent {
    /// Source, timing and acquisition provenance.
    pub meta: EventMeta,
    /// The vendor namespace defining the payload.
    pub namespace: VendorNamespaceId,
    /// The payload type within the namespace.
    pub type_id: u32,
    /// Exact owned payload bytes.
    pub payload: Vec<u8>,
}

/// A typed physical input sample before game binding or interaction policy.
#[derive(Clone, Debug, PartialEq)]
pub enum PhysicalInputEvent {
    /// A physical button transition or repeat.
    Button(ButtonEvent),
    /// An axis position or displacement.
    Axis(AxisEvent),
    /// A touch contact sample.
    Touch(TouchEvent),
    /// A pointer position or displacement.
    Pointer(PointerEvent),
    /// A three-dimensional pose.
    Pose(PoseEvent),
    /// A raw HID report for adapter parsing.
    RawHidReport(RawHidReportEvent),
    /// A vendor-defined payload.
    Custom(CustomInputEvent),
}

impl PhysicalInputEvent {
    /// Returns the common metadata without inspecting the semantic payload.
    pub fn meta(&self) -> &EventMeta {
        match self {
            Self::Button(event) => &event.meta,
            Self::Axis(event) => &event.meta,
            Self::Touch(event) => &event.meta,
            Self::Pointer(event) => &event.meta,
            Self::Pose(event) => &event.meta,
            Self::RawHidReport(event) => &event.meta,
            Self::Custom(event) => &event.meta,
        }
    }

    /// Returns mutable common metadata, leaving the semantic payload untouched.
    pub fn meta_mut(&mut self) -> &mut EventMeta {
        match self {
            Self::Button(event) => &mut event.meta,
            Self::Axis(event) => &mut event.meta,
            Self::Touch(event) => &mut event.meta,
            Self::Pointer(event) => &mut event.meta,
            Self::Pose(event) => &mut event.meta,
            Self::RawHidReport(event) => &mut event.meta,
            Self::Custom(event) => &mut event.meta,
        }
    }
}
