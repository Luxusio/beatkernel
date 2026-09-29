//! Pure physical keyboard normalization, independent of text and layout.
//!
//! The supported positional keyboard subset follows the [Windows HID/Scan 1
//! table](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input)
//! and [Linux input codes](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h).
//! Unknown codes retain their complete native identity. Windows scan 0x2B
//! cannot distinguish HID usages 0x31 and 0x32; it uses conventional usage 0x31.
//! A HID-origin input can retain the original usage through [`macos_hid_usage`].
//!
//! These helpers do not acquire input, parse Raw Input packets, infer repeat
//! state, or assemble multi-packet scan sequences.

use beatkernel::input::{BackendId, PhysicalControlId};

/// The native Windows keyboard code namespace.
pub const WINDOWS_KEYBOARD_BACKEND: BackendId = BackendId(1);
/// The native Linux evdev keyboard code namespace.
pub const LINUX_KEYBOARD_BACKEND: BackendId = BackendId(2);
/// The macOS HID acquisition namespace for native event provenance.
pub const MACOS_HID_BACKEND: BackendId = BackendId(3);

/// The extension prefix of a complete Windows Scan 1 make code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScanCodePrefix {
    /// An unprefixed make code.
    None,
    /// An E0 extended code.
    E0,
    /// An E1 extended sequence.
    E1,
}

/// Packs the prefix in the upper 16 bits and the full make code below it.
///
/// No bits are masked: unsupported make codes remain distinguishable.
pub const fn windows_native_code(make_code: u16, prefix: ScanCodePrefix) -> u32 {
    let extension = match prefix {
        ScanCodePrefix::None => 0,
        ScanCodePrefix::E0 => 0xE0,
        ScanCodePrefix::E1 => 0xE1,
    };
    (extension << 16) | make_code as u32
}

/// Converts a supported complete Windows physical scan code to a HID usage.
///
/// Pause accepts E1 plus 0x1D45, or an assembler-normalized E1 plus 0x45.
/// A lone E1 0x1D header remains native; acquisition must assemble the sequence.
/// E0 0x46 (Ctrl+Pause/Break) is a Pause alias, and unprefixed 0x54
/// (Alt+PrintScreen/SysRq) is a PrintScreen alias. Unprefixed 0x45 is NumLock.
pub fn windows_scan_code(make_code: u16, prefix: ScanCodePrefix) -> PhysicalControlId {
    let usage = match prefix {
        ScanCodePrefix::None => match make_code {
            0x54 => Some(0x46),
            0x56 => Some(0x64),
            0x57 => Some(0x44),
            0x58 => Some(0x45),
            0x59 => Some(0x67), // Keypad equals.
            _ => set1_usage(make_code),
        },
        ScanCodePrefix::E0 => match make_code {
            0x1C => Some(0x58), // Keypad Enter.
            0x1D => Some(0xE4), // Right Control.
            0x35 => Some(0x54), // Keypad slash.
            0x37 => Some(0x46), // PrintScreen.
            0x38 => Some(0xE6), // Right Alt.
            0x46 => Some(0x48), // Ctrl+Pause/Break.
            0x47 => Some(0x4A), // Home.
            0x48 => Some(0x52), // Up.
            0x49 => Some(0x4B), // Page Up.
            0x4B => Some(0x50), // Left.
            0x4D => Some(0x4F), // Right.
            0x4F => Some(0x4D), // End.
            0x50 => Some(0x51), // Down.
            0x51 => Some(0x4E), // Page Down.
            0x52 => Some(0x49), // Insert.
            0x53 => Some(0x4C), // Delete Forward.
            0x5B => Some(0xE3), // Left GUI.
            0x5C => Some(0xE7), // Right GUI.
            0x5D => Some(0x65), // Application.
            _ => None,
        },
        ScanCodePrefix::E1 => match make_code {
            0x1D45 | 0x45 => Some(0x48),
            _ => None,
        },
    };
    usage.map_or_else(
        || PhysicalControlId::Native {
            backend: WINDOWS_KEYBOARD_BACKEND,
            code: windows_native_code(make_code, prefix),
        },
        PhysicalControlId::keyboard,
    )
}

