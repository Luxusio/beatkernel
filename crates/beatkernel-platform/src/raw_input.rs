//! Safe Windows Raw Input decoding and state processing on every host.
//!
//! Literal packet fixtures exercise the same path used by native acquisition.
//! These APIs do not call Windows, register input or claim hardware timing.
//! Processing and report ownership may allocate; this is a control-thread API.
//!
//! ```
//! use beatkernel::time::{ClockDomainId, ClockMapper};
//! use beatkernel_platform::raw_input::{QpcClockMapping, WINDOWS_QPC_CLOCK_DOMAIN};
//! let host_clock = ClockDomainId(1);
//! let mapping = QpcClockMapping::new(3, 2, host_clock)?;
//! let receipt = mapping.point(3)?;
//! assert_eq!(receipt.domain, WINDOWS_QPC_CLOCK_DOMAIN);
//! // Convert absolute points first, then subtract the converted origin.
//! assert_eq!(mapping.map(receipt, host_clock).unwrap().as_nanos(), 333_333_334);
//! # Ok::<(), beatkernel_platform::raw_input::RawInputError>(())
//! ```

#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use beatkernel::{
    input::{
        BackendId, ButtonEvent, ButtonState, DeviceDescriptor, DeviceId, DeviceTransport,
        EventMeta, NativeEventMeta, PhysicalControlId, PhysicalInputEvent, RawHidReportEvent,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
};

use crate::{
    keyboard::{windows_scan_code, ScanCodePrefix},
    raw_device_registry::DeviceRegistry,
};

pub use crate::raw_hid::HidReports;

/// Maximum accepted native packet length, including header and padding.
pub const MAX_RAW_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum reports emitted from one HID acquisition, bounding owned fanout.
pub const MAX_HID_REPORTS: usize = 4096;
/// Maximum simultaneously registered native devices.
pub const MAX_RAW_INPUT_DEVICES: usize = 4096;
/// Raw Input provenance namespace; keyboard codes pack all flags and make bits.
pub const WINDOWS_RAW_INPUT_BACKEND: BackendId = BackendId(4);
/// Physical fallback namespace for scan-zero VKey input, independent of text.
pub const WINDOWS_VKEY_BACKEND: BackendId = BackendId(5);
/// Absolute QPC converted to nanoseconds; callers must reserve this domain ID.
pub const WINDOWS_QPC_CLOCK_DOMAIN: ClockDomainId = ClockDomainId(0x5751_5043);

const KEY_BREAK: u16 = 1;
const KEY_E0: u16 = 2;
const KEY_E1: u16 = 4;
const VK_PAUSE: u16 = 0x13;

/// An explicit packet, source, sequence or clock rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInputError {
    /// The header or required payload is incomplete.
    Truncated,
    /// The declared packet size differs from the supplied byte count.
    InvalidPacketSize,
    /// The packet exceeds [`MAX_RAW_INPUT_BYTES`].
    PacketTooLarge,
    /// Mouse or another unsupported native discriminator.
    UnsupportedType(u32),
    /// HID report size/count is zero or its product cannot be represented.
    InvalidHidSize,
    /// The report count exceeds [`MAX_HID_REPORTS`].
    TooManyReports(usize),
    /// A zero native handle provides no device identity.
    InvalidDeviceHandle,
    /// The native handle is unregistered or retired.
    UnknownDevice(u64),
    /// A packet or re-registration conflicts with the registered device kind.
    DeviceKindMismatch,
    /// The active device count exceeds [`MAX_RAW_INPUT_DEVICES`].
    DeviceLimit,
    /// All runtime device identities have been consumed.
    DeviceIdExhausted,
    /// The source acquisition counter cannot advance.
    SequenceExhausted(DeviceId),
    /// The keyboard reported the overrun make code, rather than a key.
    KeyboardOverrun,
    /// No mapping to the processor output clock could be established.
    UnmappedClock,
    /// QPC frequency must be strictly positive.
    InvalidQpcFrequency,
    /// QPC counters and the absolute origin must be nonnegative.
    InvalidQpcCounter,
    /// Absolute native and relative output clock domains must be distinct.
    ClockDomainCollision,
    /// A nanosecond conversion exceeds the canonical i64 timestamp range.
    TimestampOverflow,
}

