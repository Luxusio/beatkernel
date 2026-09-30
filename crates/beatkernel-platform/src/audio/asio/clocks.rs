//! Bounded SDK-free metadata for explicitly selected ASIO clock sources.

/// One actual native clock-source report, retaining its bounded raw name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsioClockSource {
    index: u32,
    associated_channel: Option<u32>,
    associated_group: Option<u32>,
    current: bool,
    name: [u8; 32],
    name_len: u8,
}

impl AsioClockSource {
    /// Validates fixed native fields without assuming UTF-8 or channel counts.
    /// Associations must both be absent (-1) or both nonnegative. The name
    /// must contain a NUL within its native 32-byte extent.
    pub fn from_raw(
        index: i32,
        associated_channel: i32,
        associated_group: i32,
        current: i32,
        name: [u8; 32],
    ) -> Result<Self, AsioClockSourceError> {
        if index < 0
            || !matches!(current, 0 | 1)
            || !((associated_channel == -1 && associated_group == -1)
                || (associated_channel >= 0 && associated_group >= 0))
        {
            return Err(AsioClockSourceError::MalformedReport);
        }
        let name_len = name
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(AsioClockSourceError::MalformedReport)? as u8;
        Ok(Self {
            index: index as u32,
            associated_channel: (associated_channel >= 0).then_some(associated_channel as u32),
            associated_group: (associated_group >= 0).then_some(associated_group as u32),
            current: current == 1,
            name,
            name_len,
        })
    }

    /// Exact native index, which need not be contiguous with other sources.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Associated native input channel, absent for an unassociated source.
    pub const fn associated_channel(&self) -> Option<u32> {
        self.associated_channel
    }

    /// Associated native channel group, absent for an unassociated source.
    pub const fn associated_group(&self) -> Option<u32> {
        self.associated_group
    }

    /// Whether the native report identifies this as the current source.
    pub const fn is_current(&self) -> bool {
        self.current
    }

    /// Original name bytes preceding the first NUL; no text encoding is assumed.
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..usize::from(self.name_len)]
    }
}

/// Clock-source metadata, capacity or explicit-selection failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsioClockSourceError {
    /// The caller's source bound is outside 1..=4096.
    InvalidLimits,
    /// The actual native source count exceeds the caller's bound.
    Capacity,
    /// Native fields, count, indices or current-source reports are inconsistent.
    MalformedReport,
    /// The explicitly requested native index was not enumerated.
    InvalidSelection {
        /// Requested native source index.
        index: u32,
    },
}

impl std::fmt::Display for AsioClockSourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimits => formatter.write_str("invalid ASIO clock-source limits"),
            Self::Capacity => formatter.write_str("ASIO clock-source capacity exceeded"),
            Self::MalformedReport => formatter.write_str("malformed ASIO clock-source report"),
            Self::InvalidSelection { index } => {
                write!(formatter, "ASIO clock source {index} was not enumerated")
            }
        }
    }
}

impl std::error::Error for AsioClockSourceError {}

/// Validates a bounded native enumeration without allocating or selecting a source.
///
/// Reports must be nonempty, indices unique and at most one source current.
/// No current source is permitted. Native indices need not be contiguous and
/// associated channel/group values are not interpreted as enumeration indices.
pub fn validate_clock_sources(
    sources: &[AsioClockSource],
    max_sources: usize,
) -> Result<(), AsioClockSourceError> {
    if !(1..=4096).contains(&max_sources) {
        return Err(AsioClockSourceError::InvalidLimits);
    }
    if sources.len() > max_sources {
        return Err(AsioClockSourceError::Capacity);
    }
    if sources.is_empty() {
        return Err(AsioClockSourceError::MalformedReport);
    }
    let mut current_seen = false;
    for (position, source) in sources.iter().enumerate() {
        if sources[..position]
            .iter()
            .any(|previous| previous.index == source.index)
            || (source.current && current_seen)
        {
            return Err(AsioClockSourceError::MalformedReport);
        }
        current_seen |= source.current;
    }
    Ok(())
}
