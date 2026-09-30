//! SDK-free ASIO registration discovery and optional SDK driver control.
//!
//! Discovery uses Windows registry metadata independently of any ASIO SDK. Registration
//! identity does not prove process-bitness compatibility, loadability or output.
//! Native enumeration is bounded but is not a transactional registry snapshot.

// This module owns the native Win32 registry FFI boundary.
#![allow(unsafe_code)]

/// Optional real SDK driver control, consumed when preparing an output stream.
#[cfg(feature = "asio-sdk")]
pub mod control;

/// Optional owned SDK output buffers and actual Mixer callbacks.
#[cfg(feature = "asio-sdk")]
pub mod stream;

use std::{error::Error, fmt, ptr};
use windows_sys::Win32::{
    Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_INVALID_HANDLE, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
        ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
    },
    System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SAM_FLAGS,
        REG_SZ,
    },
};

/// Explicit registry view; views are never merged or selected from driver names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AsioRegistryView {
    /// The calling process's native registry view.
    Native,
    /// Explicit 32-bit registry view.
    Bits32,
    /// Explicit 64-bit registry view.
    Bits64,
}
impl AsioRegistryView {
    fn flags(self) -> REG_SAM_FLAGS {
        match self {
            Self::Native => 0,
            Self::Bits32 => KEY_WOW64_32KEY,
            Self::Bits64 => KEY_WOW64_64KEY,
        }
    }
}

/// Caller-selected registry count and string-storage bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioEnumerationLimits {
    /// Maximum returned registrations; 1..4096, default 256.
    pub max_drivers: usize,
    /// Maximum stored value UTF-16 units including terminal NUL; 2..32768.
    ///
    /// Applies to Description and CLSID, not the separate 255-unit key-name cap.
    pub max_value_units: usize,
}
impl Default for AsioEnumerationLimits {
    fn default() -> Self {
        Self {
            max_drivers: 256,
            max_value_units: 4096,
        }
    }
}
impl AsioEnumerationLimits {
    /// Rejects invalid limits before allocation or registry access.
    pub fn validate(self) -> Result<(), AsioRegistryError> {
        if !(1..=4096).contains(&self.max_drivers) || !(2..=32768).contains(&self.max_value_units) {
            return Err(AsioRegistryError::InvalidLimits);
        }
        Ok(())
    }
}

/// Canonical registry identity with the caller-selected view retained.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AsioDriverId {
    /// Nonzero uppercase UUID, including braces and exact separators.
    pub clsid: String,
    /// Registry view in which this registration was discovered.
    pub view: AsioRegistryView,
}

/// Validated metadata for an installed registry entry, not an opened driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsioDriverRegistration {
    /// Nonempty Unicode subkey name, at most 255 UTF-16 units.
    pub name: String,
    /// Optional Unicode Description; an explicitly empty value is retained.
    pub description: Option<String>,
    /// Canonical CLSID and selected registry view.
    pub id: AsioDriverId,
}
impl AsioDriverRegistration {
    /// Validates supplied metadata and canonicalizes its CLSID without native IO.
    ///
    /// Accepts exact 36-character hyphenated or 38-character braced UUID strings.
    /// Value bounds include one terminal NUL on the supplied string, before CLSID
    /// canonicalization. Names reject NUL/backslash; ordinary slash is retained.
    pub fn from_values(
        name: &str,
        description: Option<&str>,
        clsid: &str,
        view: AsioRegistryView,
        limits: AsioEnumerationLimits,
    ) -> Result<Self, AsioRegistryError> {
        limits.validate()?;
        validate_name(name)?;
        if let Some(description) = description {
            validate_value(description, "description", limits)?;
        }
        validate_value(clsid, "CLSID", limits)?;
        let clsid = canonical_clsid(clsid)?;
        Ok(Self {
            name: copy_string(name)?,
            description: description.map(copy_string).transpose()?,
            id: AsioDriverId { clsid, view },
        })
    }
}