impl fmt::Display for RawInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Raw Input rejected: {self:?}")
    }
}

impl Error for RawInputError {}

/// Native pointer width in the packet's header, independent of the test host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInputLayout {
    /// 32-bit HANDLE and WPARAM; 16-byte header.
    Win32,
    /// 64-bit HANDLE and WPARAM; 24-byte header.
    Win64,
}

impl RawInputLayout {
    /// Returns the native header byte count.
    pub const fn header_size(self) -> usize {
        match self {
            Self::Win32 => 16,
            Self::Win64 => 24,
        }
    }
}

/// Every field of a decoded RAWINPUTHEADER, with handles widened losslessly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawInputHeader {
    /// Native type discriminator (keyboard 1 or HID 2).
    pub kind: u32,
    /// Complete packet byte count, including native padding.
    pub size: u32,
    /// Opaque source HANDLE; this is not a permanent hardware identity.
    pub device_handle: u64,
    /// Original input code from the header WPARAM.
    pub input_code: u64,
}

/// Every field of a decoded RAWKEYBOARD, without layout/text interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawKeyboard {
    /// Complete native make code.
    pub make_code: u16,
    /// Complete native flag word, including break and prefix bits.
    pub flags: u16,
    /// Unmodified reserved field.
    pub reserved: u16,
    /// Native virtual key, used only for explicit scan-zero fallback/filtering.
    pub virtual_key: u16,
    /// Original native keyboard message.
    pub message: u32,
    /// Original device-specific additional information.
    pub extra_information: u32,
}

impl RawKeyboard {
    /// Packs the full original flag word above the full make code.
    pub const fn native_code(self) -> u32 {
        ((self.flags as u32) << 16) | self.make_code as u32
    }
}

/// Validated native payload, excluding only unused trailing packet padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInputData<'a> {
    /// A complete keyboard payload.
    Keyboard(RawKeyboard),
    /// A borrowed length-checked batch of opaque HID wire reports.
    Hid(HidReports<'a>),
}

/// A safe, borrowed native packet whose validation cannot be bypassed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawInputPacket<'a> {
    header: RawInputHeader,
    data: RawInputData<'a>,
}

impl<'a> RawInputPacket<'a> {
    /// Validates packet size/layout and decodes all header and payload fields.
    ///
    /// Keyboard/HID trailing native padding is accepted but not emitted. Mouse
    /// packets are explicitly unsupported. No allocation or native API occurs.
    pub fn parse(bytes: &'a [u8], layout: RawInputLayout) -> Result<Self, RawInputError> {
        if bytes.len() > MAX_RAW_INPUT_BYTES {
            return Err(RawInputError::PacketTooLarge);
        }
        let offset = layout.header_size();
        if bytes.len() < offset {
            return Err(RawInputError::Truncated);
        }
        let size = u32_at(bytes, 4);
        if size as usize != bytes.len() {
            return Err(RawInputError::InvalidPacketSize);
        }
        let (device_handle, input_code) = match layout {
            RawInputLayout::Win32 => (u64::from(u32_at(bytes, 8)), u64::from(u32_at(bytes, 12))),
            RawInputLayout::Win64 => (u64_at(bytes, 8), u64_at(bytes, 16)),
        };
        let header = RawInputHeader {
            kind: u32_at(bytes, 0),
            size,
            device_handle,
            input_code,
        };
        let body = &bytes[offset..];
        let data = match header.kind {
            1 => {
                if body.len() < 16 {
                    return Err(RawInputError::Truncated);
                }
                RawInputData::Keyboard(RawKeyboard {
                    make_code: u16_at(body, 0),
                    flags: u16_at(body, 2),
                    reserved: u16_at(body, 4),
                    virtual_key: u16_at(body, 6),
                    message: u32_at(body, 8),
                    extra_information: u32_at(body, 12),
                })
            }
            2 => RawInputData::Hid(HidReports::parse(body)?),
            kind => return Err(RawInputError::UnsupportedType(kind)),
        };
        Ok(Self { header, data })
    }

    /// Returns every native header field.
    pub const fn header(&self) -> RawInputHeader {
        self.header
    }

    /// Returns the typed payload; HID data continues to borrow the input bytes.
    pub const fn data(&self) -> RawInputData<'a> {
        self.data
    }
}

