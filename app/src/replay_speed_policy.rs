//! Canonical optional source-play speed around complete application setup options.
use crate::replay_speed::ReplaySpeed;
use beatkernel::{
    replay::{
        codec::{encode_replay, ReplayCodecError, ReplayCodecLimits, ReplayFile},
        ReplayHeader,
    },
    transport::Rate,
};

pub(crate) const PREFIX: &[u8] = b"bms-speed-setup/v1:";
const VARIABLE_PREFIX: &[u8] = b"bms-speed-setup/v2:";
const FAMILY: &[u8] = b"bms-speed-setup/";

/// One actual change at an exact original-song phase, independent of output Hz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRateCut {
    whole: i128,
    numerator: u64,
    denominator: u64,
    speed: ReplaySpeed,
}
impl SourceRateCut {
    pub fn new(
        whole: i128,
        numerator: u64,
        denominator: u64,
        speed: ReplaySpeed,
    ) -> Result<Self, PolicyError> {
        if denominator == 0 || numerator >= denominator || gcd(numerator, denominator) != 1 {
            return Err(PolicyError::Invalid("noncanonical source phase"));
        }
        Ok(Self {
            whole,
            numerator,
            denominator,
            speed,
        })
    }
    pub const fn whole(self) -> i128 {
        self.whole
    }
    pub const fn numerator(self) -> u64 {
        self.numerator
    }
    pub const fn denominator(self) -> u64 {
        self.denominator
    }
    pub const fn speed(self) -> ReplaySpeed {
        self.speed
    }
    fn before(self, other: Self) -> bool {
        self.whole < other.whole
            || (self.whole == other.whole
                && u128::from(self.numerator) * u128::from(other.denominator)
                    < u128::from(other.numerator) * u128::from(self.denominator))
    }
    pub(crate) fn write_bytes(self, bytes: &mut [u8]) {
        bytes[..16].copy_from_slice(&self.whole.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.numerator.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.denominator.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.speed.rate().numerator().to_le_bytes());
        bytes[40..48].copy_from_slice(&self.speed.rate().denominator().to_le_bytes());
    }
}
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Original selected speed and ordered actual changes; returning to initial is variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRateSchedule {
    initial: ReplaySpeed,
    cuts: Vec<SourceRateCut>,
}
impl SourceRateSchedule {
    pub const fn fixed(initial: ReplaySpeed) -> Self {
        Self {
            initial,
            cuts: Vec::new(),
        }
    }
    pub const fn initial_speed(&self) -> ReplaySpeed {
        self.initial
    }
    pub fn current_speed(&self) -> ReplaySpeed {
        self.cuts.last().map_or(self.initial, |cut| cut.speed)
    }
    pub fn cuts(&self) -> &[SourceRateCut] {
        &self.cuts
    }
    pub fn is_variable(&self) -> bool {
        !self.cuts.is_empty()
    }
    pub fn try_reserve_change(&mut self) -> Result<(), PolicyError> {
        if self.cuts.len() >= u32::MAX as usize {
            return Err(PolicyError::HeaderTooLarge);
        }
        self.cuts
            .try_reserve(1)
            .map_err(|_| PolicyError::AllocationFailed)
    }
    pub fn try_push(&mut self, cut: SourceRateCut) -> Result<(), PolicyError> {
        self.validate_change(cut)?;
        self.try_reserve_change()?;
        self.cuts.push(cut);
        Ok(())
    }
    pub(crate) fn validate_change(&self, cut: SourceRateCut) -> Result<(), PolicyError> {
        if cut.speed == self.current_speed() {
            return Err(PolicyError::Invalid("redundant source rate cut"));
        }
        if self
            .cuts
            .last()
            .is_some_and(|previous| cut.before(*previous))
        {
            return Err(PolicyError::Invalid("regressing source rate cut"));
        }
        Ok(())
    }
    pub(crate) fn replace_prepared_cut(&mut self, cut: SourceRateCut) {
        *self.cuts.last_mut().expect("cold prepared rate cut") = cut;
    }
    pub(crate) fn try_copy(&self) -> Result<Self, PolicyError> {
        let mut copy = Self::fixed(self.initial);
        copy.cuts
            .try_reserve_exact(self.cuts.len())
            .map_err(|_| PolicyError::AllocationFailed)?;
        copy.cuts.extend_from_slice(&self.cuts);
        Ok(copy)
    }
}

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
    if options.starts_with(VARIABLE_PREFIX) {
        return Err(PolicyError::Invalid(
            "variable source speed requires a schedule-aware consumer",
        ));
    }
    let (inner, schedule) = split_schedule(options)?;
    Ok((inner, schedule.initial_speed()))
}

