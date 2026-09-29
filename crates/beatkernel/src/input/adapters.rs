use super::{DeviceDescriptor, PhysicalInputEvent, RawHidReportEvent};

/// Receives typed physical events emitted by an adapter.
pub trait PhysicalInputSink {
    /// Receives one event in emission order.
    fn push(&mut self, event: PhysicalInputEvent);
}

impl PhysicalInputSink for Vec<PhysicalInputEvent> {
    fn push(&mut self, event: PhysicalInputEvent) {
        Vec::push(self, event);
    }
}

/// Interprets device-specific HID reports as typed physical events.
///
/// Implementations own report validation and coordinate conventions. Outputs
/// derived from one report should retain its acquisition metadata, including
/// equal sequence numbers for fanout. Malformed reports must be handled safely.
pub trait DeviceAdapter {
    /// Reports whether this adapter can interpret the described device.
    fn accepts(&self, device: &DeviceDescriptor) -> bool;

    /// Parses a report and emits zero or more events to the supplied sink.
    fn on_report(&mut self, report: &RawHidReportEvent, out: &mut dyn PhysicalInputSink);
}