// All callers validate the fixed header/body length before these field reads.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("checked field"))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("checked field"))
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("checked field"))
}

/// The native semantic device class supported by this processor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawDeviceKind {
    /// Raw keyboard transitions with scan codes.
    Keyboard,
    /// Uninterpreted generic HID reports.
    Hid,
}

/// Device information used to construct a backend-assigned runtime descriptor.
///
/// Unknown information remains absent; interface paths are not friendly names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDeviceInfo {
    /// The registered native device class.
    pub kind: RawDeviceKind,
    /// Vendor identity, when reported.
    pub vendor_id: Option<u16>,
    /// Product identity, when reported.
    pub product_id: Option<u16>,
    /// Serial identity, when known independently of an interface path.
    pub serial: Option<String>,
    /// An optional descriptive name, rather than a raw device path.
    pub name: Option<String>,
    /// Reported transport; never inferred from an ambiguous interface path.
    pub transport: DeviceTransport,
}

impl RawDeviceInfo {
    /// Creates a class descriptor with absent hardware fields and unknown transport.
    pub const fn new(kind: RawDeviceKind) -> Self {
        Self {
            kind,
            vendor_id: None,
            product_id: None,
            serial: None,
            name: None,
            transport: DeviceTransport::Unknown,
        }
    }
}

/// Explains accepted acquisitions that intentionally produce no semantic event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInputStatus {
    /// Keyboard or HID semantic events were emitted.
    Events,
    /// An E1 Pause header awaits a matching same-source continuation.
    PendingPause,
    /// VKey 255 was intentionally filtered; the native packet is still inspectable.
    FilteredKeyboard,
}

/// Owned canonical events from one accepted native acquisition.
#[derive(Clone, Debug, PartialEq)]
pub struct InputBatch {
    /// The source acquisition sequence, including filtered/pending packets.
    pub sequence: u64,
    /// Events in wire-report order, sharing the acquisition metadata.
    pub events: Vec<PhysicalInputEvent>,
    /// The semantic output or suppression reason.
    pub status: RawInputStatus,
}

/// Device-aware single-owner Raw Input processing independent of native I/O.
///
/// Registry edits and reports may allocate. Rejected packets leave all state
/// unchanged. Descriptors and sequences are scoped to this processor lifetime.
///
/// ```
/// use beatkernel::time::ClockDomainId;
/// use beatkernel_platform::raw_input::*;
/// let clock = ClockDomainId(1);
/// let mapping = QpcClockMapping::new(1_000_000_000, 1000, clock)?;
/// let mut input = RawInputProcessor::new(clock);
/// let source = input.register_device(9, RawDeviceInfo::new(RawDeviceKind::Keyboard))?;
/// // Synthetic Win64 packet: device 9, scan 0x1e (physical A), make.
/// let mut wire = vec![0; 40];
/// wire[..4].copy_from_slice(&1u32.to_le_bytes());
/// wire[4..8].copy_from_slice(&40u32.to_le_bytes());
/// wire[8..16].copy_from_slice(&9u64.to_le_bytes());
/// wire[24..26].copy_from_slice(&0x1eu16.to_le_bytes());
/// wire[30..32].copy_from_slice(&0x41u16.to_le_bytes());
/// let packet = RawInputPacket::parse(&wire, RawInputLayout::Win64)?;
/// let batch = input.process(&packet, mapping.point(1100)?, &mapping)?;
/// assert_eq!(batch.events[0].meta().source, source);
/// assert_eq!(batch.events[0].meta().timestamp.as_nanos(), 100);
/// # Ok::<(), RawInputError>(())
/// ```
#[derive(Debug)]
pub struct RawInputProcessor {
    output_clock: ClockDomainId,
    registry: DeviceRegistry,
}

impl RawInputProcessor {
    /// Constructs an empty processor with explicit normalized output domain.
    pub fn new(output_clock: ClockDomainId) -> Self {
        Self {
            output_clock,
            registry: DeviceRegistry::new(),
        }
    }

    /// Registers a nonzero handle, assigning a fresh never-reused runtime ID.
    ///
    /// A duplicate active handle of the same kind retains its original descriptor
    /// and ID. A kind conflict is rejected. Reconnect after removal gets a new ID.
    pub fn register_device(
        &mut self,
        handle: u64,
        info: RawDeviceInfo,
    ) -> Result<DeviceId, RawInputError> {
        self.registry.register(handle, info)
    }

