//! Canonical optional source-play speed around complete application setup options.
use crate::replay_speed::ReplaySpeed;
use beatkernel::{
    replay::{codec::ReplayCodecLimits, ReplayHeader},
    transport::Rate,
};

pub(crate) const PREFIX: &[u8] = b"bms-speed-setup/v1:";
const FAMILY: &[u8] = b"bms-speed-setup/";

/// Invalid or oversized recorded source-speed metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    Invalid(&'static str),
    HeaderTooLarge,
    AllocationFailed,
}
impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay source speed policy: {self:?}")
    }
}
impl std::error::Error for PolicyError {}

/// Adds the outer source-speed identity; NORMAL preserves the original bytes.
pub fn wrap_header(
    mut header: ReplayHeader,
    source_speed: ReplaySpeed,
    limits: ReplayCodecLimits,
) -> Result<ReplayHeader, PolicyError> {
    if header.options.starts_with(FAMILY) {
        return Err(PolicyError::Invalid("nested speed setup"));
    }
    if source_speed.rate() == Rate::NORMAL {
        return Ok(header);
    }
    let inner_len = u32::try_from(header.options.len()).map_err(|_| PolicyError::HeaderTooLarge)?;
    let size = PREFIX
        .len()
        .checked_add(20)
        .and_then(|n| n.checked_add(header.options.len()))
        .ok_or(PolicyError::HeaderTooLarge)?;
    let total = size
        .checked_add(header.chart_identity.len())
        .and_then(|n| n.checked_add(header.rules_identity.len()))
        .and_then(|n| n.checked_add(env!("CARGO_PKG_VERSION").len()))
        .ok_or(PolicyError::HeaderTooLarge)?;
    if total > limits.max_header_bytes() {
        return Err(PolicyError::HeaderTooLarge);
    }
    let mut options = Vec::new();
    options
        .try_reserve_exact(size)
        .map_err(|_| PolicyError::AllocationFailed)?;
    options.extend_from_slice(PREFIX);
    options.extend_from_slice(&inner_len.to_le_bytes());
    options.extend_from_slice(&source_speed.rate().numerator().to_le_bytes());
    options.extend_from_slice(&source_speed.rate().denominator().to_le_bytes());
    options.extend_from_slice(&header.options);
    header.options = options;
    Ok(header)
}

/// Borrows the exact inner options and validates the canonical positive fraction.
pub fn split_options(options: &[u8]) -> Result<(&[u8], ReplaySpeed), PolicyError> {
    let Some(bytes) = options.strip_prefix(PREFIX) else {
        if options.starts_with(FAMILY) {
            return Err(PolicyError::Invalid("unknown speed setup version"));
        }
        return Ok((
            options,
            ReplaySpeed::new(Rate::NORMAL).expect("positive normal rate"),
        ));
    };
    let fixed = bytes
        .get(..20)
        .ok_or(PolicyError::Invalid("truncated speed setup"))?;
    let inner_len = u32::from_le_bytes(fixed[..4].try_into().unwrap()) as usize;
    let numerator = i64::from_le_bytes(fixed[4..12].try_into().unwrap());
    let denominator = u64::from_le_bytes(fixed[12..20].try_into().unwrap());
    let end = 20usize
        .checked_add(inner_len)
        .ok_or(PolicyError::Invalid("speed extent overflow"))?;
    let inner = bytes
        .get(20..end)
        .ok_or(PolicyError::Invalid("truncated inner setup"))?;
    if bytes.len() != end {
        return Err(PolicyError::Invalid("trailing speed setup bytes"));
    }
    if inner.starts_with(FAMILY) {
        return Err(PolicyError::Invalid("nested speed setup"));
    }
    let rate = Rate::new(numerator, denominator)
        .map_err(|_| PolicyError::Invalid("invalid speed fraction"))?;
    let speed =
        ReplaySpeed::new(rate).map_err(|_| PolicyError::Invalid("nonpositive source speed"))?;
    if rate.numerator() != numerator || rate.denominator() != denominator {
        return Err(PolicyError::Invalid("unreduced source speed"));
    }
    if rate == Rate::NORMAL {
        return Err(PolicyError::Invalid("redundant normal speed"));
    }
    Ok((inner, speed))
}
