//! SDK-free canonical registration identity; this grants no driver permission.
/// A malformed/nonzero-required CLSID or allocation refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioDriverIdError {
    /// Not an exact nonzero UUID/CLSID spelling.
    Malformed,
    /// The canonical identity allocation was refused.
    Capacity,
}
impl std::fmt::Display for AsioDriverIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ASIO driver identity: {self:?}")
    }
}
impl std::error::Error for AsioDriverIdError {}
/// Canonicalizes exact 36-character UUIDs or braced 38-character CLSIDs.
/// Zero identities refuse; native enumeration and explicit trust stay external.
pub fn canonical_asio_clsid(value: &str) -> Result<String, AsioDriverIdError> {
    let bytes = value.as_bytes();
    let body = match bytes.len() {
        36 => bytes,
        38 if bytes[0] == b'{' && bytes[37] == b'}' => &bytes[1..37],
        _ => return Err(AsioDriverIdError::Malformed),
    };
    let mut nonzero = false;
    for (index, &byte) in body.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return Err(AsioDriverIdError::Malformed);
            }
        } else {
            if !byte.is_ascii_hexdigit() {
                return Err(AsioDriverIdError::Malformed);
            }
            nonzero |= byte != b'0';
        }
    }
    if !nonzero {
        return Err(AsioDriverIdError::Malformed);
    }
    let mut canonical = String::new();
    canonical
        .try_reserve_exact(38)
        .map_err(|_| AsioDriverIdError::Capacity)?;
    canonical.push('{');
    for &byte in body {
        canonical.push(char::from(byte.to_ascii_uppercase()));
    }
    canonical.push('}');
    Ok(canonical)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_id_is_identical_for_both_spellings_and_case() {
        let plain = "abcdef12-3456-7890-abcd-ef1234567890";
        let expected = "{ABCDEF12-3456-7890-ABCD-EF1234567890}";
        assert_eq!(canonical_asio_clsid(plain).unwrap(), expected);
        assert_eq!(canonical_asio_clsid(expected).unwrap(), expected);
    }
    #[test]
    fn malformed_zero_unicode_and_separator_identities_refuse() {
        for value in [
            "",
            "00000000-0000-0000-0000-000000000000",
            "{00000000-0000-0000-0000-000000000000}",
            " ABCDEF12-3456-7890-ABCD-EF1234567890",
            "abcdef12_3456-7890-abcd-ef1234567890",
            "abcdef12-3456-7890-abcd-ef123456789z",
            "{abcdef12-3456-7890-abcd-ef1234567890)",
            "가",
        ] {
            assert_eq!(
                canonical_asio_clsid(value),
                Err(AsioDriverIdError::Malformed)
            );
        }
    }
}
