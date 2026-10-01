use super::sys::{ioctl_bytes, request, supported_abi};
use super::{LinuxError, MonotonicClock};
use beatkernel::{
    input::{
        AxisEvent, AxisMode, BackendId, ButtonEvent, ButtonState, DeviceCapabilities,
        DeviceDescriptor, DeviceId, DeviceTransport, EventMeta, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent, RawHidReportEvent,
    },
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

const EVDEV: BackendId = BackendId(2);
const HIDRAW: BackendId = BackendId(4);
const NONBLOCK: i32 = 0x800;
const KEY_BYTES: usize = 96;
const ABS_BYTES: usize = 8;

/// Keyboard-capable evdev metadata or a disabled, unqueryable candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvdevKeyboardDevice {
    /// Exact numeric /dev/input/eventN path; never a runtime DeviceId.
    pub path: String,
    /// Strictly validated optional native device name.
    pub name: Option<String>,
    /// Bounded capability description or reason metadata could not be queried.
    pub detail: String,
    /// Whether metadata confirms keyboard capability and the node is readable.
    /// Opening the same path for actual gameplay still requires separate checks.
    pub selectable: bool,
}

/// Enumerates native keyboard metadata without reading or grabbing events.
///
/// Limits are 1..=4096 numeric event candidates and 1..=16384 bytes per path,
/// name or detail. All directory entries, including non-candidates, count toward
/// a separate fixed 4096-entry scan limit. Exceeding a cap rejects the result;
/// paths are sorted by numerical suffix, then exact filename for equal numbers.
///
/// A keyboard must advertise EV_KEY and KEY_A, KEY_Z and KEY_ENTER. Successfully
/// queried other input devices are omitted. Unreadable/non-character/malformed
/// candidates remain visible with selectable=false and a bounded error reason.
/// Names use read-only EVIOCGNAME; no clock selection, event read or grab occurs.
/// The 14-bit ioctl size field imposes a 16382-byte complete-name ceiling even
/// with a larger caller limit. Missing directory/scan errors are returned.
///
/// This identifies a standard keyboard capability subset, not every possible
/// keypad/controller. Opening a descriptor can activate the kernel input device
/// until it is closed; this query does not acquire events for gameplay.
/// See the [Linux input UAPI](https://github.com/torvalds/linux/blob/master/include/uapi/linux/input.h).
pub fn evdev_keyboard_devices(
    max_devices: usize,
    max_text_bytes: usize,
) -> Result<Vec<EvdevKeyboardDevice>, LinuxError> {
    validate_keyboard_limits(max_devices, max_text_bytes)?;
    supported_abi()?;
    let mut paths = Vec::new();
    paths
        .try_reserve_exact(max_devices)
        .map_err(|_| keyboard_allocation_error())?;
    for (examined, entry) in std::fs::read_dir("/dev/input")?.enumerate() {
        if examined == 4096 {
            return Err(LinuxError::InvalidConfiguration(
                "evdev discovery directory-entry limit exceeded",
            ));
        }
        let entry = entry?;
        let name = entry.file_name();
        let bytes = name.as_bytes();
        if !numeric_event_name(bytes) {
            continue;
        }
        if paths.len() == max_devices {
            return Err(LinuxError::InvalidConfiguration(
                "evdev discovery candidate limit exceeded",
            ));
        }
        // Numeric event names are ASCII, so exact UTF-8 conversion cannot fail.
        let name = std::str::from_utf8(bytes)
            .map_err(|_| LinuxError::MalformedInput("invalid evdev path UTF-8"))?;
        let path_length = "/dev/input/".len() + name.len();
        if path_length > max_text_bytes {
            return Err(LinuxError::MalformedInput(
                "evdev path exceeds selected text limit",
            ));
        }
        let mut path = String::new();
        path.try_reserve_exact(path_length)
            .map_err(|_| keyboard_allocation_error())?;
        path.push_str("/dev/input/");
        path.push_str(name);
        paths.push(path);
    }
    paths.sort_unstable_by(|left, right| compare_event_paths(left, right));
    let mut devices = Vec::new();
    devices
        .try_reserve_exact(paths.len())
        .map_err(|_| keyboard_allocation_error())?;
    for path in paths {
        match keyboard_metadata(&path, max_text_bytes) {
            Ok(Some(name)) => devices.push(EvdevKeyboardDevice {
                path,
                name,
                detail: keyboard_owned_text("EV_KEY: KEY_A, KEY_Z, KEY_ENTER", max_text_bytes)?,
                selectable: true,
            }),
            Ok(None) => {} // Successfully queried non-keyboard.
            Err(error) => devices.push(EvdevKeyboardDevice {
                path,
                name: None,
                detail: keyboard_error_detail(&error, max_text_bytes)?,
                selectable: false,
            }),
        }
    }
    Ok(devices)
}

