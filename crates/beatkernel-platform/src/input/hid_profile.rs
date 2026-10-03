//! Explicit, bounded HID bit-field profiles shared by native and browser hosts.
//!
//! Acquisition, report-ID framing and logical game bindings remain separate.
//! Profiles declare physical controls and units; they do not infer descriptors.

use std::sync::Arc;

use beatkernel::input::{
    AxisEvent, AxisMode, ButtonEvent, ButtonState, DeviceAdapter, DeviceDescriptor, DeviceId,
    PhysicalControlId, PhysicalInputEvent, PhysicalInputSink, RawHidReportEvent,
};

const MAX_REPORTS: usize = 256;
const MAX_FIELDS: usize = 256;
const MAX_PAYLOAD_BYTES: usize = 1024;

/// Optional exact hardware identifiers, in addition to required raw-HID capability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HidProfileMatch {
    /// Require this reported vendor ID; `None` accepts any vendor or an absent ID.
    pub vendor_id: Option<u16>,
    /// Require this reported product ID; `None` accepts any product or an absent ID.
    pub product_id: Option<u16>,
}

/// Bit numbering within bytes and significance within an extracted field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidBitOrder {
    /// Stream offset zero is the first byte's low bit; the first field bit is its low bit.
    LeastSignificantFirst,
    /// Stream offset zero is the first byte's high bit; the first field bit is its high bit.
    MostSignificantFirst,
}

/// Explicit interpretation of an extracted unsigned bit pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HidFieldKind {
    /// A nonzero raw value is pressed; only level transitions produce events.
    Button {
        /// Invert the raw nonzero test before observing button state.
        invert: bool,
    },
    /// An axis in profile-defined units, converted through a wide floating intermediate.
    Axis {
        /// Interpret the declared width using two's complement before scaling.
        signed: bool,
        /// Absolute samples suppress unchanged f32 bits; relative samples always emit.
        mode: AxisMode,
        /// Finite multiplier applied before the offset.
        scale: f32,
        /// Finite offset added after scaling.
        offset: f32,
    },
}

/// One physical control extracted from contiguous bits in the report payload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HidFieldSpec {
    /// Actual physical output identity; unique across the complete profile.
    pub control: PhysicalControlId,
    /// Zero-based bit position in the declared bit stream, excluding any report ID.
    pub offset_bits: u32,
    /// Field width from one through sixty-four bits.
    pub width_bits: u8,
    /// Explicit stream bit numbering and field significance.
    pub order: HidBitOrder,
    /// Button or axis interpretation and units.
    pub kind: HidFieldKind,
}

/// The exact extent and ordered fields for one separately identified report.
#[derive(Clone, Debug, PartialEq)]
pub struct HidReportSpec {
    /// `None` is unnumbered; numbered IDs must be nonzero and unique.
    pub report_id: Option<u8>,
    /// Exact payload bytes, excluding the separate report ID, from zero through 1024.
    pub payload_bytes: usize,
    /// Ordered fields; an individual report may have none.
    pub fields: Vec<HidFieldSpec>,
}

/// Setup or complete-report refusal, before retained decoder state or output changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidProfileError {
    /// Profiles require one through 256 report specifications.
    InvalidReportCount,
    /// `Some(0)` is not a canonical numbered report ID.
    InvalidReportId,
    /// Two report specifications select the same separate ID.
    DuplicateReportId,
    /// A declared payload exceeds 1024 bytes.
    PayloadCapacity,
    /// The complete profile requires one through 256 fields.
    InvalidFieldCount,
    /// More than one field assigns the same physical control.
    DuplicateControl,
    /// Field width is outside 1..=64 or its checked end exceeds the payload.
    InvalidFieldExtent,
    /// An axis scale or offset is not finite.
    NonFiniteParameters,
    /// The selected report payload differs from its exact declared extent.
    ReportLength {
        /// Declared payload bytes.
        expected: usize,
        /// Received payload bytes.
        actual: usize,
    },
    /// A scaled axis value cannot be represented by a finite core f32 sample.
    NonFiniteValue,
    /// A matching report uses another source before the adapter is reset.
    SourceMismatch {
        /// Source adopted by the first successfully validated matching report.
        expected: DeviceId,
        /// Source carried by the rejected report.
        actual: DeviceId,
    },
}

