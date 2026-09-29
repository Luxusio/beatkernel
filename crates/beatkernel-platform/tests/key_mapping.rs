//! Literal cross-platform fixtures for the canonical-input contract.
//!
//! Values are independently taken from Microsoft's HID/Scan 1 table and Linux's
//! input-event-codes.h, linked in REQ__canonical-input.md, not production tables.

use std::collections::{BTreeMap, HashSet};

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

use ScanCodePrefix::{None as Plain, E0, E1};

// (Windows make code, prefix, Linux EV_KEY code, USB keyboard-page usage).
const LETTERS: &[(u16, ScanCodePrefix, u16, u16)] = &[
    (0x1e, Plain, 30, 0x04), // A
    (0x30, Plain, 48, 0x05), // B
    (0x2e, Plain, 46, 0x06), // C
    (0x20, Plain, 32, 0x07), // D
    (0x12, Plain, 18, 0x08), // E
    (0x21, Plain, 33, 0x09), // F
    (0x22, Plain, 34, 0x0a), // G
    (0x23, Plain, 35, 0x0b), // H
    (0x17, Plain, 23, 0x0c), // I
    (0x24, Plain, 36, 0x0d), // J
    (0x25, Plain, 37, 0x0e), // K
    (0x26, Plain, 38, 0x0f), // L
    (0x32, Plain, 50, 0x10), // M
    (0x31, Plain, 49, 0x11), // N
    (0x18, Plain, 24, 0x12), // O
    (0x19, Plain, 25, 0x13), // P
    (0x10, Plain, 16, 0x14), // Q
    (0x13, Plain, 19, 0x15), // R
    (0x1f, Plain, 31, 0x16), // S
    (0x14, Plain, 20, 0x17), // T
    (0x16, Plain, 22, 0x18), // U
    (0x2f, Plain, 47, 0x19), // V
    (0x11, Plain, 17, 0x1a), // W
    (0x2d, Plain, 45, 0x1b), // X
    (0x15, Plain, 21, 0x1c), // Y
    (0x2c, Plain, 44, 0x1d), // Z
];

const MODIFIERS: &[(u16, ScanCodePrefix, u16, u16)] = &[
    (0x1d, Plain, 29, 0xe0),
    (0x2a, Plain, 42, 0xe1),
    (0x38, Plain, 56, 0xe2),
    (0x5b, E0, 125, 0xe3),
    (0x1d, E0, 97, 0xe4),
    (0x36, Plain, 54, 0xe5),
    (0x38, E0, 100, 0xe6),
    (0x5c, E0, 126, 0xe7),
];

const DIGITS_AND_KEYPAD: &[(u16, ScanCodePrefix, u16, u16)] = &[
    (0x02, Plain, 2, 0x1e),
    (0x03, Plain, 3, 0x1f),
    (0x04, Plain, 4, 0x20),
    (0x05, Plain, 5, 0x21),
    (0x06, Plain, 6, 0x22),
    (0x07, Plain, 7, 0x23),
    (0x08, Plain, 8, 0x24),
    (0x09, Plain, 9, 0x25),
    (0x0a, Plain, 10, 0x26),
    (0x0b, Plain, 11, 0x27),
    (0x45, Plain, 69, 0x53),
    (0x35, E0, 98, 0x54),
    (0x37, Plain, 55, 0x55),
    (0x4a, Plain, 74, 0x56),
    (0x4e, Plain, 78, 0x57),
    (0x1c, E0, 96, 0x58),
    (0x4f, Plain, 79, 0x59),
    (0x50, Plain, 80, 0x5a),
    (0x51, Plain, 81, 0x5b),
    (0x4b, Plain, 75, 0x5c),
    (0x4c, Plain, 76, 0x5d),
    (0x4d, Plain, 77, 0x5e),
    (0x47, Plain, 71, 0x5f),
    (0x48, Plain, 72, 0x60),
    (0x49, Plain, 73, 0x61),
    (0x52, Plain, 82, 0x62),
    (0x53, Plain, 83, 0x63),
    (0x59, Plain, 117, 0x67),
];

