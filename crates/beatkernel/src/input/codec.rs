//! Versioned bounded little-endian serialization of complete physical events.

use super::{
    AxisEvent, AxisMode, BackendId, ButtonEvent, ButtonState, ContactId, CustomInputEvent,
    DeviceId, EventMeta, NativeEventMeta, PhysicalControlId, PhysicalInputEvent, PointerEvent,
    PointerMode, PoseEvent, Position2, Position3, Quaternion, RawHidReportEvent, TouchEvent,
    TouchPhase, VendorNamespaceId,
};
use crate::time::{ClockDomainId, ClockPoint, Timestamp};
use std::fmt;

const MAGIC: &[u8; 4] = b"BKPI";
/// Current canonical physical input blob schema.
pub const INPUT_CODEC_VERSION: u16 = 1;

/// Explicit encoded and opaque payload byte budgets, validated at construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecLimits {
    max_encoded_bytes: usize,
    max_payload_bytes: usize,
}
impl CodecLimits {
    /// Requires header capacity, a representable Vec byte extent and a payload
    /// budget no larger than the encoded budget. Zero-length payloads are valid.
    pub fn new(
        max_encoded_bytes: usize,
        max_payload_bytes: usize,
    ) -> Result<Self, InputCodecError> {
        if max_encoded_bytes < 6
            || max_encoded_bytes > isize::MAX as usize
            || max_payload_bytes > max_encoded_bytes
        {
            return Err(InputCodecError::InvalidLimits);
        }
        Ok(Self {
            max_encoded_bytes,
            max_payload_bytes,
        })
    }
    /// Maximum complete blob size including header and provenance.
    pub const fn max_encoded_bytes(self) -> usize {
        self.max_encoded_bytes
    }
    /// Maximum opaque RawHID/Custom payload length.
    pub const fn max_payload_bytes(self) -> usize {
        self.max_payload_bytes
    }
}

/// Explicit structural, resource or allocation failure for one blob.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputCodecError {
    /// Caller budgets cannot form a valid bounded byte allocation policy.
    InvalidLimits,
    /// Header magic does not identify a canonical input blob.
    InvalidMagic,
    /// The schema version is not implemented.
    UnsupportedVersion(u16),
    /// A strict enum/option discriminant was outside its declared tag set.
    InvalidTag {
        /// The affected wire field.
        field: &'static str,
        /// Rejected raw tag.
        tag: u8,
    },
    /// Bytes end before the requested field or declared payload.
    Truncated,
    /// Additional bytes follow the complete decoded event.
    TrailingBytes,
    /// Complete blob exceeds the explicit encoded-byte budget.
    EncodedTooLarge,
    /// Opaque bytes exceed the independent payload-byte budget.
    PayloadTooLarge,
    /// A length/cursor cannot be represented on this host or wire schema.
    LengthOverflow,
    /// A fallible Vec reservation could not allocate the required bounded bytes.
    AllocationFailed,
}
impl fmt::Display for InputCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "physical input codec: {self:?}")
    }
}
impl std::error::Error for InputCodecError {}