/// Validates complete extents and canonical changes before allocating a schedule.
pub fn split_schedule(options: &[u8]) -> Result<(&[u8], SourceRateSchedule), PolicyError> {
    if let Some(bytes) = options.strip_prefix(VARIABLE_PREFIX) {
        let fixed = bytes
            .get(..24)
            .ok_or(PolicyError::Invalid("truncated variable speed setup"))?;
        let inner_len = u32::from_le_bytes(fixed[..4].try_into().unwrap()) as usize;
        let initial = decode_speed(&fixed[4..20])?;
        let count = u32::from_le_bytes(fixed[20..24].try_into().unwrap()) as usize;
        if count == 0 {
            return Err(PolicyError::Invalid("redundant variable speed setup"));
        }
        let cuts_end = count
            .checked_mul(48)
            .and_then(|n| n.checked_add(24))
            .ok_or(PolicyError::Invalid("speed extent overflow"))?;
        let end = cuts_end
            .checked_add(inner_len)
            .ok_or(PolicyError::Invalid("speed extent overflow"))?;
        if bytes.len() != end {
            return Err(PolicyError::Invalid("variable speed extent differs"));
        }
        let inner = &bytes[cuts_end..end];
        if inner.starts_with(FAMILY) {
            return Err(PolicyError::Invalid("nested speed setup"));
        }
        // Validate before allocating from untrusted count or accepting any prefix.
        let mut previous: Option<SourceRateCut> = None;
        let mut speed = initial;
        for encoded in bytes[24..cuts_end].chunks_exact(48) {
            let cut = decode_cut(encoded)?;
            if cut.speed == speed {
                return Err(PolicyError::Invalid("redundant source rate cut"));
            }
            if previous.is_some_and(|previous| cut.before(previous)) {
                return Err(PolicyError::Invalid("regressing source rate cut"));
            }
            previous = Some(cut);
            speed = cut.speed;
        }
        let mut schedule = SourceRateSchedule::fixed(initial);
        schedule
            .cuts
            .try_reserve_exact(count)
            .map_err(|_| PolicyError::AllocationFailed)?;
        for encoded in bytes[24..cuts_end].chunks_exact(48) {
            schedule.cuts.push(decode_cut(encoded)?);
        }
        return Ok((inner, schedule));
    }
    let (inner, speed) = split_fixed_options(options)?;
    Ok((inner, SourceRateSchedule::fixed(speed)))
}

fn split_fixed_options(options: &[u8]) -> Result<(&[u8], ReplaySpeed), PolicyError> {
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

fn decode_speed(bytes: &[u8]) -> Result<ReplaySpeed, PolicyError> {
    let numerator = i64::from_le_bytes(bytes[..8].try_into().unwrap());
    let denominator = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let rate = Rate::new(numerator, denominator)
        .map_err(|_| PolicyError::Invalid("invalid speed fraction"))?;
    let speed =
        ReplaySpeed::new(rate).map_err(|_| PolicyError::Invalid("nonpositive source speed"))?;
    if rate.numerator() != numerator || rate.denominator() != denominator {
        return Err(PolicyError::Invalid("unreduced source speed"));
    }
    Ok(speed)
}
fn decode_cut(bytes: &[u8]) -> Result<SourceRateCut, PolicyError> {
    SourceRateCut::new(
        i128::from_le_bytes(bytes[..16].try_into().unwrap()),
        u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        decode_speed(&bytes[32..48])?,
    )
}

/// Wraps full source conditions. Fixed schedules retain exact NORMAL/v1 identity.
pub fn wrap_header_with_schedule(
    mut header: ReplayHeader,
    schedule: &SourceRateSchedule,
    limits: ReplayCodecLimits,
) -> Result<ReplayHeader, PolicyError> {
    if !schedule.is_variable() {
        return wrap_header(header, schedule.initial, limits);
    }
    if header.options.starts_with(FAMILY) {
        return Err(PolicyError::Invalid("nested speed setup"));
    }
    let inner_len = u32::try_from(header.options.len()).map_err(|_| PolicyError::HeaderTooLarge)?;
    let count = u32::try_from(schedule.cuts.len()).map_err(|_| PolicyError::HeaderTooLarge)?;
    let size = schedule
        .cuts
        .len()
        .checked_mul(48)
        .and_then(|n| n.checked_add(VARIABLE_PREFIX.len() + 24))
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
    options.extend_from_slice(VARIABLE_PREFIX);
    options.extend_from_slice(&inner_len.to_le_bytes());
    options.extend_from_slice(&schedule.initial.rate().numerator().to_le_bytes());
    options.extend_from_slice(&schedule.initial.rate().denominator().to_le_bytes());
    options.extend_from_slice(&count.to_le_bytes());
    for cut in &schedule.cuts {
        let start = options.len();
        options.resize(start + 48, 0);
        cut.write_bytes(&mut options[start..]);
    }
    options.extend_from_slice(&header.options);
    header.options = options;
    // The combined variable fields may fit the header budget while the complete
    // empty file still exceeds its outer envelope budget. Reuse the codec so
    // its framing stays authoritative instead of copying an overhead constant.
    let file = ReplayFile::new(header, Vec::new());
    encode_replay(&file, limits).map_err(|error| match error {
        ReplayCodecError::AllocationFailed => PolicyError::AllocationFailed,
        ReplayCodecError::HeaderTooLarge
        | ReplayCodecError::FileTooLarge
        | ReplayCodecError::LengthOverflow => PolicyError::HeaderTooLarge,
        _ => PolicyError::Invalid("invalid replay header"),
    })?;
    Ok(file.header)
}

pub(crate) fn prepared_cut_offset(previous_count: usize) -> usize {
    VARIABLE_PREFIX.len() + 24 + previous_count * 48
}