    /// Removes a source and its held/pending state, returning its last descriptor.
    pub fn unregister_device(&mut self, handle: u64) -> Option<DeviceDescriptor> {
        self.registry
            .records
            .remove(&handle)
            .map(|record| record.descriptor)
    }

    /// Returns the current descriptor for an active native handle.
    pub fn device(&self, handle: u64) -> Option<&DeviceDescriptor> {
        self.registry
            .records
            .get(&handle)
            .map(|record| &record.descriptor)
    }

    /// Iterates active descriptors in ascending native-handle order.
    pub fn devices(&self) -> impl Iterator<Item = &DeviceDescriptor> {
        self.registry
            .records
            .values()
            .map(|record| &record.descriptor)
    }

    /// Normalizes and processes a validated packet using its native receipt point.
    ///
    /// Each accepted packet advances sequence once. Reports fan out without
    /// losing wire bytes or ordering. Native time remains receipt time, never a
    /// hardware timestamp. Same-domain receipt points bypass the mapper.
    pub fn process(
        &mut self,
        packet: &RawInputPacket<'_>,
        receipt: ClockPoint,
        mapper: &dyn ClockMapper,
    ) -> Result<InputBatch, RawInputError> {
        let handle = packet.header.device_handle;
        if handle == 0 {
            return Err(RawInputError::InvalidDeviceHandle);
        }
        let record = self
            .registry
            .records
            .get_mut(&handle)
            .ok_or(RawInputError::UnknownDevice(handle))?;
        let kind = match packet.data {
            RawInputData::Keyboard(_) => RawDeviceKind::Keyboard,
            RawInputData::Hid(_) => RawDeviceKind::Hid,
        };
        if record.kind != kind {
            return Err(RawInputError::DeviceKindMismatch);
        }
        let sequence = record
            .sequence
            .checked_add(1)
            .ok_or(RawInputError::SequenceExhausted(
                record.descriptor.runtime_id,
            ))?;
        let mut meta = EventMeta::new(record.descriptor.runtime_id, receipt, sequence);
        meta.native = Some(NativeEventMeta {
            backend: WINDOWS_RAW_INPUT_BACKEND,
            code: match packet.data {
                RawInputData::Keyboard(key) => Some(key.native_code()),
                RawInputData::Hid(_) => None,
            },
            timestamp: Some(receipt),
        });
        if receipt.domain != self.output_clock {
            meta.timestamp = mapper
                .map(receipt, self.output_clock)
                .ok_or(RawInputError::UnmappedClock)?;
            meta.clock_domain = self.output_clock;
            meta.original_clock_point = Some(receipt);
        }
        let (events, status) = match packet.data {
            RawInputData::Keyboard(key) => {
                if key.make_code == 0xff {
                    return Err(RawInputError::KeyboardOverrun);
                }
                if key.virtual_key == 255 {
                    record.pause_header = false;
                    (Vec::new(), RawInputStatus::FilteredKeyboard)
                } else if key.make_code == 0x1d
                    && key.flags == KEY_E1
                    && key.virtual_key == VK_PAUSE
                {
                    record.pause_header = true;
                    (Vec::new(), RawInputStatus::PendingPause)
                } else {
                    let completes_pause = record.pause_header
                        && key.make_code == 0x45
                        && key.virtual_key == VK_PAUSE
                        && matches!(key.flags, 0 | KEY_E1);
                    let control = if completes_pause {
                        PhysicalControlId::keyboard(0x48)
                    } else {
                        keyboard_control(key)
                    };
                    let pause = control == PhysicalControlId::keyboard(0x48);
                    let state = if key.flags & KEY_BREAK != 0 {
                        ButtonState::Up
                    } else if !pause && record.held.contains(&control) {
                        ButtonState::Repeat
                    } else {
                        ButtonState::Down
                    };
                    let events = vec![PhysicalInputEvent::Button(ButtonEvent {
                        meta,
                        control,
                        state,
                    })];
                    record.pause_header = false;
                    if !pause {
                        if state == ButtonState::Up {
                            record.held.remove(&control);
                        } else {
                            record.held.insert(control);
                        }
                    }
                    (events, RawInputStatus::Events)
                }
            }
            RawInputData::Hid(reports) => {
                let events = reports
                    .reports()
                    .map(|wire| {
                        PhysicalInputEvent::RawHidReport(RawHidReportEvent {
                            meta,
                            report_id: None,
                            data: wire.to_vec(),
                        })
                    })
                    .collect();
                (events, RawInputStatus::Events)
            }
        };
        record.sequence = sequence;
        Ok(InputBatch {
            sequence,
            events,
            status,
        })
    }
}