fn validate_keyboard_limits(max_devices: usize, max_text_bytes: usize) -> Result<(), LinuxError> {
    if !(1..=4096).contains(&max_devices) || !(1..=16384).contains(&max_text_bytes) {
        return Err(LinuxError::InvalidConfiguration(
            "evdev discovery limits must be 1..4096 nodes and 1..16384 text bytes",
        ));
    }
    Ok(())
}
fn numeric_event_name(name: &[u8]) -> bool {
    name.strip_prefix(b"event")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.iter().all(u8::is_ascii_digit))
}
fn compare_event_paths(left: &str, right: &str) -> std::cmp::Ordering {
    let left_number = left
        .rsplit('/')
        .next()
        .unwrap_or(left)
        .strip_prefix("event")
        .unwrap_or("")
        .trim_start_matches('0');
    let right_number = right
        .rsplit('/')
        .next()
        .unwrap_or(right)
        .strip_prefix("event")
        .unwrap_or("")
        .trim_start_matches('0');
    left_number
        .len()
        .cmp(&right_number.len())
        .then_with(|| left_number.cmp(right_number))
        .then_with(|| left.cmp(right))
}
fn keyboard_capable(types: &[u8], keys: &[u8]) -> bool {
    bit(types, 1) && [28, 30, 44].into_iter().all(|code| bit(keys, code))
}
fn keyboard_metadata(
    path: &str,
    max_text_bytes: usize,
) -> Result<Option<Option<String>>, LinuxError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(NONBLOCK)
        .open(path)?;
    if !file.metadata()?.file_type().is_char_device() {
        return Err(LinuxError::MalformedInput(
            "evdev candidate is not a character device",
        ));
    }
    let mut types = [0u8; 8];
    let type_count = ioctl_bytes(&file, request(2, b'E', 0x20, types.len()), &mut types)?;
    if type_count == 0 || type_count > types.len() {
        return Err(LinuxError::MalformedInput(
            "invalid EVIOCGBIT event-type length",
        ));
    }
    if !bit(&types[..type_count], 1) {
        return Ok(None);
    }
    let mut keys = [0u8; KEY_BYTES];
    let key_count = ioctl_bytes(&file, request(2, b'E', 0x21, keys.len()), &mut keys)?;
    if key_count < 6 || key_count > keys.len() {
        return Err(LinuxError::MalformedInput(
            "invalid EVIOCGBIT key-capability length",
        ));
    }
    if !keyboard_capable(&types[..type_count], &keys[..key_count]) {
        return Ok(None);
    }
    let capacity = (max_text_bytes + 1).min(16383); // _IOC_SIZEBITS = 14.
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| keyboard_allocation_error())?;
    bytes.resize(capacity, 0);
    let count = match ioctl_bytes(&file, request(2, b'E', 6, capacity), &mut bytes) {
        Err(LinuxError::Io(error)) if error.raw_os_error() == Some(2) => return Ok(Some(None)), // No native name.
        result => result?,
    };
    Ok(Some(parse_keyboard_name(&bytes, count, max_text_bytes)?))
}
fn parse_keyboard_name(
    bytes: &[u8],
    count: usize,
    max_text_bytes: usize,
) -> Result<Option<String>, LinuxError> {
    if count == 0 || count > bytes.len() {
        return Err(LinuxError::MalformedInput(
            "invalid EVIOCGNAME returned length",
        ));
    }
    let bytes = &bytes[..count];
    if bytes.last() != Some(&0) || bytes[..count - 1].contains(&0) {
        return Err(LinuxError::MalformedInput(
            "EVIOCGNAME lacks a complete native terminator",
        ));
    }
    let text = std::str::from_utf8(&bytes[..count - 1])
        .map_err(|_| LinuxError::MalformedInput("evdev name is not UTF-8"))?;
    if text.chars().any(char::is_control) {
        return Err(LinuxError::MalformedInput("evdev name contains controls"));
    }
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(keyboard_owned_text(text, max_text_bytes)?))
}
fn keyboard_owned_text(value: &str, limit: usize) -> Result<String, LinuxError> {
    if value.len() > limit {
        return Err(LinuxError::MalformedInput(
            "evdev metadata exceeds selected text limit",
        ));
    }
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| keyboard_allocation_error())?;
    owned.push_str(value);
    Ok(owned)
}
fn keyboard_error_detail(error: &LinuxError, limit: usize) -> Result<String, LinuxError> {
    let detail = match error {
        LinuxError::Io(error) => match error.raw_os_error() {
            Some(code) => format!("Metadata unavailable (errno={code})"),
            None => format!("Metadata unavailable ({:?})", error.kind()),
        },
        LinuxError::MalformedInput(reason) | LinuxError::InvalidConfiguration(reason) => {
            format!("Metadata unavailable: {reason}")
        }
        _ => "Metadata query failed".into(),
    };
    keyboard_owned_text(&detail, limit)
}
fn keyboard_allocation_error() -> LinuxError {
    LinuxError::Io(io::Error::from(io::ErrorKind::OutOfMemory))
}

