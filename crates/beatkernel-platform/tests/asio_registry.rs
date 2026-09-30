//! Pure registration fixtures: no registry enumeration or ASIO driver loading.
#![cfg(target_os = "windows")]

use beatkernel_platform::windows::asio::{
    AsioDriverRegistration, AsioEnumerationLimits, AsioRegistryError, AsioRegistryView,
};
use std::{collections::HashSet, error::Error};

const CLSID: &str = "{12345678-9ABC-DEF0-1234-56789ABCDEF0}";
const PLAIN: &str = "12345678-9abc-def0-1234-56789abcdef0";

fn registration(
    name: &str,
    description: Option<&str>,
    clsid: &str,
    limits: AsioEnumerationLimits,
) -> Result<AsioDriverRegistration, AsioRegistryError> {
    AsioDriverRegistration::from_values(name, description, clsid, AsioRegistryView::Native, limits)
}

#[test]
fn clsid_spellings_canonicalize_without_erasing_selected_registry_view() {
    let views = [
        AsioRegistryView::Native,
        AsioRegistryView::Bits32,
        AsioRegistryView::Bits64,
    ];
    assert_eq!(views.into_iter().collect::<HashSet<_>>().len(), 3);
    for view in views {
        for supplied in [PLAIN, CLSID, "{12345678-9abc-def0-1234-56789abcdef0}"] {
            let driver = AsioDriverRegistration::from_values(
                "Device Ω",
                Some("日本語 output"),
                supplied,
                view,
                AsioEnumerationLimits::default(),
            )
            .unwrap();
            assert_eq!(driver.name, "Device Ω");
            assert_eq!(driver.description.as_deref(), Some("日本語 output"));
            assert_eq!(driver.id.clsid, CLSID);
            assert_eq!(driver.id.view, view);
        }
    }
}

#[test]
fn absent_empty_and_unicode_descriptions_remain_distinct_exact_values() {
    for description in [None, Some(""), Some(" 😀 Ω e\u{301} ")] {
        let driver = registration(
            "Driver/left",
            description,
            CLSID,
            AsioEnumerationLimits::default(),
        )
        .unwrap();
        assert_eq!(driver.name, "Driver/left");
        assert_eq!(driver.description.as_deref(), description);
    }
}

#[test]
fn defaults_and_inclusive_limit_ranges_validate_before_registration_values() {
    let defaults = AsioEnumerationLimits::default();
    assert_eq!(defaults.max_drivers, 256);
    assert_eq!(defaults.max_value_units, 4096);
    defaults.validate().unwrap();
    for limits in [
        AsioEnumerationLimits {
            max_drivers: 1,
            max_value_units: 2,
        },
        AsioEnumerationLimits {
            max_drivers: 4096,
            max_value_units: 32768,
        },
    ] {
        limits.validate().unwrap();
    }
    for limits in [
        AsioEnumerationLimits {
            max_drivers: 0,
            ..defaults
        },
        AsioEnumerationLimits {
            max_drivers: 4097,
            ..defaults
        },
        AsioEnumerationLimits {
            max_drivers: usize::MAX,
            ..defaults
        },
        AsioEnumerationLimits {
            max_value_units: 0,
            ..defaults
        },
        AsioEnumerationLimits {
            max_value_units: 1,
            ..defaults
        },
        AsioEnumerationLimits {
            max_value_units: 32769,
            ..defaults
        },
        AsioEnumerationLimits {
            max_value_units: usize::MAX,
            ..defaults
        },
    ] {
        assert_eq!(limits.validate(), Err(AsioRegistryError::InvalidLimits));
        assert!(matches!(
            registration("", Some("bad\0"), "bad", limits),
            Err(AsioRegistryError::InvalidLimits)
        ));
    }
}