fn keyboard_control(key: RawKeyboard) -> PhysicalControlId {
    let flags = key.flags & !KEY_BREAK;
    if key.make_code == 0 {
        return PhysicalControlId::Native {
            backend: WINDOWS_VKEY_BACKEND,
            code: (u32::from(flags) << 16) | u32::from(key.virtual_key),
        };
    }
    let prefix = match flags {
        0 => ScanCodePrefix::None,
        KEY_E0 => ScanCodePrefix::E0,
        KEY_E1 => ScanCodePrefix::E1,
        _ => {
            return PhysicalControlId::Native {
                backend: WINDOWS_RAW_INPUT_BACKEND,
                code: (u32::from(flags) << 16) | u32::from(key.make_code),
            };
        }
    };
    windows_scan_code(key.make_code, prefix)
}

/// Explicit mapping from absolute QPC nanoseconds to elapsed host nanoseconds.
///
/// Native points use [`WINDOWS_QPC_CLOCK_DOMAIN`]. Absolute counter conversion
/// truncates nonnegative rational values; elapsed time subtracts the converted
/// origin. It does not instead round a tick delta. No native API is called.
#[derive(Clone, Copy, Debug)]
pub struct QpcClockMapping {
    frequency: i64,
    origin_counter: i64,
    origin: Timestamp,
    output: ClockDomainId,
}

impl QpcClockMapping {
    /// Validates a positive frequency, nonnegative origin and distinct output domain.
    pub fn new(
        frequency: i64,
        origin_counter: i64,
        output: ClockDomainId,
    ) -> Result<Self, RawInputError> {
        if frequency <= 0 {
            return Err(RawInputError::InvalidQpcFrequency);
        }
        if output == WINDOWS_QPC_CLOCK_DOMAIN {
            return Err(RawInputError::ClockDomainCollision);
        }
        Ok(Self {
            frequency,
            origin_counter,
            origin: qpc_ns(origin_counter, frequency)?,
            output,
        })
    }

    /// Converts a nonnegative QPC counter into an absolute native clock point.
    pub fn point(&self, counter: i64) -> Result<ClockPoint, RawInputError> {
        Ok(ClockPoint {
            domain: WINDOWS_QPC_CLOCK_DOMAIN,
            timestamp: qpc_ns(counter, self.frequency)?,
        })
    }

    /// Returns the validated native counter frequency in ticks per second.
    pub const fn frequency(&self) -> i64 {
        self.frequency
    }

    /// Returns the native counter chosen as the normalized host-time origin.
    pub const fn origin_counter(&self) -> i64 {
        self.origin_counter
    }
}

impl ClockMapper for QpcClockMapping {
    fn map(&self, point: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if point.domain == to {
            return Some(point.timestamp);
        }
        if point.domain == WINDOWS_QPC_CLOCK_DOMAIN && to == self.output {
            point
                .timestamp
                .as_nanos()
                .checked_sub(self.origin.as_nanos())
                .map(Timestamp::from_nanos)
        } else {
            None
        }
    }

    fn quality(&self) -> ClockMappingQuality {
        // Integer arithmetic is exact for this explicitly quantized relation;
        // this is not a measurement of device-to-receipt latency.
        ClockMappingQuality::Exact
    }
}

fn qpc_ns(counter: i64, frequency: i64) -> Result<Timestamp, RawInputError> {
    if counter < 0 {
        return Err(RawInputError::InvalidQpcCounter);
    }
    let ns = i128::from(counter) * 1_000_000_000 / i128::from(frequency);
    i64::try_from(ns)
        .map(Timestamp::from_nanos)
        .map_err(|_| RawInputError::TimestampOverflow)
}

#[cfg(test)]
#[path = "../tests/support/raw_input_boundaries.rs"]
mod boundary_tests;
