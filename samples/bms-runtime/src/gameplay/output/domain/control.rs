//! Session-isolated bounded output requests; no device or synchronization ownership.
use crate::settings::{NativeSettings, SettingsHost};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputCapability {
    pub host: SettingsHost,
    pub current_args: Vec<String>,
}
impl OutputCapability {
    pub fn validate(&self) -> Result<(), String> {
        NativeSettings::output_only(&self.current_args, self.host).map(|_| ())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputRequest {
    pub id: u64,
    pub args: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputReply {
    pub id: u64,
    pub result: Result<OutputCapability, String>,
}
pub struct OutputControls {
    next: Option<u64>,
    capability: Option<OutputCapability>,
    queued: Option<OutputRequest>,
    flight: Option<u64>,
    reply: Option<OutputReply>,
    closed: bool,
}
impl Default for OutputControls {
    fn default() -> Self {
        Self::new()
    }
}
impl OutputControls {
    pub fn new() -> Self {
        Self {
            next: Some(1),
            capability: None,
            queued: None,
            flight: None,
            reply: None,
            closed: false,
        }
    }
    pub fn capability(&self) -> Option<&OutputCapability> {
        self.capability.as_ref()
    }
    pub fn pending(&self) -> bool {
        self.queued.is_some() || self.flight.is_some() || self.reply.is_some()
    }
    pub fn queued(&self) -> bool {
        self.queued.is_some()
    }
    pub fn advertise(&mut self, capability: Option<OutputCapability>) -> Result<(), String> {
        if self.closed {
            return Err("output controls closed".into());
        }
        if let Some(cap) = &capability {
            cap.validate()?;
        }
        self.capability = capability;
        Ok(())
    }
    pub fn request(&mut self, args: Vec<String>) -> Result<u64, String> {
        if self.closed {
            return Err("output controls closed".into());
        }
        let cap = self
            .capability
            .as_ref()
            .ok_or("live output changes are unsupported")?;
        if self.pending() {
            return Err("an output request is still pending".into());
        }
        NativeSettings::output_only(&args, cap.host)?;
        let id = self.next.ok_or("output request identity exhausted")?;
        self.queued = Some(OutputRequest { id, args });
        self.next = id.checked_add(1);
        Ok(id)
    }
    pub fn take_request(&mut self) -> Option<OutputRequest> {
        let request = self.queued.take()?;
        self.flight = Some(request.id);
        Some(request)
    }
    pub fn reply(&mut self, reply: &OutputReply) -> Result<(), String> {
        if self.flight != Some(reply.id) || self.reply.is_some() {
            return Err("output reply identity differs".into());
        }
        match &reply.result {
            Ok(cap) => cap.validate()?,
            Err(error) if error.len() > 4096 || error.chars().any(|ch| ch.is_control()) => {
                return Err("output diagnostic exceeds limits".into());
            }
            _ => {}
        }
        if let Ok(cap) = &reply.result {
            self.capability = Some(cap.clone());
        }
        self.reply = Some(reply.clone());
        self.flight = None;
        Ok(())
    }
    pub fn take_reply(&mut self) -> Option<OutputReply> {
        self.reply.take()
    }
    pub fn settle(&mut self, message: &str) {
        let id = self
            .queued
            .take()
            .map(|request| request.id)
            .or(self.flight.take());
        if let Some(id) = id {
            self.reply = Some(OutputReply {
                id,
                result: Err(message
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(512)
                    .collect()),
            });
        }
    }
    pub fn close(&mut self, message: &str) {
        self.settle(message);
        self.closed = true;
        self.capability = None;
    }
}
#[cfg(test)]
#[path = "control_fixtures.rs"]
pub(crate) mod fixtures;