#[test]
fn reg_sz_budget_counts_utf16_units_and_terminating_nul_even_for_clsid() {
    let limits = AsioEnumerationLimits {
        max_drivers: 1,
        max_value_units: 39,
    };
    let exact = "😀".repeat(19); // 38 UTF-16 units plus one terminal NUL.
    let driver = registration("name", Some(&exact), CLSID, limits).unwrap();
    assert_eq!(driver.description.as_deref(), Some(exact.as_str()));
    assert!(matches!(
        registration("name", Some(&(exact + "a")), CLSID, limits),
        Err(AsioRegistryError::Capacity)
    ));
    assert!(matches!(
        registration(
            "name",
            None,
            CLSID,
            AsioEnumerationLimits {
                max_value_units: 38,
                ..limits
            }
        ),
        Err(AsioRegistryError::Capacity)
    ));
    // An unbraced 36-unit native value fits a 37-unit read budget, even though
    // the owned canonical identity subsequently adds braces.
    let driver = registration(
        "name",
        None,
        PLAIN,
        AsioEnumerationLimits {
            max_value_units: 37,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(driver.id.clsid, CLSID);
    assert!(matches!(
        registration(
            "name",
            None,
            PLAIN,
            AsioEnumerationLimits {
                max_value_units: 36,
                ..limits
            }
        ),
        Err(AsioRegistryError::Capacity)
    ));
}

#[test]
fn names_have_separate_255_unit_key_budget_not_reg_sz_value_budget() {
    let exact = "😀".repeat(127) + "a"; // 255 UTF-16 units, not 128 units.
    let limits = AsioEnumerationLimits {
        max_drivers: 1,
        max_value_units: 39,
    };
    let driver = registration(&exact, None, CLSID, limits).unwrap();
    assert_eq!(driver.name, exact);
    assert!(matches!(
        registration(&(exact + "b"), None, CLSID, limits),
        Err(AsioRegistryError::Capacity)
    ));
    assert!(registration(&"x".repeat(255), None, CLSID, limits).is_ok());
    assert!(matches!(
        registration(&"x".repeat(256), None, CLSID, limits),
        Err(AsioRegistryError::Capacity)
    ));
}

#[test]
fn malformed_names_descriptions_and_uuid_shapes_report_their_exact_field() {
    for name in [
        "",
        "\0",
        "driver\0tail",
        "driver\\child",
        "\\driver",
        "driver\\",
    ] {
        assert!(
            matches!(
                registration(name, None, CLSID, AsioEnumerationLimits::default()),
                Err(AsioRegistryError::MalformedRegistration { field: "name" })
            ),
            "{name:?}"
        );
    }
    for description in ["\0", "description\0tail", "description\0"] {
        assert!(matches!(
            registration(
                "driver",
                Some(description),
                CLSID,
                AsioEnumerationLimits::default()
            ),
            Err(AsioRegistryError::MalformedRegistration {
                field: "description"
            })
        ));
    }
    for clsid in [
        "",
        "12345678-9abc-def0-1234-56789abcdef",
        "123456789abc-def0-1234-56789abcdef0",
        "12345678_9abc-def0-1234-56789abcdef0",
        "12345678-9abc_def0-1234-56789abcdef0",
        "12345678-9abc-def0_1234-56789abcdef0",
        "12345678-9abc-def0-1234_56789abcdef0",
        "G2345678-9abc-def0-1234-56789abcdef0",
        "12345678-9abc-def0-1234-56789abcdeg0",
        "{12345678-9abc-def0-1234-56789abcdef0",
        "12345678-9abc-def0-1234-56789abcdef0}",
        "{{12345678-9abc-def0-1234-56789abcdef0}}",
        "[12345678-9abc-def0-1234-56789abcdef0]",
        " 12345678-9abc-def0-1234-56789abcdef0",
        "12345678-9abc-def0-1234-56789abcdef0 ",
        "12345678-9abc-def0-1234-56789abcdef0\0",
        "12345678-9abc-def0-1234-56789abc\0ef0",
        "00000000-0000-0000-0000-000000000000",
        "{00000000-0000-0000-0000-000000000000}",
    ] {
        assert!(
            matches!(
                registration("driver", None, clsid, AsioEnumerationLimits::default()),
                Err(AsioRegistryError::MalformedRegistration { field: "CLSID" })
            ),
            "{clsid:?}"
        );
    }
    let driver = registration(
        "driver",
        None,
        "00000000-0000-0000-0000-000000000001",
        AsioEnumerationLimits::default(),
    )
    .unwrap();
    assert_eq!(driver.id.clsid, "{00000000-0000-0000-0000-000000000001}");
}

#[test]
fn public_errors_are_equatable_debuggable_and_usable_as_standard_errors() {
    fn standard_error(error: &dyn Error) {
        assert!(!error.to_string().is_empty());
    }
    for error in [
        AsioRegistryError::InvalidLimits,
        AsioRegistryError::Capacity,
        AsioRegistryError::MalformedRegistration {
            field: "description",
        },
        AsioRegistryError::Native { code: 5 },
    ] {
        standard_error(&error);
        assert!(!format!("{error:?}").is_empty());
    }
    assert_ne!(
        AsioRegistryError::Native { code: 5 },
        AsioRegistryError::Native { code: 6 }
    );
    assert!(AsioRegistryError::Native { code: 5 }
        .to_string()
        .contains('5'));
    assert!(AsioRegistryError::MalformedRegistration {
        field: "description"
    }
    .to_string()
    .contains("description"));
}