impl std::fmt::Display for HidProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HID profile: {self:?}")
    }
}
impl std::error::Error for HidProfileError {}

/// Immutable validated report definitions, shareable among independent device adapters.
#[derive(Debug)]
pub struct HidProfile {
    matcher: HidProfileMatch,
    reports: Vec<HidReportSpec>,
}

impl HidProfile {
    /// Validates bounded reports, distinct controls, bit extents and finite axis parameters.
    ///
    /// Different controls may deliberately overlap bits. No report payload or device
    /// descriptor is interpreted during setup, and the supplied vectors are retained.
    pub fn new(
        matcher: HidProfileMatch,
        reports: Vec<HidReportSpec>,
    ) -> Result<Self, HidProfileError> {
        if reports.is_empty() || reports.len() > MAX_REPORTS {
            return Err(HidProfileError::InvalidReportCount);
        }
        let mut total = 0usize;
        for report in &reports {
            total = total
                .checked_add(report.fields.len())
                .filter(|count| *count <= MAX_FIELDS)
                .ok_or(HidProfileError::InvalidFieldCount)?;
        }
        if total == 0 {
            return Err(HidProfileError::InvalidFieldCount);
        }
        let mut ids = [false; MAX_REPORTS];
        let mut controls = [None; MAX_FIELDS];
        let mut count = 0;
        for report in &reports {
            let id = match report.report_id {
                None => 0,
                Some(0) => return Err(HidProfileError::InvalidReportId),
                Some(id) => usize::from(id),
            };
            if ids[id] {
                return Err(HidProfileError::DuplicateReportId);
            }
            ids[id] = true;
            if report.payload_bytes > MAX_PAYLOAD_BYTES {
                return Err(HidProfileError::PayloadCapacity);
            }
            for field in &report.fields {
                if controls[..count].contains(&Some(field.control)) {
                    return Err(HidProfileError::DuplicateControl);
                }
                if !(1..=64).contains(&field.width_bits)
                    || field
                        .offset_bits
                        .checked_add(u32::from(field.width_bits))
                        .is_none_or(|end| end > (report.payload_bytes * 8) as u32)
                {
                    return Err(HidProfileError::InvalidFieldExtent);
                }
                if let HidFieldKind::Axis { scale, offset, .. } = field.kind {
                    if !scale.is_finite() || !offset.is_finite() {
                        return Err(HidProfileError::NonFiniteParameters);
                    }
                }
                controls[count] = Some(field.control);
                count += 1;
            }
        }
        Ok(Self { matcher, reports })
    }

    /// Returns the optional exact hardware matching criteria.
    pub fn matcher(&self) -> HidProfileMatch {
        self.matcher
    }

    /// Borrows validated report specifications in deterministic profile order.
    pub fn reports(&self) -> &[HidReportSpec] {
        &self.reports
    }
}

#[derive(Clone, Copy, Debug)]
enum Value {
    Button(bool),
    Axis(f32),
}

/// Fixed-storage report interpreter for one attached source at a time.
///
/// Create a fresh adapter for each registry attachment. Whole matching reports
/// validate before state adoption or output; sink allocation is owned by the caller.
#[derive(Debug)]
pub struct HidProfileAdapter {
    profile: Arc<HidProfile>,
    source: Option<DeviceId>,
    state: [Option<Value>; MAX_FIELDS],
    scratch: [Option<Value>; MAX_FIELDS],
}

impl HidProfileAdapter {
    /// Prepares independent level state and scratch for an immutable validated profile.
    pub fn new(profile: Arc<HidProfile>) -> Self {
        Self {
            profile,
            source: None,
            state: [None; MAX_FIELDS],
            scratch: [None; MAX_FIELDS],
        }
    }

    /// Forgets all levels and the attached source without inventing release events.
    pub fn reset(&mut self) {
        self.source = None;
        self.state = [None; MAX_FIELDS];
        self.scratch = [None; MAX_FIELDS];
    }