/// Saturating observations, never an inferred exact count of lost native events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinuxInputCounters {
    /// Native evdev records or hidraw reports read.
    pub records: u64,
    /// SYN_DROPPED barriers observed; exact lost-event count is unknown.
    pub dropped_barriers: u64,
    /// Records intentionally discarded while awaiting resynchronization.
    pub discarded_records: u64,
    /// Unknown native types ignored without inventing semantics.
    pub ignored_records: u64,
}

/// Queried state after a loss barrier; not historical input reconstruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvdevSnapshot {
    /// Actual source identity.
    pub device: DeviceId,
    /// Userspace monotonic observation after ioctl queries, not an atomic snapshot.
    pub observed_at: ClockPoint,
    /// Native key/button codes currently reported down.
    pub pressed_keys: Vec<u16>,
    /// Supported absolute axes with current native value/min/max/fuzz/flat/resolution.
    pub absolute_axes: Vec<(u16, [i32; 6])>,
}

/// One nonblocking evdev observation.
#[derive(Clone, Debug, PartialEq)]
pub enum EvdevItem {
    /// Canonical button or type-qualified native axis sample.
    Event(PhysicalInputEvent),
    /// No record is currently readable.
    WouldBlock,
    /// SYN_DROPPED observed; discard until the following SYN_REPORT.
    Dropped,
    /// Current state must be reconciled and explicitly acknowledged by the host.
    /// Returned repeatedly while the barrier remains unacknowledged.
    Resync(EvdevSnapshot),
    /// A synchronization or unsupported record was consumed.
    Ignored,
}

/// Read-only nonblocking evdev handle with mandatory monotonic kernel timestamps.
pub struct EvdevDevice {
    file: File,
    path: PathBuf,
    descriptor: DeviceDescriptor,
    clock: MonotonicClock,
    sequence: u64,
    abs_bits: [u8; ABS_BYTES],
    dropping: bool,
    resync: Option<EvdevSnapshot>,
    counters: LinuxInputCounters,
}

