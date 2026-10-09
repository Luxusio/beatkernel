//! Cold UI/owner practice mailbox. Never used by the audio callback.
use beatkernel::time::Timestamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeCapability {
    pub generation: u64,
    pub min_target: Timestamp,
    pub max_target: Timestamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeAction {
    Scrub { target: Timestamp },
    Loop { start: Timestamp, end: Timestamp },
    DisableLoop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeRequest {
    pub id: u64,
    pub generation: u64,
    pub action: PracticeAction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeApplied {
    pub generation: u64,
    pub physical_frame: u64,
    pub playback_frame: u64,
    pub requested_target: Timestamp,
    pub applied_target: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PracticeReply {
    pub id: u64,
    /// Generation in the admitted request, not the applied generation.
    pub generation: u64,
    pub result: Result<PracticeApplied, String>,
}
/// A consumed response paired with the original request in one mailbox operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PracticeResponse {
    pub request: PracticeRequest,
    pub reply: PracticeReply,
}
#[derive(Default)]
pub(crate) struct PracticeControls {
    capability: Option<PracticeCapability>,
    next_id: Option<u64>,
    pending: Option<PracticeRequest>,
    taken: bool,
    reply: Option<PracticeReply>,
}
impl PracticeControls {
    pub fn new() -> Self {
        Self {
            next_id: Some(1),
            ..Self::default()
        }
    }
    pub fn capability(&self) -> Option<PracticeCapability> {
        self.capability
    }
    pub fn pending_request(&self) -> Option<PracticeRequest> {
        self.pending
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn advertise(&mut self, capability: Option<PracticeCapability>) -> Result<(), String> {
        if let Some(cap) = capability {
            if cap.min_target.as_nanos() < 0 || cap.max_target < cap.min_target {
                return Err("invalid practice capability range".into());
            }
            if self
                .capability
                .is_some_and(|old| cap.generation < old.generation)
            {
                return Err("stale practice capability".into());
            }
            if self.pending()
                && self.capability.is_some_and(|old| {
                    old.min_target != cap.min_target || old.max_target != cap.max_target
                })
            {
                return Err("practice range cannot change while acknowledgement is pending".into());
            }
        } else if self.pending() {
            self.revoke("practice capability revoked");
        }
        self.capability = capability;
        Ok(())
    }
    pub fn request(&mut self, action: PracticeAction) -> Result<u64, String> {
        let cap = self.capability.ok_or("retained practice is unsupported")?;
        if self.pending() {
            return Err("practice request is pending".into());
        }
        let in_range = |time| time >= cap.min_target && time <= cap.max_target;
        match action {
            PracticeAction::Scrub { target } if !in_range(target) => {
                return Err("practice target is outside the original chart".into())
            }
            PracticeAction::Loop { start, end }
                if !in_range(start) || !in_range(end) || end <= start =>
            {
                return Err("invalid practice loop region".into())
            }
            _ => {}
        }
        let id = self.next_id.ok_or("practice request identity exhausted")?;
        self.next_id = id.checked_add(1);
        self.pending = Some(PracticeRequest {
            id,
            generation: cap.generation,
            action,
        });
        self.taken = false;
        Ok(id)
    }
    pub fn take_request(&mut self) -> Option<PracticeRequest> {
        if self.taken || self.reply.is_some() {
            return None;
        }
        let request = self.pending?;
        self.taken = true;
        Some(request)
    }
    pub fn reply(&mut self, reply: &PracticeReply) -> Result<(), String> {
        let request = self.pending.ok_or("no practice request is pending")?;
        if !self.taken
            || self.reply.is_some()
            || reply.id != request.id
            || reply.generation != request.generation
        {
            return Err("stale practice acknowledgement".into());
        }
        if let Ok(applied) = reply.result {
            if (if request.action == PracticeAction::DisableLoop {
                applied.generation < request.generation
            } else {
                applied.generation <= request.generation
            }) || applied.playback_frame > applied.physical_frame
            {
                return Err("invalid practice boundary acknowledgement".into());
            }
            let target = match request.action {
                PracticeAction::Scrub { target } => Some(target),
                PracticeAction::Loop { start, .. } => Some(start),
                PracticeAction::DisableLoop => None,
            };
            let cap = self.capability.ok_or("practice capability was revoked")?;
            if applied.generation < cap.generation {
                return Err("practice acknowledgement precedes current generation".into());
            }
            if target.is_some_and(|target| target != applied.requested_target)
                || applied.applied_target < cap.min_target
                || applied.applied_target > cap.max_target
            {
                return Err("practice acknowledgement target mismatch".into());
            }
            self.capability = Some(PracticeCapability {
                generation: applied.generation,
                ..cap
            });
        }
        self.reply = Some(reply.clone());
        Ok(())
    }
    pub fn take_response(&mut self) -> Option<PracticeResponse> {
        let request = self.pending?;
        let reply = self.reply.take()?;
        self.pending = None;
        self.taken = false;
        Some(PracticeResponse { request, reply })
    }
    pub fn revoke(&mut self, reason: &str) {
        self.capability = None;
        if self.reply.is_none() {
            if let Some(request) = self.pending {
                self.reply = Some(PracticeReply {
                    id: request.id,
                    generation: request.generation,
                    result: Err(reason.into()),
                });
            }
        }
    }
}