/// Converts supported Linux evdev key codes to physical HID keyboard usages.
///
/// Unsupported codes retain their full u16 value in the Linux namespace.
/// EV_KEY transition/repeat values are separate from the key code.
pub fn linux_evdev_key(code: u16) -> PhysicalControlId {
    let usage = match code {
        86 => Some(0x64),  // KEY_102ND.
        87 => Some(0x44),  // KEY_F11.
        88 => Some(0x45),  // KEY_F12.
        96 => Some(0x58),  // KEY_KPENTER.
        97 => Some(0xE4),  // KEY_RIGHTCTRL.
        98 => Some(0x54),  // KEY_KPSLASH.
        99 => Some(0x46),  // KEY_SYSRQ.
        100 => Some(0xE6), // KEY_RIGHTALT.
        102 => Some(0x4A), // KEY_HOME.
        103 => Some(0x52), // KEY_UP.
        104 => Some(0x4B), // KEY_PAGEUP.
        105 => Some(0x50), // KEY_LEFT.
        106 => Some(0x4F), // KEY_RIGHT.
        107 => Some(0x4D), // KEY_END.
        108 => Some(0x51), // KEY_DOWN.
        109 => Some(0x4E), // KEY_PAGEDOWN.
        110 => Some(0x49), // KEY_INSERT.
        111 => Some(0x4C), // KEY_DELETE.
        117 => Some(0x67), // KEY_KPEQUAL.
        119 => Some(0x48), // KEY_PAUSE.
        125 => Some(0xE3), // KEY_LEFTMETA.
        126 => Some(0xE7), // KEY_RIGHTMETA.
        127 => Some(0x65), // KEY_COMPOSE/Application.
        _ => set1_usage(code),
    };
    usage.map_or_else(
        || PhysicalControlId::Native {
            backend: LINUX_KEYBOARD_BACKEND,
            code: u32::from(code),
        },
        PhysicalControlId::keyboard,
    )
}

/// Preserves the HID page and usage supplied by macOS IOHID acquisition.
///
/// This accepts nonkeyboard pages without guessing a keyboard interpretation.
pub const fn macos_hid_usage(usage_page: u16, usage: u16) -> PhysicalControlId {
    PhysicalControlId::HidUsage { usage_page, usage }
}

// Linux KEY_* values 1..=83 share the original Set 1 positions. Keeping the
// common positional subset together avoids two diverging keyboard tables.
fn set1_usage(code: u16) -> Option<u16> {
    Some(match code {
        0x01 => 0x29,               // Escape.
        0x02..=0x0A => code + 0x1C, // Top-row 1..9.
        0x0B => 0x27,               // Top-row 0.
        0x0C => 0x2D,
        0x0D => 0x2E,
        0x0E => 0x2A, // Backspace.
        0x0F => 0x2B, // Tab.
        0x10 => 0x14, // Q.
        0x11 => 0x1A, // W.
        0x12 => 0x08, // E.
        0x13 => 0x15, // R.
        0x14 => 0x17, // T.
        0x15 => 0x1C, // Y.
        0x16 => 0x18, // U.
        0x17 => 0x0C, // I.
        0x18 => 0x12, // O.
        0x19 => 0x13, // P.
        0x1A => 0x2F,
        0x1B => 0x30,
        0x1C => 0x28, // Main Enter.
        0x1D => 0xE0, // Left Control.
        0x1E => 0x04, // A.
        0x1F => 0x16, // S.
        0x20 => 0x07, // D.
        0x21 => 0x09, // F.
        0x22 => 0x0A, // G.
        0x23 => 0x0B, // H.
        0x24 => 0x0D, // J.
        0x25 => 0x0E, // K.
        0x26 => 0x0F, // L.
        0x27 => 0x33,
        0x28 => 0x34,
        0x29 => 0x35,
        0x2A => 0xE1, // Left Shift.
        0x2B => 0x31,
        0x2C => 0x1D, // Z.
        0x2D => 0x1B, // X.
        0x2E => 0x06, // C.
        0x2F => 0x19, // V.
        0x30 => 0x05, // B.
        0x31 => 0x11, // N.
        0x32 => 0x10, // M.
        0x33 => 0x36,
        0x34 => 0x37,
        0x35 => 0x38,
        0x36 => 0xE5,            // Right Shift.
        0x37 => 0x55,            // Keypad multiply.
        0x38 => 0xE2,            // Left Alt.
        0x39 => 0x2C,            // Space.
        0x3A => 0x39,            // Caps Lock.
        0x3B..=0x44 => code - 1, // F1..F10.
        0x45 => 0x53,            // Num Lock.
        0x46 => 0x47,            // Scroll Lock.
        0x47 => 0x5F,            // Keypad 7.
        0x48 => 0x60,            // Keypad 8.
        0x49 => 0x61,            // Keypad 9.
        0x4A => 0x56,            // Keypad minus.
        0x4B => 0x5C,            // Keypad 4.
        0x4C => 0x5D,            // Keypad 5.
        0x4D => 0x5E,            // Keypad 6.
        0x4E => 0x57,            // Keypad plus.
        0x4F => 0x59,            // Keypad 1.
        0x50 => 0x5A,            // Keypad 2.
        0x51 => 0x5B,            // Keypad 3.
        0x52 => 0x62,            // Keypad 0.
        0x53 => 0x63,            // Keypad period.
        _ => return None,
    })
}