impl EvdevDevice {
    /// Opens exactly this path and selects CLOCK_MONOTONIC; no fallback clock.
    pub fn open(
        path: impl AsRef<Path>,
        id: DeviceId,
        domain: ClockDomainId,
    ) -> Result<Self, LinuxError> {
        supported_abi()?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(NONBLOCK)
            .open(path.as_ref())?;
        let mut clock_id = 1i32.to_ne_bytes();
        ioctl_bytes(&file, request(1, b'E', 0xa0, 4), &mut clock_id)?;
        let mut identity = [0u8; 8];
        ioctl_bytes(&file, request(2, b'E', 2, 8), &mut identity)?;
        let mut types = [0u8; 4];
        ioctl_bytes(&file, request(2, b'E', 0x20, types.len()), &mut types)?;
        let mut abs_bits = [0u8; ABS_BYTES];
        if bit(&types, 3) {
            ioctl_bytes(&file, request(2, b'E', 0x23, abs_bits.len()), &mut abs_bits)?;
        }
        let descriptor = DeviceDescriptor {
            runtime_id: id,
            vendor_id: Some(u16::from_ne_bytes(identity[2..4].try_into().unwrap())),
            product_id: Some(u16::from_ne_bytes(identity[4..6].try_into().unwrap())),
            serial: query_string(&file, b'E', 8),
            name: query_string(&file, b'E', 6),
            transport: transport(u16::from_ne_bytes(identity[..2].try_into().unwrap())),
            capabilities: DeviceCapabilities {
                button: bit(&types, 1),
                axis: bit(&types, 2) || bit(&types, 3),
                ..Default::default()
            },
        };
        Ok(Self {
            file,
            path: path.as_ref().to_owned(),
            descriptor,
            clock: MonotonicClock::new(domain),
            sequence: 0,
            abs_bits,
            dropping: false,
            resync: None,
            counters: LinuxInputCounters::default(),
        })
    }

    /// Native metadata for this opened device; runtime ID remains fixed.
    pub const fn descriptor(&self) -> &DeviceDescriptor {
        &self.descriptor
    }
    /// Character-device number of the opened handle, for same-node alias rejection.
    /// Queries the owned descriptor rather than re-resolving its path.
    pub fn native_device_number(&self) -> Result<u64, LinuxError> {
        let metadata = self.file.metadata()?;
        if !metadata.file_type().is_char_device() {
            return Err(LinuxError::InvalidConfiguration(
                "evdev handle is not a character device",
            ));
        }
        Ok(metadata.rdev())
    }
    /// Explicit path selected at open.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Current acquisition/loss observations.
    pub const fn counters(&self) -> LinuxInputCounters {
        self.counters
    }