/// Encodes exactly one complete event with raw float bits and all provenance.
///
/// ```
/// use beatkernel::input::*;
/// use beatkernel::time::*;
/// let event = PhysicalInputEvent::Button(ButtonEvent {
///     meta: EventMeta::new(DeviceId(2), ClockPoint {
///         domain: ClockDomainId(1), timestamp: Timestamp::from_nanos(-5) }, 7),
///     control: PhysicalControlId::keyboard(4), state: ButtonState::Down,
/// });
/// let limits = CodecLimits::new(4096, 1024)?;
/// let blob = encode_event(&event, limits)?;
/// assert_eq!(decode_event(&blob, limits)?, event);
/// # Ok::<(), InputCodecError>(())
/// ```
pub fn encode_event(
    event: &PhysicalInputEvent,
    limits: CodecLimits,
) -> Result<Vec<u8>, InputCodecError> {
    let mut out = Writer {
        bytes: Vec::new(),
        limits,
    };
    out.bytes(MAGIC)?;
    out.bytes(&INPUT_CODEC_VERSION.to_le_bytes())?;
    let variant = match event {
        PhysicalInputEvent::Button(_) => 0,
        PhysicalInputEvent::Axis(_) => 1,
        PhysicalInputEvent::Touch(_) => 2,
        PhysicalInputEvent::Pointer(_) => 3,
        PhysicalInputEvent::Pose(_) => 4,
        PhysicalInputEvent::RawHidReport(_) => 5,
        PhysicalInputEvent::Custom(_) => 6,
    };
    out.u8(variant)?;
    out.meta(*event.meta())?;
    match event {
        PhysicalInputEvent::Button(value) => {
            out.control(value.control)?;
            out.u8(match value.state {
                ButtonState::Down => 0,
                ButtonState::Up => 1,
                ButtonState::Repeat => 2,
            })?;
        }
        PhysicalInputEvent::Axis(value) => {
            out.control(value.control)?;
            out.float(value.value)?;
            out.u8(match value.mode {
                AxisMode::Absolute => 0,
                AxisMode::Relative => 1,
            })?;
        }
        PhysicalInputEvent::Touch(value) => {
            out.control(value.control)?;
            out.u64(value.contact.0)?;
            out.u8(match value.phase {
                TouchPhase::Down => 0,
                TouchPhase::Move => 1,
                TouchPhase::Up => 2,
                TouchPhase::Cancel => 3,
            })?;
            out.position(value.position)?;
            out.option(value.pressure, Writer::float)?;
        }
        PhysicalInputEvent::Pointer(value) => {
            out.control(value.control)?;
            out.position(value.position)?;
            out.u8(match value.mode {
                PointerMode::Absolute => 0,
                PointerMode::Relative => 1,
            })?;
        }
        PhysicalInputEvent::Pose(value) => {
            out.control(value.control)?;
            for component in [
                value.position.x,
                value.position.y,
                value.position.z,
                value.orientation.x,
                value.orientation.y,
                value.orientation.z,
                value.orientation.w,
            ] {
                out.float(component)?;
            }
        }
        PhysicalInputEvent::RawHidReport(value) => {
            out.option(value.report_id, Writer::u8)?;
            out.payload(&value.data)?;
        }
        PhysicalInputEvent::Custom(value) => {
            out.u32(value.namespace.0)?;
            out.u32(value.type_id)?;
            out.payload(&value.payload)?;
        }
    }
    Ok(out.bytes)
}

