//! Bounded, portable host composition for per-device vendor report adapters.

use beatkernel::input::{
    DeviceAdapter, DeviceDescriptor, DeviceId, EventMeta, PhysicalInputEvent, PhysicalInputSink,
    RawHidReportEvent,
};

/// Caller-assigned adapter registration identity, independent of device identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterId(pub u32);

/// Trusted factory creating fresh state implementing the existing core adapter.
pub type AdapterFactory = Box<dyn Fn() -> Box<dyn DeviceAdapter>>;

/// Finite control-thread storage bounds for an owned registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterRegistryLimits {
    adapters: usize,
    devices: usize,
    identities: usize,
    emissions: usize,
    payload_bytes: usize,
    descriptor_bytes: usize,
}
impl AdapterRegistryLimits {
    /// Validates positive caps: adapters <= 1,024; devices <= 65,536; lifetime
    /// identities <= 1,048,576; emissions <= 65,536; byte caps <= 1 MiB.
    /// Lifetime identity capacity must be at least connected-device capacity.
    pub fn new(
        adapters: usize,
        devices: usize,
        identities: usize,
        emissions: usize,
        payload_bytes: usize,
        descriptor_bytes: usize,
    ) -> Result<Self, AdapterRegistryError> {
        if adapters == 0
            || adapters > 1024
            || devices == 0
            || devices > 65_536
            || identities < devices
            || identities > 1_048_576
            || emissions == 0
            || emissions > 65_536
            || payload_bytes == 0
            || payload_bytes > 1_048_576
            || descriptor_bytes == 0
            || descriptor_bytes > 1_048_576
        {
            return Err(AdapterRegistryError::InvalidLimits);
        }
        Ok(Self {
            adapters,
            devices,
            identities,
            emissions,
            payload_bytes,
            descriptor_bytes,
        })
    }
}

/// Reason a complete adapter emission batch was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmissionError {
    /// Adapter invented or altered some acquisition metadata.
    MetadataMismatch,
    /// Numeric coordinate, pressure, axis or quaternion is not finite.
    NonFinite,
    /// Raw/custom event payload exceeds the configured byte cap.
    PayloadCapacity,
    /// Adapter attempted more than the configured number of emitted events.
    EmissionCapacity,
}

/// Explicit configuration, attachment, routing or validation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterRegistryError {
    /// Limits are zero, inconsistent or exceed implementation ceilings.
    InvalidLimits,
    /// Registration changes require every connected device to be removed first.
    DevicesConnected,
    /// This adapter registration ID already exists.
    DuplicateAdapter,
    /// Requested registration ID is absent.
    UnknownAdapter,
    /// Adapter registration capacity has been reached.
    AdapterCapacity,
    /// Connected-device capacity has been reached.
    DeviceCapacity,
    /// Lifetime connected/retired identity-history capacity has been reached.
    IdentityCapacity,
    /// This runtime device identity is already connected.
    DuplicateDevice,
    /// This runtime identity was retired and cannot be reused.
    RetiredDevice,
    /// Requested runtime device is not currently connected.
    UnknownDevice,
    /// Descriptor does not advertise raw HID acquisition.
    NotRawHid,
    /// Combined descriptor string bytes exceed the configured cap.
    DescriptorCapacity,
    /// More than one registered adapter accepts the descriptor.
    AmbiguousAdapters {
        /// First matching registration.
        first: AdapterId,
        /// Second matching registration.
        second: AdapterId,
    },
    /// Report acquisition identity differs from the explicitly routed device.
    ReportDeviceMismatch,
    /// Incoming raw payload exceeds the configured cap.
    ReportCapacity,
    /// Per-device report sequence decreased.
    ReportSequence,
    /// Equal sequence has different full acquisition metadata, so is not packet fanout.
    ReportAcquisitionMismatch,
    /// Report clock domain changed or its timestamp decreased on this attachment.
    ReportChronology,
    /// Complete canonical batch rejected at an exact zero-based emission index.
    InvalidEmission {
        /// Index of the first rejected emission.
        index: usize,
        /// Validation or storage failure.
        reason: EmissionError,
    },
}
impl std::fmt::Display for AdapterRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "device adapter registry: {self:?}")
    }
}
impl std::error::Error for AdapterRegistryError {}

