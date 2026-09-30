//! Explicit native report framing without vendor interpretation or clock inference.

use beatkernel::input::{EventMeta, RawHidReportEvent};

/// Caller-declared placement of an optional native report ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReportLayout {
    /// Native bytes contain only payload; the report ID is supplied separately.
    SeparateId,
    /// Numbered bytes begin with their matching nonzero report ID.
    /// Unnumbered reports (native ID zero) retain every byte as payload.
    LeadingId,
}

/// Report representation or bounded-copy failure; no partial report is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidReportConversionError {
    /// The native-byte limit is zero or exceeds the finite implementation ceiling.
    InvalidLimit,
    /// The native integer ID is outside the HID byte range.
    InvalidReportId,
    /// A numbered leading-ID report has no prefix byte.
    MissingReportId,
    /// Its leading byte differs from the separately supplied native ID.
    ReportIdMismatch,
    /// Native bytes, including any leading ID, exceed the caller-selected bound.
    ReportCapacity,
    /// The canonical payload could not reserve its bounded storage.
    AllocationFailed,
}

impl std::fmt::Display for HidReportConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native HID report conversion: {self:?}")
    }
}
impl std::error::Error for HidReportConversionError {}

/// Maximum caller-selected native report bytes, including any prefix.
pub const MAX_NATIVE_REPORT_BYTES: usize = 16 * 1024 * 1024;

/// Copies a native report into canonical form using an explicitly declared layout.
///
/// Zero native ID represents an unnumbered report; every byte is preserved.
/// A positive ID must fit a byte. LeadingId validates and removes its prefix,
/// while SeparateId retains the full slice even when its first byte equals the ID.
/// Acquisition metadata is unchanged. No report descriptor, vendor semantics,
/// timestamp quality or callback allocation suitability is inferred here.
pub fn normalize_report(
    meta: EventMeta,
    native_report_id: u32,
    bytes: &[u8],
    layout: NativeReportLayout,
    max_native_bytes: usize,
) -> Result<RawHidReportEvent, HidReportConversionError> {
    if max_native_bytes == 0 || max_native_bytes > MAX_NATIVE_REPORT_BYTES {
        return Err(HidReportConversionError::InvalidLimit);
    }
    let id =
        u8::try_from(native_report_id).map_err(|_| HidReportConversionError::InvalidReportId)?;
    if bytes.len() > max_native_bytes {
        return Err(HidReportConversionError::ReportCapacity);
    }
    let report_id = (id != 0).then_some(id);
    let payload = if layout == NativeReportLayout::LeadingId && id != 0 {
        let (&prefix, payload) = bytes
            .split_first()
            .ok_or(HidReportConversionError::MissingReportId)?;
        if prefix != id {
            return Err(HidReportConversionError::ReportIdMismatch);
        }
        payload
    } else {
        bytes
    };
    let mut data = Vec::new();
    data.try_reserve_exact(payload.len())
        .map_err(|_| HidReportConversionError::AllocationFailed)?;
    data.extend_from_slice(payload);
    Ok(RawHidReportEvent {
        meta,
        report_id,
        data,
    })
}
