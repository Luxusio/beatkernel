/// A caller-assigned device identity within one runtime session.
///
/// This is distinct from serial numbers and hardware fingerprints. A reconnect
/// must use a new ID after the previous device is retired from a virtual backend.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(
    /// The caller-assigned identity value.
    pub u64,
);

/// The device's reported connection transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceTransport {
    /// A USB connection.
    Usb,
    /// A Bluetooth connection.
    Bluetooth,
    /// A synthetic device without native acquisition.
    Virtual,
    /// No transport is known.
    Unknown,
}

/// Describes the semantic input kinds a device can provide.
///
/// All capabilities are false by default. These flags describe a device rather
/// than restricting which event variants the virtual backend accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DeviceCapabilities {
    /// Button transitions are available.
    pub button: bool,
    /// Axis samples are available.
    pub axis: bool,
    /// Touch contacts are available.
    pub touch: bool,
    /// Pointer positions or deltas are available.
    pub pointer: bool,
    /// Three-dimensional poses are available.
    pub pose: bool,
    /// Raw HID report payloads are available.
    pub raw_hid: bool,
    /// Vendor-defined event payloads are available.
    pub custom: bool,
}

/// Device identity and optional descriptive information.
///
/// Devices with identical descriptive fields can have different runtime IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceDescriptor {
    /// The caller-assigned runtime identity.
    pub runtime_id: DeviceId,
    /// The reported hardware vendor ID, when known.
    pub vendor_id: Option<u16>,
    /// The reported hardware product ID, when known.
    pub product_id: Option<u16>,
    /// The reported serial number, when available.
    pub serial: Option<String>,
    /// The reported display name, when available.
    pub name: Option<String>,
    /// The reported connection transport.
    pub transport: DeviceTransport,
    /// The input kinds reported by the device.
    pub capabilities: DeviceCapabilities,
}
