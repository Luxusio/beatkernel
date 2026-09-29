//! Length-checked opaque Windows HID report batches; no native API calls.

#![forbid(unsafe_code)]

use crate::raw_input::{RawInputError, MAX_HID_REPORTS};

/// A validated borrowed RAWHID batch, excluding native trailing padding.
///
/// Each report includes all its wire bytes. No separate report ID is guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidReports<'a> {
    bytes: &'a [u8],
    size: usize,
}

impl<'a> HidReports<'a> {
    pub(crate) fn parse(body: &'a [u8]) -> Result<Self, RawInputError> {
        if body.len() < 8 {
            return Err(RawInputError::Truncated);
        }
        let size = u32::from_le_bytes(body[..4].try_into().expect("checked prefix")) as usize;
        let count = u32::from_le_bytes(body[4..8].try_into().expect("checked prefix")) as usize;
        if size == 0 || count == 0 {
            return Err(RawInputError::InvalidHidSize);
        }
        if count > MAX_HID_REPORTS {
            return Err(RawInputError::TooManyReports(count));
        }
        let length = size
            .checked_mul(count)
            .ok_or(RawInputError::InvalidHidSize)?;
        let bytes = body
            .get(8..)
            .and_then(|data| data.get(..length))
            .ok_or(RawInputError::Truncated)?;
        Ok(Self { bytes, size })
    }

    /// Returns the number of complete reports, at most [`MAX_HID_REPORTS`].
    pub fn report_count(&self) -> usize {
        self.bytes.len() / self.size
    }

    /// Returns the wire byte size of each report, including any embedded ID.
    pub const fn report_size(&self) -> usize {
        self.size
    }

    /// Iterates exact wire reports in packet order without copying or allocating.
    pub fn reports(&self) -> std::slice::ChunksExact<'a, u8> {
        self.bytes.chunks_exact(self.size)
    }
}