/// Decodes exactly one complete blob; rejects trailing data and unknown tags.
/// Opaque payload lengths are checked against bytes/budgets before allocation.
pub fn decode_event(
    bytes: &[u8],
    limits: CodecLimits,
) -> Result<PhysicalInputEvent, InputCodecError> {
    if bytes.len() > limits.max_encoded_bytes {
        return Err(InputCodecError::EncodedTooLarge);
    }
    let mut input = Reader {
        bytes,
        position: 0,
        limits,
    };
    if input.take(4)? != MAGIC {
        return Err(InputCodecError::InvalidMagic);
    }
    let version = input.u16()?;
    if version != INPUT_CODEC_VERSION {
        return Err(InputCodecError::UnsupportedVersion(version));
    }
    let variant = input.tag("event", 6)?;
    let meta = input.meta()?;
    let event = match variant {
        0 => PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: input.control()?,
            state: match input.tag("button state", 2)? {
                0 => ButtonState::Down,
                1 => ButtonState::Up,
                _ => ButtonState::Repeat,
            },
        }),
        1 => PhysicalInputEvent::Axis(AxisEvent {
            meta,
            control: input.control()?,
            value: input.float()?,
            mode: match input.tag("axis mode", 1)? {
                0 => AxisMode::Absolute,
                _ => AxisMode::Relative,
            },
        }),
        2 => PhysicalInputEvent::Touch(TouchEvent {
            meta,
            control: input.control()?,
            contact: ContactId(input.u64()?),
            phase: match input.tag("touch phase", 3)? {
                0 => TouchPhase::Down,
                1 => TouchPhase::Move,
                2 => TouchPhase::Up,
                _ => TouchPhase::Cancel,
            },
            position: input.position()?,
            pressure: input.option(Reader::float)?,
        }),
        3 => PhysicalInputEvent::Pointer(PointerEvent {
            meta,
            control: input.control()?,
            position: input.position()?,
            mode: match input.tag("pointer mode", 1)? {
                0 => PointerMode::Absolute,
                _ => PointerMode::Relative,
            },
        }),
        4 => PhysicalInputEvent::Pose(PoseEvent {
            meta,
            control: input.control()?,
            position: Position3 {
                x: input.float()?,
                y: input.float()?,
                z: input.float()?,
            },
            orientation: Quaternion {
                x: input.float()?,
                y: input.float()?,
                z: input.float()?,
                w: input.float()?,
            },
        }),
        5 => PhysicalInputEvent::RawHidReport(RawHidReportEvent {
            meta,
            report_id: input.option(Reader::u8)?,
            data: input.payload()?,
        }),
        6 => PhysicalInputEvent::Custom(CustomInputEvent {
            meta,
            namespace: VendorNamespaceId(input.u32()?),
            type_id: input.u32()?,
            payload: input.payload()?,
        }),
        _ => unreachable!("event tag was validated"),
    };
    if input.position != bytes.len() {
        return Err(InputCodecError::TrailingBytes);
    }
    Ok(event)
}