/// Explicit enumeration/registration failures; no malformed entries are skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioRegistryError {
    /// Driver count or value-storage limits fall outside documented bounds.
    InvalidLimits,
    /// Configured count/string capacity, key-name extent or allocation exhausted.
    Capacity,
    /// Invalid syntax, REG_SZ extent/type or Unicode for the indicated field.
    MalformedRegistration {
        /// One of "name", "description" or "CLSID".
        field: &'static str,
    },
    /// A native registry operation failed, including concurrent mutation errors.
    Native {
        /// Unmodified Win32 status code.
        code: u32,
    },
}
impl fmt::Display for AsioRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => {
                f.write_str("ASIO registry limits require drivers 1..4096 and value units 2..32768")
            }
            Self::Capacity => f.write_str("ASIO registry capacity or bounded allocation exhausted"),
            Self::MalformedRegistration { field } => {
                write!(f, "malformed ASIO registration field {field}")
            }
            Self::Native { code } => {
                write!(f, "ASIO registry operation failed with Win32 status {code}")
            }
        }
    }
}
impl Error for AsioRegistryError {}

fn malformed(field: &'static str) -> AsioRegistryError {
    AsioRegistryError::MalformedRegistration { field }
}
fn validate_name(name: &str) -> Result<(), AsioRegistryError> {
    if name.is_empty() || name.contains('\0') || name.contains('\\') {
        return Err(malformed("name"));
    }
    if name.encode_utf16().count() > 255 {
        return Err(AsioRegistryError::Capacity);
    }
    Ok(())
}
fn validate_value(
    value: &str,
    field: &'static str,
    limits: AsioEnumerationLimits,
) -> Result<(), AsioRegistryError> {
    if value.contains('\0') {
        return Err(malformed(field));
    }
    let units = value
        .encode_utf16()
        .count()
        .checked_add(1)
        .ok_or(AsioRegistryError::Capacity)?;
    if units > limits.max_value_units {
        return Err(AsioRegistryError::Capacity);
    }
    Ok(())
}
fn copy_string(value: &str) -> Result<String, AsioRegistryError> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len())
        .map_err(|_| AsioRegistryError::Capacity)?;
    copy.push_str(value);
    Ok(copy)
}
fn canonical_clsid(value: &str) -> Result<String, AsioRegistryError> {
    let bytes = value.as_bytes();
    let body = match bytes.len() {
        36 => bytes,
        38 if bytes[0] == b'{' && bytes[37] == b'}' => &bytes[1..37],
        _ => return Err(malformed("CLSID")),
    };
    let mut nonzero = false;
    for (index, &byte) in body.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return Err(malformed("CLSID"));
            }
        } else {
            if !byte.is_ascii_hexdigit() {
                return Err(malformed("CLSID"));
            }
            nonzero |= byte != b'0';
        }
    }
    if !nonzero {
        return Err(malformed("CLSID"));
    }
    let mut canonical = String::new();
    canonical
        .try_reserve_exact(38)
        .map_err(|_| AsioRegistryError::Capacity)?;
    canonical.push('{');
    for &byte in body {
        canonical.push(char::from(byte.to_ascii_uppercase()));
    }
    canonical.push('}');
    Ok(canonical)
}
fn decode_string(units: &[u16], field: &'static str) -> Result<String, AsioRegistryError> {
    // Every UTF-16 unit needs at most three UTF-8 bytes; surrogate pairs need
    // four for two units. Reserve fallibly before decoding untrusted strings.
    let capacity = units
        .len()
        .checked_mul(3)
        .ok_or(AsioRegistryError::Capacity)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity)
        .map_err(|_| AsioRegistryError::Capacity)?;
    for decoded in std::char::decode_utf16(units.iter().copied()) {
        text.push(decoded.map_err(|_| malformed(field))?);
    }
    Ok(text)
}

// Only handles returned successfully by RegOpenKeyExW enter this owner. The
// predefined HKEY_LOCAL_MACHINE is borrowed and is never closed by this type.
struct RegistryKey(Option<HKEY>);
impl RegistryKey {
    fn handle(&self) -> HKEY {
        self.0.expect("live owned registry key")
    }
    fn close(mut self) -> Result<(), AsioRegistryError> {
        let handle = self.0.take().expect("live owned registry key");
        // SAFETY: exclusive owner takes the successfully opened handle exactly
        // once; no reference or native operation retains it after this call.
        let code = unsafe { RegCloseKey(handle) };
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(AsioRegistryError::Native { code })
        }
    }
}
impl Drop for RegistryKey {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // SAFETY: remaining successfully opened handle is owned exclusively;
            // fallback cleanup closes it once on every early return/unwind.
            let _ = unsafe { RegCloseKey(handle) };
        }
    }
}
fn open_key(
    parent: HKEY,
    name: &[u16],
    access: REG_SAM_FLAGS,
) -> Result<RegistryKey, AsioRegistryError> {
    let mut handle = ptr::null_mut();
    // SAFETY: parent is a predefined borrowed key or live scoped owner; name is
    // a checked NUL-terminated UTF-16 array alive through the synchronous call.
    // Output points to an initialized handle slot; requested rights are read-only.
    let code = unsafe { RegOpenKeyExW(parent, name.as_ptr(), 0, access, &mut handle) };
    if code != ERROR_SUCCESS {
        return Err(AsioRegistryError::Native { code });
    }
    if handle.is_null() {
        return Err(AsioRegistryError::Native {
            code: ERROR_INVALID_HANDLE,
        });
    }
    Ok(RegistryKey(Some(handle)))
}

