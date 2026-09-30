//! Versioned bounded durable logs; custom interaction checkpoints stay in memory.
use super::{ReplayError, ReplayHeader, ReplayOperation, ReplayRecord, REPLAY_VERSION};
use crate::{
    input::{
        decode_event, encode_event, CodecLimits, GameControlId, GameInputEvent, InputCodecError,
    },
    time::{ClockDomainId, Timestamp},
};

const MAGIC: &[u8; 8] = b"BKREPLAY";
/// Durable file envelope schema; distinct from the logical replay header version.
pub const REPLAY_FILE_VERSION: u32 = 1;

/// Portable log and application-provided reconstruction metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayFile {
    /// Runtime build/version identity; newly constructed files use crate version.
    pub runtime_version: String,
    /// Optional application-defined calibration bytes; never trusted implicitly.
    pub calibration_metadata: Option<Vec<u8>>,
    /// Logical replay identities/options and normalized input clock.
    pub header: ReplayHeader,
    /// Strictly ordered admitted inputs/advances; no serialized trait objects.
    pub records: Vec<ReplayRecord>,
}
impl ReplayFile {
    /// Wraps an owned log without judging or silently rewriting its chronology.
    pub fn new(header: ReplayHeader, records: Vec<ReplayRecord>) -> Self {
        Self {
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            calibration_metadata: None,
            header,
            records,
        }
    }
}

/// Explicit encoded/decode allocation limits, with nested input policy.
#[derive(Clone, Copy, Debug)]
pub struct ReplayCodecLimits {
    max_file_bytes: usize,
    max_records: usize,
    max_header_bytes: usize,
    input: CodecLimits,
}
impl ReplayCodecLimits {
    /// Validates byte/operation bounds before parsing or allocating.
    pub fn new(
        max_file_bytes: usize,
        max_records: usize,
        max_header_bytes: usize,
        input: CodecLimits,
    ) -> Result<Self, ReplayCodecError> {
        if max_file_bytes == 0
            || max_file_bytes > isize::MAX as usize
            || max_records == 0
            || max_records > isize::MAX as usize
            || max_header_bytes == 0
            || max_header_bytes > max_file_bytes
        {
            return Err(ReplayCodecError::InvalidLimits);
        }
        Ok(Self {
            max_file_bytes,
            max_records,
            max_header_bytes,
            input,
        })
    }
    /// Total encoded file byte cap.
    pub const fn max_file_bytes(self) -> usize {
        self.max_file_bytes
    }
    /// Decoded operation count cap.
    pub const fn max_records(self) -> usize {
        self.max_records
    }
    /// Combined variable header field byte cap.
    pub const fn max_header_bytes(self) -> usize {
        self.max_header_bytes
    }
    /// Physical-input/payload caps for each embedded event.
    pub const fn input_limits(self) -> CodecLimits {
        self.input
    }
}

/// Malformed, unsupported or over-capacity durable file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayCodecError {
    /// Invalid nonzero/representable configured bounds.
    InvalidLimits,
    /// Input does not start with the expected durable file magic.
    InvalidMagic,
    /// Unsupported durable envelope version.
    UnsupportedVersion(u32),
    /// Invalid discriminant or option tag.
    InvalidTag(u8),
    /// Field/count extent exceeds remaining bytes.
    Truncated,
    /// Bytes remain after the last operation.
    TrailingBytes,
    /// Total file byte capacity exceeded.
    FileTooLarge,
    /// Combined variable header data exceeded its capacity.
    HeaderTooLarge,
    /// Operation count capacity exceeded.
    TooManyRecords,
    /// Runtime version bytes are not UTF-8.
    InvalidRuntimeVersion,
    /// A length or arithmetic result cannot be represented.
    LengthOverflow,
    /// Explicit allocation failed.
    AllocationFailed,
    /// Header, normalized domain, ordinal or song chronology validation failed.
    Replay(ReplayError),
    /// Embedded canonical physical input is malformed or over its own limit.
    Input(InputCodecError),
}
impl From<InputCodecError> for ReplayCodecError {
    fn from(error: InputCodecError) -> Self {
        Self::Input(error)
    }
}
impl From<ReplayError> for ReplayCodecError {
    fn from(error: ReplayError) -> Self {
        Self::Replay(error)
    }
}
impl std::fmt::Display for ReplayCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay file: {self:?}")
    }
}
impl std::error::Error for ReplayCodecError {}