struct Writer {
    bytes: Vec<u8>,
    limits: CodecLimits,
}
impl Writer {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), InputCodecError> {
        let end = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(InputCodecError::LengthOverflow)?;
        if end > self.limits.max_encoded_bytes {
            return Err(InputCodecError::EncodedTooLarge);
        }
        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(|_| InputCodecError::AllocationFailed)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn u8(&mut self, value: u8) -> Result<(), InputCodecError> {
        self.bytes(&[value])
    }
    fn u16(&mut self, value: u16) -> Result<(), InputCodecError> {
        self.bytes(&value.to_le_bytes())
    }
    fn u32(&mut self, value: u32) -> Result<(), InputCodecError> {
        self.bytes(&value.to_le_bytes())
    }
    fn u64(&mut self, value: u64) -> Result<(), InputCodecError> {
        self.bytes(&value.to_le_bytes())
    }
    fn i64(&mut self, value: i64) -> Result<(), InputCodecError> {
        self.bytes(&value.to_le_bytes())
    }
    fn float(&mut self, value: f32) -> Result<(), InputCodecError> {
        self.u32(value.to_bits())
    }
    fn position(&mut self, value: Position2) -> Result<(), InputCodecError> {
        self.float(value.x)?;
        self.float(value.y)
    }
    fn option<T>(
        &mut self,
        value: Option<T>,
        write: impl FnOnce(&mut Self, T) -> Result<(), InputCodecError>,
    ) -> Result<(), InputCodecError> {
        match value {
            None => self.u8(0),
            Some(value) => {
                self.u8(1)?;
                write(self, value)
            }
        }
    }
    fn point(&mut self, value: ClockPoint) -> Result<(), InputCodecError> {
        self.u32(value.domain.0)?;
        self.i64(value.timestamp.as_nanos())
    }
    fn meta(&mut self, value: EventMeta) -> Result<(), InputCodecError> {
        self.u64(value.source.0)?;
        self.i64(value.timestamp.as_nanos())?;
        self.u32(value.clock_domain.0)?;
        self.u64(value.sequence)?;
        self.option(value.native, |out, native| {
            out.u32(native.backend.0)?;
            out.option(native.code, Writer::u32)?;
            out.option(native.timestamp, Writer::point)
        })?;
        self.option(value.original_clock_point, Writer::point)
    }
    fn control(&mut self, value: PhysicalControlId) -> Result<(), InputCodecError> {
        match value {
            PhysicalControlId::HidUsage { usage_page, usage } => {
                self.u8(0)?;
                self.u16(usage_page)?;
                self.u16(usage)
            }
            PhysicalControlId::Native { backend, code } => {
                self.u8(1)?;
                self.u32(backend.0)?;
                self.u32(code)
            }
            PhysicalControlId::Vendor { namespace, code } => {
                self.u8(2)?;
                self.u32(namespace.0)?;
                self.u32(code)
            }
        }
    }
    fn payload(&mut self, value: &[u8]) -> Result<(), InputCodecError> {
        if value.len() > self.limits.max_payload_bytes {
            return Err(InputCodecError::PayloadTooLarge);
        }
        self.u64(u64::try_from(value.len()).map_err(|_| InputCodecError::LengthOverflow)?)?;
        self.bytes(value)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    limits: CodecLimits,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], InputCodecError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(InputCodecError::LengthOverflow)?;
        let result = self
            .bytes
            .get(self.position..end)
            .ok_or(InputCodecError::Truncated)?;
        self.position = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, InputCodecError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, InputCodecError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| InputCodecError::Truncated)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, InputCodecError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| InputCodecError::Truncated)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, InputCodecError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| InputCodecError::Truncated)?,
        ))
    }
    fn i64(&mut self) -> Result<i64, InputCodecError> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| InputCodecError::Truncated)?,
        ))
    }
    fn float(&mut self) -> Result<f32, InputCodecError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn position(&mut self) -> Result<Position2, InputCodecError> {
        Ok(Position2 {
            x: self.float()?,
            y: self.float()?,
        })
    }
    fn tag(&mut self, field: &'static str, maximum: u8) -> Result<u8, InputCodecError> {
        let tag = self.u8()?;
        if tag <= maximum {
            Ok(tag)
        } else {
            Err(InputCodecError::InvalidTag { field, tag })
        }
    }
    fn option<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, InputCodecError>,
    ) -> Result<Option<T>, InputCodecError> {
        if self.tag("option", 1)? == 0 {
            Ok(None)
        } else {
            read(self).map(Some)
        }
    }
    fn point(&mut self) -> Result<ClockPoint, InputCodecError> {
        Ok(ClockPoint {
            domain: ClockDomainId(self.u32()?),
            timestamp: Timestamp::from_nanos(self.i64()?),
        })
    }
    fn meta(&mut self) -> Result<EventMeta, InputCodecError> {
        Ok(EventMeta {
            source: DeviceId(self.u64()?),
            timestamp: Timestamp::from_nanos(self.i64()?),
            clock_domain: ClockDomainId(self.u32()?),
            sequence: self.u64()?,
            native: self.option(|input| {
                Ok(NativeEventMeta {
                    backend: BackendId(input.u32()?),
                    code: input.option(Reader::u32)?,
                    timestamp: input.option(Reader::point)?,
                })
            })?,
            original_clock_point: self.option(Reader::point)?,
        })
    }
    fn control(&mut self) -> Result<PhysicalControlId, InputCodecError> {
        Ok(match self.tag("control", 2)? {
            0 => PhysicalControlId::HidUsage {
                usage_page: self.u16()?,
                usage: self.u16()?,
            },
            1 => PhysicalControlId::Native {
                backend: BackendId(self.u32()?),
                code: self.u32()?,
            },
            _ => PhysicalControlId::Vendor {
                namespace: VendorNamespaceId(self.u32()?),
                code: self.u32()?,
            },
        })
    }
    fn payload(&mut self) -> Result<Vec<u8>, InputCodecError> {
        let len = usize::try_from(self.u64()?).map_err(|_| InputCodecError::LengthOverflow)?;
        if len > self.limits.max_payload_bytes {
            return Err(InputCodecError::PayloadTooLarge);
        }
        let bytes = self.take(len)?;
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(len)
            .map_err(|_| InputCodecError::AllocationFailed)?;
        payload.extend_from_slice(bytes);
        Ok(payload)
    }
}