const ROOT: &[u16] = &[83, 79, 70, 84, 87, 65, 82, 69, 92, 65, 83, 73, 79, 0]; // SOFTWARE\ASIO
const DESCRIPTION: &[u16] = &[68, 101, 115, 99, 114, 105, 112, 116, 105, 111, 110, 0];
const CLSID: &[u16] = &[67, 76, 83, 73, 68, 0];

fn read_sz(
    key: &RegistryKey,
    value_name: &[u16],
    field: &'static str,
    limits: AsioEnumerationLimits,
) -> Result<Option<String>, AsioRegistryError> {
    let mut units = Vec::new();
    units
        .try_reserve_exact(limits.max_value_units)
        .map_err(|_| AsioRegistryError::Capacity)?;
    units.resize(limits.max_value_units, 0u16);
    let capacity_bytes = u32::try_from(
        units
            .len()
            .checked_mul(2)
            .ok_or(AsioRegistryError::Capacity)?,
    )
    .map_err(|_| AsioRegistryError::Capacity)?;
    let mut byte_count = capacity_bytes;
    let mut kind = 0;
    // SAFETY: scoped key stays open, literal value name is NUL-terminated, and
    // initialized aligned u16 storage exposes exactly capacity_bytes writable
    // bytes. Both output scalars are valid. No pointers escape this call.
    let code = unsafe {
        RegQueryValueExW(
            key.handle(),
            value_name.as_ptr(),
            ptr::null(),
            &mut kind,
            units.as_mut_ptr().cast(),
            &mut byte_count,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code == ERROR_MORE_DATA {
        return Err(AsioRegistryError::Capacity);
    }
    if code != ERROR_SUCCESS {
        return Err(AsioRegistryError::Native { code });
    }
    Ok(Some(decode_sz(&units, byte_count, kind, field)?))
}

fn decode_sz(
    units: &[u16],
    byte_count: u32,
    kind: u32,
    field: &'static str,
) -> Result<String, AsioRegistryError> {
    // RegQueryValueExW does not guarantee string termination. Never search past
    // the returned extent or infer a terminator from zero-initialized capacity:
    // https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regqueryvalueexw
    let capacity_bytes = units
        .len()
        .checked_mul(2)
        .ok_or(AsioRegistryError::Capacity)?;
    if byte_count as usize > capacity_bytes {
        return Err(AsioRegistryError::Capacity);
    }
    if kind != REG_SZ || byte_count < 2 || byte_count % 2 != 0 {
        return Err(malformed(field));
    }
    let length = byte_count as usize / 2;
    let value = &units[..length];
    if value[length - 1] != 0 || value[..length - 1].contains(&0) {
        return Err(malformed(field));
    }
    decode_string(&value[..length - 1], field)
}
fn read_registration(
    key: &RegistryKey,
    name: &str,
    view: AsioRegistryView,
    limits: AsioEnumerationLimits,
) -> Result<AsioDriverRegistration, AsioRegistryError> {
    let description = read_sz(key, DESCRIPTION, "description", limits)?;
    let clsid = read_sz(key, CLSID, "CLSID", limits)?.ok_or_else(|| malformed("CLSID"))?;
    AsioDriverRegistration::from_values(name, description.as_deref(), &clsid, view, limits)
}
fn enumerate_root(
    root: &RegistryKey,
    view: AsioRegistryView,
    limits: AsioEnumerationLimits,
) -> Result<Vec<AsioDriverRegistration>, AsioRegistryError> {
    let mut drivers = Vec::new();
    drivers
        .try_reserve_exact(limits.max_drivers)
        .map_err(|_| AsioRegistryError::Capacity)?;
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut units = name.len() as u32;
        // SAFETY: root is a live scoped key; the fixed initialized buffer and
        // unit-count slot remain writable for the call. All optional outputs and
        // reserved pointer are null. Windows length excludes the terminal NUL.
        let code = unsafe {
            RegEnumKeyExW(
                root.handle(),
                index,
                name.as_mut_ptr(),
                &mut units,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            break;
        }
        if code == ERROR_MORE_DATA {
            return Err(AsioRegistryError::Capacity);
        }
        if code != ERROR_SUCCESS {
            return Err(AsioRegistryError::Native { code });
        }
        // Check the extra registration, not just a full list: exactly the limit
        // remains valid when the next native call returns NO_MORE_ITEMS.
        if drivers.len() == limits.max_drivers || units > 255 {
            return Err(AsioRegistryError::Capacity);
        }
        let text = decode_string(&name[..units as usize], "name")?;
        validate_name(&text)?;
        name[units as usize] = 0;
        let key = open_key(
            root.handle(),
            &name[..units as usize + 1],
            KEY_QUERY_VALUE | view.flags(),
        )?;
        let registration = read_registration(&key, &text, view, limits);
        let close = key.close();
        let registration = registration?;
        close?;
        drivers.push(registration);
        index = index.checked_add(1).ok_or(AsioRegistryError::Capacity)?;
    }
    // No allocation for sorting; equal name/CLSID entries need no special view
    // choice because this invocation reads only one explicitly selected view.
    drivers.sort_unstable_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.clsid.cmp(&right.id.clsid))
    });
    Ok(drivers)
}