const FUNCTION_KEYS: &[(u16, ScanCodePrefix, u16, u16)] = &[
    (0x3b, Plain, 59, 0x3a),
    (0x3c, Plain, 60, 0x3b),
    (0x3d, Plain, 61, 0x3c),
    (0x3e, Plain, 62, 0x3d),
    (0x3f, Plain, 63, 0x3e),
    (0x40, Plain, 64, 0x3f),
    (0x41, Plain, 65, 0x40),
    (0x42, Plain, 66, 0x41),
    (0x43, Plain, 67, 0x42),
    (0x44, Plain, 68, 0x43),
    (0x57, Plain, 87, 0x44),
    (0x58, Plain, 88, 0x45),
];

const NAVIGATION_AND_PUNCTUATION: &[(u16, ScanCodePrefix, u16, u16)] = &[
    (0x1c, Plain, 28, 0x28),
    (0x01, Plain, 1, 0x29),
    (0x0e, Plain, 14, 0x2a),
    (0x0f, Plain, 15, 0x2b),
    (0x39, Plain, 57, 0x2c),
    (0x0c, Plain, 12, 0x2d),
    (0x0d, Plain, 13, 0x2e),
    (0x1a, Plain, 26, 0x2f),
    (0x1b, Plain, 27, 0x30),
    (0x2b, Plain, 43, 0x31),
    (0x27, Plain, 39, 0x33),
    (0x28, Plain, 40, 0x34),
    (0x29, Plain, 41, 0x35),
    (0x33, Plain, 51, 0x36),
    (0x34, Plain, 52, 0x37),
    (0x35, Plain, 53, 0x38),
    (0x3a, Plain, 58, 0x39),
    (0x46, Plain, 70, 0x47),
    (0x52, E0, 110, 0x49),
    (0x47, E0, 102, 0x4a),
    (0x49, E0, 104, 0x4b),
    (0x53, E0, 111, 0x4c),
    (0x4f, E0, 107, 0x4d),
    (0x51, E0, 109, 0x4e),
    (0x4d, E0, 106, 0x4f),
    (0x4b, E0, 105, 0x50),
    (0x50, E0, 108, 0x51),
    (0x48, E0, 103, 0x52),
    (0x56, Plain, 86, 0x64),
    (0x5d, E0, 127, 0x65),
];

fn assert_convergence(fixtures: &[(u16, ScanCodePrefix, u16, u16)]) {
    for &(make, prefix, evdev, usage) in fixtures {
        let expected = PhysicalControlId::HidUsage {
            usage_page: 0x07,
            usage,
        };
        assert_eq!(
            windows_scan_code(make, prefix),
            expected,
            "Windows {make:#06x}, {prefix:?}"
        );
        assert_eq!(linux_evdev_key(evdev), expected, "Linux {evdev}");
        assert_eq!(macos_hid_usage(0x07, usage), expected, "macOS {usage:#06x}");
    }
}

#[test]
fn all_letter_positions_converge_independently_of_text_layout() {
    assert_convergence(LETTERS);
}

#[test]
fn eight_modifiers_retain_left_and_right_identity() {
    assert_convergence(MODIFIERS);
    let normalized: HashSet<_> = MODIFIERS
        .iter()
        .map(|&(code, prefix, _, _)| windows_scan_code(code, prefix))
        .collect();
    assert_eq!(normalized.len(), 8);
}

#[test]
fn number_row_and_keypad_remain_distinct_physical_controls() {
    assert_convergence(DIGITS_AND_KEYPAD);
    assert_ne!(windows_scan_code(0x1c, Plain), windows_scan_code(0x1c, E0));
    assert_ne!(windows_scan_code(0x47, Plain), windows_scan_code(0x47, E0));
    assert_ne!(linux_evdev_key(28), linux_evdev_key(96));
    assert_ne!(linux_evdev_key(71), linux_evdev_key(102));
}

#[test]
fn function_keys_navigation_punctuation_and_international_key_converge() {
    assert_convergence(FUNCTION_KEYS);
    assert_convergence(NAVIGATION_AND_PUNCTUATION);
}