    /// Decodes a whole matching report and returns its emitted event count.
    ///
    /// Unknown IDs return zero without adopting a source. All successful output
    /// preserves the original metadata, including equal sequence numbers for fanout.
    /// Valid state commits before calling the infallible sink; caller panics or an
    /// enclosing registry's emission limit do not roll back the decoder.
    pub fn decode_report(
        &mut self,
        report: &RawHidReportEvent,
        out: &mut dyn PhysicalInputSink,
    ) -> Result<usize, HidProfileError> {
        let mut base = 0;
        let mut selected = None;
        for spec in &self.profile.reports {
            if spec.report_id == report.report_id {
                selected = Some(spec);
                break;
            }
            base += spec.fields.len();
        }
        let Some(spec) = selected else {
            return Ok(0);
        };
        if report.data.len() != spec.payload_bytes {
            return Err(HidProfileError::ReportLength {
                expected: spec.payload_bytes,
                actual: report.data.len(),
            });
        }
        if let Some(expected) = self.source {
            if expected != report.meta.source {
                return Err(HidProfileError::SourceMismatch {
                    expected,
                    actual: report.meta.source,
                });
            }
        }
        // Copy fixed storage, retaining other report IDs' levels without any
        // report-time allocation. A failed conversion changes scratch only.
        self.scratch = self.state;
        for (index, field) in spec.fields.iter().enumerate() {
            let raw = extract_bits(&report.data, *field);
            let value = match field.kind {
                HidFieldKind::Button { invert } => Value::Button((raw != 0) ^ invert),
                HidFieldKind::Axis {
                    signed,
                    scale,
                    offset,
                    ..
                } => {
                    let numeric = if signed {
                        let shift = 64 - u32::from(field.width_bits);
                        (((raw << shift) as i64) >> shift) as f64
                    } else {
                        raw as f64
                    };
                    let value = (numeric * f64::from(scale) + f64::from(offset)) as f32;
                    if !value.is_finite() {
                        return Err(HidProfileError::NonFiniteValue);
                    }
                    Value::Axis(value)
                }
            };
            self.scratch[base + index] = Some(value);
        }
        self.source = Some(report.meta.source);
        std::mem::swap(&mut self.state, &mut self.scratch);
        let mut emitted = 0;
        for (index, field) in spec.fields.iter().enumerate() {
            let previous = self.scratch[base + index];
            let event = match (field.kind, self.state[base + index]) {
                (HidFieldKind::Button { .. }, Some(Value::Button(down))) => {
                    let changed = match previous {
                        Some(Value::Button(old)) => old != down,
                        None => down,
                        _ => unreachable!("validated profile field kind is immutable"),
                    };
                    changed.then_some(PhysicalInputEvent::Button(ButtonEvent {
                        meta: report.meta,
                        control: field.control,
                        state: if down {
                            ButtonState::Down
                        } else {
                            ButtonState::Up
                        },
                    }))
                }
                (HidFieldKind::Axis { mode, .. }, Some(Value::Axis(value))) => {
                    let changed = match previous {
                        Some(Value::Axis(old)) => old.to_bits() != value.to_bits(),
                        None => true,
                        _ => unreachable!("validated profile field kind is immutable"),
                    };
                    (mode == AxisMode::Relative || changed).then_some(PhysicalInputEvent::Axis(
                        AxisEvent {
                            meta: report.meta,
                            control: field.control,
                            value,
                            mode,
                        },
                    ))
                }
                _ => unreachable!("validated report populated every selected field"),
            };
            if let Some(event) = event {
                emitted += 1;
                out.push(event);
            }
        }
        Ok(emitted)
    }
}

impl DeviceAdapter for HidProfileAdapter {
    fn accepts(&self, device: &DeviceDescriptor) -> bool {
        device.capabilities.raw_hid
            && self
                .profile
                .matcher
                .vendor_id
                .is_none_or(|id| device.vendor_id == Some(id))
            && self
                .profile
                .matcher
                .product_id
                .is_none_or(|id| device.product_id == Some(id))
    }

    fn on_report(&mut self, report: &RawHidReportEvent, out: &mut dyn PhysicalInputSink) {
        let _ = self.decode_report(report, out);
    }
}

fn extract_bits(payload: &[u8], field: HidFieldSpec) -> u64 {
    let mut value = 0u64;
    for index in 0..usize::from(field.width_bits) {
        let position = field.offset_bits as usize + index;
        let (byte_bit, value_bit) = match field.order {
            HidBitOrder::LeastSignificantFirst => (position % 8, index),
            HidBitOrder::MostSignificantFirst => {
                (7 - position % 8, usize::from(field.width_bits) - 1 - index)
            }
        };
        value |= u64::from((payload[position / 8] >> byte_bit) & 1) << value_bit;
    }
    value
}