/// Enumerates bounded HKLM SOFTWARE\ASIO metadata in one explicit registry view.
///
/// An absent root returns an empty list. Missing Description is None; missing
/// CLSID, malformed metadata, inaccessible keys, native mutation errors and
/// capacity exhaustion reject the entire result rather than silently skip entries.
/// Owned registry handles close on success/error. No COM, DLL or audio is opened.
pub fn enumerate_asio_drivers(
    view: AsioRegistryView,
    limits: AsioEnumerationLimits,
) -> Result<Vec<AsioDriverRegistration>, AsioRegistryError> {
    limits.validate()?;
    let root = match open_key(
        HKEY_LOCAL_MACHINE,
        ROOT,
        KEY_ENUMERATE_SUB_KEYS | view.flags(),
    ) {
        Ok(root) => root,
        Err(AsioRegistryError::Native { code })
            if matches!(code, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) =>
        {
            return Ok(Vec::new())
        }
        Err(error) => return Err(error),
    };
    let result = enumerate_root(&root, view, limits);
    let close = root.close();
    let drivers = result?;
    close?;
    Ok(drivers)
}

#[cfg(test)]
mod extent_fixtures {
    use super::*;

    #[test]
    fn returned_extent_requires_its_own_single_terminal_nul() {
        let expected = malformed("description");
        // Zero-initialized capacity beyond returned bytes is not a terminator.
        assert_eq!(decode_sz(&[65, 0], 2, REG_SZ, "description"), Err(expected));
        assert_eq!(
            decode_sz(&[65, 0, 66, 0], 8, REG_SZ, "description"),
            Err(expected)
        );
        assert_eq!(decode_sz(&[0, 0], 4, REG_SZ, "description"), Err(expected));
        assert_eq!(decode_sz(&[0], 2, REG_SZ, "description").unwrap(), "");
        assert_eq!(
            decode_sz(&[65, 0, 66], 4, REG_SZ, "description").unwrap(),
            "A"
        );
    }

    #[test]
    fn type_byte_alignment_and_capacity_are_checked_before_decoding() {
        for bytes in [0, 1, 3] {
            assert_eq!(
                decode_sz(&[65, 0], bytes, REG_SZ, "CLSID"),
                Err(malformed("CLSID"))
            );
        }
        assert_eq!(decode_sz(&[65, 0], 4, 0, "CLSID"), Err(malformed("CLSID")));
        assert_eq!(
            decode_sz(&[65, 0], 6, REG_SZ, "CLSID"),
            Err(AsioRegistryError::Capacity)
        );
    }

    #[test]
    fn unicode_pairs_are_retained_and_unpaired_surrogates_reject() {
        assert_eq!(
            decode_sz(&[0xd83c, 0xdfb9, 0], 6, REG_SZ, "description").unwrap(),
            "🎹"
        );
        for units in [[0xd800, 0], [0xdc00, 0]] {
            assert_eq!(
                decode_sz(&units, 4, REG_SZ, "description"),
                Err(malformed("description"))
            );
        }
    }
}