#[test]
fn print_screen_and_complete_pause_sequences_preserve_special_key_identity() {
    assert_convergence(&[(0x37, E0, 99, 0x46), (0x1d45, E1, 119, 0x48)]);
    assert_eq!(
        windows_scan_code(0x54, Plain),
        PhysicalControlId::keyboard(0x46)
    );
    assert_eq!(
        windows_scan_code(0x45, E1),
        PhysicalControlId::keyboard(0x48)
    );
    assert_eq!(
        windows_scan_code(0x46, E0),
        PhysicalControlId::keyboard(0x48)
    );
    assert_eq!(
        windows_scan_code(0x45, Plain),
        PhysicalControlId::keyboard(0x53)
    );
    assert_eq!(
        windows_scan_code(0x1d, E1),
        PhysicalControlId::Native {
            backend: WINDOWS_KEYBOARD_BACKEND,
            code: 0x00e1_001d,
        }
    );
}

#[test]
fn unknown_native_controls_keep_full_codes_prefixes_and_backend_namespaces() {
    assert_eq!(WINDOWS_KEYBOARD_BACKEND, BackendId(1));
    assert_eq!(LINUX_KEYBOARD_BACKEND, BackendId(2));
    assert_eq!(MACOS_HID_BACKEND, BackendId(3));
    assert_eq!(
        HashSet::from([
            WINDOWS_KEYBOARD_BACKEND,
            LINUX_KEYBOARD_BACKEND,
            MACOS_HID_BACKEND
        ])
        .len(),
        3
    );
    for code in [0, 0x011e, 0x8000, 0xffff] {
        for (prefix, encoded_prefix) in [(Plain, 0), (E0, 0xe0), (E1, 0xe1)] {
            let expected_code = (encoded_prefix << 16) | u32::from(code);
            assert_eq!(windows_native_code(code, prefix), expected_code);
            assert_eq!(
                windows_scan_code(code, prefix),
                PhysicalControlId::Native {
                    backend: WINDOWS_KEYBOARD_BACKEND,
                    code: expected_code,
                }
            );
        }
    }
    for code in [0, 0x8000, 0xffff] {
        assert_eq!(
            linux_evdev_key(code),
            PhysicalControlId::Native {
                backend: LINUX_KEYBOARD_BACKEND,
                code: u32::from(code),
            }
        );
    }
    assert_ne!(windows_scan_code(0xffff, Plain), linux_evdev_key(0xffff));
}

#[test]
fn macos_preserves_arbitrary_hid_pages_and_usages_without_keyboard_reinterpretation() {
    for (page, usage) in [
        (0, 0),
        (1, 0x81),
        (0x0c, 0xe9),
        (0xff00, 4),
        (0xffff, 0xffff),
        (7, 0xffff),
    ] {
        assert_eq!(
            macos_hid_usage(page, usage),
            PhysicalControlId::HidUsage {
                usage_page: page,
                usage,
            }
        );
    }
    assert_ne!(macos_hid_usage(7, 4), macos_hid_usage(0xff00, 4));
}

#[test]
fn exhaustive_windows_input_namespace_has_only_documented_hid_aliases() {
    let mut mapped: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
    let mut unknown = HashSet::new();
    for (prefix, encoded_prefix) in [(Plain, 0), (E0, 0xe0), (E1, 0xe1)] {
        for make in 0..=u16::MAX {
            let native = (encoded_prefix << 16) | u32::from(make);
            assert_eq!(windows_native_code(make, prefix), native);
            match windows_scan_code(make, prefix) {
                PhysicalControlId::HidUsage { usage_page, usage } => {
                    assert_eq!(usage_page, 7);
                    assert!(
                        (1..=0xe7).contains(&usage),
                        "invalid mapped HID {usage:#06x}"
                    );
                    mapped.entry(usage).or_default().push(native);
                }
                PhysicalControlId::Native { backend, code } => {
                    assert_eq!(backend, WINDOWS_KEYBOARD_BACKEND);
                    assert_eq!(code, native);
                    assert!(unknown.insert(code), "lost prefix or make-code bits");
                }
                other => panic!("unexpected keyboard control namespace: {other:?}"),
            }
        }
    }
    let aliases: BTreeMap<_, _> = mapped
        .into_iter()
        .filter_map(|(usage, mut codes)| {
            codes.sort_unstable();
            (codes.len() > 1).then_some((usage, codes))
        })
        .collect();
    assert_eq!(
        aliases,
        BTreeMap::from([
            (0x46, vec![0x0000_0054, 0x00e0_0037]),
            (0x48, vec![0x00e0_0046, 0x00e1_0045, 0x00e1_1d45]),
        ])
    );
}