fn validate_header(header: &ReplayHeader) -> Result<(), ReplayCodecError> {
    if header.version != REPLAY_VERSION {
        return Err(ReplayError::UnsupportedVersion(header.version).into());
    }
    Ok(())
}
fn validate_record(
    record: &ReplayRecord,
    index: usize,
    previous: Option<Timestamp>,
    domain: ClockDomainId,
) -> Result<(), ReplayCodecError> {
    if record.ordinal != u64::try_from(index).map_err(|_| ReplayCodecError::LengthOverflow)? {
        return Err(ReplayError::InvalidOrdinal.into());
    }
    if previous.is_some_and(|time| time > record.song_time) {
        return Err(ReplayError::NonMonotonicSongTime.into());
    }
    if let ReplayOperation::Input(input) = &record.operation {
        if input.physical.meta().clock_domain != domain {
            return Err(ReplayError::ClockDomainMismatch.into());
        }
    }
    Ok(())
}
struct Writer {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Writer {
    fn put(&mut self, value: &[u8]) -> Result<(), ReplayCodecError> {
        let end = self
            .bytes
            .len()
            .checked_add(value.len())
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if end > self.maximum {
            return Err(ReplayCodecError::FileTooLarge);
        }
        self.bytes
            .try_reserve_exact(value.len())
            .map_err(|_| ReplayCodecError::AllocationFailed)?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }
    fn blob(&mut self, value: &[u8]) -> Result<(), ReplayCodecError> {
        self.put(
            &u64::try_from(value.len())
                .map_err(|_| ReplayCodecError::LengthOverflow)?
                .to_le_bytes(),
        )?;
        self.put(value)
    }
}
/// Encodes a complete validated log without file IO or judge execution.
pub fn encode_replay(
    file: &ReplayFile,
    limits: ReplayCodecLimits,
) -> Result<Vec<u8>, ReplayCodecError> {
    validate_header(&file.header)?;
    if file.records.len() > limits.max_records {
        return Err(ReplayCodecError::TooManyRecords);
    }
    let fields = [
        file.runtime_version.as_bytes(),
        &file.header.chart_identity,
        &file.header.rules_identity,
        &file.header.options,
        file.calibration_metadata.as_deref().unwrap_or(&[]),
    ];
    let header_size = fields
        .iter()
        .try_fold(0usize, |size, field| size.checked_add(field.len()))
        .ok_or(ReplayCodecError::LengthOverflow)?;
    if header_size > limits.max_header_bytes {
        return Err(ReplayCodecError::HeaderTooLarge);
    }
    let mut previous = None;
    for (index, record) in file.records.iter().enumerate() {
        validate_record(record, index, previous, file.header.normalized_clock)?;
        previous = Some(record.song_time);
    }
    let mut out = Writer {
        bytes: Vec::new(),
        maximum: limits.max_file_bytes,
    };
    out.put(MAGIC)?;
    out.put(&REPLAY_FILE_VERSION.to_le_bytes())?;
    out.put(&file.header.version.to_le_bytes())?;
    out.put(&file.header.normalized_clock.0.to_le_bytes())?;
    out.put(&file.header.seed.to_le_bytes())?;
    for field in &fields[..4] {
        out.blob(field)?;
    }
    out.put(&[u8::from(file.calibration_metadata.is_some())])?;
    if let Some(metadata) = &file.calibration_metadata {
        out.blob(metadata)?;
    }
    out.put(
        &u64::try_from(file.records.len())
            .map_err(|_| ReplayCodecError::LengthOverflow)?
            .to_le_bytes(),
    )?;
    for record in &file.records {
        out.put(&record.ordinal.to_le_bytes())?;
        out.put(&record.song_time.as_nanos().to_le_bytes())?;
        match &record.operation {
            ReplayOperation::Advance => out.put(&[1])?,
            ReplayOperation::Input(input) => {
                out.put(&[0])?;
                out.put(&input.game_control.0.to_le_bytes())?;
                out.blob(&encode_event(&input.physical, limits.input)?)?;
            }
        }
    }
    Ok(out.bytes)
}
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ReplayCodecError> {
        let end = self
            .cursor
            .checked_add(count)
            .ok_or(ReplayCodecError::LengthOverflow)?;
        let bytes = self
            .bytes
            .get(self.cursor..end)
            .ok_or(ReplayCodecError::Truncated)?;
        self.cursor = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, ReplayCodecError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, ReplayCodecError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| ReplayCodecError::Truncated)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, ReplayCodecError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ReplayCodecError::Truncated)?,
        ))
    }
    fn blob(&mut self) -> Result<&'a [u8], ReplayCodecError> {
        let length = usize::try_from(self.u64()?).map_err(|_| ReplayCodecError::LengthOverflow)?;
        self.take(length)
    }
    fn header_blob(
        &mut self,
        used: &mut usize,
        maximum: usize,
    ) -> Result<Vec<u8>, ReplayCodecError> {
        let bytes = self.blob()?;
        *used = used
            .checked_add(bytes.len())
            .ok_or(ReplayCodecError::LengthOverflow)?;
        if *used > maximum {
            return Err(ReplayCodecError::HeaderTooLarge);
        }
        let mut out = Vec::new();
        out.try_reserve_exact(bytes.len())
            .map_err(|_| ReplayCodecError::AllocationFailed)?;
        out.extend_from_slice(bytes);
        Ok(out)
    }
}
/// Decodes a complete validated log, preserving raw input bytes and IEEE bits.
pub fn decode_replay(
    bytes: &[u8],
    limits: ReplayCodecLimits,
) -> Result<ReplayFile, ReplayCodecError> {
    if bytes.len() > limits.max_file_bytes {
        return Err(ReplayCodecError::FileTooLarge);
    }
    let mut input = Reader { bytes, cursor: 0 };
    if input.take(8)? != MAGIC {
        return Err(ReplayCodecError::InvalidMagic);
    }
    let version = input.u32()?;
    if version != REPLAY_FILE_VERSION {
        return Err(ReplayCodecError::UnsupportedVersion(version));
    }
    let version = input.u32()?;
    if version != REPLAY_VERSION {
        return Err(ReplayError::UnsupportedVersion(version).into());
    }
    let normalized_clock = ClockDomainId(input.u32()?);
    let seed = input.u64()?;
    let mut used = 0;
    let runtime_version = String::from_utf8(input.header_blob(&mut used, limits.max_header_bytes)?)
        .map_err(|_| ReplayCodecError::InvalidRuntimeVersion)?;
    let chart_identity = input.header_blob(&mut used, limits.max_header_bytes)?;
    let rules_identity = input.header_blob(&mut used, limits.max_header_bytes)?;
    let options = input.header_blob(&mut used, limits.max_header_bytes)?;
    let calibration_metadata = match input.u8()? {
        0 => None,
        1 => Some(input.header_blob(&mut used, limits.max_header_bytes)?),
        tag => return Err(ReplayCodecError::InvalidTag(tag)),
    };
    let count = usize::try_from(input.u64()?).map_err(|_| ReplayCodecError::LengthOverflow)?;
    if count > limits.max_records {
        return Err(ReplayCodecError::TooManyRecords);
    }
    if count > (bytes.len() - input.cursor) / 17 {
        return Err(ReplayCodecError::Truncated);
    }
    let mut records = Vec::new();
    records
        .try_reserve_exact(count)
        .map_err(|_| ReplayCodecError::AllocationFailed)?;
    let mut previous = None;
    for index in 0..count {
        let ordinal = input.u64()?;
        let song_time = Timestamp::from_nanos(i64::from_le_bytes(
            input
                .take(8)?
                .try_into()
                .map_err(|_| ReplayCodecError::Truncated)?,
        ));
        let operation = match input.u8()? {
            1 => ReplayOperation::Advance,
            0 => ReplayOperation::Input(GameInputEvent {
                game_control: GameControlId(input.u32()?),
                physical: decode_event(input.blob()?, limits.input)?,
            }),
            tag => return Err(ReplayCodecError::InvalidTag(tag)),
        };
        let record = ReplayRecord {
            ordinal,
            song_time,
            operation,
        };
        validate_record(&record, index, previous, normalized_clock)?;
        previous = Some(song_time);
        records.push(record);
    }
    if input.cursor != bytes.len() {
        return Err(ReplayCodecError::TrailingBytes);
    }
    Ok(ReplayFile {
        runtime_version,
        calibration_metadata,
        header: ReplayHeader {
            version,
            chart_identity,
            rules_identity,
            options,
            seed,
            normalized_clock,
        },
        records,
    })
}