/// Explicit raw-path selection or complete canonical report fanout.
#[derive(Clone, Debug, PartialEq)]
pub enum AdapterRoute {
    /// No registered adapter accepted this device; original borrowed raw report remains available.
    Unhandled,
    /// The selected adapter emitted a complete accepted batch, possibly empty.
    Handled {
        /// Selected registration identity.
        adapter: AdapterId,
        /// Canonical events in callback emission order with exact acquisition metadata.
        events: Vec<PhysicalInputEvent>,
    },
}
struct Registration {
    id: AdapterId,
    factory: AdapterFactory,
}
struct Connected {
    descriptor: DeviceDescriptor,
    adapter: Option<(AdapterId, Box<dyn DeviceAdapter>)>,
    last_report: Option<EventMeta>,
}

/// Owned registrations, connected descriptors and independent per-device adapters.
///
/// No OS acquisition or game binding is performed. Factories/callbacks are trusted
/// off-thread code; rejected batches do not roll back their internal mutations.
pub struct DeviceAdapterRegistry {
    limits: AdapterRegistryLimits,
    registrations: Vec<Registration>,
    connected: Vec<Connected>,
    retired: Vec<DeviceId>,
}
impl DeviceAdapterRegistry {
    /// Creates an empty registry with already validated finite limits.
    pub fn new(limits: AdapterRegistryLimits) -> Self {
        Self {
            limits,
            registrations: Vec::new(),
            connected: Vec::new(),
            retired: Vec::new(),
        }
    }
    /// Adds an owned trusted factory; each attachment receives independent state.
    pub fn register(
        &mut self,
        id: AdapterId,
        factory: AdapterFactory,
    ) -> Result<(), AdapterRegistryError> {
        if !self.connected.is_empty() {
            return Err(AdapterRegistryError::DevicesConnected);
        }
        if self.registrations.iter().any(|r| r.id == id) {
            return Err(AdapterRegistryError::DuplicateAdapter);
        }
        if self.registrations.len() == self.limits.adapters {
            return Err(AdapterRegistryError::AdapterCapacity);
        }
        self.registrations.push(Registration { id, factory });
        Ok(())
    }
    /// Removes an unused registration; connected-device routing must be retired first.
    pub fn unregister(&mut self, id: AdapterId) -> Result<(), AdapterRegistryError> {
        if !self.connected.is_empty() {
            return Err(AdapterRegistryError::DevicesConnected);
        }
        let index = self
            .registrations
            .iter()
            .position(|r| r.id == id)
            .ok_or(AdapterRegistryError::UnknownAdapter)?;
        self.registrations.remove(index);
        Ok(())
    }
    /// Attaches a descriptor with unique deterministic adapter selection.
    ///
    /// Selection failure does not attach the descriptor or reserve its identity,
    /// but trusted factories and predicates may already have performed side effects.
    pub fn attach(
        &mut self,
        descriptor: DeviceDescriptor,
    ) -> Result<Option<AdapterId>, AdapterRegistryError> {
        let id = descriptor.runtime_id;
        if self.connected.iter().any(|d| d.descriptor.runtime_id == id) {
            return Err(AdapterRegistryError::DuplicateDevice);
        }
        if self.retired.contains(&id) {
            return Err(AdapterRegistryError::RetiredDevice);
        }
        if self.connected.len() == self.limits.devices {
            return Err(AdapterRegistryError::DeviceCapacity);
        }
        if self.connected.len() + self.retired.len() == self.limits.identities {
            return Err(AdapterRegistryError::IdentityCapacity);
        }
        if !descriptor.capabilities.raw_hid {
            return Err(AdapterRegistryError::NotRawHid);
        }
        let bytes = descriptor
            .name
            .as_ref()
            .map_or(0, String::len)
            .checked_add(descriptor.serial.as_ref().map_or(0, String::len))
            .ok_or(AdapterRegistryError::DescriptorCapacity)?;
        if bytes > self.limits.descriptor_bytes {
            return Err(AdapterRegistryError::DescriptorCapacity);
        }
        let mut selected: Option<(AdapterId, Box<dyn DeviceAdapter>)> = None;
        for registration in &self.registrations {
            let candidate = (registration.factory)();
            if candidate.accepts(&descriptor) {
                if let Some((first, _)) = &selected {
                    return Err(AdapterRegistryError::AmbiguousAdapters {
                        first: *first,
                        second: registration.id,
                    });
                }
                selected = Some((registration.id, candidate));
            }
        }
        let selected_id = selected.as_ref().map(|(id, _)| *id);
        self.connected.push(Connected {
            descriptor,
            adapter: selected,
            last_report: None,
        });
        Ok(selected_id)
    }
    /// Retires a connected identity and drops its adapter state; capacity was reserved at attach.
    pub fn remove(&mut self, id: DeviceId) -> Result<DeviceDescriptor, AdapterRegistryError> {
        let index = self
            .connected
            .iter()
            .position(|d| d.descriptor.runtime_id == id)
            .ok_or(AdapterRegistryError::UnknownDevice)?;
        let connection = self.connected.remove(index);
        self.retired.push(id);
        Ok(connection.descriptor)
    }
    /// Borrows the descriptor owned by an explicitly attached runtime identity.
    pub fn device(&self, id: DeviceId) -> Option<&DeviceDescriptor> {
        self.connected
            .iter()
            .find(|d| d.descriptor.runtime_id == id)
            .map(|d| &d.descriptor)
    }
    /// Routes a borrowed acquired report; errors never consume or rewrite the raw report.
    pub fn route(
        &mut self,
        device: DeviceId,
        report: &RawHidReportEvent,
    ) -> Result<AdapterRoute, AdapterRegistryError> {
        if report.meta.source != device {
            return Err(AdapterRegistryError::ReportDeviceMismatch);
        }
        let connection = self
            .connected
            .iter_mut()
            .find(|d| d.descriptor.runtime_id == device)
            .ok_or(AdapterRegistryError::UnknownDevice)?;
        if report.data.len() > self.limits.payload_bytes {
            return Err(AdapterRegistryError::ReportCapacity);
        }
        if let Some(last) = connection.last_report {
            if report.meta.sequence < last.sequence {
                return Err(AdapterRegistryError::ReportSequence);
            }
            if report.meta.sequence == last.sequence && report.meta != last {
                return Err(AdapterRegistryError::ReportAcquisitionMismatch);
            }
            if report.meta.clock_domain != last.clock_domain
                || report.meta.timestamp < last.timestamp
            {
                return Err(AdapterRegistryError::ReportChronology);
            }
        }
        // Observe acquisition order before mutable decoding. Equal full metadata
        // permits native packet fanout; the core has no report subordinal to
        // distinguish fanout from a retransmitted report.
        connection.last_report = Some(report.meta);
        let Some((id, adapter)) = &mut connection.adapter else {
            return Ok(AdapterRoute::Unhandled);
        };
        let mut sink = CollectingSink {
            meta: report.meta,
            limits: self.limits,
            events: Vec::new(),
            attempted: 0,
            error: None,
        };
        adapter.on_report(report, &mut sink);
        if let Some((index, reason)) = sink.error {
            return Err(AdapterRegistryError::InvalidEmission { index, reason });
        }
        Ok(AdapterRoute::Handled {
            adapter: *id,
            events: sink.events,
        })
    }
}
struct CollectingSink {
    meta: EventMeta,
    limits: AdapterRegistryLimits,
    events: Vec<PhysicalInputEvent>,
    attempted: usize,
    error: Option<(usize, EmissionError)>,
}
impl PhysicalInputSink for CollectingSink {
    fn push(&mut self, event: PhysicalInputEvent) {
        let index = self.attempted;
        self.attempted = self.attempted.saturating_add(1);
        if self.error.is_some() {
            return;
        }
        let error = if index >= self.limits.emissions {
            Some(EmissionError::EmissionCapacity)
        } else if *event.meta() != self.meta {
            Some(EmissionError::MetadataMismatch)
        } else {
            validate_event(&event, self.limits.payload_bytes)
        };
        if let Some(reason) = error {
            self.events.clear();
            self.error = Some((index, reason));
        } else {
            self.events.push(event);
        }
    }
}
fn validate_event(event: &PhysicalInputEvent, payload_limit: usize) -> Option<EmissionError> {
    let finite = match event {
        PhysicalInputEvent::Button(_) => true,
        PhysicalInputEvent::Axis(e) => e.value.is_finite(),
        PhysicalInputEvent::Touch(e) => {
            e.position.x.is_finite()
                && e.position.y.is_finite()
                && e.pressure.is_none_or(f32::is_finite)
        }
        PhysicalInputEvent::Pointer(e) => e.position.x.is_finite() && e.position.y.is_finite(),
        PhysicalInputEvent::Pose(e) => [
            e.position.x,
            e.position.y,
            e.position.z,
            e.orientation.x,
            e.orientation.y,
            e.orientation.z,
            e.orientation.w,
        ]
        .iter()
        .all(|n| n.is_finite()),
        PhysicalInputEvent::RawHidReport(e) => {
            return (e.data.len() > payload_limit).then_some(EmissionError::PayloadCapacity)
        }
        PhysicalInputEvent::Custom(e) => {
            return (e.payload.len() > payload_limit).then_some(EmissionError::PayloadCapacity)
        }
    };
    (!finite).then_some(EmissionError::NonFinite)
}