#[test]
fn exhaustive_linux_input_namespace_is_lossless_and_has_no_unintended_hid_aliases() {
    let mut mapped = HashSet::new();
    for code in 0..=u16::MAX {
        match linux_evdev_key(code) {
            PhysicalControlId::HidUsage { usage_page, usage } => {
                assert_eq!(usage_page, 7);
                assert!((1..=0xe7).contains(&usage));
                assert!(
                    mapped.insert(usage),
                    "unintended Linux alias at native code {code}"
                );
            }
            PhysicalControlId::Native {
                backend,
                code: native,
            } => {
                assert_eq!(backend, LINUX_KEYBOARD_BACKEND);
                assert_eq!(native, u32::from(code));
            }
            other => panic!("unexpected keyboard control namespace: {other:?}"),
        }
    }
}

struct FixtureMapper;

impl ClockMapper for FixtureMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if to != ClockDomainId(30) {
            return None;
        }
        let offset = match from.domain {
            ClockDomainId(10) => 1000,
            ClockDomainId(20) => -500,
            _ => return None,
        };
        from.timestamp
            .as_nanos()
            .checked_add(offset)
            .map(Timestamp::from_nanos)
    }

    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

fn fixture_device(id: u64) -> DeviceDescriptor {
    DeviceDescriptor {
        runtime_id: DeviceId(id),
        vendor_id: Some(0x1234),
        product_id: Some(0x5678),
        serial: None,
        name: Some("identical fixture keyboard".into()),
        transport: DeviceTransport::Virtual,
        capabilities: DeviceCapabilities {
            button: true,
            ..DeviceCapabilities::default()
        },
    }
}

#[test]
fn normalized_same_key_from_two_devices_keeps_source_native_codes_and_clock_provenance() {
    let mut queue = VirtualInputBackend::new(ClockDomainId(30));
    queue.register_device(fixture_device(101)).unwrap();
    queue.register_device(fixture_device(102)).unwrap();
    let fixtures = [
        (
            DeviceId(101),
            ClockDomainId(10),
            75,
            WINDOWS_KEYBOARD_BACKEND,
            0x00e0_001c,
            windows_scan_code(0x1c, E0),
            1075,
        ),
        (
            DeviceId(102),
            ClockDomainId(20),
            4000,
            LINUX_KEYBOARD_BACKEND,
            96,
            linux_evdev_key(96),
            3500,
        ),
    ];
    let mut expected = Vec::new();
    for (source, domain, nanos, backend, native_code, control, normalized) in fixtures {
        let original = ClockPoint {
            domain,
            timestamp: Timestamp::from_nanos(nanos),
        };
        let native_point = ClockPoint {
            domain: ClockDomainId(100 + u32::from(backend == LINUX_KEYBOARD_BACKEND)),
            timestamp: Timestamp::from_nanos(nanos - 10),
        };
        let mut meta = EventMeta::new(source, original, 0);
        meta.native = Some(NativeEventMeta {
            backend,
            code: Some(native_code),
            timestamp: Some(native_point),
        });
        queue
            .push(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control,
                    state: ButtonState::Down,
                }),
                &FixtureMapper,
            )
            .unwrap();
        meta.timestamp = Timestamp::from_nanos(normalized);
        meta.clock_domain = ClockDomainId(30);
        meta.original_clock_point = Some(original);
        expected.push(PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(0x58),
            state: ButtonState::Down,
        }));
    }
    let received: Vec<_> = queue.drain_events().collect();
    assert_eq!(received, expected);
    assert_ne!(received[0].meta().source, received[1].meta().source);
    assert_ne!(received[0].meta().native, received[1].meta().native);
    assert_eq!(queue.devices().count(), 2);
}