    /// Reads one fixed-width native record without blocking or sorting by time.
    pub fn read_next(&mut self) -> Result<EvdevItem, LinuxError> {
        if let Some(snapshot) = &self.resync {
            return Ok(EvdevItem::Resync(snapshot.clone()));
        }
        let mut record = [0u8; 24];
        match self.file.read(&mut record) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(EvdevItem::WouldBlock);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                return Ok(EvdevItem::Ignored);
            }
            Err(error) => return Err(error.into()),
            Ok(0) => {
                return Err(
                    io::Error::new(io::ErrorKind::UnexpectedEof, "evdev disconnected").into(),
                );
            }
            Ok(24) => {}
            Ok(_) => return Err(LinuxError::MalformedInput("partial evdev input_event")),
        }
        self.counters.records = self.counters.records.saturating_add(1);
        self.sequence = self.sequence.checked_add(1).ok_or(LinuxError::Overflow)?;
        let kind = u16::from_ne_bytes(record[16..18].try_into().unwrap());
        let code = u16::from_ne_bytes(record[18..20].try_into().unwrap());
        let value = i32::from_ne_bytes(record[20..24].try_into().unwrap());
        if kind == 0 && code == 3 {
            self.dropping = true;
            self.counters.dropped_barriers = self.counters.dropped_barriers.saturating_add(1);
            return Ok(EvdevItem::Dropped);
        }
        if self.dropping {
            self.counters.discarded_records = self.counters.discarded_records.saturating_add(1);
            if kind == 0 && code == 0 {
                // Query failure keeps acquisition behind the loss barrier.
                let snapshot = self.query_state()?;
                self.dropping = false;
                self.resync = Some(snapshot.clone());
                return Ok(EvdevItem::Resync(snapshot));
            }
            return Ok(EvdevItem::Ignored);
        }
        let seconds = i64::from_ne_bytes(record[..8].try_into().unwrap());
        let micros = i64::from_ne_bytes(record[8..16].try_into().unwrap());
        if !(0..1_000_000).contains(&micros) {
            return Err(LinuxError::MalformedInput("invalid evdev timeval"));
        }
        let nanos = i64::try_from(i128::from(seconds) * 1_000_000_000 + i128::from(micros) * 1000)
            .map_err(|_| LinuxError::Overflow)?;
        let point = ClockPoint {
            domain: self.clock.domain(),
            timestamp: Timestamp::from_nanos(nanos),
        };
        let native_code = (u32::from(kind) << 16) | u32::from(code);
        let mut meta = EventMeta::new(self.descriptor.runtime_id, point, self.sequence);
        meta.native = Some(NativeEventMeta {
            backend: EVDEV,
            code: Some(native_code),
            timestamp: Some(point),
        });
        let event = match kind {
            1 => PhysicalInputEvent::Button(ButtonEvent {
                meta,
                control: crate::keyboard::linux_evdev_key(code),
                state: match value {
                    0 => ButtonState::Up,
                    1 => ButtonState::Down,
                    2 => ButtonState::Repeat,
                    _ => return Err(LinuxError::MalformedInput("invalid EV_KEY value")),
                },
            }),
            2 | 3 => PhysicalInputEvent::Axis(AxisEvent {
                meta,
                control: PhysicalControlId::Native {
                    backend: EVDEV,
                    code: native_code,
                },
                value: value as f32,
                mode: if kind == 2 {
                    AxisMode::Relative
                } else {
                    AxisMode::Absolute
                },
            }),
            _ => {
                self.counters.ignored_records = self.counters.ignored_records.saturating_add(1);
                return Ok(EvdevItem::Ignored);
            }
        };
        Ok(EvdevItem::Event(event))
    }

    /// Queries present key/ABS state without synthesizing gameplay transitions.
    pub fn query_state(&self) -> Result<EvdevSnapshot, LinuxError> {
        let mut keys = [0u8; KEY_BYTES];
        if self.descriptor.capabilities.button {
            ioctl_bytes(&self.file, request(2, b'E', 0x18, keys.len()), &mut keys)?;
        }
        let pressed_keys = (0..KEY_BYTES * 8)
            .filter(|&code| bit(&keys, code))
            .map(|code| code as u16)
            .collect();
        let mut absolute_axes = Vec::new();
        for code in 0..ABS_BYTES * 8 {
            if !bit(&self.abs_bits, code) {
                continue;
            }
            let mut bytes = [0u8; 24];
            ioctl_bytes(
                &self.file,
                request(2, b'E', 0x40 + code as u8, bytes.len()),
                &mut bytes,
            )?;
            let values = std::array::from_fn(|i| {
                i32::from_ne_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            absolute_axes.push((code as u16, values));
        }
        Ok(EvdevSnapshot {
            device: self.descriptor.runtime_id,
            observed_at: self.clock.now()?,
            pressed_keys,
            absolute_axes,
        })
    }

    /// Resumes event delivery only after the host reconciles the reported state.
    /// Returns false when no resynchronization snapshot is awaiting acknowledgment.
    pub fn acknowledge_resync(&mut self) -> bool {
        self.resync.take().is_some()
    }
}

/// Nonblocking opaque HID input; reports retain kernel payloads and descriptor.
pub struct HidrawDevice {
    file: File,
    descriptor: DeviceDescriptor,
    report_descriptor: Vec<u8>,
    numbered: bool,
    report_buffer: Vec<u8>,
    clock: MonotonicClock,
    sequence: u64,
    counters: LinuxInputCounters,
}
impl HidrawDevice {
    /// Opens exactly this hidraw path and reads its real descriptor/identity.
    pub fn open(
        path: impl AsRef<Path>,
        id: DeviceId,
        domain: ClockDomainId,
    ) -> Result<Self, LinuxError> {
        supported_abi()?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(NONBLOCK)
            .open(path)?;
        let mut size = [0u8; 4];
        ioctl_bytes(&file, request(2, b'H', 1, 4), &mut size)?;
        let size = u32::from_ne_bytes(size) as usize;
        if size == 0 || size > 4096 {
            return Err(LinuxError::MalformedInput("invalid HID descriptor length"));
        }
        let mut descriptor_bytes = [0u8; 4100];
        descriptor_bytes[..4].copy_from_slice(&(size as u32).to_ne_bytes());
        ioctl_bytes(
            &file,
            request(2, b'H', 2, descriptor_bytes.len()),
            &mut descriptor_bytes,
        )?;
        let report_descriptor = descriptor_bytes[4..4 + size].to_vec();
        let (numbered, max_report_bytes) = input_report_layout(&report_descriptor)?;
        let mut info = [0u8; 8];
        ioctl_bytes(&file, request(2, b'H', 3, 8), &mut info)?;
        let bus = u32::from_ne_bytes(info[..4].try_into().unwrap());
        let descriptor = DeviceDescriptor {
            runtime_id: id,
            vendor_id: Some(u16::from_ne_bytes(info[4..6].try_into().unwrap())),
            product_id: Some(u16::from_ne_bytes(info[6..8].try_into().unwrap())),
            serial: query_string(&file, b'H', 8),
            name: query_string(&file, b'H', 4),
            transport: transport(bus as u16),
            capabilities: DeviceCapabilities {
                raw_hid: true,
                ..Default::default()
            },
        };
        Ok(Self {
            file,
            descriptor,
            report_descriptor,
            numbered,
            report_buffer: vec![0u8; max_report_bytes + 1],
            clock: MonotonicClock::new(domain),
            sequence: 0,
            counters: LinuxInputCounters::default(),
        })
    }
    /// Actual native metadata passed to a caller-selected DeviceAdapter.
    pub const fn descriptor(&self) -> &DeviceDescriptor {
        &self.descriptor
    }
    /// Exact report descriptor; vendor interpretation remains adapter-owned.
    pub fn report_descriptor(&self) -> &[u8] {
        &self.report_descriptor
    }
    /// Current native report observations.
    pub const fn counters(&self) -> LinuxInputCounters {
        self.counters
    }
    /// Returns one complete report or None for a currently empty nonblocking fd.
    /// Its timestamp is userspace acquisition time, not a hardware timestamp.
    pub fn read_report(&mut self) -> Result<Option<RawHidReportEvent>, LinuxError> {
        // One sentinel byte beyond the descriptor bound detects oversized
        // native reports instead of forwarding silently truncated payloads.
        let bytes = &mut self.report_buffer;
        let len = match self.file.read(bytes) {
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::Interrupted =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
            Ok(0) => {
                return Err(
                    io::Error::new(io::ErrorKind::UnexpectedEof, "hidraw disconnected").into(),
                );
            }
            Ok(len) => len,
        };
        if len == bytes.len() {
            return Err(LinuxError::MalformedInput(
                "HID report exceeds descriptor input bound",
            ));
        }
        self.sequence = self.sequence.checked_add(1).ok_or(LinuxError::Overflow)?;
        self.counters.records = self.counters.records.saturating_add(1);
        let point = self.clock.now()?;
        let mut meta = EventMeta::new(self.descriptor.runtime_id, point, self.sequence);
        meta.native = Some(NativeEventMeta {
            backend: HIDRAW,
            code: None,
            timestamp: None,
        });
        let (report_id, data) = if self.numbered {
            (Some(bytes[0]), bytes[1..len].to_vec())
        } else {
            (None, bytes[..len].to_vec())
        };
        Ok(Some(RawHidReportEvent {
            meta,
            report_id,
            data,
        }))
    }
}

fn bit(bytes: &[u8], code: usize) -> bool {
    bytes
        .get(code / 8)
        .is_some_and(|value| value & (1 << (code % 8)) != 0)
}
fn transport(bus: u16) -> DeviceTransport {
    match bus {
        3 => DeviceTransport::Usb,
        5 => DeviceTransport::Bluetooth,
        6 => DeviceTransport::Virtual,
        _ => DeviceTransport::Unknown,
    }
}
fn query_string(file: &File, kind: u8, number: u8) -> Option<String> {
    let mut bytes = [0u8; 256];
    ioctl_bytes(file, request(2, kind, number, bytes.len()), &mut bytes).ok()?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    (!bytes[..end].is_empty()).then(|| String::from_utf8_lossy(&bytes[..end]).into_owned())
}
// Parse only structural Input lengths and Report IDs. Vendor field semantics
// remain opaque; Output/Feature report IDs must not mark Input as numbered.
fn input_report_layout(bytes: &[u8]) -> Result<(bool, usize), LinuxError> {
    let mut offset = 0;
    let mut globals = (0usize, 0usize, 0u8); // report size, count, ID
    let mut stack = Vec::new();
    let mut input_bits = [0usize; 256];
    while offset < bytes.len() {
        let tag = bytes[offset];
        offset += 1;
        let size = if tag == 0xfe {
            if offset + 2 > bytes.len() {
                return Err(LinuxError::MalformedInput("truncated HID long item"));
            }
            let size = usize::from(bytes[offset]);
            offset += 2;
            size
        } else {
            match tag & 3 {
                3 => 4,
                value => usize::from(value),
            }
        };
        if offset + size > bytes.len() {
            return Err(LinuxError::MalformedInput("truncated HID item"));
        }
        if tag != 0xfe {
            let mut payload = [0u8; 4];
            payload[..size].copy_from_slice(&bytes[offset..offset + size]);
            let value = u32::from_le_bytes(payload) as usize;
            match tag & 0xfc {
                0x74 => globals.0 = value,
                0x94 => globals.1 = value,
                0x84 => {
                    if size != 1 || value == 0 {
                        return Err(LinuxError::MalformedInput("invalid HID report ID"));
                    }
                    globals.2 = value as u8;
                }
                0xa4 => stack.push(globals),
                0xb4 => {
                    globals = stack
                        .pop()
                        .ok_or(LinuxError::MalformedInput("HID global stack underflow"))?
                }
                0x80 => {
                    let field_bits = globals
                        .0
                        .checked_mul(globals.1)
                        .ok_or(LinuxError::Overflow)?;
                    let bits = &mut input_bits[usize::from(globals.2)];
                    *bits = bits.checked_add(field_bits).ok_or(LinuxError::Overflow)?;
                }
                _ => {}
            }
        }
        offset += size;
    }
    let numbered = input_bits[1..].iter().any(|&bits| bits != 0);
    if numbered && input_bits[0] != 0 {
        return Err(LinuxError::MalformedInput(
            "mixed numbered and unnumbered HID input",
        ));
    }
    let maximum = input_bits.iter().copied().max().unwrap_or(0).div_ceil(8) + usize::from(numbered);
    if maximum == 0 || maximum > 16_384 {
        return Err(LinuxError::InvalidConfiguration(
            "HID input size must be 1..16384 bytes",
        ));
    }
    Ok((numbered, maximum))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_discovery_names_and_numeric_order_are_exact() {
        for value in [
            b"event0".as_slice(),
            b"event007",
            b"event999999999999999999999999999999",
        ] {
            assert!(numeric_event_name(value));
        }
        for value in [
            b"event".as_slice(),
            b"event-1",
            b"event1\n",
            b"mouse0",
            b"event\xff",
        ] {
            assert!(!numeric_event_name(value));
        }
        let mut paths = [
            "/dev/input/event10",
            "/dev/input/event2",
            "/dev/input/event002",
            "/dev/input/event0",
        ];
        paths.sort_by(|a, b| compare_event_paths(a, b));
        assert_eq!(
            paths,
            [
                "/dev/input/event0",
                "/dev/input/event002",
                "/dev/input/event2",
                "/dev/input/event10"
            ]
        );
        assert!(validate_keyboard_limits(0, 1).is_err());
        assert!(validate_keyboard_limits(1, 16385).is_err());
        assert!(validate_keyboard_limits(4096, 16384).is_ok());
    }

    #[test]
    fn keyboard_discovery_requires_key_type_and_all_standard_keys() {
        let mut types = [0u8; 8];
        let mut keys = [0u8; KEY_BYTES];
        types[0] = 1 << 1;
        for code in [28, 30, 44] {
            keys[code / 8] |= 1 << (code % 8);
        }
        assert!(keyboard_capable(&types, &keys));
        keys[44 / 8] &= !(1 << (44 % 8));
        assert!(!keyboard_capable(&types, &keys));
        keys[44 / 8] |= 1 << (44 % 8);
        types[0] = 0;
        assert!(!keyboard_capable(&types, &keys));
        assert!(!keyboard_capable(&[], &[]));
    }

    #[test]
    fn keyboard_native_name_never_accepts_truncation_lossy_utf8_or_controls() {
        assert_eq!(
            parse_keyboard_name(b"name\0", 5, 4).unwrap().as_deref(),
            Some("name")
        );
        assert_eq!(parse_keyboard_name(b"\0", 1, 1).unwrap(), None);
        assert!(parse_keyboard_name(b"name", 4, 4).is_err());
        assert!(parse_keyboard_name(b"name\0", 5, 3).is_err());
        assert!(parse_keyboard_name(b"\xff\0", 2, 2).is_err());
        assert!(parse_keyboard_name(b"a\nb\0", 4, 4).is_err());
        assert!(parse_keyboard_name(b"a\0b\0", 4, 4).is_err());
        assert!(parse_keyboard_name(b"name\0", 6, 8).is_err());
        assert!(parse_keyboard_name(b"name\0", 0, 8).is_err());
        let error = LinuxError::Io(io::Error::from_raw_os_error(13));
        assert_eq!(
            keyboard_error_detail(&error, 64).unwrap(),
            "Metadata unavailable (errno=13)"
        );
        assert!(keyboard_error_detail(&error, 4).is_err());
    }

    #[test]
    fn output_report_id_does_not_strip_unnumbered_input() {
        let descriptor = [0x75, 8, 0x95, 2, 0x81, 0, 0x85, 7, 0x91, 0];
        assert_eq!(input_report_layout(&descriptor).unwrap(), (false, 2));
    }

    #[test]
    fn input_id_and_global_push_pop_affect_exact_read_bound() {
        let descriptor = [
            0x85, 3, 0x75, 8, 0x95, 2, 0x81, 0, 0xa4, 0x95, 4, 0x81, 0, 0xb4, 0x81, 0,
        ];
        assert_eq!(input_report_layout(&descriptor).unwrap(), (true, 9));
    }

    #[test]
    fn malformed_or_excessive_descriptor_lengths_fail_explicitly() {
        assert!(input_report_layout(&[0x75]).is_err());
        assert!(input_report_layout(&[0xb4]).is_err());
        assert!(input_report_layout(&[0x75, 32, 0x96, 0xff, 0xff, 0x81, 0]).is_err());
        assert!(input_report_layout(&[0x85, 0, 0x75, 8, 0x95, 1, 0x81, 0]).is_err());
    }
}
