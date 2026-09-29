/// The HID usage page for keyboard and keypad controls.
pub const KEYBOARD_USAGE_PAGE: u16 = 0x07;

/// Identifies the namespace of a native acquisition backend.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackendId(
    /// The caller-assigned backend namespace.
    pub u32,
);

/// Identifies a vendor-defined control or payload namespace.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VendorNamespaceId(
    /// The caller-assigned vendor namespace.
    pub u32,
);

/// The identity of a physical control independently of text and game bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhysicalControlId {
    /// A control identified by a HID usage page and usage.
    HidUsage {
        /// The HID usage page.
        usage_page: u16,
        /// The HID usage within that page.
        usage: u16,
    },
    /// An uninterpreted native control code in a backend namespace.
    Native {
        /// The backend that defines the code.
        backend: BackendId,
        /// The complete native control code.
        code: u32,
    },
    /// A control defined by a vendor namespace.
    Vendor {
        /// The namespace that defines the code.
        namespace: VendorNamespaceId,
        /// The control code within that namespace.
        code: u32,
    },
}

impl PhysicalControlId {
    /// Constructs a keyboard or keypad control on HID usage page 0x07.
    pub const fn keyboard(usage: u16) -> Self {
        Self::HidUsage {
            usage_page: KEYBOARD_USAGE_PAGE,
            usage,
        }
    }
}
